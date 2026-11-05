//! Pure admission for a strict inline CSS resource profile. Wired to the
//! DOM link arm for retained data: hrefs, so it is reachable from preview
//! static paint paths through `parse_style_tree`; not wired to navigation,
//! network loading, or shipping app screens. Unsupported representations
//! are not assertions that a Web URL is invalid.

/// The existing CSS scanner limit, not a new or increased resource budget.
pub const MAX_DECODED_BYTES: usize = crate::style_syntax::MAX_BYTES;
const MAX_HEADER_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    UnsupportedScheme,
    UnsupportedMedia,
    UnsupportedRepresentation,
    HeaderBudget,
    EncodedBudget,
    DecodedBudget,
    InvalidBase64,
    InvalidUtf8,
    InvalidCssByte,
    Allocation,
}

/// Per-resource sizes only. This does not authorize adding resources to a page
/// or bypass the existing two-sheet, input, DOM-loss, or aggregate memory gates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StylesheetAdmission {
    pub encoded_bytes: usize,
    pub decoded_bytes: usize,
}

/// Immutable UTF-8 bytes and admission facts. No source URL is retained.
#[derive(Debug, Eq, PartialEq)]
pub struct InlineStylesheet {
    css: String,
    admission: StylesheetAdmission,
}
impl InlineStylesheet {
    #[must_use]
    pub fn css(&self) -> &str {
        &self.css
    }
    #[must_use]
    pub const fn admission(&self) -> StylesheetAdmission {
        self.admission
    }
}

/// Decode only `data:text/css[;charset=utf-8];base64,...`. Header tokens are
/// ASCII case-insensitive; payload is canonical RFC4648 base64. No whitespace,
/// URL-escaped payload, fragments, other media/charsets/parameters or network
/// fallback is supported. Empty CSS and a UTF-8 BOM are retained unchanged.
///
/// # Errors
/// Returns a content-free refusal for unsupported syntax, invalid base64/UTF-8,
/// the existing per-sheet budget, integer overflow, or allocation failure.
pub fn decode(uri: &str) -> Result<InlineStylesheet, AdmissionError> {
    decode_with_max(uri, MAX_DECODED_BYTES)
}

/// [`decode`] with a caller-chosen decoded-byte budget (`decode` passes the
/// canonical `MAX_DECODED_BYTES`).
///
/// # Errors
/// As [`decode`].
pub fn decode_with_max(uri: &str, max_decoded: usize) -> Result<InlineStylesheet, AdmissionError> {
    let Some(scheme) = uri.get(..5) else {
        return Err(AdmissionError::UnsupportedScheme);
    };
    if !scheme.eq_ignore_ascii_case("data:") {
        return Err(AdmissionError::UnsupportedScheme);
    }
    // Search at most a bounded header; never scan an arbitrary payload to
    // discover a missing comma and never copy the encoded input.
    let header_window = &uri.as_bytes()[5..uri.len().min(5 + MAX_HEADER_BYTES + 1)];
    let Some(comma) = header_window.iter().position(|&b| b == b',') else {
        return Err(AdmissionError::HeaderBudget);
    };
    if comma > MAX_HEADER_BYTES {
        return Err(AdmissionError::HeaderBudget);
    }
    let header = uri
        .get(5..5 + comma)
        .ok_or(AdmissionError::UnsupportedRepresentation)?;
    let mut tokens = header.split(';');
    if !tokens
        .next()
        .is_some_and(|t| t.eq_ignore_ascii_case("text/css"))
    {
        return Err(AdmissionError::UnsupportedMedia);
    }
    let Some(mut encoding) = tokens.next() else {
        return Err(AdmissionError::UnsupportedRepresentation);
    };
    if encoding.eq_ignore_ascii_case("charset=utf-8") {
        encoding = tokens
            .next()
            .ok_or(AdmissionError::UnsupportedRepresentation)?;
    }
    if !encoding.eq_ignore_ascii_case("base64") || tokens.next().is_some() {
        return Err(AdmissionError::UnsupportedRepresentation);
    }
    let payload = &uri.as_bytes()[6 + comma..];
    let max_encoded = max_encoded_len(max_decoded).ok_or(AdmissionError::EncodedBudget)?;
    if payload.len() > max_encoded {
        return Err(AdmissionError::EncodedBudget);
    }
    if payload.len() % 4 != 0 {
        return Err(AdmissionError::InvalidBase64);
    }
    let padding = if payload.ends_with(b"==") {
        2
    } else {
        usize::from(payload.ends_with(b"="))
    };
    let decoded = decoded_len(payload.len(), padding).ok_or(AdmissionError::InvalidBase64)?;
    if decoded > max_decoded {
        return Err(AdmissionError::DecodedBudget);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(decoded)
        .map_err(|_| AdmissionError::Allocation)?;
    for (index, chunk) in payload.chunks_exact(4).enumerate() {
        let last = index == payload.len() / 4 - 1;
        let a = sextet(chunk[0])?;
        let b = sextet(chunk[1])?;
        let tail = if last { padding } else { 0 };
        let c = if tail == 2 { 0 } else { sextet(chunk[2])? };
        let d = if tail > 0 { 0 } else { sextet(chunk[3])? };
        if (tail == 2 && b & 15 != 0) || (tail == 1 && c & 3 != 0) {
            return Err(AdmissionError::InvalidBase64);
        }
        bytes.push((a << 2) | (b >> 4));
        if tail < 2 {
            bytes.push((b << 4) | (c >> 2));
        }
        if tail == 0 {
            bytes.push((c << 6) | d);
        }
    }
    if bytes.len() != decoded {
        return Err(AdmissionError::InvalidBase64);
    }
    if bytes.contains(&0) {
        return Err(AdmissionError::InvalidCssByte);
    }
    let css = String::from_utf8(bytes).map_err(|_| AdmissionError::InvalidUtf8)?;
    Ok(InlineStylesheet {
        css,
        admission: StylesheetAdmission {
            encoded_bytes: payload.len(),
            decoded_bytes: decoded,
        },
    })
}

fn max_encoded_len(decoded: usize) -> Option<usize> {
    decoded.checked_add(2)?.checked_div(3)?.checked_mul(4)
}
fn decoded_len(encoded: usize, padding: usize) -> Option<usize> {
    encoded.checked_div(4)?.checked_mul(3)?.checked_sub(padding)
}
fn sextet(byte: u8) -> Result<u8, AdmissionError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(AdmissionError::InvalidBase64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn size_arithmetic_is_checked_before_allocation() {
        assert_eq!(max_encoded_len(usize::MAX), None);
        assert_eq!(max_encoded_len(usize::MAX - 2), None);
        assert_eq!(decoded_len(0, 1), None);
        assert_eq!(max_encoded_len(0), Some(0));
        assert_eq!(max_encoded_len(MAX_DECODED_BYTES), Some(349_528));
    }
}
