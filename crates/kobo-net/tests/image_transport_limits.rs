#![cfg(debug_assertions)]
use kobo_protocol::TaskError;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

#[test]
fn image_get_redirect_ceiling_tls_upgrade_and_stalled_cancel() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    kobo_net::fixture::install(&format!("localhost={}", listener.local_addr().unwrap())).unwrap();
    kobo_net::trust_owner_root(include_bytes!("fixtures/localhost-ca.der").to_vec()).unwrap();
    let config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(
                    include_bytes!("fixtures/localhost-cert.der").to_vec(),
                )],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    include_bytes!("fixtures/localhost-key.der").to_vec(),
                )),
            )
            .unwrap(),
    );
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut paths = Vec::new();
        for i in 0..9 {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut tls =
                StreamOwned::new(ServerConnection::new(Arc::clone(&config)).unwrap(), socket);
            let mut req = Vec::new();
            while !req.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                tls.read_exact(&mut b).unwrap();
                req.push(b[0]);
                assert!(req.len() < 8192);
            }
            assert_eq!(tls.conn.server_name(), Some("localhost"));
            let req = String::from_utf8(req).unwrap();
            assert!(!req.to_ascii_lowercase().contains("authorization:"));
            assert!(!req.to_ascii_lowercase().contains("cookie:"));
            paths.push(req.lines().next().unwrap().to_owned());
            if i == 8 {
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                break;
            }
            let response = if i < 6 {
                format!("HTTP/1.1 302 Found\r\nLocation: /loop/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",i+1)
            } else if i == 6 {
                "HTTP/1.1 302 Found\r\nLocation: http://localhost/final.png\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\npng".into()
            };
            tls.write_all(response.as_bytes()).unwrap();
            tls.flush().unwrap();
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        paths
    });
    assert_eq!(
        kobo_net::fetch("https://localhost/loop/0", 1024),
        Err(TaskError::Unreachable)
    );
    assert_eq!(
        kobo_net::fetch("https://localhost/upgrade.png", 1024),
        Ok(b"png".to_vec())
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let child = Arc::clone(&cancel);
    let client = std::thread::spawn(move || {
        kobo_net::fetch_from_controlled(
            "https://localhost/stall.png",
            0,
            1024,
            None,
            &[("Accept", "image/png")],
            kobo_net::RequestOptions::default(),
            &child,
        )
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let at = Instant::now();
    cancel.store(true, Ordering::Release);
    assert_eq!(client.join().unwrap(), Err(TaskError::TimedOut));
    assert!(at.elapsed() < Duration::from_secs(3));
    release_tx.send(()).unwrap();
    let paths = server.join().unwrap();
    assert_eq!(paths.len(), 9);
    assert_eq!(&paths[0], "GET /loop/0 HTTP/1.1");
    assert_eq!(&paths[5], "GET /loop/5 HTTP/1.1");
    assert_eq!(&paths[7], "GET /final.png HTTP/1.1");
}
