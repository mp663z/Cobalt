use crate::{
    FormatError, FORMAT_VERSION, MAX_APP_ID_BYTES, MAX_BINARY_BYTES, MAX_CAPABILITIES,
    MAX_CATALOG_BYTES, MAX_CATALOG_ENTRIES, MAX_DISPLAY_NAME_BYTES, MAX_GLYPH_BYTES,
    MAX_MANIFEST_BYTES, MAX_PACKAGE_BYTES, MAX_PACKAGE_URL_BYTES, MAX_SHORT_LABEL_BYTES,
    MAX_SUMMARY_BYTES, MAX_VERSION_BYTES,
};
use kobo_json::Value;
use kobo_policy::{Capability, Declared};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

const MANIFEST_FIELDS: [&str; 12] = [
    "format_version",
    "id",
    "display_name",
    "short_label",
    "summary",
    "version",
    "minimum_cobalt_version",
    "glyph",
    "capabilities",
    "binary_sha256",
    "binary_bytes",
    // Optional: the app quality manifest block
    // (docs/quality/contracts/app-quality-manifest.md).
    "quality",
];
const CATALOG_FIELDS: [&str; 2] = ["format_version", "entries"];
const ENTRY_FIELDS: [&str; 4] = ["manifest", "package_url", "package_sha256", "package_bytes"];
const MAX_CAPABILITY_NAME_BYTES: usize = 32;

static PUBLIC_RESERVED_APP_IDS: &[&str] = &[
    "books", "cobalt", "kobod", "launcher", "settings", "store", "terminal",
];

/// Platform-owned application identifiers that public packages may not use.
#[must_use]
pub fn public_reserved_app_ids() -> &'static [&'static str] {
    PUBLIC_RESERVED_APP_IDS
}

/// Returns whether `id` is reserved for Cobalt itself.
#[must_use]
pub fn is_public_reserved_app_id(id: &str) -> bool {
    PUBLIC_RESERVED_APP_IDS.contains(&id)
}

/// Returns whether the released runtime can render this manifest glyph.
#[must_use]
pub fn is_public_glyph(name: &str) -> bool {
    matches!(
        name,
        "app"
            | "book"
            | "note"
            | "clock"
            | "settings"
            | "folder"
            | "chart"
            | "search"
            | "wifi"
            | "battery"
            | "reader"
            | "power"
            | "grid"
            | "circle"
            | "check"
            | "terminal"
            | "chat"
            | "news"
            | "rss"
            | "light"
            | "close"
            | "download"
            | "bookmark"
            | "filter"
            | "person"
            | "tag"
            | "globe"
            | "refresh"
            | "more"
            | "bluetooth"
            | "key"
            | "magnet"
            | "play"
            | "pause"
            | "rewind30"
            | "forward30"
            | "volume-down"
            | "volume-up"
            | "more-vertical"
            | "trash"
            | "previous"
            | "next"
            | "plus"
            | "headphones"
    )
}

/// Returns whether `current` satisfies a manifest's minimum Cobalt version.
#[must_use]
pub fn cobalt_version_at_least(current: &str, minimum: &str) -> bool {
    match (parse_cobalt_version(current), parse_cobalt_version(minimum)) {
        (Some(current), Some(minimum)) => current >= minimum,
        _ => false,
    }
}

/// A validated lowercase SHA-256 digest.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Parses exactly 64 lowercase hexadecimal characters.
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::InvalidValue`] for any other spelling.
    pub fn parse(value: &str, field: &'static str) -> Result<Self, FormatError> {
        if is_lower_hex(value, 64) {
            Ok(Self(value.to_owned()))
        } else {
            Err(FormatError::InvalidValue {
                field,
                reason: "must be 64 lowercase hexadecimal characters",
            })
        }
    }

    /// The canonical lowercase hexadecimal spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Sha256Digest {
    type Err = FormatError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value, "sha256")
    }
}

/// Unvalidated owned fields used to construct a [`Manifest`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestInput {
    /// The app quality manifest block; required by the release tooling,
    /// optional on the wire so older fixtures keep parsing.
    pub quality: Option<QualityInput>,
    pub id: String,
    pub display_name: String,
    pub short_label: String,
    pub summary: String,
    pub version: String,
    pub minimum_cobalt_version: String,
    pub glyph: String,
    pub capabilities: Vec<String>,
    pub binary_sha256: String,
    pub binary_bytes: u64,
}

/// A fully validated application manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    quality: Option<Quality>,
    id: String,
    display_name: String,
    short_label: String,
    summary: String,
    version: String,
    minimum_cobalt_version: String,
    glyph: String,
    capabilities: Declared,
    binary_sha256: Sha256Digest,
    binary_bytes: u64,
}

impl Manifest {
    /// Validates fields, rejecting identifiers present in `reserved_ids`.
    ///
    /// # Errors
    ///
    /// Returns an error when any field is malformed, unbounded, reserved, or
    /// contains an invalid capability declaration.
    pub fn new(input: ManifestInput, reserved_ids: &[&str]) -> Result<Self, FormatError> {
        validate_identifier(&input.id, "id", MAX_APP_ID_BYTES)?;
        if reserved_ids.contains(&input.id.as_str()) {
            return Err(FormatError::ReservedAppId(input.id));
        }
        validate_text(&input.display_name, "display_name", MAX_DISPLAY_NAME_BYTES)?;
        validate_text(&input.short_label, "short_label", MAX_SHORT_LABEL_BYTES)?;
        validate_text(&input.summary, "summary", MAX_SUMMARY_BYTES)?;
        validate_version(&input.version, "version")?;
        validate_cobalt_version(&input.minimum_cobalt_version)?;
        validate_identifier(&input.glyph, "glyph", MAX_GLYPH_BYTES)?;
        let capabilities = validate_capabilities(&input.capabilities)?;
        let quality = input
            .quality
            .map(|quality| validate_quality(quality, &input.capabilities))
            .transpose()?;
        let binary_sha256 = Sha256Digest::parse(&input.binary_sha256, "binary_sha256")?;
        validate_count(input.binary_bytes, "binary_bytes", MAX_BINARY_BYTES)?;

        Ok(Self {
            quality,
            id: input.id,
            display_name: input.display_name,
            short_label: input.short_label,
            summary: input.summary,
            version: input.version,
            minimum_cobalt_version: input.minimum_cobalt_version,
            glyph: input.glyph,
            capabilities,
            binary_sha256,
            binary_bytes: input.binary_bytes,
        })
    }

    /// Validates fields against Cobalt's public reserved identifiers.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::new`].
    pub fn new_public(input: ManifestInput) -> Result<Self, FormatError> {
        Self::validate_public_manifest(Self::new(input, public_reserved_app_ids())?)
    }

    /// Parses strict UTF-8 manifest JSON with caller-supplied reserved IDs.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid UTF-8/JSON, unknown or duplicate fields,
    /// unsupported format versions, or invalid manifest values.
    pub fn parse(json: &[u8], reserved_ids: &[&str]) -> Result<Self, FormatError> {
        let value = parse_document(json, MAX_MANIFEST_BYTES)?;
        parse_manifest_value(&value, reserved_ids)
    }

    /// Parses strict UTF-8 manifest JSON using Cobalt's reserved IDs.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::parse`].
    pub fn parse_public(json: &[u8]) -> Result<Self, FormatError> {
        Self::validate_public_manifest(Self::parse(json, public_reserved_app_ids())?)
    }

    fn validate_public_manifest(manifest: Manifest) -> Result<Manifest, FormatError> {
        manifest.ensure_public()?;
        Ok(manifest)
    }

    pub(crate) fn ensure_public(&self) -> Result<(), FormatError> {
        if !is_public_glyph(&self.glyph) {
            return Err(FormatError::InvalidValue {
                field: "glyph",
                reason: "is not supported by released Cobalt runtimes",
            });
        }
        if self.capabilities.holds(Capability::Shell) {
            return Err(FormatError::InvalidValue {
                field: "capabilities",
                reason: "public applications cannot request shell access",
            });
        }
        Ok(())
    }

    /// Deterministic compact JSON used for signatures.
    #[must_use]
    pub fn to_canonical_json(&self) -> String {
        let mut out = String::with_capacity(512);
        write_manifest(self, &mut out);
        out
    }

    /// Deterministic UTF-8 JSON bytes used for signatures.
    #[must_use]
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        self.to_canonical_json().into_bytes()
    }

    /// The app quality manifest block, when the catalog carries it.
    #[must_use]
    pub fn quality(&self) -> Option<&Quality> {
        self.quality.as_ref()
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub fn short_label(&self) -> &str {
        &self.short_label
    }

    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn minimum_cobalt_version(&self) -> &str {
        &self.minimum_cobalt_version
    }

    #[must_use]
    pub fn glyph(&self) -> &str {
        &self.glyph
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.capabilities
            .iter()
            .map(kobo_policy::Capability::manifest_name)
    }

    #[must_use]
    pub fn declared_capabilities(&self) -> &Declared {
        &self.capabilities
    }

    #[must_use]
    pub fn binary_sha256(&self) -> &Sha256Digest {
        &self.binary_sha256
    }

    #[must_use]
    pub fn binary_bytes(&self) -> u64 {
        self.binary_bytes
    }
}

/// Unvalidated package fields used to construct a [`CatalogEntry`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntryInput {
    pub manifest: Manifest,
    pub package_url: String,
    pub package_sha256: String,
    pub package_bytes: u64,
}

/// A validated catalog entry and the package carrying its bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntry {
    manifest: Manifest,
    package_url: String,
    package_sha256: Sha256Digest,
    package_bytes: u64,
}

impl CatalogEntry {
    /// Validates package metadata around an already validated manifest.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-HTTPS URL, malformed digest, or invalid
    /// package byte count.
    pub fn new(input: CatalogEntryInput) -> Result<Self, FormatError> {
        validate_package_url(&input.package_url)?;
        let package_sha256 = Sha256Digest::parse(&input.package_sha256, "package_sha256")?;
        validate_count(input.package_bytes, "package_bytes", MAX_PACKAGE_BYTES)?;
        Ok(Self {
            manifest: input.manifest,
            package_url: input.package_url,
            package_sha256,
            package_bytes: input.package_bytes,
        })
    }

    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    #[must_use]
    pub fn package_url(&self) -> &str {
        &self.package_url
    }

    #[must_use]
    pub fn package_sha256(&self) -> &Sha256Digest {
        &self.package_sha256
    }

    #[must_use]
    pub fn package_bytes(&self) -> u64 {
        self.package_bytes
    }
}

/// A bounded catalog with unique application identifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

impl Catalog {
    /// Builds a catalog, rejecting too many entries and duplicate app IDs.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry count exceeds [`MAX_CATALOG_ENTRIES`]
    /// or two manifests carry the same app ID.
    pub fn new(entries: Vec<CatalogEntry>) -> Result<Self, FormatError> {
        if entries.len() > MAX_CATALOG_ENTRIES {
            return Err(FormatError::InvalidValue {
                field: "entries",
                reason: "too many catalog entries",
            });
        }
        let mut ids = BTreeSet::new();
        for entry in &entries {
            if !ids.insert(entry.manifest.id()) {
                return Err(FormatError::DuplicateAppId(entry.manifest.id().to_owned()));
            }
        }
        let catalog = Self { entries };
        if catalog.to_canonical_bytes().len() > MAX_CATALOG_BYTES {
            return Err(FormatError::DocumentTooLarge {
                maximum: MAX_CATALOG_BYTES,
            });
        }
        Ok(catalog)
    }

    /// Parses strict UTF-8 catalog JSON with caller-supplied reserved IDs.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed structure, entries, package metadata,
    /// duplicate app IDs, or an unsupported format version.
    pub fn parse(json: &[u8], reserved_ids: &[&str]) -> Result<Self, FormatError> {
        let value = parse_document(json, MAX_CATALOG_BYTES)?;
        let object = StrictObject::new(&value, "catalog", &CATALOG_FIELDS)?;
        validate_format(object.value("format_version")?)?;
        let entries = object
            .value("entries")?
            .as_array()
            .ok_or(FormatError::InvalidType("entries"))?;
        if entries.len() > MAX_CATALOG_ENTRIES {
            return Err(FormatError::InvalidValue {
                field: "entries",
                reason: "too many catalog entries",
            });
        }
        let parsed = entries
            .iter()
            .map(|entry| parse_catalog_entry(entry, reserved_ids))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(parsed)
    }

    /// Parses strict UTF-8 catalog JSON using Cobalt's reserved IDs.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::parse`].
    pub fn parse_public(json: &[u8]) -> Result<Self, FormatError> {
        let catalog = Self::parse(json, public_reserved_app_ids())?;
        for entry in &catalog.entries {
            entry.manifest.ensure_public()?;
        }
        Ok(catalog)
    }

    /// Deterministic compact JSON used for signatures.
    #[must_use]
    pub fn to_canonical_json(&self) -> String {
        let mut out = String::with_capacity(self.entries.len().saturating_mul(640));
        out.push_str("{\"format_version\":1,\"entries\":[");
        for (index, entry) in self.entries.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            out.push_str("{\"manifest\":");
            write_manifest(&entry.manifest, &mut out);
            out.push_str(",\"package_url\":");
            kobo_json::escape_into(&entry.package_url, &mut out);
            out.push_str(",\"package_sha256\":");
            kobo_json::escape_into(entry.package_sha256.as_str(), &mut out);
            out.push_str(",\"package_bytes\":");
            out.push_str(&entry.package_bytes.to_string());
            out.push('}');
        }
        out.push_str("]}");
        out
    }

    /// Deterministic UTF-8 JSON bytes used for signatures.
    #[must_use]
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        self.to_canonical_json().into_bytes()
    }

    #[must_use]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }
}

fn parse_catalog_entry(value: &Value, reserved_ids: &[&str]) -> Result<CatalogEntry, FormatError> {
    let object = StrictObject::new(value, "catalog entry", &ENTRY_FIELDS)?;
    CatalogEntry::new(CatalogEntryInput {
        manifest: parse_manifest_value(object.value("manifest")?, reserved_ids)?,
        package_url: string_field(&object, "package_url")?,
        package_sha256: string_field(&object, "package_sha256")?,
        package_bytes: count_field(&object, "package_bytes")?,
    })
}

/// What happens to one kind of user-created data when the app is removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retention {
    /// The data stays on the reader.
    Retained,
    /// The uninstall flow offers an export, then deletes the data.
    ExportedThenDeleted,
    /// The data is deleted with the app.
    Deleted,
}

impl Retention {
    /// The canonical manifest spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retained => "retained",
            Self::ExportedThenDeleted => "exported-then-deleted",
            Self::Deleted => "deleted",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "retained" => Some(Self::Retained),
            "exported-then-deleted" => Some(Self::ExportedThenDeleted),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

/// One kind of user-created data, as declared by the app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKindInput {
    /// A short name for the kind ("kept papers"), not a sentence.
    pub kind: String,
    /// Where it lives, in user-facing language.
    pub location: String,
    /// How it leaves the reader, or why it does not.
    pub export: String,
    /// What removal does to it: retained, exported-then-deleted, deleted.
    pub on_remove: String,
}

/// A required capability and its user-facing purpose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityPurposeInput {
    /// A capability the app also declares.
    pub name: String,
    /// What the app needs it for, in user-facing language.
    pub purpose: String,
}

/// An optional capability and the feature it gates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityGateInput {
    /// A capability the app also declares.
    pub name: String,
    /// The feature that works only with it.
    pub gates: String,
}

/// The app quality manifest block
/// (docs/quality/contracts/app-quality-manifest.md), unvalidated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityInput {
    /// Who the app serves.
    pub user: String,
    /// The one job it does.
    pub job: String,
    /// What works with no network, as user-visible behavior.
    pub offline: String,
    /// Each kind of user-created data; empty means the app creates none.
    pub data: Vec<DataKindInput>,
    /// Capabilities the app fails without.
    pub capabilities_required: Vec<CapabilityPurposeInput>,
    /// Capabilities the app degrades gracefully without.
    pub capabilities_optional: Vec<CapabilityGateInput>,
    /// Simulator profiles the app supports.
    pub profiles: Vec<String>,
    /// The named owner.
    pub maintainer: String,
    /// Where issues go.
    pub support: String,
    /// What the app deliberately does not do.
    pub non_goals: Vec<String>,
}

/// A validated kind of user-created data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKind {
    kind: String,
    location: String,
    export: String,
    on_remove: Retention,
}

impl DataKind {
    /// A short name for the kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Where it lives, in user-facing language.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    /// How it leaves the reader, or why it does not.
    #[must_use]
    pub fn export(&self) -> &str {
        &self.export
    }

    /// What removal does to it.
    #[must_use]
    pub const fn on_remove(&self) -> Retention {
        self.on_remove
    }
}

/// A validated required capability with its purpose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityPurpose {
    name: String,
    purpose: String,
}

impl CapabilityPurpose {
    /// The capability name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the app needs it for, in user-facing language.
    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
}

/// A validated optional capability with the feature it gates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityGate {
    name: String,
    gates: String,
}

impl CapabilityGate {
    /// The capability name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The feature that works only with it.
    #[must_use]
    pub fn gates(&self) -> &str {
        &self.gates
    }
}

/// A validated app quality manifest block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quality {
    user: String,
    job: String,
    offline: String,
    data: Vec<DataKind>,
    capabilities_required: Vec<CapabilityPurpose>,
    capabilities_optional: Vec<CapabilityGate>,
    profiles: Vec<String>,
    maintainer: String,
    support: String,
    non_goals: Vec<String>,
}

impl Quality {
    /// Who the app serves.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The one job it does.
    #[must_use]
    pub fn job(&self) -> &str {
        &self.job
    }

    /// What works with no network, as user-visible behavior.
    #[must_use]
    pub fn offline(&self) -> &str {
        &self.offline
    }

    /// Each kind of user-created data.
    #[must_use]
    pub fn data(&self) -> &[DataKind] {
        &self.data
    }

    /// Capabilities the app fails without.
    #[must_use]
    pub fn capabilities_required(&self) -> &[CapabilityPurpose] {
        &self.capabilities_required
    }

    /// Capabilities the app degrades gracefully without.
    #[must_use]
    pub fn capabilities_optional(&self) -> &[CapabilityGate] {
        &self.capabilities_optional
    }

    /// Simulator profiles the app supports.
    #[must_use]
    pub fn profiles(&self) -> &[String] {
        &self.profiles
    }

    /// The named owner.
    #[must_use]
    pub fn maintainer(&self) -> &str {
        &self.maintainer
    }

    /// Where issues go.
    #[must_use]
    pub fn support(&self) -> &str {
        &self.support
    }

    /// What the app deliberately does not do.
    #[must_use]
    pub fn non_goals(&self) -> &[String] {
        &self.non_goals
    }

    /// The block as canonical JSON, the same bytes the manifest spelling
    /// carries and signatures cover.
    #[must_use]
    pub fn to_canonical_json(&self) -> String {
        let mut out = String::with_capacity(512);
        write_quality_object(self, &mut out);
        out
    }
}

const QUALITY_FIELDS: [&str; 10] = [
    "user",
    "job",
    "offline",
    "data",
    "capabilities_required",
    "capabilities_optional",
    "profiles",
    "maintainer",
    "support",
    "non_goals",
];
const DATA_KIND_FIELDS: [&str; 4] = ["kind", "location", "export", "on_remove"];
const PURPOSE_FIELDS: [&str; 2] = ["name", "purpose"];
const GATE_FIELDS: [&str; 2] = ["name", "gates"];
const MAX_QUALITY_SENTENCE_BYTES: usize = 280;
const MAX_QUALITY_KIND_BYTES: usize = 80;
const MAX_QUALITY_EXPORT_BYTES: usize = 120;
const MAX_QUALITY_SUPPORT_BYTES: usize = 200;
const MAX_QUALITY_DATA_KINDS: usize = 8;
const MAX_QUALITY_NON_GOALS: usize = 8;
const MAX_QUALITY_PROFILES: usize = 8;

/// Shape-parses a quality manifest JSON value into unvalidated input.
///
/// The release tooling reads registry rows with this; `Manifest::new` then
/// validates the content like any other manifest field.
///
/// # Errors
///
/// Returns an error for a malformed block: wrong types, or unknown,
/// duplicate or missing fields.
pub fn parse_quality_json(value: &Value) -> Result<QualityInput, FormatError> {
    parse_quality_value(value)
}

fn parse_quality_value(value: &Value) -> Result<QualityInput, FormatError> {
    let object = StrictObject::new(value, "quality", &QUALITY_FIELDS)?;
    let data = object
        .value("data")?
        .as_array()
        .ok_or(FormatError::InvalidType("data"))?
        .iter()
        .map(|item| {
            let item = StrictObject::new(item, "data kind", &DATA_KIND_FIELDS)?;
            Ok(DataKindInput {
                kind: string_field(&item, "kind")?,
                location: string_field(&item, "location")?,
                export: string_field(&item, "export")?,
                on_remove: string_field(&item, "on_remove")?,
            })
        })
        .collect::<Result<Vec<_>, FormatError>>()?;
    let capabilities_required = object
        .value("capabilities_required")?
        .as_array()
        .ok_or(FormatError::InvalidType("capabilities_required"))?
        .iter()
        .map(|item| {
            let item = StrictObject::new(item, "required capability", &PURPOSE_FIELDS)?;
            Ok(CapabilityPurposeInput {
                name: string_field(&item, "name")?,
                purpose: string_field(&item, "purpose")?,
            })
        })
        .collect::<Result<Vec<_>, FormatError>>()?;
    let capabilities_optional = object
        .value("capabilities_optional")?
        .as_array()
        .ok_or(FormatError::InvalidType("capabilities_optional"))?
        .iter()
        .map(|item| {
            let item = StrictObject::new(item, "optional capability", &GATE_FIELDS)?;
            Ok(CapabilityGateInput {
                name: string_field(&item, "name")?,
                gates: string_field(&item, "gates")?,
            })
        })
        .collect::<Result<Vec<_>, FormatError>>()?;
    let string_list = |field: &'static str| -> Result<Vec<String>, FormatError> {
        object
            .value(field)?
            .as_array()
            .ok_or(FormatError::InvalidType(field))?
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or(FormatError::InvalidType(field))
            })
            .collect()
    };
    Ok(QualityInput {
        user: string_field(&object, "user")?,
        job: string_field(&object, "job")?,
        offline: string_field(&object, "offline")?,
        data,
        capabilities_required,
        capabilities_optional,
        profiles: string_list("profiles")?,
        maintainer: string_field(&object, "maintainer")?,
        support: string_field(&object, "support")?,
        non_goals: string_list("non_goals")?,
    })
}

fn validate_quality_sentence(
    value: &str,
    field: &'static str,
    maximum: usize,
    minimum: usize,
) -> Result<(), FormatError> {
    validate_text(value, field, maximum)?;
    if value.trim().len() < minimum {
        return Err(FormatError::InvalidValue {
            field,
            reason: "must be a meaningful sentence",
        });
    }
    Ok(())
}

fn validate_data_kind(item: DataKindInput) -> Result<DataKind, FormatError> {
    validate_quality_sentence(&item.kind, "kind", MAX_QUALITY_KIND_BYTES, 2)?;
    validate_quality_sentence(&item.location, "location", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    validate_quality_sentence(&item.export, "export", MAX_QUALITY_EXPORT_BYTES, 12)?;
    let on_remove = Retention::parse(&item.on_remove).ok_or(FormatError::InvalidValue {
        field: "on_remove",
        reason: "must be retained, exported-then-deleted, or deleted",
    })?;
    Ok(DataKind {
        kind: item.kind,
        location: item.location,
        export: item.export,
        on_remove,
    })
}

fn validate_quality_capability_name(name: &str, declared: &[String]) -> Result<(), FormatError> {
    validate_text(name, "name", MAX_CAPABILITY_NAME_BYTES)?;
    if !declared.iter().any(|capability| capability == name) {
        return Err(FormatError::InvalidValue {
            field: "name",
            reason: "must be one of the app's declared capabilities",
        });
    }
    Ok(())
}

fn validate_capability_purpose(
    item: CapabilityPurposeInput,
    declared: &[String],
) -> Result<CapabilityPurpose, FormatError> {
    validate_quality_capability_name(&item.name, declared)?;
    validate_quality_sentence(&item.purpose, "purpose", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    Ok(CapabilityPurpose {
        name: item.name,
        purpose: item.purpose,
    })
}

fn validate_capability_gate(
    item: CapabilityGateInput,
    declared: &[String],
) -> Result<CapabilityGate, FormatError> {
    validate_quality_capability_name(&item.name, declared)?;
    validate_quality_sentence(&item.gates, "gates", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    Ok(CapabilityGate {
        name: item.name,
        gates: item.gates,
    })
}

fn validate_quality(input: QualityInput, declared: &[String]) -> Result<Quality, FormatError> {
    validate_quality_sentence(&input.user, "user", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    validate_quality_sentence(&input.job, "job", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    validate_quality_sentence(&input.offline, "offline", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    if input.data.len() > MAX_QUALITY_DATA_KINDS {
        return Err(FormatError::InvalidValue {
            field: "data",
            reason: "too many data kinds",
        });
    }
    let data = input
        .data
        .into_iter()
        .map(validate_data_kind)
        .collect::<Result<Vec<_>, FormatError>>()?;
    if input.capabilities_required.len() > MAX_CAPABILITIES
        || input.capabilities_optional.len() > MAX_CAPABILITIES
    {
        return Err(FormatError::InvalidValue {
            field: "capabilities_required",
            reason: "too many capability entries",
        });
    }
    let capabilities_required = input
        .capabilities_required
        .into_iter()
        .map(|item| validate_capability_purpose(item, declared))
        .collect::<Result<Vec<_>, FormatError>>()?;
    let capabilities_optional = input
        .capabilities_optional
        .into_iter()
        .map(|item| validate_capability_gate(item, declared))
        .collect::<Result<Vec<_>, FormatError>>()?;
    if input.profiles.is_empty() || input.profiles.len() > MAX_QUALITY_PROFILES {
        return Err(FormatError::InvalidValue {
            field: "profiles",
            reason: "must name at least one supported profile",
        });
    }
    for profile in &input.profiles {
        validate_text(profile, "profiles", MAX_CAPABILITY_NAME_BYTES)?;
    }
    validate_quality_sentence(
        &input.maintainer,
        "maintainer",
        MAX_QUALITY_SENTENCE_BYTES,
        12,
    )?;
    validate_quality_sentence(&input.support, "support", MAX_QUALITY_SUPPORT_BYTES, 12)?;
    if input.non_goals.is_empty() || input.non_goals.len() > MAX_QUALITY_NON_GOALS {
        return Err(FormatError::InvalidValue {
            field: "non_goals",
            reason: "must name at least one non-goal",
        });
    }
    for non_goal in &input.non_goals {
        validate_quality_sentence(non_goal, "non_goals", MAX_QUALITY_SENTENCE_BYTES, 12)?;
    }
    Ok(Quality {
        user: input.user,
        job: input.job,
        offline: input.offline,
        data,
        capabilities_required,
        capabilities_optional,
        profiles: input.profiles,
        maintainer: input.maintainer,
        support: input.support,
        non_goals: input.non_goals,
    })
}

fn write_quality(quality: &Quality, out: &mut String) {
    out.push_str(",\"quality\":");
    write_quality_object(quality, out);
}

fn write_quality_object(quality: &Quality, out: &mut String) {
    out.push_str("{\"user\":");
    kobo_json::escape_into(&quality.user, out);
    out.push_str(",\"job\":");
    kobo_json::escape_into(&quality.job, out);
    out.push_str(",\"offline\":");
    kobo_json::escape_into(&quality.offline, out);
    out.push_str(",\"data\":[");
    for (index, item) in quality.data.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str("{\"kind\":");
        kobo_json::escape_into(&item.kind, out);
        out.push_str(",\"location\":");
        kobo_json::escape_into(&item.location, out);
        out.push_str(",\"export\":");
        kobo_json::escape_into(&item.export, out);
        out.push_str(",\"on_remove\":");
        kobo_json::escape_into(item.on_remove.as_str(), out);
        out.push('}');
    }
    out.push_str("],\"capabilities_required\":[");
    for (index, item) in quality.capabilities_required.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        kobo_json::escape_into(&item.name, out);
        out.push_str(",\"purpose\":");
        kobo_json::escape_into(&item.purpose, out);
        out.push('}');
    }
    out.push_str("],\"capabilities_optional\":[");
    for (index, item) in quality.capabilities_optional.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        kobo_json::escape_into(&item.name, out);
        out.push_str(",\"gates\":");
        kobo_json::escape_into(&item.gates, out);
        out.push('}');
    }
    out.push_str("],\"profiles\":[");
    for (index, profile) in quality.profiles.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        kobo_json::escape_into(profile, out);
    }
    out.push_str("],\"maintainer\":");
    kobo_json::escape_into(&quality.maintainer, out);
    out.push_str(",\"support\":");
    kobo_json::escape_into(&quality.support, out);
    out.push_str(",\"non_goals\":[");
    for (index, non_goal) in quality.non_goals.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        kobo_json::escape_into(non_goal, out);
    }
    out.push_str("]}");
}

fn parse_manifest_value(value: &Value, reserved_ids: &[&str]) -> Result<Manifest, FormatError> {
    let object = StrictObject::with_optional(value, "manifest", &MANIFEST_FIELDS, &["quality"])?;
    validate_format(object.value("format_version")?)?;
    let quality = match object.fields.iter().find(|(name, _)| name == "quality") {
        Some((_, value)) => Some(parse_quality_value(value)?),
        None => None,
    };
    Manifest::new(
        ManifestInput {
            quality,
            id: string_field(&object, "id")?,
            display_name: string_field(&object, "display_name")?,
            short_label: string_field(&object, "short_label")?,
            summary: string_field(&object, "summary")?,
            version: string_field(&object, "version")?,
            minimum_cobalt_version: string_field(&object, "minimum_cobalt_version")?,
            glyph: string_field(&object, "glyph")?,
            capabilities: capability_fields(&object)?,
            binary_sha256: string_field(&object, "binary_sha256")?,
            binary_bytes: count_field(&object, "binary_bytes")?,
        },
        reserved_ids,
    )
}

fn parse_document(json: &[u8], maximum: usize) -> Result<Value, FormatError> {
    if json.len() > maximum {
        return Err(FormatError::DocumentTooLarge { maximum });
    }
    let text = std::str::from_utf8(json).map_err(|_| FormatError::InvalidUtf8)?;
    kobo_json::parse(text).map_err(FormatError::from)
}

struct StrictObject<'a> {
    fields: &'a [(String, Value)],
    name: &'static str,
}

impl<'a> StrictObject<'a> {
    fn new(
        value: &'a Value,
        name: &'static str,
        expected: &[&'static str],
    ) -> Result<Self, FormatError> {
        Self::with_optional(value, name, expected, &[])
    }

    fn with_optional(
        value: &'a Value,
        name: &'static str,
        expected: &[&'static str],
        optional: &[&'static str],
    ) -> Result<Self, FormatError> {
        let Value::Object(fields) = value else {
            return Err(FormatError::ExpectedObject(name));
        };
        let mut seen = BTreeSet::new();
        for (field, _) in fields {
            if !expected.contains(&field.as_str()) {
                return Err(FormatError::UnknownField {
                    object: name,
                    field: field.clone(),
                });
            }
            if !seen.insert(field.as_str()) {
                return Err(FormatError::DuplicateField {
                    object: name,
                    field: field.clone(),
                });
            }
        }
        for field in expected {
            if !seen.contains(field) && !optional.contains(field) {
                return Err(FormatError::MissingField {
                    object: name,
                    field,
                });
            }
        }
        Ok(Self { fields, name })
    }

    fn value(&self, key: &'static str) -> Result<&'a Value, FormatError> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
            .ok_or(FormatError::MissingField {
                object: self.name,
                field: key,
            })
    }
}

fn string_field(object: &StrictObject<'_>, field: &'static str) -> Result<String, FormatError> {
    object
        .value(field)?
        .as_str()
        .map(str::to_owned)
        .ok_or(FormatError::InvalidType(field))
}

fn count_field(object: &StrictObject<'_>, field: &'static str) -> Result<u64, FormatError> {
    let value = object
        .value(field)?
        .as_i64()
        .ok_or(FormatError::InvalidType(field))?;
    u64::try_from(value).map_err(|_| FormatError::InvalidValue {
        field,
        reason: "must be a positive integer",
    })
}

fn capability_fields(object: &StrictObject<'_>) -> Result<Vec<String>, FormatError> {
    object
        .value("capabilities")?
        .as_array()
        .ok_or(FormatError::InvalidType("capabilities"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(FormatError::InvalidType("capabilities"))
        })
        .collect()
}

fn validate_format(value: &Value) -> Result<(), FormatError> {
    let version = value
        .as_i64()
        .ok_or(FormatError::InvalidType("format_version"))?;
    if u64::try_from(version) == Ok(FORMAT_VERSION) {
        Ok(())
    } else {
        Err(FormatError::UnsupportedFormat(version))
    }
}

fn validate_text(value: &str, field: &'static str, maximum: usize) -> Result<(), FormatError> {
    if value.is_empty() {
        return Err(FormatError::InvalidValue {
            field,
            reason: "must not be empty",
        });
    }
    if value.len() > maximum {
        return Err(FormatError::InvalidValue {
            field,
            reason: "text is too long",
        });
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(FormatError::InvalidValue {
            field,
            reason: "must be trimmed single-line text",
        });
    }
    Ok(())
}

fn validate_version(value: &str, field: &'static str) -> Result<(), FormatError> {
    validate_text(value, field, MAX_VERSION_BYTES)?;
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        Ok(())
    } else {
        Err(FormatError::InvalidValue {
            field,
            reason: "must be an ASCII version token",
        })
    }
}

fn validate_cobalt_version(value: &str) -> Result<(), FormatError> {
    if parse_cobalt_version(value).is_some() {
        Ok(())
    } else {
        Err(FormatError::InvalidValue {
            field: "minimum_cobalt_version",
            reason: "must be dot-separated numeric components",
        })
    }
}

fn parse_cobalt_version(version: &str) -> Option<Vec<u64>> {
    if version.is_empty() || version.len() > MAX_VERSION_BYTES {
        return None;
    }
    version
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                part.parse::<u64>().ok()
            }
        })
        .collect()
}

fn validate_identifier(
    value: &str,
    field: &'static str,
    maximum: usize,
) -> Result<(), FormatError> {
    if value.is_empty() || value.len() > maximum {
        return Err(FormatError::InvalidValue {
            field,
            reason: "invalid length",
        });
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase()
        || bytes.last() == Some(&b'-')
        || bytes.windows(2).any(|pair| pair == b"--")
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err(FormatError::InvalidValue {
            field,
            reason: "must be a lowercase ASCII identifier",
        });
    }
    Ok(())
}

fn validate_capabilities(names: &[String]) -> Result<Declared, FormatError> {
    if names.len() > MAX_CAPABILITIES {
        return Err(FormatError::InvalidValue {
            field: "capabilities",
            reason: "too many capabilities",
        });
    }
    let mut seen = BTreeSet::new();
    for name in names {
        if name.is_empty() || name.len() > MAX_CAPABILITY_NAME_BYTES {
            return Err(FormatError::InvalidValue {
                field: "capabilities",
                reason: "invalid capability name length",
            });
        }
        if !seen.insert(name.as_str()) {
            return Err(FormatError::InvalidValue {
                field: "capabilities",
                reason: "duplicate capability",
            });
        }
    }
    Declared::parse(names.iter().map(String::as_str)).map_err(FormatError::from)
}

fn validate_count(value: u64, field: &'static str, maximum: u64) -> Result<(), FormatError> {
    if value == 0 || value > maximum {
        Err(FormatError::InvalidValue {
            field,
            reason: "byte count is outside its limit",
        })
    } else {
        Ok(())
    }
}

fn validate_package_url(value: &str) -> Result<(), FormatError> {
    if value.is_empty() || value.len() > MAX_PACKAGE_URL_BYTES || kobo_net::parse(value).is_err() {
        Err(FormatError::InvalidValue {
            field: "package_url",
            reason: "must be a valid bounded HTTPS URL without credentials",
        })
    } else {
        Ok(())
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_manifest(manifest: &Manifest, out: &mut String) {
    out.push_str("{\"format_version\":1,\"id\":");
    kobo_json::escape_into(&manifest.id, out);
    out.push_str(",\"display_name\":");
    kobo_json::escape_into(&manifest.display_name, out);
    out.push_str(",\"short_label\":");
    kobo_json::escape_into(&manifest.short_label, out);
    out.push_str(",\"summary\":");
    kobo_json::escape_into(&manifest.summary, out);
    out.push_str(",\"version\":");
    kobo_json::escape_into(&manifest.version, out);
    out.push_str(",\"minimum_cobalt_version\":");
    kobo_json::escape_into(&manifest.minimum_cobalt_version, out);
    out.push_str(",\"glyph\":");
    kobo_json::escape_into(&manifest.glyph, out);
    out.push_str(",\"capabilities\":[");
    for (index, capability) in manifest.capabilities().enumerate() {
        if index != 0 {
            out.push(',');
        }
        kobo_json::escape_into(capability, out);
    }
    out.push_str("],\"binary_sha256\":");
    kobo_json::escape_into(manifest.binary_sha256.as_str(), out);
    out.push_str(",\"binary_bytes\":");
    out.push_str(&manifest.binary_bytes.to_string());
    if let Some(quality) = &manifest.quality {
        write_quality(quality, out);
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobo_net::sha256::hex_digest;

    fn input(id: &str, binary: &[u8]) -> ManifestInput {
        ManifestInput {
            quality: None,
            id: id.to_owned(),
            display_name: "Daily Brief".to_owned(),
            short_label: "Brief".to_owned(),
            summary: "A concise daily briefing.".to_owned(),
            version: "1.2.3".to_owned(),
            minimum_cobalt_version: "0.1.9".to_owned(),
            glyph: "clock".to_owned(),
            capabilities: vec![
                "scheduled-wake".to_owned(),
                "network".to_owned(),
                "background-network".to_owned(),
            ],
            binary_sha256: hex_digest(binary),
            binary_bytes: binary.len() as u64,
        }
    }

    fn manifest(id: &str) -> Manifest {
        Manifest::new(input(id, b"binary"), &[]).expect("valid manifest")
    }

    fn entry(id: &str) -> CatalogEntry {
        CatalogEntry::new(CatalogEntryInput {
            manifest: manifest(id),
            package_url: format!("https://apps.example/{id}.cobalt"),
            package_sha256: hex_digest(id.as_bytes()),
            package_bytes: 100,
        })
        .expect("valid entry")
    }

    #[test]
    fn canonical_manifest_round_trip_normalizes_capability_order() {
        let manifest = manifest("daily-brief");
        let json = manifest.to_canonical_json();
        assert_eq!(Manifest::parse(json.as_bytes(), &[]), Ok(manifest));
        assert_eq!(
            json,
            format!(
                concat!(
                    r#"{{"format_version":1,"id":"daily-brief","display_name":"Daily Brief","#,
                    r#""short_label":"Brief","summary":"A concise daily briefing.","version":"1.2.3","#,
                    r#""minimum_cobalt_version":"0.1.9","glyph":"clock","capabilities":["network","#,
                    r#""background-network","scheduled-wake"],"binary_sha256":"{}","binary_bytes":6}}"#
                ),
                hex_digest(b"binary")
            )
        );
        assert_eq!(
            json,
            Manifest::parse(json.as_bytes(), &[])
                .unwrap()
                .to_canonical_json()
        );
    }

    #[test]
    fn public_applications_cannot_request_a_root_shell() {
        let mut requested = input("shell-tool", b"binary");
        requested.capabilities = vec!["shell".to_owned()];
        assert!(matches!(
            Manifest::new_public(requested.clone()),
            Err(FormatError::InvalidValue {
                field: "capabilities",
                ..
            })
        ));

        let manifest = Manifest::new(requested, &[]).expect("generic shell manifest");
        let catalog = Catalog::new(vec![CatalogEntry::new(CatalogEntryInput {
            manifest,
            package_url: "https://apps.example/shell-tool.cobalt-app".to_owned(),
            package_sha256: hex_digest(b"package"),
            package_bytes: 100,
        })
        .expect("entry")])
        .expect("catalog");
        assert!(matches!(
            Catalog::parse_public(&catalog.to_canonical_bytes()),
            Err(FormatError::InvalidValue {
                field: "capabilities",
                ..
            })
        ));
    }

    #[test]
    fn canonical_catalog_round_trip_is_deterministic() {
        let catalog = Catalog::new(vec![entry("brief"), entry("news")]).expect("catalog");
        let first = catalog.to_canonical_json();
        let second = catalog.to_canonical_json();
        assert_eq!(first, second);
        assert_eq!(Catalog::parse(first.as_bytes(), &[]), Ok(catalog));
    }

    fn quality_input() -> QualityInput {
        QualityInput {
            user: "A reader who keeps papers on their Kobo.".to_owned(),
            job: "Browse, keep, and read papers offline.".to_owned(),
            offline: "Every kept paper opens and reads with no network at all.".to_owned(),
            data: vec![DataKindInput {
                kind: "kept papers".to_owned(),
                location: "the app's own folder on the reader".to_owned(),
                export: "the original PDF bytes".to_owned(),
                on_remove: "retained".to_owned(),
            }],
            capabilities_required: vec![CapabilityPurposeInput {
                name: "network".to_owned(),
                purpose: "Search and download papers from the source.".to_owned(),
            }],
            capabilities_optional: vec![CapabilityGateInput {
                name: "scheduled-wake".to_owned(),
                gates: "checking for new papers overnight.".to_owned(),
            }],
            profiles: vec!["clara-bw-391".to_owned()],
            maintainer: "The Cobalt app maintainers.".to_owned(),
            support: "the issue tracker".to_owned(),
            non_goals: vec!["It does not sync a reading position between devices.".to_owned()],
        }
    }

    #[test]
    fn quality_block_round_trips_through_canonical_json() {
        let mut manifest_input = input("daily-brief", b"binary");
        manifest_input.quality = Some(quality_input());
        let manifest = Manifest::new(manifest_input, &[]).expect("valid manifest");
        let json = manifest.to_canonical_json();
        let parsed = Manifest::parse(json.as_bytes(), &[]).expect("round trip parses");
        assert_eq!(parsed, manifest);
        let quality = parsed.quality().expect("quality survives");
        assert_eq!(quality.data()[0].on_remove(), Retention::Retained);
        assert_eq!(quality.capabilities_required()[0].name(), "network");
        assert_eq!(
            quality.capabilities_optional()[0].gates(),
            "checking for new papers overnight."
        );
        assert!(json.contains("\"on_remove\":\"retained\""));
    }

    #[test]
    fn manifests_without_quality_still_parse() {
        let manifest = Manifest::new(input("daily-brief", b"binary"), &[]).expect("valid");
        let json = manifest.to_canonical_json();
        assert!(!json.contains("quality"));
        let parsed = Manifest::parse(json.as_bytes(), &[]).expect("parses");
        assert_eq!(parsed.quality(), None);
    }

    #[test]
    fn quality_rejects_an_unknown_retention_value() {
        let mut quality = quality_input();
        quality.data[0].on_remove = "archived".to_owned();
        let mut manifest_input = input("daily-brief", b"binary");
        manifest_input.quality = Some(quality);
        let error = Manifest::new(manifest_input, &[]).expect_err("archived is not retention");
        assert!(error.to_string().contains("on_remove"));
    }

    #[test]
    fn quality_capability_names_must_be_declared() {
        let mut quality = quality_input();
        quality.capabilities_required[0].name = "shell".to_owned();
        let mut manifest_input = input("daily-brief", b"binary");
        manifest_input.quality = Some(quality);
        let error = Manifest::new(manifest_input, &[]).expect_err("shell is not declared");
        assert!(error.to_string().contains("declared capabilities"));
    }

    #[test]
    fn quality_rejects_empty_profiles_and_empty_non_goals() {
        let mut quality = quality_input();
        quality.profiles = vec![];
        let mut manifest_input = input("daily-brief", b"binary");
        manifest_input.quality = Some(quality.clone());
        assert!(Manifest::new(manifest_input, &[]).is_err());
        quality.profiles = vec!["clara-bw-391".to_owned()];
        quality.non_goals = vec![];
        let mut manifest_input = input("daily-brief", b"binary");
        manifest_input.quality = Some(quality);
        assert!(Manifest::new(manifest_input, &[]).is_err());
    }

    #[test]
    fn quality_json_with_a_missing_field_is_rejected() {
        let mut manifest_input = input("daily-brief", b"binary");
        manifest_input.quality = Some(quality_input());
        let manifest = Manifest::new(manifest_input, &[]).expect("valid manifest");
        let json = manifest
            .to_canonical_json()
            .replace("\"maintainer\":\"The Cobalt app maintainers.\",", "");
        let error = Manifest::parse(json.as_bytes(), &[]).expect_err("missing maintainer");
        assert!(error.to_string().contains("maintainer"));
    }

    #[test]
    fn catalog_constructor_enforces_the_serialized_size_limit() {
        let entries = (0..MAX_CATALOG_ENTRIES)
            .map(|index| {
                let id = format!("app-{index:03}");
                let manifest = Manifest::new(
                    ManifestInput {
                        quality: None,
                        id: id.clone(),
                        display_name: "\"".repeat(MAX_DISPLAY_NAME_BYTES),
                        short_label: "\"".repeat(MAX_SHORT_LABEL_BYTES),
                        summary: "\"".repeat(MAX_SUMMARY_BYTES),
                        version: "v".repeat(MAX_VERSION_BYTES),
                        minimum_cobalt_version: "1.1.1.1.1.1.1.1.1.1".to_owned(),
                        glyph: "g".repeat(MAX_GLYPH_BYTES),
                        capabilities: kobo_policy::Capability::ALL
                            .iter()
                            .take(MAX_CAPABILITIES)
                            .map(ToString::to_string)
                            .collect(),
                        binary_sha256: "a".repeat(64),
                        binary_bytes: 1,
                    },
                    &[],
                )
                .expect("maximal manifest");
                let package_url_prefix = format!("https://example.test/{id}/");
                CatalogEntry::new(CatalogEntryInput {
                    manifest,
                    package_url: format!(
                        "{package_url_prefix}{}",
                        "a".repeat(MAX_PACKAGE_URL_BYTES - package_url_prefix.len())
                    ),
                    package_sha256: "b".repeat(64),
                    package_bytes: 1,
                })
                .expect("maximal entry")
            })
            .collect();
        assert!(matches!(
            Catalog::new(entries),
            Err(FormatError::DocumentTooLarge {
                maximum: MAX_CATALOG_BYTES
            })
        ));
    }

    #[test]
    fn minimum_cobalt_versions_use_the_runtime_comparison_grammar() {
        for invalid in ["1..0", "0.2.0-beta", ".2.0", "0.2."] {
            let mut requested = input("version-test", b"binary");
            requested.minimum_cobalt_version = invalid.to_owned();
            assert!(matches!(
                Manifest::new(requested, &[]),
                Err(FormatError::InvalidValue {
                    field: "minimum_cobalt_version",
                    ..
                })
            ));
        }
        assert!(cobalt_version_at_least("0.2.0", "0.1.9"));
        assert!(!cobalt_version_at_least("0.1.8", "0.1.9"));
        assert!(!cobalt_version_at_least("nightly", "0.1.9"));
    }

    #[test]
    fn parser_refuses_unknown_missing_and_duplicate_fields() {
        let json = manifest("brief").to_canonical_json();
        assert!(Manifest::parse(
            json.replace("\"id\":", "\"extra\":0,\"id\":").as_bytes(),
            &[]
        )
        .is_err());
        assert!(Manifest::parse(
            json.replace("\"summary\":\"A concise daily briefing.\",", "")
                .as_bytes(),
            &[]
        )
        .is_err());
        assert!(Manifest::parse(
            json.replace("\"id\":\"brief\",", "\"id\":\"brief\",\"id\":\"other\",")
                .as_bytes(),
            &[]
        )
        .is_err());
    }

    #[test]
    fn parser_refuses_unknown_formats_and_non_utf8() {
        let json = manifest("brief").to_canonical_json();
        assert_eq!(
            Manifest::parse(json.replacen(":1", ":2", 1).as_bytes(), &[]),
            Err(FormatError::UnsupportedFormat(2))
        );
        assert_eq!(Manifest::parse(&[0xff], &[]), Err(FormatError::InvalidUtf8));
    }

    #[test]
    fn ids_are_lowercase_pathless_and_reservable() {
        for id in ["Bad", "two--dashes", "trailing-", "../escape", "a_b"] {
            assert!(Manifest::new(input(id, b"x"), &[]).is_err(), "{id}");
        }
        assert_eq!(
            Manifest::new(input("private", b"x"), &["private"]),
            Err(FormatError::ReservedAppId("private".to_owned()))
        );
        assert!(is_public_reserved_app_id("launcher"));
        assert!(is_public_reserved_app_id("books"));
        assert!(Manifest::new_public(input("launcher", b"x")).is_err());
        assert!(Manifest::new_public(input("books", b"x")).is_err());
    }

    #[test]
    fn malformed_digests_and_counts_are_refused() {
        let mut bad = input("brief", b"x");
        bad.binary_sha256 = "A".repeat(64);
        assert!(Manifest::new(bad, &[]).is_err());
        let mut bad = input("brief", b"x");
        bad.binary_bytes = 0;
        assert!(Manifest::new(bad, &[]).is_err());
        let mut bad = input("brief", b"x");
        bad.binary_bytes = MAX_BINARY_BYTES + 1;
        assert!(Manifest::new(bad, &[]).is_err());

        let result = CatalogEntry::new(CatalogEntryInput {
            manifest: manifest("brief"),
            package_url: "https://apps.example/brief".to_owned(),
            package_sha256: "0".repeat(63),
            package_bytes: 1,
        });
        assert!(result.is_err());
    }

    #[test]
    fn malformed_urls_are_refused() {
        for url in [
            "http://apps.example/a",
            "https://",
            "https://user:password@apps.example/a",
            "not a url",
        ] {
            let result = CatalogEntry::new(CatalogEntryInput {
                manifest: manifest("brief"),
                package_url: url.to_owned(),
                package_sha256: "a".repeat(64),
                package_bytes: 1,
            });
            assert!(result.is_err(), "{url}");
        }
    }

    #[test]
    fn unknown_duplicate_and_incomplete_capabilities_are_refused() {
        let mut bad = input("brief", b"x");
        bad.capabilities = vec!["sudo".to_owned()];
        assert!(Manifest::new(bad, &[]).is_err());
        let mut bad = input("brief", b"x");
        bad.capabilities = vec!["network".to_owned(), "network".to_owned()];
        assert!(Manifest::new(bad, &[]).is_err());
        let mut bad = input("brief", b"x");
        bad.capabilities = vec!["background-network".to_owned()];
        assert!(Manifest::new(bad, &[]).is_err());
    }

    #[test]
    fn duplicate_catalog_ids_are_refused() {
        assert_eq!(
            Catalog::new(vec![entry("brief"), entry("brief")]),
            Err(FormatError::DuplicateAppId("brief".to_owned()))
        );
    }

    #[test]
    fn oversized_fields_lists_documents_and_counts_are_refused() {
        let mut bad = input("brief", b"x");
        bad.summary = "x".repeat(MAX_SUMMARY_BYTES + 1);
        assert!(Manifest::new(bad, &[]).is_err());
        let mut bad = input("brief", b"x");
        bad.capabilities = vec!["network".to_owned(); MAX_CAPABILITIES + 1];
        assert!(Manifest::new(bad, &[]).is_err());
        assert_eq!(
            Manifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1], &[]),
            Err(FormatError::DocumentTooLarge {
                maximum: MAX_MANIFEST_BYTES
            })
        );
        assert!(Catalog::new(vec![entry("brief"); MAX_CATALOG_ENTRIES + 1]).is_err());
        let result = CatalogEntry::new(CatalogEntryInput {
            manifest: manifest("brief"),
            package_url: "https://apps.example/a".to_owned(),
            package_sha256: "a".repeat(64),
            package_bytes: MAX_PACKAGE_BYTES + 1,
        });
        assert!(result.is_err());
    }
}
