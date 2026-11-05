//! What the reader typed into the address field, and where it goes.
//!
//! One field does both jobs, as it does on every browser a reader has used:
//! something that looks like an address is opened, anything else is searched
//! for. The rules are deliberately few, because the reader cannot see them.

use kobo_web_document::Url;

/// The search used when nothing else is configured.
///
/// `DuckDuckGo`'s HTML endpoint answers a plain GET with a page of results
/// that needs no script, which is the only kind this browser can read.
pub const DEFAULT_SEARCH: &str = "https://html.duckduckgo.com/html/?q=";

/// Where a typed entry leads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Address {
    /// An address, opened as typed, with `https://` supplied when no scheme
    /// was.
    Page(Url),
    /// Anything else: the words, and the search results page for them.
    Search { query: String, url: Url },
}

impl Address {
    #[must_use]
    pub fn url(&self) -> &Url {
        match self {
            Self::Page(url) | Self::Search { url, .. } => url,
        }
    }
}

/// Decides what `typed` means. `search` is a URL prefix the query is
/// appended to, encoded, such as [`DEFAULT_SEARCH`].
///
/// Returns `None` for an empty entry, and for a search prefix that is not
/// itself an address this browser can open.
#[must_use]
pub fn resolve(typed: &str, search: &str) -> Option<Address> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    if !typed.contains(char::is_whitespace) {
        if let Some(url) = as_address(typed) {
            return Some(Address::Page(url));
        }
    }
    let url = Url::parse(&format!("{search}{}", encode_query(typed))).ok()?;
    Some(Address::Search {
        query: typed.to_owned(),
        url,
    })
}

fn as_address(typed: &str) -> Option<Url> {
    let lower = typed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Url::parse(typed).ok();
    }
    let authority = typed.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    // `mailto:someone` and `javascript:alert(1)` have a colon too; a port is
    // digits and nothing else.
    if port.is_some_and(|port| port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit())) {
        return None;
    }
    let looks_like_host = host.eq_ignore_ascii_case("localhost")
        || (host.contains('.')
            && !host.starts_with('.')
            && !host.ends_with('.')
            && host
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '.' | '-')));
    if !looks_like_host {
        return None;
    }
    Url::parse(&format!("https://{typed}")).ok()
}

/// Encodes a query the way an HTML form does: spaces as `+`, everything
/// outside the unreserved set as `%XX` of its UTF-8 bytes.
#[must_use]
pub fn encode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(typed: &str) -> String {
        match resolve(typed, DEFAULT_SEARCH) {
            Some(Address::Page(url)) => url.to_string(),
            other => panic!("{typed:?} did not open as a page: {other:?}"),
        }
    }

    fn searched(typed: &str) -> String {
        match resolve(typed, DEFAULT_SEARCH) {
            Some(Address::Search { query, url }) => {
                assert_eq!(query, typed.trim());
                url.to_string()
            }
            other => panic!("{typed:?} was not searched for: {other:?}"),
        }
    }

    #[test]
    fn an_address_opens_with_https_supplied() {
        assert_eq!(page("example.com"), "https://example.com/");
        assert_eq!(
            page("en.wikipedia.org/wiki/E_Ink"),
            "https://en.wikipedia.org/wiki/E_Ink"
        );
        assert_eq!(page("  docs.rs  "), "https://docs.rs/");
        assert_eq!(page("localhost:8080/a"), "https://localhost:8080/a");
    }

    #[test]
    fn a_typed_scheme_is_kept() {
        assert_eq!(page("http://example.com/x"), "http://example.com/x");
        assert_eq!(page("HTTPS://Example.com"), "https://example.com/");
    }

    #[test]
    fn words_are_searched_for() {
        assert_eq!(
            searched("e ink refresh"),
            "https://html.duckduckgo.com/html/?q=e+ink+refresh"
        );
        assert_eq!(searched("rust"), "https://html.duckduckgo.com/html/?q=rust");
        assert_eq!(
            searched("what is 2+2?"),
            "https://html.duckduckgo.com/html/?q=what+is+2%2B2%3F"
        );
        assert_eq!(
            searched("café"),
            "https://html.duckduckgo.com/html/?q=caf%C3%A9"
        );
    }

    #[test]
    fn other_schemes_are_never_opened() {
        for typed in [
            "javascript:alert(1)",
            "mailto:someone@example.com",
            "file:///etc/passwd",
            "data:text/html,hi",
        ] {
            searched(typed);
        }
    }

    #[test]
    fn nothing_typed_goes_nowhere() {
        assert_eq!(resolve("   ", DEFAULT_SEARCH), None);
        assert_eq!(resolve("words", "not a url "), None);
    }
}
