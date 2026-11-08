//! The browser against a real HTTPS server on loopback, through the same
//! network code the runtime uses.
//!
//! Every response here is one a real server sends: slow, chunked, lying
//! about its length, compressed past any sane size, redirecting in a
//! circle or off to another host, labelled with the wrong type, or hanging
//! up halfway. The runtime turns each into bytes or an error; the app is
//! handed exactly that and must end on a screen that says what happened.

use super::*;
use kobo_sdk::AppRunner;
use kobo_ui::TextScale;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const PAGE: &str =
    "<!doctype html><title>Fixture page</title><h1>Fixture page</h1><p>Served over loopback TLS.";

fn respond(stream: &mut impl Write, path: &str) {
    let ok = |kind: &str, body: &[u8]| {
        let mut head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        head.extend_from_slice(body);
        head
    };
    let bytes: Vec<u8> = match path {
        "/page" => ok("text/html; charset=utf-8", PAGE.as_bytes()),
        "/slow" => {
            std::thread::sleep(Duration::from_millis(1500));
            ok("text/html", PAGE.as_bytes())
        }
        "/chunked" => {
            let mut out = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
            for piece in PAGE.as_bytes().chunks(7) {
                out.extend_from_slice(format!("{:x}\r\n", piece.len()).as_bytes());
                out.extend_from_slice(piece);
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(b"0\r\n\r\n");
            out
        }
        "/short" => {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                PAGE.len() + 5000
            )
            .into_bytes();
            out.extend_from_slice(PAGE.as_bytes());
            out
        }
        "/long" => {
            let mut out = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 60\r\nConnection: close\r\n\r\n".to_vec();
            out.extend_from_slice(PAGE.as_bytes());
            out.extend_from_slice(b"<p>and a tail the length did not count");
            out
        }
        "/bomb" => {
            let body = include_bytes!("../tests-fixtures/bomb.gz");
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            out.extend_from_slice(body);
            out
        }
        "/loop" => b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        "/away" => b"HTTP/1.1 302 Found\r\nLocation: https://elsewhere.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        "/moved" => b"HTTP/1.1 301 Moved Permanently\r\nLocation: /page\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        "/pdf" => ok("text/html", b"%PDF-1.7\n1 0 obj <<>> endobj"),
        "/wikipedia" => ok(
            "text/html; charset=utf-8",
            include_bytes!("../tests/corpus/wikipedia-e-reader.html"),
        ),
        "/mislabelled" => ok("application/octet-stream", PAGE.as_bytes()),
        "/text" => ok("text/plain", b"RFC 0000\n\n    An indented line.\n"),
        "/drop" => b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Le".to_vec(),
        "/gone" => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        _ => b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    };
    let _ = stream.write_all(&bytes);
    let _ = stream.flush();
}

fn serve(listener: &TcpListener, config: &Arc<ServerConfig>) {
    for socket in listener.incoming() {
        let Ok(socket) = socket else { continue };
        let config = Arc::clone(config);
        std::thread::spawn(move || handle(socket, &config));
    }
}

fn handle(socket: TcpStream, config: &Arc<ServerConfig>) {
    let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(connection) = ServerConnection::new(Arc::clone(config)) else {
        return;
    };
    let mut stream = StreamOwned::new(connection, socket);
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
        let mut byte = [0];
        if stream.read_exact(&mut byte).is_err() {
            return;
        }
        request.push(byte[0]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split(' ').nth(1).unwrap_or("/");
    respond(&mut stream, path);
    stream.conn.send_close_notify();
    let _ = stream.flush();
}

/// The route and the trusted root are process-wide, so every test shares
/// one server.
fn start_server() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(start);
}

fn start() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    kobo_net::fixture::install(&format!(
        "localhost={}",
        listener.local_addr().expect("addr")
    ))
    .expect("fixture route");
    kobo_net::trust_owner_root(
        include_bytes!("../../../crates/kobo-net/tests/fixtures/localhost-ca.der").to_vec(),
    )
    .expect("trust");
    let config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(
                    include_bytes!("../../../crates/kobo-net/tests/fixtures/localhost-cert.der")
                        .to_vec(),
                )],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    include_bytes!("../../../crates/kobo-net/tests/fixtures/localhost-key.der")
                        .to_vec(),
                )),
            )
            .expect("cert"),
    );
    std::thread::spawn(move || serve(&listener, &config));
}

/// Opens `path` in a fresh browser, answering its fetch with what the
/// runtime's network code made of the server's reply.
fn visit(path: &str) -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(
        Browser::default(),
        DisplayMetrics {
            text_scale: TextScale::Default,
            ..kobo_ui::CLARA_BW_METRICS
        },
    );
    runner.start();
    let url = Url::parse(&format!("https://localhost{path}")).expect("url");
    runner.app_mut().retry = Some(url.clone());
    runner.action(action_id("retry"));
    let task = runner.app().pending.as_ref().expect("fetching").task;
    let outcome = match kobo_net::fetch(&url.to_string(), fetch::MAX_PAGE_BYTES) {
        Ok(body) => TaskOutcome::Completed(body),
        Err(error) => TaskOutcome::Failed(error),
    };
    runner.task_outcome(task, outcome);
    runner
}

fn shows(runner: &AppRunner<Browser>) -> String {
    match &runner.app().view {
        View::Page => format!(
            "page: {}",
            runner
                .app()
                .loaded
                .as_ref()
                .map_or("", |loaded| loaded.title.as_str())
        ),
        View::Failed(_, failure) => format!("failed: {failure:?}"),
        View::Unsupported(_, what) => format!("unsupported: {what}"),
        other => format!("{other:?}"),
    }
}

#[test]
fn every_kind_of_server_reply_ends_on_a_screen_that_says_what_happened() {
    start_server();
    let expected = [
        ("/page", "page: Fixture page"),
        ("/slow", "page: Fixture page"),
        ("/chunked", "page: Fixture page"),
        ("/moved", "page: Fixture page"),
        ("/mislabelled", "page: Fixture page"),
        ("/text", "page: localhost"),
        ("/pdf", "unsupported: a PDF"),
        ("/gone", "failed: NotFound"),
        // A body cut short of its Content-Length, or a connection dropped
        // mid-header, is a host that did not answer usefully.
        ("/short", "failed: Unreachable"),
        ("/drop", "failed: Unreachable"),
        // A body longer than its Content-Length is read to the length.
        ("/long", "page: Fixture page"),
        // 16 MB of gzip in 16 KB stops at the page ceiling, not in memory.
        ("/bomb", "failed: TooLarge"),
        // The runtime gives up after five redirects and does not say why,
        // so a loop reads as a host that did not answer.
        ("/loop", "failed: Unreachable"),
        // Only the fixture host is routable here, so a redirect elsewhere
        // is refused. On a device it is followed, and the page's links are
        // resolved against the address asked for (see README, Limits).
        ("/away", "failed: Denied"),
    ];
    for (path, want) in expected {
        assert_eq!(shows(&visit(path)), want, "{path}");
    }
}

#[test]
fn a_page_opens_where_its_main_content_starts() {
    start_server();
    let mut runner = visit("/wikipedia");
    assert_eq!(shows(&runner), "page: E-reader - Wikipedia");
    let loaded = runner.app().loaded.as_ref().expect("loaded");
    let start = loaded
        .paginator
        .page_of("firstHeading")
        .expect("main start");
    assert!(
        start > 0,
        "the article's menus fill at least the first page"
    );
    assert_eq!(loaded.page, start);
    let words = |runner: &AppRunner<Browser>| {
        let loaded = runner.app().loaded.as_ref().expect("loaded");
        let mut text = String::new();
        for piece in &loaded.paginator.pages()[loaded.page] {
            text.push_str(&format!("{piece:?}"));
        }
        text
    };
    assert!(words(&runner).contains("E-reader"));
    // The menus are still there, a page turn back.
    for _ in 0..start {
        runner.page_turn(false);
    }
    assert_eq!(runner.app().loaded.as_ref().expect("loaded").page, 0);
    assert!(words(&runner).contains("Main menu") || words(&runner).contains("Jump to content"));
}

#[test]
fn the_reader_view_keeps_the_article_and_goes_back_to_where_the_page_was() {
    start_server();
    let mut runner = visit("/wikipedia");
    let text = |runner: &AppRunner<Browser>| {
        let loaded = runner.app().loaded.as_ref().expect("loaded");
        format!("{:?}", loaded.paginator.pages()[loaded.page])
    };
    let whole = runner.app().loaded.as_ref().expect("loaded").page;
    let blocks = runner
        .app()
        .loaded
        .as_ref()
        .expect("loaded")
        .document
        .blocks
        .len();

    runner.action(action_id("reader"));
    let loaded = runner.app().loaded.as_ref().expect("loaded");
    assert_eq!(loaded.page, 0);
    assert!(loaded.document.blocks.len() < blocks);
    let first = text(&runner);
    assert!(first.contains("E-reader"), "{first}");
    for menu in ["Main menu", "Afrikaans", "Contents"] {
        assert!(!first.contains(menu), "{menu} in the reader view: {first}");
    }

    runner.action(action_id("reader"));
    let loaded = runner.app().loaded.as_ref().expect("loaded");
    assert!(loaded.full.is_none());
    assert_eq!(loaded.document.blocks.len(), blocks);
    assert_eq!(loaded.page, whole);
}

#[test]
fn reader_sections_and_links_only_name_content_in_that_view() {
    start_server();
    let mut runner = visit("/wikipedia");
    runner.action(action_id("reader"));
    let headings = headings(runner.app().loaded.as_ref().unwrap());
    assert!(headings.iter().any(|(_, _, title)| title == "E-reader"));
    assert!(!headings.iter().any(|(_, _, title)| title == "Main menu"));
    let visible = runner.app().page_links();
    let total = runner.app().loaded.as_ref().unwrap().document.links.len();
    assert!(visible.len() < total);
    runner.action(action_id("navigate"));
    assert_eq!(runner.app().view, View::Navigate);
    runner.action(action_id("links"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    let pages = links_pages(loaded, &visible, &runner.context().metrics());
    assert_eq!(pages.iter().flatten().copied().collect::<Vec<_>>(), visible);
    runner.action(action_id("return"));
    runner.action(action_id("navigate"));
    runner.action(action_id("sections"));
    let target = headings
        .iter()
        .find(|(_, _, title)| title == "E-reader")
        .unwrap()
        .0;
    runner.action(action_id(&section_action(target)));
    assert_eq!(runner.app().view, View::Page);
    assert!(runner.app().loaded.as_ref().unwrap().full.is_some());
}
