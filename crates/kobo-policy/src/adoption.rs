//! Staged, atomic adoption of content into the shared library.
//!
//! Import validates the name, the type and the size before anything is
//! written, writes to a staging area the shared views never list, and
//! commits with one rename: a failure at any phase removes the staging
//! file and leaves the library exactly as it stood. The same bytes under
//! the same name are adopted once; the same name over different bytes is
//! a conflict, never a silent overwrite.
//! Contract: docs/quality/contracts/content-identity.md.

use crate::identity::{ContentIdentity, Provenance};
use crate::library;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The most an import may be. The same ceiling the shelf download path
/// enforces (`kobo-sdk`'s `MAX_SHELF_DOWNLOAD`, 32 MiB); repeated here
/// because the policy crate sits below the SDK.
const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;

/// Why an import was refused at the boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Rejection {
    /// The name is not a plain file name: empty, absolute, or carrying
    /// path parts.
    MalformedName,
    /// The suffix names no document kind the library holds.
    UnsupportedType,
    /// Larger than the ceiling the shelf download path already enforces.
    Oversized,
    /// No bytes: not a document.
    Empty,
    /// The name is already taken by different bytes.
    Conflict,
}

impl Rejection {
    /// The boundary report, in words an app can show.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Self::MalformedName => "the name is not a plain file name",
            Self::UnsupportedType => "the file type is not one the library holds",
            Self::Oversized => "the file is larger than the import ceiling",
            Self::Empty => "the file is empty",
            Self::Conflict => "the name is already taken by a different file",
        }
    }
}

/// What an adoption did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// New content, committed.
    Adopted(ContentIdentity),
    /// The same bytes under the same name were already here; nothing
    /// was rewritten.
    AlreadyPresent(ContentIdentity),
}

/// The staging directory: inside Cobalt's own folder, which the shared
/// listing never walks, and on the same filesystem as the destination
/// so the commit rename is atomic.
fn staging_dir(root: &Path) -> PathBuf {
    root.join(".adds/adoptions")
}

/// Whether `name` is a plain file name: not empty, not a dot path, no
/// separators, no leading dot. Import and restore share this boundary.
#[must_use]
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.starts_with('.')
}

/// Validates the boundary rules before a byte is staged.
fn check(name: &str, bytes: &[u8]) -> Result<library::Kind, Rejection> {
    if !plain_name(name) {
        return Err(Rejection::MalformedName);
    }
    let kind = library::Kind::from_name(name).ok_or(Rejection::UnsupportedType)?;
    if bytes.is_empty() {
        return Err(Rejection::Empty);
    }
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(Rejection::Oversized);
    }
    Ok(kind)
}

/// Adopts `bytes` as `name` under `root`, first seen from `provenance`.
///
/// The metadata key needs a title and author; at the boundary the best
/// available title is the file name without its suffix, exactly what a
/// tile shows before the document is opened.
///
/// # Errors
///
/// Returns the [`Rejection`] naming why the boundary refused the import.
pub fn adopt_in(
    root: &Path,
    name: &str,
    bytes: &[u8],
    provenance: Provenance,
) -> Result<Outcome, Rejection> {
    check(name, bytes)?;
    let title = name.rfind('.').map_or(name, |dot| &name[..dot]);
    let identity = ContentIdentity::identify(bytes, title, "", provenance);
    let destination = root.join(name);
    if let Ok(existing) = fs::read(&destination) {
        return if existing == bytes {
            Ok(Outcome::AlreadyPresent(identity))
        } else {
            Err(Rejection::Conflict)
        };
    }

    let staging = staging_dir(root);
    fs::create_dir_all(&staging).map_err(|_| Rejection::MalformedName)?;
    let staged = staging.join(name);
    let staged_name = staged.clone();
    let result = (|| -> Result<(), Rejection> {
        let mut file = fs::File::create(&staged).map_err(|_| Rejection::MalformedName)?;
        file.write_all(bytes)
            .map_err(|_| Rejection::MalformedName)?;
        file.sync_all().map_err(|_| Rejection::MalformedName)?;
        drop(file);
        fs::rename(&staged, &destination).map_err(|_| Rejection::MalformedName)?;
        // The rename is durable once the directory is.
        if let Ok(directory) = fs::File::open(root) {
            let _ignored = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        // A failed adoption removes its staging and leaves the library
        // untouched; the removal is best-effort because the error the
        // owner gets is the first one.
        let _ignored = fs::remove_file(&staged_name);
    }
    result.map(|()| Outcome::Adopted(identity))
}

#[cfg(test)]
mod tests {
    use super::{adopt_in, Outcome, Rejection, MAX_IMPORT_BYTES};
    use crate::identity::Provenance;
    use crate::library;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::current_dir()
            .expect("working directory")
            .join("target")
            .join(format!("kobo-policy-adopt-{name}-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("scratch folder");
        folder
    }

    fn source() -> Provenance {
        Provenance::new("test", "test")
    }

    #[test]
    fn the_boundary_rejects_what_the_contract_names() {
        let root = scratch("rejections");
        let bytes = b"plain text";
        assert_eq!(
            adopt_in(&root, "../escape.txt", bytes, source()),
            Err(Rejection::MalformedName)
        );
        assert_eq!(
            adopt_in(&root, "folder/book.txt", bytes, source()),
            Err(Rejection::MalformedName)
        );
        assert_eq!(
            adopt_in(&root, ".hidden.txt", bytes, source()),
            Err(Rejection::MalformedName)
        );
        assert_eq!(
            adopt_in(&root, "book.xyz", bytes, source()),
            Err(Rejection::UnsupportedType)
        );
        assert_eq!(
            adopt_in(&root, "book.txt", b"", source()),
            Err(Rejection::Empty)
        );
        let oversized = vec![0u8; MAX_IMPORT_BYTES + 1];
        assert_eq!(
            adopt_in(&root, "book.txt", &oversized, source()),
            Err(Rejection::Oversized)
        );
        // Nothing was written: the library is exactly as it stood.
        assert!(fs::read_dir(&root).expect("root").next().is_none());
        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_document_is_staged_then_committed_and_listed() {
        let root = scratch("adopted");
        let outcome =
            adopt_in(&root, "story.txt", b"once upon a time", source()).expect("adoption succeeds");
        let Outcome::Adopted(identity) = outcome else {
            panic!("a first adoption commits");
        };
        assert_eq!(identity.key(), "story/");
        assert_eq!(
            fs::read(root.join("story.txt")).expect("committed"),
            b"once upon a time"
        );
        // Staging leaves nothing, and the shared listing sees the book.
        assert!(fs::read_dir(root.join(".adds/adoptions"))
            .expect("staging")
            .next()
            .is_none());
        let listing = library::list_in(&[root.clone()]);
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].title, "story");
        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_bytes_again_are_adopted_once_without_a_rewrite() {
        let root = scratch("repeat");
        adopt_in(&root, "story.txt", b"same bytes", source()).expect("first");
        let written = fs::metadata(root.join("story.txt")).expect("committed");
        let outcome = adopt_in(
            &root,
            "story.txt",
            b"same bytes",
            Provenance::new("opds", "opds"),
        )
        .expect("a repeat is not an error");
        let Outcome::AlreadyPresent(identity) = outcome else {
            panic!("the same bytes adopt once");
        };
        assert_eq!(identity.provenance()[0].source(), "opds");
        let reread = fs::metadata(root.join("story.txt")).expect("committed");
        assert_eq!(
            written.modified().expect("mtime"),
            reread.modified().expect("mtime")
        );
        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_name_over_different_bytes_is_a_conflict_never_an_overwrite() {
        let root = scratch("conflict");
        adopt_in(&root, "story.txt", b"the original", source()).expect("first");
        assert_eq!(
            adopt_in(&root, "story.txt", b"a different book", source()),
            Err(Rejection::Conflict)
        );
        assert_eq!(
            fs::read(root.join("story.txt")).expect("untouched"),
            b"the original"
        );
        assert!(fs::read_dir(root.join(".adds/adoptions"))
            .expect("staging")
            .next()
            .is_none());
        let _ignored = fs::remove_dir_all(&root);
    }
}
