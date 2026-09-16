//! Deterministic export bundles for one library item.
//!
//! A bundle is a directory holding the original file, its normalized
//! metadata, optional reading state and annotations, and a checksum of
//! every part, so a restore can verify the whole before it applies any
//! of it. Writing a bundle never mutates the library it came from, and
//! a name already taken earns the next numbered suffix rather than an
//! overwrite. Contract: docs/quality/contracts/content-identity.md.

use crate::adoption::plain_name;
use crate::identity::Provenance;
use kobo_json::{ObjectBuilder, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The most one part of a bundle may be: the import ceiling, since a
/// bundle never holds more than what the library was allowed to adopt.
const MAX_PART_BYTES: usize = 32 * 1024 * 1024;

/// Why a bundle could not be written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportError {
    /// The original's name is not a plain file name.
    BadName,
    /// A part exceeds the ceiling the library adopts under.
    Oversized,
    /// The destination filesystem refused the write.
    Io,
}

/// One content item packaged for export. Reading state and annotations
/// arrive already serialized by the app that owns their shape; the
/// bundle treats them as bytes and answers for their integrity, not
/// their grammar. Secret values have no place here: provenance names
/// sources, never credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bundle {
    name: String,
    original: Vec<u8>,
    title: String,
    author: String,
    provenance: Vec<Provenance>,
    reading_state: Option<Vec<u8>>,
    annotations: Option<Vec<u8>>,
}

impl Bundle {
    /// The item named `name` with `original` bytes, titled and authored
    /// as its metadata says, carrying the provenance it accumulated.
    #[must_use]
    pub fn new(
        name: &str,
        original: Vec<u8>,
        title: &str,
        author: &str,
        provenance: Vec<Provenance>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            original,
            title: title.to_owned(),
            author: author.to_owned(),
            provenance,
            reading_state: None,
            annotations: None,
        }
    }

    /// Attaches the reading state, serialized by its owner.
    #[must_use]
    pub fn with_reading_state(mut self, state: Vec<u8>) -> Self {
        self.reading_state = Some(state);
        self
    }

    /// Attaches the annotations, serialized by their owner.
    #[must_use]
    pub fn with_annotations(mut self, annotations: Vec<u8>) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// The bundle's base directory name: the original's stem plus a
    /// marker suffix, before any conflict numbering.
    #[must_use]
    pub fn directory_name(&self) -> String {
        let stem = self
            .name
            .rfind('.')
            .map_or(self.name.as_str(), |dot| &self.name[..dot]);
        format!("{stem}.cobalt-export")
    }

    /// The parts every bundle carries, in their fixed order: original
    /// first, metadata second, optional parts after. Restore reads this
    /// same order from [`Self::write_in`]'s checksums.
    fn parts(&self) -> Vec<(String, Vec<u8>)> {
        let mut parts = vec![
            (format!("original/{}", self.name), self.original.clone()),
            (
                "metadata.json".to_owned(),
                self.metadata_json().into_bytes(),
            ),
        ];
        if let Some(state) = &self.reading_state {
            parts.push(("reading-state.json".to_owned(), state.clone()));
        }
        if let Some(annotations) = &self.annotations {
            parts.push(("annotations.json".to_owned(), annotations.clone()));
        }
        parts
    }

    fn metadata_json(&self) -> String {
        let mut metadata = ObjectBuilder::new()
            .set("name", self.name.as_str())
            .set("title", self.title.as_str())
            .set("author", self.author.as_str())
            .set("sha256", kobo_net::sha256::hex_digest(&self.original));
        let provenance = self
            .provenance
            .iter()
            .map(|record| {
                ObjectBuilder::new()
                    .set("source", record.source())
                    .set("name", record.name())
                    .build()
            })
            .collect::<Vec<_>>();
        metadata = metadata.set("provenance", Value::Array(provenance));
        metadata.build().to_json()
    }

    /// Writes the bundle under `dir`, returning the bundle directory.
    /// A taken name earns the next numbered suffix; nothing existing is
    /// overwritten.
    ///
    /// # Errors
    ///
    /// Refuses a non-plain original name, an oversized part, or a
    /// destination the filesystem will not write.
    pub fn write_in(&self, dir: &Path) -> Result<PathBuf, ExportError> {
        if !plain_name(&self.name) {
            return Err(ExportError::BadName);
        }
        let parts = self.parts();
        if self.original.is_empty() || parts.iter().any(|(_, bytes)| bytes.len() > MAX_PART_BYTES) {
            return Err(ExportError::Oversized);
        }
        let base = dir.join(self.directory_name());
        let mut destination = base.clone();
        for index in 2.. {
            if !destination.exists() {
                break;
            }
            destination = PathBuf::from(format!("{}-{index}", base.display()));
            if index > 1000 {
                return Err(ExportError::Io);
            }
        }
        let original_dir = destination.join("original");
        fs::create_dir_all(&original_dir).map_err(|_| ExportError::Io)?;
        let checksums = parts
            .iter()
            .map(|(part, bytes)| format!("{}  {part}", kobo_net::sha256::hex_digest(bytes)))
            .collect::<Vec<_>>()
            .join("\n");
        let write = |relative: &str, bytes: &[u8]| -> Result<(), ExportError> {
            let path = destination.join(relative);
            let mut file = fs::File::create(path).map_err(|_| ExportError::Io)?;
            file.write_all(bytes).map_err(|_| ExportError::Io)?;
            file.sync_all().map_err(|_| ExportError::Io)
        };
        let result = (|| {
            for (part, bytes) in &parts {
                write(part, bytes)?;
            }
            write("checksums.txt", format!("{checksums}\n").as_bytes())
        })();
        if result.is_err() {
            // A failed export removes its partial bundle; the library it
            // came from was never touched.
            let _ignored = fs::remove_dir_all(&destination);
        }
        result.map(|()| destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cobalt-export-test-{tag}-{stamp}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn bundle() -> Bundle {
        Bundle::new(
            "hobbit.epub",
            b"the road goes ever on".to_vec(),
            "The Hobbit",
            "J.R.R. Tolkien",
            vec![Provenance::new("usb", "hobbit.epub")],
        )
        .with_reading_state(br#"{"page": 61}"#.to_vec())
        .with_annotations(br#"[{"page": 12, "note": "second breakfast"}]"#.to_vec())
    }

    #[test]
    fn bundle_writes_original_metadata_state_annotations_and_checksums() {
        let dir = temp_dir("layout");
        let out = bundle().write_in(&dir).expect("write bundle");
        assert_eq!(out.file_name().expect("name"), "hobbit.cobalt-export");
        for part in [
            "original/hobbit.epub",
            "metadata.json",
            "reading-state.json",
            "annotations.json",
            "checksums.txt",
        ] {
            assert!(out.join(part).is_file(), "missing {part}");
        }
        let checksums = fs::read_to_string(out.join("checksums.txt")).expect("checksums");
        assert!(checksums.contains("original/hobbit.epub"));
        assert!(checksums.contains("metadata.json"));
        assert!(checksums.contains("reading-state.json"));
        assert!(checksums.contains("annotations.json"));
        let metadata = fs::read_to_string(out.join("metadata.json")).expect("metadata");
        let parsed = kobo_json::parse(&metadata).expect("valid json");
        assert_eq!(
            parsed.get("title").and_then(Value::as_str),
            Some("The Hobbit")
        );
        assert_eq!(
            parsed.get("name").and_then(Value::as_str),
            Some("hobbit.epub")
        );
        assert_eq!(
            parsed.get("sha256").and_then(Value::as_str),
            Some(kobo_net::sha256::hex_digest(b"the road goes ever on").as_str())
        );
        let provenance = parsed
            .get("provenance")
            .and_then(Value::as_array)
            .expect("provenance");
        assert_eq!(provenance.len(), 1);
        assert_eq!(
            provenance[0].get("source").and_then(Value::as_str),
            Some("usb")
        );
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn taken_bundle_name_earns_numbered_suffix() {
        let dir = temp_dir("suffix");
        let first = bundle().write_in(&dir).expect("first");
        let second = bundle().write_in(&dir).expect("second");
        let third = bundle().write_in(&dir).expect("third");
        assert_eq!(first.file_name().expect("name"), "hobbit.cobalt-export");
        assert_eq!(second.file_name().expect("name"), "hobbit.cobalt-export-2");
        assert_eq!(third.file_name().expect("name"), "hobbit.cobalt-export-3");
        assert_eq!(
            fs::read(first.join("original/hobbit.epub")).expect("first original"),
            b"the road goes ever on"
        );
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn export_never_touches_the_source() {
        let dir = temp_dir("untouched");
        let source = dir.join("hobbit.epub");
        fs::write(&source, b"the road goes ever on").expect("source");
        bundle().write_in(&dir).expect("write bundle");
        assert_eq!(
            fs::read(&source).expect("source intact"),
            b"the road goes ever on"
        );
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn non_plain_names_and_empty_originals_are_refused() {
        let dir = temp_dir("refused");
        for name in ["", "../escape.epub", "a/b.epub", ".hidden"] {
            let bad = Bundle::new(name, b"bytes".to_vec(), "T", "A", vec![]);
            assert_eq!(bad.write_in(&dir), Err(ExportError::BadName), "{name}");
        }
        let empty = Bundle::new("hobbit.epub", vec![], "T", "A", vec![]);
        assert_eq!(empty.write_in(&dir), Err(ExportError::Oversized));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn metadata_round_trips_without_optional_parts() {
        let dir = temp_dir("minimal");
        let bare = Bundle::new("notes.md", b"hi".to_vec(), "Notes", "Me", vec![]);
        let out = bare.write_in(&dir).expect("write");
        assert!(!out.join("reading-state.json").exists());
        assert!(!out.join("annotations.json").exists());
        let metadata = fs::read_to_string(out.join("metadata.json")).expect("metadata");
        let parsed = kobo_json::parse(&metadata).expect("valid json");
        assert_eq!(parsed.get("author").and_then(Value::as_str), Some("Me"));
        assert_eq!(
            parsed
                .get("provenance")
                .and_then(Value::as_array)
                .expect("provenance")
                .len(),
            0
        );
        let _ignored = fs::remove_dir_all(dir);
    }
}
