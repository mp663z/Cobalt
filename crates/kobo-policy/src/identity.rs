//! One identity per content item across the platform.
//!
//! Identity is the hash of the original bytes plus a normalized metadata
//! key: identical bytes arriving from two connectors resolve to one
//! identity that accumulates a provenance record per source, while the
//! same filename over different bytes is a conflict and never a silent
//! duplicate. Contract: docs/quality/contracts/content-identity.md.

/// Where a copy of a content item came from. One record per connector
/// that delivered the same bytes; the identity stays one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provenance {
    source: String,
    name: String,
}

impl Provenance {
    /// Records that `source` delivered the item under `name`.
    pub fn new(source: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            name: name.into(),
        }
    }

    /// The connector or app that delivered this copy.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The name the copy carried, exactly as delivered.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The identity of one content item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentIdentity {
    hash: String,
    key: String,
    provenance: Vec<Provenance>,
}

impl ContentIdentity {
    /// Identifies `bytes`, titled and authored as the metadata says, first
    /// seen from `provenance`.
    pub fn identify(bytes: &[u8], title: &str, author: &str, provenance: Provenance) -> Self {
        Self {
            hash: kobo_net::sha256::hex_digest(bytes),
            key: normalized_key(title, author),
            provenance: vec![provenance],
        }
    }

    /// The hash of the original bytes.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The normalized metadata key: the title and author reduced to a
    /// form that spelling and spacing differences cannot multiply.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Every source that delivered these bytes, first delivery first.
    pub fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }
}

/// What a newly arrived item is to one already known.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arrival {
    /// The same bytes again: record the provenance, keep one identity.
    Same,
    /// Different bytes under a name already taken: a conflict to
    /// surface, never a silent duplicate.
    Conflict,
    /// Nothing known by this identity.
    New,
}

/// Classifies an arrival against the identity already recorded for it.
pub fn classify(known: &ContentIdentity, bytes: &[u8], name: &str) -> Arrival {
    let hash = kobo_net::sha256::hex_digest(bytes);
    if hash == known.hash {
        return Arrival::Same;
    }
    if known.provenance.iter().any(|record| record.name == name) {
        return Arrival::Conflict;
    }
    Arrival::New
}

/// Records another delivery of the same bytes: the identity stays one
/// and gains the provenance, unless this source already delivered it
/// under the same name.
pub fn adopt(identity: &mut ContentIdentity, provenance: Provenance) {
    if !identity.provenance.contains(&provenance) {
        identity.provenance.push(provenance);
    }
}

/// The normalized metadata key: lowercase, whitespace runs collapsed,
/// ends trimmed. Case and spacing are not identity.
fn normalized_key(title: &str, author: &str) -> String {
    let normalize = |text: &str| {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    format!("{}/{}", normalize(title), normalize(author))
}

#[cfg(test)]
mod tests {
    use super::{adopt, classify, Arrival, ContentIdentity, Provenance};

    fn book() -> ContentIdentity {
        ContentIdentity::identify(
            b"the same bytes wherever they come from",
            "The  Hobbit",
            "J.R.R.  Tolkien",
            Provenance::new("web", "hobbit.epub"),
        )
    }

    #[test]
    fn identical_bytes_from_two_connectors_resolve_to_one_identity() {
        let mut identity = book();
        adopt(&mut identity, Provenance::new("opds", "The Hobbit.epub"));
        assert_eq!(identity.provenance().len(), 2);
        assert_eq!(identity.provenance()[0].source(), "web");
        assert_eq!(identity.provenance()[1].source(), "opds");
    }

    #[test]
    fn a_source_repeating_itself_does_not_multiply_provenance() {
        let mut identity = book();
        adopt(&mut identity, Provenance::new("web", "hobbit.epub"));
        assert_eq!(identity.provenance().len(), 1);
    }

    #[test]
    fn metadata_spelling_and_spacing_are_not_identity() {
        let identity = book();
        assert_eq!(identity.key(), "the hobbit/j.r.r. tolkien");
    }

    #[test]
    fn the_same_bytes_again_are_the_same_item() {
        let identity = book();
        assert_eq!(
            classify(&identity, b"the same bytes wherever they come from", "other.epub"),
            Arrival::Same
        );
    }

    #[test]
    fn the_same_name_over_different_bytes_is_a_conflict() {
        let identity = book();
        assert_eq!(
            classify(&identity, b"different bytes entirely", "hobbit.epub"),
            Arrival::Conflict
        );
    }

    #[test]
    fn different_bytes_under_a_fresh_name_are_new() {
        let identity = book();
        assert_eq!(classify(&identity, b"different bytes entirely", "other.epub"), Arrival::New);
    }
}
