//! Pure, strict UTF-8 linked-stylesheet byte boundary. Not wired to resource
//! loading or the cascade. Embedded style elements are already Unicode and
//! must not pass through this byte-encoding detector.
//!
//! CSS Syntax 3 sections 3.2 and 8.3: BOM overrides fallback; protocol hints
//! precede the exact leading encoding signature, then environment, then UTF-8.
//! This profile supports only UTF-8, not all Web encoding labels/aliases.
//! The signature remains in decoded text. The scanner separately recognizes
//! only the exact leading UTF-8 signature; byte decoding does not remove it.

/// Existing per-sheet limit, applied before decoding or signature inspection.
pub const MAX_BYTES: usize = crate::style_syntax::MAX_BYTES;
const SIGNATURE_WINDOW: usize = 1024;

/// Caller-resolved encoding metadata. `ConfirmedUtf8` requires source evidence;
/// it must not be inferred from the fact that bytes happen to validate as UTF-8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingHint {
    Absent,
    ConfirmedUtf8,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkedContext {
    pub protocol: EncodingHint,
    pub environment: EncodingHint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingSource {
    Bom,
    Protocol,
    Signature,
    Environment,
    Default,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Budget,
    UnsupportedEncoding,
    InvalidUtf8,
}

/// Borrowed output: no stylesheet copy, fetching, replacement decoding, or
/// unknown-rule suppression. A leading UTF-8 BOM alone is removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedStylesheet<'a> {
    pub text: &'a str,
    pub source: EncodingSource,
    /// Exact byte-signature label if present, even if BOM/protocol wins.
    pub signature_label: Option<&'a str>,
}

/// Validate a linked stylesheet under the strict UTF-8-only profile.
/// Unsupported labels (including UTF-8 aliases) deliberately refuse when
/// selected; this is not a general implementation of Web encoding detection.
/// A signature must start at byte zero, use the exact ASCII `@charset "` prefix,
/// and end with `";` within the first 1024 bytes. Its label is case-insensitive.
/// Lookalikes remain literal CSS for the existing strict scanner to reject.
///
/// # Errors
/// Returns content-free errors for the existing byte budget, a selected
/// unsupported encoding/BOM, or invalid UTF-8. No partial text is returned.
pub fn decode_linked(
    bytes: &[u8],
    context: LinkedContext,
) -> Result<DecodedStylesheet<'_>, DecodeError> {
    if bytes.len() > MAX_BYTES {
        return Err(DecodeError::Budget);
    }
    let label = signature(bytes);
    let (body, source) = if let Some(body) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        (body, EncodingSource::Bom)
    } else if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(DecodeError::UnsupportedEncoding);
    } else {
        let source = match context.protocol {
            EncodingHint::ConfirmedUtf8 => EncodingSource::Protocol,
            EncodingHint::Unsupported => return Err(DecodeError::UnsupportedEncoding),
            EncodingHint::Absent => {
                if let Some(label) = label {
                    if !label.eq_ignore_ascii_case(b"utf-8") {
                        return Err(DecodeError::UnsupportedEncoding);
                    }
                    EncodingSource::Signature
                } else {
                    match context.environment {
                        EncodingHint::ConfirmedUtf8 => EncodingSource::Environment,
                        EncodingHint::Unsupported => return Err(DecodeError::UnsupportedEncoding),
                        EncodingHint::Absent => EncodingSource::Default,
                    }
                }
            }
        };
        (bytes, source)
    };
    let text = core::str::from_utf8(body).map_err(|_| DecodeError::InvalidUtf8)?;
    // The signature detector accepts ASCII only, so this conversion cannot
    // fail; keep it fallible rather than add a content-reachable panic.
    let signature_label = label
        .map(core::str::from_utf8)
        .transpose()
        .map_err(|_| DecodeError::InvalidUtf8)?;
    Ok(DecodedStylesheet {
        text,
        source,
        signature_label,
    })
}

fn signature(bytes: &[u8]) -> Option<&[u8]> {
    let window = &bytes[..bytes.len().min(SIGNATURE_WINDOW)];
    let label = window.strip_prefix(b"@charset \"")?;
    let end = label.iter().position(|&byte| byte == b'"')?;
    if label.get(end + 1) != Some(&b';') || !label[..end].is_ascii() {
        return None;
    }
    Some(&label[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    const ABSENT: LinkedContext = LinkedContext {
        protocol: EncodingHint::Absent,
        environment: EncodingHint::Absent,
    };

    #[test]
    fn exact_signature_metadata_is_preserved_for_scanner() {
        for label in ["utf-8", "UTF-8", "uTf-8"] {
            let css = format!("@charset \"{label}\";p{{color:red}}");
            let decoded = decode_linked(css.as_bytes(), ABSENT).unwrap();
            assert_eq!(decoded.text, css);
            assert_eq!(decoded.text.as_ptr(), css.as_ptr());
            assert_eq!(decoded.source, EncodingSource::Signature);
            assert_eq!(decoded.signature_label, Some(label));
            assert!(!crate::style_syntax::scan(decoded.text).unsupported_at_rule);
        }
    }

    #[test]
    fn lookalikes_are_not_byte_signatures() {
        for css in [
            " @charset \"utf-8\";",
            "/*x*/@charset \"utf-8\";",
            "@CHARSET \"utf-8\";",
            "@charset  \"utf-8\";",
            "@charset\t\"utf-8\";",
            "@charset 'utf-8';",
            "@charset \"utf-8\" ;",
            "@charset \"utf-8\"",
            "@charset \"ü\";",
            "p{}@charset \"utf-8\";",
        ] {
            let decoded = decode_linked(css.as_bytes(), ABSENT).unwrap();
            assert_eq!(decoded.text, css);
            assert_eq!(decoded.source, EncodingSource::Default);
            assert_eq!(decoded.signature_label, None);
        }
    }

    #[test]
    fn precedence_is_bom_protocol_signature_environment_default() {
        let unsupported = LinkedContext {
            protocol: EncodingHint::Unsupported,
            environment: EncodingHint::Unsupported,
        };
        let bom = decode_linked(b"\xef\xbb\xbf@charset \"windows-1252\";p{}", unsupported).unwrap();
        assert_eq!(bom.source, EncodingSource::Bom);
        assert_eq!(bom.text, "@charset \"windows-1252\";p{}");
        assert_eq!(bom.signature_label, None); // BOM means no byte-zero signature.
        let protocol = LinkedContext {
            protocol: EncodingHint::ConfirmedUtf8,
            environment: EncodingHint::Unsupported,
        };
        assert_eq!(
            decode_linked(b"@charset \"windows-1252\";", protocol)
                .unwrap()
                .source,
            EncodingSource::Protocol
        );
        let environment = LinkedContext {
            protocol: EncodingHint::Absent,
            environment: EncodingHint::Unsupported,
        };
        assert_eq!(
            decode_linked(b"@charset \"utf-8\";", environment)
                .unwrap()
                .source,
            EncodingSource::Signature
        );
        assert_eq!(
            decode_linked(b"p{}", environment),
            Err(DecodeError::UnsupportedEncoding)
        );
        assert_eq!(
            decode_linked(
                b"p{}",
                LinkedContext {
                    protocol: EncodingHint::Absent,
                    environment: EncodingHint::ConfirmedUtf8,
                }
            )
            .unwrap()
            .source,
            EncodingSource::Environment
        );
    }

    #[test]
    fn encoding_and_byte_budget_refuse_without_partial_output() {
        for label in [
            "windows-1252",
            "utf-16le",
            "utf-16be",
            "utf8",
            "",
            " utf-8 ",
        ] {
            let css = format!("@charset \"{label}\";");
            assert_eq!(
                decode_linked(css.as_bytes(), ABSENT),
                Err(DecodeError::UnsupportedEncoding)
            );
        }
        for bytes in [&b"\xff\xfe"[..], &b"\xfe\xff"[..]] {
            assert_eq!(
                decode_linked(bytes, ABSENT),
                Err(DecodeError::UnsupportedEncoding)
            );
        }
        for bytes in [&b"\xff"[..], &b"\xef\xbb\xbf\xff"[..]] {
            assert_eq!(decode_linked(bytes, ABSENT), Err(DecodeError::InvalidUtf8));
        }
        assert!(decode_linked(&vec![b' '; MAX_BYTES], ABSENT).is_ok());
        assert_eq!(
            decode_linked(&vec![b' '; MAX_BYTES + 1], ABSENT),
            Err(DecodeError::Budget)
        );
        assert_eq!(
            decode_linked(b"", ABSENT).unwrap().source,
            EncodingSource::Default
        );
    }

    #[test]
    fn signature_window_is_exact_and_does_not_scan_later_text() {
        for total in [1023, 1024, 1025] {
            let css = format!("@charset \"{}\";", "x".repeat(total - 12));
            assert_eq!(css.len(), total);
            assert_eq!(signature(css.as_bytes()).is_some(), total <= 1024);
        }
    }

    #[test]
    fn embedded_unicode_and_other_at_rules_keep_existing_refusals() {
        // Embedded CSS must go straight to the Unicode scanner, no byte hints.
        for css in ["@font-face{}p{}", "@import 'x';p{}"] {
            assert!(crate::style_syntax::scan(css).unsupported_at_rule);
        }
        assert!(crate::style_syntax::scan(&" ".repeat(MAX_BYTES + 1)).truncated);
    }
}
