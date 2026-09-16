//! Session-level protocol compatibility matrix.
//!
//! Contract: docs/quality/contracts/protocol-compatibility.md. The version an
//! application greets with is fixed for the life of its session: every frame
//! the runtime sends afterwards - Welcome, lifecycle, and anything else the
//! writer carries - rides that version, and an unsupported version is refused
//! before a single session byte goes back.
//!
//! The transcripts this writes under `scripts/fixtures/protocol/` are the
//! immutable wire fixtures the contract lane replays. Regenerate them only
//! from a known-good runtime with `KOBO_BLESS=1 cargo test -p kobo-sim
//! protocol_matrix`; a byte difference otherwise means the wire format or the
//! session-version rule drifted, and that is a contract change, not a test
//! update.

use super::*;
use crate::tests::private_temp_dir;

use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

const SUPPORTED: [u8; 6] = [
    kobo_protocol::LEGACY_VERSION,
    kobo_protocol::FOLIO_VERSION,
    kobo_protocol::SELECTED_GRID_VERSION,
    kobo_protocol::SERVER_ACCOUNT_VERSION,
    kobo_protocol::STORE_PROVENANCE_VERSION,
    kobo_protocol::VERSION,
];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fixtures/protocol")
}

fn greet(version: u8) -> (AppServer, AppSession, UnixStream, PathBuf) {
    let root = private_temp_dir();
    let socket_path = root.join("app.sock");
    let server = AppServer::bind("127.0.0.1:0", &socket_path).expect("bind app server");
    let client_path = socket_path.clone();
    let client = thread::spawn(move || -> io::Result<UnixStream> {
        let mut stream = UnixStream::connect(client_path)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write_protocol_frame(
            &mut stream,
            &Frame {
                version,
                request_id: 1,
                message: Message::Hello {
                    name: "test-app".into(),
                },
            },
        )?;
        Ok(stream)
    });
    let session = server.accept_app().expect("session starts");
    let client = client
        .join()
        .expect("client thread")
        .expect("connect and greet");
    (server, session, client, root)
}

/// Reads one whole wire frame, bytes exactly as sent: 14-byte header, then
/// the declared payload.
fn read_raw_frame(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut header = [0_u8; kobo_protocol::HEADER_LEN];
    stream.read_exact(&mut header)?;
    let payload_len = u32::from_be_bytes([header[6], header[7], header[8], header[9]]) as usize;
    let mut frame = header.to_vec();
    frame.resize(kobo_protocol::HEADER_LEN + payload_len, 0);
    stream.read_exact(&mut frame[kobo_protocol::HEADER_LEN..])?;
    Ok(frame)
}

#[test]
fn every_supported_version_keeps_its_session_version() {
    fs::create_dir_all(fixture_dir()).expect("fixture directory");
    for version in SUPPORTED {
        let (server, session, mut client, root) = greet(version);
        let mut transcript = Vec::new();

        let welcome = read_raw_frame(&mut client).expect("welcome frame");
        assert_eq!(welcome[4], version, "Welcome rides the greeted version");
        transcript.extend_from_slice(&welcome);

        for lifecycle in [Lifecycle::Foreground, Lifecycle::Background] {
            session.send_lifecycle(lifecycle).expect("lifecycle sends");
            let frame = read_raw_frame(&mut client).expect("lifecycle frame");
            assert_eq!(
                frame[4], version,
                "lifecycle frames keep the greeted version"
            );
            transcript.extend_from_slice(&frame);
        }

        let fixture = fixture_dir().join(format!("session-v{version}.bin"));
        if std::env::var_os("KOBO_BLESS").is_some() {
            fs::write(&fixture, &transcript).expect("write blessed transcript");
        } else {
            let golden = fs::read(&fixture).unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}; regenerate with KOBO_BLESS=1 from a known-good runtime",
                    fixture.display()
                )
            });
            assert_eq!(
                transcript, golden,
                "session-v{version} wire transcript drifted from the committed fixture"
            );
        }

        drop(session);
        drop(server);
        drop(client);
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn unsupported_versions_are_refused_before_welcome() {
    for bad in [
        kobo_protocol::LEGACY_VERSION - 1,
        kobo_protocol::VERSION + 1,
    ] {
        let root = private_temp_dir();
        let socket_path = root.join("app.sock");
        let server = AppServer::bind("127.0.0.1:0", &socket_path).expect("bind app server");
        let client_path = socket_path.clone();
        let client = thread::spawn(move || -> io::Result<bool> {
            let mut stream = UnixStream::connect(client_path)?;
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            // Hand-built header: valid magic and a Hello kind byte, but a
            // version the runtime does not speak. The payload is never read.
            let mut header = b"KOBO".to_vec();
            header.push(bad);
            header.push(1);
            header.extend_from_slice(&6_u32.to_be_bytes());
            header.extend_from_slice(&1_u32.to_be_bytes());
            stream.write_all(&header)?;
            stream.flush()?;
            let mut buf = [0_u8; kobo_protocol::HEADER_LEN];
            Ok(stream.read_exact(&mut buf).is_err())
        });
        assert!(
            server.accept_app().is_err(),
            "version {bad} must not start a session"
        );
        let refused = client.join().expect("client thread").expect("client io");
        assert!(refused, "version {bad} received session bytes");
        drop(server);
        let _ = fs::remove_dir_all(root);
    }
}
