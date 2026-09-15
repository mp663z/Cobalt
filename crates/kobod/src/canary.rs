//! Post-install launch canary: the exact package just staged is launched
//! against this runtime before the install commits, so a package that cannot
//! complete a handshake and draw its first screen never becomes the version
//! the owner sees.
//!
//! The check answers the failure behind the catalog's oldest compatibility
//! bug: a package was accepted because its declared minimum version matched,
//! then received frames it did not understand. Declared versions are still
//! checked, but the canary proves the package/runtime pair itself: connect,
//! Hello, Welcome at a session version both sides speak, first screen inside
//! a deadline, then a supervised stop. Whatever the package says on the way,
//! including its dying words on standard error, becomes the diagnostics the
//! quarantine record keeps.

use std::fs;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use kobo_protocol::{Frame, Message};

/// The panel a canary Welcome describes: the one the application would
/// actually be launched onto, so it measures and lays out for the real thing.
#[derive(Clone, Copy, Debug)]
pub struct Panel {
    pub width: u16,
    pub height: u16,
    pub pixels_per_inch: u16,
    pub text_scale: kobo_ui::TextScale,
}

/// Runs the launch canary against one staged package binary.
///
/// Returns a one-line success summary for the install journal, or diagnostics
/// specific enough that the quarantine record tells the owner what the
/// package did instead of launching.
pub fn run(
    binary: &Path,
    expected_name: &str,
    panel: Panel,
    deadline: Duration,
) -> Result<String, String> {
    let scratch = Scratch::new();
    let socket_path = scratch.path();
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("canary socket {}: {error}", socket_path.display()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("canary socket setup: {error}"))?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("KOBO_SOCKET", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // A dying application explains itself on standard error and nowhere
        // else; the canary keeps those words for the diagnostics.
        .stderr(Stdio::piped());
    kobo_abi::process_group::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("start {}: {error}", binary.display()))?;
    let started = Instant::now();
    let outcome = (|| {
        let stream = accept_before(&listener, started, deadline)?;
        converse(stream, expected_name, panel, started, deadline)
    })();
    // The canary's child never outlives the canary, whatever it said.
    let _ignored = child.kill();
    let _ignored = child.wait();
    let mut outcome = outcome;
    if outcome.is_err() {
        if let Some(mut stderr) = child.stderr.take() {
            use std::io::Read;
            let mut said = String::new();
            let _ignored = stderr.read_to_string(&mut said);
            let said = said.trim();
            if !said.is_empty() {
                let detail = said.lines().last().unwrap_or(said);
                outcome = outcome.map_err(|error| {
                    format!("{error}; the application said: {}", &detail[..detail.len().min(160)])
                });
            }
        }
    }
    outcome
}

/// The conversation itself, over an already-connected stream: identity,
/// session version, Welcome, first screen. Split from process management so
/// the protocol contract is testable without spawning anything.
fn converse(
    mut stream: UnixStream,
    expected_name: &str,
    panel: Panel,
    started: Instant,
    deadline: Duration,
) -> Result<String, String> {
    let left = remaining(started, deadline)?;
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(left)))
        .map_err(|error| format!("canary stream setup: {error}"))?;
    let hello = kobo_protocol::read_from(&mut stream).map_err(|error| match error {
        kobo_protocol::StreamError::Protocol(
            kobo_protocol::ProtocolError::UnsupportedVersion(version),
        ) => format!(
            "the application speaks protocol {version}, which this runtime does not serve"
        ),
        other => format!("no handshake from the application: {other}"),
    })?;
    let Message::Hello { name } = hello.message else {
        return Err("the first application message was not Hello".to_owned());
    };
    if name != expected_name {
        return Err(format!(
            "launched {expected_name:?} but the application introduced itself as {name:?}"
        ));
    }
    if hello.version == 0 || hello.version > kobo_protocol::VERSION {
        return Err(format!(
            "the application speaks protocol {} but this runtime speaks at most {}",
            hello.version,
            kobo_protocol::VERSION
        ));
    }
    kobo_protocol::write_to(
        &mut stream,
        &Frame {
            version: hello.version,
            request_id: hello.request_id,
            message: Message::Welcome {
                width: panel.width,
                height: panel.height,
                pixels_per_inch: panel.pixels_per_inch,
                text_scale: panel.text_scale,
            },
        },
    )
    .map_err(|error| format!("welcome: {error}"))?;
    stream
        .set_read_timeout(Some(remaining(started, deadline)?))
        .map_err(|error| format!("canary stream setup: {error}"))?;
    let first = kobo_protocol::read_from(&mut stream)
        .map_err(|error| format!("no first screen within {}s: {error}", deadline.as_secs()))?;
    match first.message {
        Message::SetScreen(_) => Ok(format!(
            "handshake at protocol {}, first screen drawn for {}x{}",
            hello.version, panel.width, panel.height
        )),
        other => Err(format!(
            "the application's first message after Welcome was {other:?}, not a screen"
        )),
    }
}

fn accept_before(
    listener: &UnixListener,
    started: Instant,
    deadline: Duration,
) -> Result<UnixStream, String> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let _left = remaining(started, deadline)
                    .map_err(|_| "the application never connected".to_owned())?;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(format!("accepting the application: {error}")),
        }
    }
}

fn remaining(started: Instant, deadline: Duration) -> Result<Duration, String> {
    let left = deadline.saturating_sub(started.elapsed());
    if left.is_zero() {
        Err(format!("the canary deadline of {}s passed", deadline.as_secs()))
    } else {
        Ok(left)
    }
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cobalt-canary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ignored = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("canary scratch directory");
        Self { root }
    }

    fn path(&self) -> PathBuf {
        self.root.join("canary.socket")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Panel = Panel {
        width: 1264,
        height: 1680,
        pixels_per_inch: 300,
        text_scale: kobo_ui::TextScale::Default,
    };

    /// Plays the application side of a canary conversation.
    fn play_app(
        stream: UnixStream,
        name: &str,
        version: u8,
        then: impl FnOnce(&mut UnixStream) + Send + 'static,
    ) -> std::thread::JoinHandle<()> {
        let name = name.to_owned();
        std::thread::spawn(move || {
            let mut stream = stream;
            kobo_protocol::write_to(
                &mut stream,
                &Frame {
                    version,
                    request_id: 1,
                    message: Message::Hello { name },
                },
            )
            .unwrap();
            let welcome = kobo_protocol::read_from(&mut stream).unwrap();
            assert!(matches!(welcome.message, Message::Welcome { .. }));
            then(&mut stream);
        })
    }

    #[test]
    fn a_first_screen_inside_the_deadline_passes() {
        let (app, runtime) = UnixStream::pair().unwrap();
        let app = play_app(app, "todo", kobo_protocol::VERSION, |stream| {
            kobo_protocol::write_to(
                stream,
                &Frame {
                    version: kobo_protocol::VERSION,
                    request_id: 2,
                    message: Message::SetScreen(kobo_ui::Screen {
                id: 1,
                top_bar: None,
                nodes: vec![],
                nav_bar: None,
                bottom_action: None,
                page_turns: None,
                owns_back: false,
                reading: false,
                legacy_typography: false,
                reading_font: None,
                text_scale: None,
                hold: None,
                overlay: None,
            }),
                },
            )
            .unwrap();
        });
        let summary = converse(
            runtime,
            "todo",
            PANEL,
            Instant::now(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(summary.contains("first screen"), "{summary}");
        app.join().unwrap();
    }

    #[test]
    fn an_identity_mismatch_is_refused() {
        let (app, runtime) = UnixStream::pair().unwrap();
        let app = std::thread::spawn(move || {
            let mut stream = app;
            kobo_protocol::write_to(
                &mut stream,
                &Frame {
                    version: kobo_protocol::LEGACY_VERSION,
                    request_id: 1,
                    message: Message::Hello {
                        name: "impostor".into(),
                    },
                },
            )
            .unwrap();
        });
        let error = converse(
            runtime,
            "todo",
            PANEL,
            Instant::now(),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(error.contains("impostor"), "{error}");
        app.join().unwrap();
    }

    #[test]
    fn a_protocol_newer_than_the_runtime_is_refused() {
        // A genuinely newer SDK writes its own version byte; the runtime must
        // refuse the frame rather than answer it in a dialect the
        // application cannot read.
        let (mut app, runtime) = UnixStream::pair().unwrap();
        let app_thread = std::thread::spawn(move || {
            use std::io::Write;
            let mut frame = kobo_protocol::encode(&Frame {
                version: kobo_protocol::VERSION,
                request_id: 1,
                message: Message::Hello {
                    name: "todo".into(),
                },
            })
            .unwrap();
            frame[4] = kobo_protocol::VERSION + 1;
            app.write_all(&frame).unwrap();
        });
        let error = converse(
            runtime,
            "todo",
            PANEL,
            Instant::now(),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(error.contains("protocol 16"), "{error}");
        app_thread.join().unwrap();
    }

    #[test]
    fn no_first_screen_inside_the_deadline_fails() {
        let (app, runtime) = UnixStream::pair().unwrap();
        let app = play_app(app, "todo", kobo_protocol::VERSION, |_stream| {
            std::thread::sleep(Duration::from_secs(5));
        });
        let error = converse(
            runtime,
            "todo",
            PANEL,
            Instant::now(),
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(error.contains("first screen"), "{error}");
        drop(app);
    }

    #[test]
    fn a_first_message_that_is_not_a_screen_fails() {
        let (app, runtime) = UnixStream::pair().unwrap();
        let app = play_app(app, "todo", kobo_protocol::VERSION, |stream| {
            kobo_protocol::write_to(
                stream,
                &Frame {
                    version: kobo_protocol::VERSION,
                    request_id: 2,
                    message: Message::Exit,
                },
            )
            .unwrap();
        });
        let error = converse(
            runtime,
            "todo",
            PANEL,
            Instant::now(),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(error.contains("not a screen"), "{error}");
        app.join().unwrap();
    }
}
