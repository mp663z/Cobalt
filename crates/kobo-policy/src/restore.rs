//! Restore inspection for an export bundle.
//!
//! Before anything is applied, the bundle is verified whole: every part
//! must be present and match its checksum, and the metadata must parse
//! and carry a plain name. Then the bundle's original is compared with
//! the library: absent is compatible, identical is a skip, different is
//! a conflict a person must settle. This is the preview logic as pure,
//! tested policy; the screen that presents it is a separate concern.
//! Contract: docs/quality/contracts/content-identity.md.

use crate::adoption::plain_name;
use std::fs;
use std::path::Path;

/// How a verified bundle stands against a library.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// The library holds nothing under this name: a restore applies
    /// cleanly.
    Compatible,
    /// The library already holds these exact bytes under this name: a
    /// restore would change nothing.
    Skipped,
    /// The library holds different bytes under this name: a person must
    /// choose before anything moves.
    Conflict,
}

/// Why a bundle could not be read as valid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invalid {
    /// A listed part is absent or unreadable.
    MissingPart,
    /// A part's bytes do not match its recorded checksum.
    CorruptPart,
    /// The metadata is unparseable, incomplete, or names something that
    /// is not a plain file name.
    BadMetadata,
}

/// Reads the metadata of a bundle that has already passed verification.
fn metadata(bundle: &Path) -> Result<(String, String), Invalid> {
    let text =
        fs::read_to_string(bundle.join("metadata.json")).map_err(|_| Invalid::MissingPart)?;
    let parsed = kobo_json::parse(&text).map_err(|_| Invalid::BadMetadata)?;
    let name = parsed
        .get("name")
        .and_then(kobo_json::Value::as_str)
        .ok_or(Invalid::BadMetadata)?;
    let title = parsed
        .get("title")
        .and_then(kobo_json::Value::as_str)
        .unwrap_or("");
    if !plain_name(name) {
        return Err(Invalid::BadMetadata);
    }
    Ok((name.to_owned(), title.to_owned()))
}

/// Verifies every part listed in `checksums.txt` against its bytes on
/// disk. A line is `<sha256>  <relative path>`; paths must stay inside
/// the bundle.
fn verify(bundle: &Path) -> Result<(), Invalid> {
    let checksums =
        fs::read_to_string(bundle.join("checksums.txt")).map_err(|_| Invalid::MissingPart)?;
    for line in checksums.lines() {
        let (digest, relative) = line.split_once("  ").ok_or(Invalid::BadMetadata)?;
        if digest.len() != 64
            || relative.is_empty()
            || relative.starts_with('/')
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(Invalid::BadMetadata);
        }
        let bytes = fs::read(bundle.join(relative)).map_err(|_| Invalid::MissingPart)?;
        if kobo_net::sha256::hex_digest(&bytes) != digest {
            return Err(Invalid::CorruptPart);
        }
    }
    Ok(())
}

/// Inspects `bundle` against `library_root`, returning how a restore
/// would stand. Nothing is written; verification precedes comparison,
/// so a corrupt bundle never masquerades as a conflict.
///
/// # Errors
///
/// Returns the first reason the bundle fails to read as valid.
pub fn inspect(bundle: &Path, library_root: &Path) -> Result<Verdict, Invalid> {
    verify(bundle)?;
    let (name, _title) = metadata(bundle)?;
    let original =
        fs::read(bundle.join("original").join(&name)).map_err(|_| Invalid::MissingPart)?;
    let existing = library_root.join(&name);
    if !existing.exists() {
        return Ok(Verdict::Compatible);
    }
    let present = fs::read(&existing).map_err(|_| Invalid::MissingPart)?;
    Ok(if present == original {
        Verdict::Skipped
    } else {
        Verdict::Conflict
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::Bundle;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cobalt-restore-test-{tag}-{stamp}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn bundle_in(dir: &Path) -> PathBuf {
        Bundle::new(
            "hobbit.epub",
            b"first edition".to_vec(),
            "The Hobbit",
            "Tolkien",
            vec![],
        )
        .with_reading_state(br#"{"page": 61}"#.to_vec())
        .write_in(dir)
        .expect("write bundle")
    }

    #[test]
    fn absent_original_is_compatible() {
        let dir = temp_dir("compatible");
        let bundle = bundle_in(&dir);
        let library = dir.join("library");
        fs::create_dir_all(&library).expect("library");
        assert_eq!(inspect(&bundle, &library), Ok(Verdict::Compatible));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn identical_original_is_skipped() {
        let dir = temp_dir("skipped");
        let bundle = bundle_in(&dir);
        let library = dir.join("library");
        fs::create_dir_all(&library).expect("library");
        fs::write(library.join("hobbit.epub"), b"first edition").expect("existing");
        assert_eq!(inspect(&bundle, &library), Ok(Verdict::Skipped));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn different_original_is_conflict() {
        let dir = temp_dir("conflict");
        let bundle = bundle_in(&dir);
        let library = dir.join("library");
        fs::create_dir_all(&library).expect("library");
        fs::write(library.join("hobbit.epub"), b"second edition").expect("existing");
        assert_eq!(inspect(&bundle, &library), Ok(Verdict::Conflict));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupted_part_is_invalid_not_conflict() {
        let dir = temp_dir("corrupt");
        let bundle = bundle_in(&dir);
        let library = dir.join("library");
        fs::create_dir_all(&library).expect("library");
        fs::write(bundle.join("reading-state.json"), b"tampered").expect("tamper");
        assert_eq!(inspect(&bundle, &library), Err(Invalid::CorruptPart));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_part_is_invalid() {
        let dir = temp_dir("missing");
        let bundle = bundle_in(&dir);
        let library = dir.join("library");
        fs::create_dir_all(&library).expect("library");
        fs::remove_file(bundle.join("original/hobbit.epub")).expect("remove part");
        assert_eq!(inspect(&bundle, &library), Err(Invalid::MissingPart));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn metadata_with_traversal_name_is_invalid() {
        let dir = temp_dir("traversal");
        let bundle = bundle_in(&dir);
        let library = dir.join("library");
        fs::create_dir_all(&library).expect("library");
        fs::write(
            bundle.join("metadata.json"),
            r#"{"name":"../evil.epub","title":"x","author":"y","sha256":"0","provenance":[]}"#,
        )
        .expect("rewrite metadata");
        // metadata.json changed, so its checksum must be rewritten for the
        // traversal itself to be what fails.
        let rewritten = fs::read(bundle.join("metadata.json")).expect("metadata bytes");
        let digest = kobo_net::sha256::hex_digest(&rewritten);
        let checksums = fs::read_to_string(bundle.join("checksums.txt")).expect("checksums");
        let updated = checksums
            .lines()
            .map(|line| {
                if line.ends_with("metadata.json") {
                    format!("{digest}  metadata.json")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(bundle.join("checksums.txt"), format!("{updated}\n")).expect("checksums");
        assert_eq!(inspect(&bundle, &library), Err(Invalid::BadMetadata));
        let _ignored = fs::remove_dir_all(dir);
    }
}
