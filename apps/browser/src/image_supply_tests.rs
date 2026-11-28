//! External image supply: bounded fetches, refusals, the census, and a real Page view render.
use super::*;
use crate::image_supply::{sniff, ImageSupply, ACCEPT, MAX_IMAGES, MAX_IMAGE_BYTES, MAX_TOTAL_BYTES};
use crate::page_images::External;
use kobo_sdk::{AppRunner, Command};
use kobo_ui::TextScale;

const METRICS: DisplayMetrics = DisplayMetrics {
    width: 600,
    height: 800,
    pixels_per_inch: 300,
    text_scale: TextScale::Default,
};
const JPEG: &[u8] = include_bytes!("../tests/images/baseline.jpg");
const PNG: &[u8] = include_bytes!("../tests/images/transparent-logo.png");
const GIF: &[u8] = include_bytes!("../tests/images/not-supported.gif");
const WEBP: &[u8] = include_bytes!("../tests/images/not-supported.webp");
const HTML_ERROR: &[u8] = include_bytes!("../tests/images/error-page.jpg");

fn runner() -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(Browser::default(), METRICS);
    runner.start();
    runner
}

fn sources(n: usize) -> Vec<(String, Url)> {
    (0..n)
        .map(|i| {
            let src = format!("/img/{i}.png");
            (src.clone(), Url::parse(&format!("https://cdn.example.test{src}")).unwrap())
        })
        .collect()
}

fn spawned(commands: &[Command]) -> Vec<(TaskId, Task)> {
    commands
        .iter()
        .filter_map(|c| match c {
            Command::Spawn { task, work } => Some((*task, work.clone())),
            _ => None,
        })
        .collect()
}

fn url_of(task: &Task) -> &str {
    match task {
        Task::Fetch { url, .. } => url,
        _ => panic!("not a fetch"),
    }
}

/// Serves every outstanding fetch with `body(src index)` until the supply settles.
fn serve(
    runner: &AppRunner<Browser>,
    supply: &mut ImageSupply,
    mut first: Vec<(TaskId, Task)>,
    body: impl Fn(usize) -> TaskOutcome,
) {
    while let Some((task, work)) = first.pop() {
        let index: usize = url_of(&work).rsplit('/').next().unwrap().trim_end_matches(".png").parse().unwrap();
        // A fresh context has no tasks in flight, so each answer frees the queue like the runtime does.
        let mut context = runner.context();
        assert!(supply.on_task(&mut context, task, &body(index)));
        first.extend(spawned(&context.take_commands()));
    }
}

#[test]
fn sniff_accepts_only_png_and_jpeg_signatures() {
    assert!(sniff(PNG) && sniff(JPEG));
    assert!(!sniff(GIF) && !sniff(WEBP) && !sniff(HTML_ERROR));
    assert!(!sniff(b"<svg xmlns='http://www.w3.org/2000/svg'/>") && !sniff(b"") && !sniff(&[0x89, b'P']));
}

#[test]
fn fetches_are_https_credential_free_accept_limited_and_byte_capped() {
    let runner = runner();
    let mut context = runner.context();
    let mut supply = ImageSupply::default();
    supply.begin(&mut context, &sources(2));
    let fetched = spawned(&context.take_commands());
    assert_eq!(fetched.len(), 2);
    for (_, work) in &fetched {
        let Task::Fetch { url, offset, max_bytes, credential, headers } = work else { panic!() };
        assert!(url.starts_with("https://cdn.example.test/img/"));
        assert_eq!((*offset, credential.is_none()), (0, true));
        assert_eq!(*max_bytes as usize, MAX_IMAGE_BYTES);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0], Header::new("Accept", ACCEPT));
    }
    assert!(supply.outcome().is_none());
}

#[test]
fn good_bodies_are_kept_by_authored_src_and_unsupported_formats_are_refused_per_image() {
    let runner = runner();
    let mut context = runner.context();
    let mut supply = ImageSupply::default();
    supply.begin(&mut context, &sources(4));
    let first = spawned(&context.take_commands());
    serve(&runner, &mut supply, first, |i| match i {
        0 => TaskOutcome::Completed(PNG.to_vec()),
        1 => TaskOutcome::Completed(JPEG.to_vec()),
        2 => TaskOutcome::Completed(GIF.to_vec()),
        _ => TaskOutcome::Completed(WEBP.to_vec()),
    });
    let bodies = supply.outcome().expect("settled");
    assert_eq!(bodies.keys().map(String::as_str).collect::<Vec<_>>(), ["/img/0.png", "/img/1.png"]);
    let c = supply.counts();
    assert_eq!((c.kept, c.wrong_type, c.failed), (2, 2, 0));
    assert_eq!(supply.in_flight(), 0);
}

#[test]
fn error_pages_svg_failures_and_cancellations_never_become_images() {
    let runner = runner();
    let mut context = runner.context();
    let mut supply = ImageSupply::default();
    supply.begin(&mut context, &sources(4));
    let first = spawned(&context.take_commands());
    serve(&runner, &mut supply, first, |i| match i {
        0 => TaskOutcome::Completed(HTML_ERROR.to_vec()),
        1 => TaskOutcome::Completed(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
        2 => TaskOutcome::Failed(kobo_sdk::TaskError::NotFound),
        _ => TaskOutcome::Cancelled,
    });
    assert!(supply.outcome().expect("settled").is_empty());
    let c = supply.counts();
    assert_eq!((c.kept, c.wrong_type, c.failed), (0, 2, 2));
}

#[test]
fn an_oversize_body_is_refused_even_if_the_layer_delivered_it() {
    let runner = runner();
    let mut context = runner.context();
    let mut supply = ImageSupply::default();
    supply.begin(&mut context, &sources(1));
    let first = spawned(&context.take_commands());
    let mut big = PNG.to_vec();
    big.resize(MAX_IMAGE_BYTES + 1, 0);
    serve(&runner, &mut supply, first, |_| TaskOutcome::Completed(big.clone()));
    assert!(supply.outcome().unwrap().is_empty());
    assert_eq!(supply.counts().failed, 1);
}

#[test]
fn the_page_total_budget_drops_the_image_that_would_pass_it() {
    let runner = runner();
    let mut context = runner.context();
    // Four fetches fit the runtime ceiling; a 2.5-body budget keeps two and refuses two.
    let body = PNG.to_vec();
    let mut supply = ImageSupply::with_limits(MAX_IMAGES, MAX_IMAGE_BYTES, body.len() * 5 / 2);
    supply.begin(&mut context, &sources(4));
    let first = spawned(&context.take_commands());
    serve(&runner, &mut supply, first, |_| TaskOutcome::Completed(body.clone()));
    let c = supply.counts();
    assert_eq!((c.kept, c.over_budget), (2, 2));
    assert!(supply.outcome().unwrap().values().map(Vec::len).sum::<usize>() <= body.len() * 5 / 2);
}

#[test]
fn sources_past_the_per_page_count_are_never_requested() {
    let runner = runner();
    let mut context = runner.context();
    let mut supply = ImageSupply::default();
    supply.begin(&mut context, &sources(MAX_IMAGES + 5));
    let started = spawned(&context.take_commands());
    // The runtime ceiling starts four; the other twelve wait their turn; five are refused up front.
    assert_eq!(started.len(), kobo_sdk::MAX_TASKS_IN_FLIGHT);
    assert_eq!(supply.counts().not_requested, 5);
    assert_eq!(supply.in_flight(), kobo_sdk::MAX_TASKS_IN_FLIGHT);
    let small = {
        let mut small = ImageSupply::with_limits(2, MAX_IMAGE_BYTES, MAX_TOTAL_BYTES);
        let mut c2 = runner.context();
        small.begin(&mut c2, &sources(4));
        let n = spawned(&c2.take_commands()).len();
        (n, small.counts().not_requested)
    };
    assert_eq!(small, (2, 2));
}

#[test]
fn plain_http_sources_are_never_requested_and_clear_cancels_outstanding_work() {
    let runner = runner();
    let mut context = runner.context();
    let mut supply = ImageSupply::default();
    let mut list = sources(2);
    list.push(("http://x.test/a.png".into(), Url::parse("http://x.test/a.png").unwrap()));
    supply.begin(&mut context, &list);
    let fetched = spawned(&context.take_commands());
    assert_eq!(fetched.len(), 2);
    assert!(fetched.iter().all(|(_, w)| url_of(w).starts_with("https://")));
    assert_eq!(supply.counts().failed, 1);
    supply.clear(&mut context);
    let cancelled: Vec<TaskId> = context
        .take_commands()
        .iter()
        .filter_map(|c| if let Command::Cancel(t) = c { Some(*t) } else { None })
        .collect();
    assert_eq!(cancelled.len(), 2);
    // A late answer for a cancelled fetch changes nothing.
    assert!(!supply.on_task(&mut context, fetched[0].0, &TaskOutcome::Completed(PNG.to_vec())));
    assert!(supply.outcome().is_none());
}

fn census(html: &str) -> Vec<(String, String)> {
    let base = Url::parse("https://example.test/dir/page.html").unwrap();
    crate::page_view::PageView::external_image_sources(html.as_bytes(), &[], 400, &base, 64)
        .into_iter()
        .map(|(src, url)| (src, url.to_string()))
        .collect()
}

#[test]
fn the_census_resolves_dedupes_filters_and_skips_hidden_images() {
    let html = "<!doctype html><html><head><title>t</title><style>.h{display:none}</style></head><body>\
        <img src=\"a.png\"><img src=\"a.png\"><img src=\"/b.jpg\"><img src=\"//cdn.test/c.png?x=1&amp;y=2\">\
        <img src=\"http://plain.test/d.png\"><img class=\"h\" src=\"hidden.png\"><img src=\"data:image/png;base64,AAAA\">\
        <img src=\"https://user:pw@creds.test/e.png\"><img src=\"javascript:alert(1)\"><img src=\"\"><img alt=\"none\"></body></html>";
    let found = census(html);
    let urls: Vec<&str> = found.iter().map(|(_, u)| u.as_str()).collect();
    assert_eq!(
        urls,
        ["https://example.test/dir/a.png", "https://example.test/b.jpg", "https://cdn.test/c.png?x=1&y=2"],
        "{found:?}"
    );
    assert_eq!(found[2].0, "//cdn.test/c.png?x=1&y=2");
}

#[test]
fn the_census_honours_its_limit() {
    let imgs: String = (0..40).map(|i| format!("<img src=\"/i{i}.png\">")).collect();
    let html = format!("<!doctype html><html><body>{imgs}</body></html>");
    let base = Url::parse("https://example.test/").unwrap();
    let found = crate::page_view::PageView::external_image_sources(html.as_bytes(), &[], 400, &base, 5);
    assert_eq!(found.len(), 5);
}

// ---- real Page view renders -------------------------------------------------------------------------------------

fn paint(html: &str, external: &External) -> (crate::page_mode::PageMode, Vec<u8>) {
    let mode = crate::page_mode::PageMode::build_zoom_with_resources(html.as_bytes(), &[], external, "t", &kobo_ui::CLARA_BW_METRICS, 100)
        .expect("page mode");
    let rgb = mode.view.raster_page_rgb(0);
    (mode, rgb)
}

const PAGE: &str = "<!doctype html><html><head><title>t</title></head><body><p>Before</p>\
    <img src=\"https://cdn.example.test/logo.png\" alt=\"Logo alt text\"><p>After</p></body></html>";

#[test]
fn a_fetched_png_replaces_the_alt_text_and_a_refused_body_keeps_it() {
    let none = External::new();
    let (without, rgb_without) = paint(PAGE, &none);
    let mut good = External::new();
    good.insert("https://cdn.example.test/logo.png".into(), PNG.to_vec());
    let (with, rgb_with) = paint(PAGE, &good);
    assert_ne!(rgb_without, rgb_with, "the image must change the pixels");
    let marker = |m: &crate::page_mode::PageMode| m.view.stats().approximations.get("native-image-marker-geometry").copied();
    assert_eq!(marker(&with), Some(1));
    assert_eq!(marker(&without), None);
    let mut bad = External::new();
    bad.insert("https://cdn.example.test/logo.png".into(), GIF.to_vec());
    let (_, rgb_bad) = paint(PAGE, &bad);
    assert_eq!(rgb_bad, rgb_without, "an undecodable body must leave the page exactly as it was");
}

fn runner_app() -> AppRunner<Browser> {
    runner()
}

/// The whole seam: open a page, switch to Page view, the supply asks for its images, bodies arrive, the page repaints.
#[test]
fn page_view_fetches_external_images_and_repaints_with_them() {
    let mut runner = runner_app();
    let url = Url::parse("https://img.example.test/dir/page.html").unwrap();
    let html = "<!doctype html><title>Pics</title><p>Before the picture.</p>\
        <img src=\"logo.png\" alt=\"Logo alt text\"><img src=\"//other.example.test/photo.jpg\" alt=\"Photo alt\">\
        <img src=\"http://plain.example.test/x.png\" alt=\"Plain alt\"><p>After the picture.</p>";
    let mut context = runner.context();
    runner.app_mut().show_document(&mut context, &url, html.as_bytes());
    let commands = runner.action(action_id("page-view"));
    let fetches: Vec<(TaskId, Task)> = commands
        .iter()
        .filter_map(|c| match c {
            Command::Spawn { task, work } => Some((*task, work.clone())),
            _ => None,
        })
        .collect();
    let urls: Vec<&str> = fetches.iter().map(|(_, w)| url_of(w)).collect();
    // Exactly the two https images are requested, never the plain-http one.
    assert_eq!(urls.len(), 2, "{urls:?}");
    assert!(urls.contains(&"https://img.example.test/dir/logo.png") && urls.contains(&"https://other.example.test/photo.jpg"));
    let before = runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().view.raster_page_rgb(0);
    for (task, work) in &fetches {
        let body = if url_of(work).ends_with(".png") { PNG.to_vec() } else { JPEG.to_vec() };
        // The runner numbers tasks per context, so an earlier pager sleep can share an id with the first image.
        runner.app_mut().pager = None;
        runner.task_outcome(*task, TaskOutcome::Completed(body));
    }
    let loaded = runner.app().loaded.as_ref().unwrap();
    assert_eq!(loaded.linked_images.len(), 2);
    let after = loaded.painted.as_ref().unwrap().view.raster_page_rgb(0);
    assert_ne!(before, after, "the fetched images must repaint the page");
    // Leaving the page cancels nothing that is still needed and drops the bodies with the document.
    assert_eq!(runner.app().images.in_flight(), 0);
}

/// Writes every page of the corpus page `cut-short` through the shipped PageMode route, once with no
/// external bodies and once with real downloaded thumbnails. `KOBO_DUMP_IMG_DIR=<dir holding kindle.jpg and
/// calibre.png>`, `KOBO_DUMP_OUT=<dir>`; ignored by default.
#[test]
#[ignore]
fn dump_corpus_page_with_external_images() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let images = std::env::var("KOBO_DUMP_IMG_DIR").expect("KOBO_DUMP_IMG_DIR");
    let out = std::env::var("KOBO_DUMP_OUT").expect("KOBO_DUMP_OUT");
    let html = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus/cut-short.html")).unwrap();
    let text = String::from_utf8_lossy(&html).to_string();
    let url = kobo_web_document::Url::parse("https://en.wikipedia.org/wiki/E-reader").unwrap();
    let sources = crate::page_mode::PageMode::external_image_sources(&html, &[], &url, &kobo_ui::CLARA_BW_METRICS, 100);
    let mut external = crate::page_images::External::new();
    for (src, url) in &sources {
        let file = if url.to_string().contains("Amazon_Kindle_3.JPG") {
            "kindle.jpg"
        } else if url.to_string().contains("Calibre-2.12-en.png") {
            "calibre.png"
        } else {
            continue;
        };
        external.insert(src.clone(), std::fs::read(format!("{images}/{file}")).unwrap());
    }
    assert_eq!(external.len(), 2);
    eprintln!("sources={:?}", sources.iter().map(|(s, u)| (s.len(), u.to_string())).collect::<Vec<_>>());
    for (label, set) in [("before", crate::page_images::External::new()), ("after", external)] {
        let mode = crate::page_mode::PageMode::build_zoom_with_resources(&html, &[], &set, "t", &kobo_ui::CLARA_BW_METRICS, 100)
            .expect("page mode");
        let (w, h) = mode.view.screen_device_px();
        eprintln!("{label}: pages={} px={w}x{h}", mode.view.pages());
        for page in 0..mode.view.pages() {
            let rgb = mode.view.raster_page_rgb(page);
            std::fs::write(format!("{out}/{label}-{page}.png"), kobo_image::encode_png_rgb(w, h, &rgb).unwrap()).unwrap();
        }
    }
}
