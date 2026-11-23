//! The part of URL handling a reader browser needs.
//!
//! Absolute `https` and `http` URLs are parsed into their parts, relative
//! references are resolved against a base with the RFC 3986 algorithm, and
//! everything else is refused. A refused URL is not an error in a page: the
//! link is kept as inert text so a reader can see that it was there.

use std::fmt;

/// Longest URL accepted, in bytes. Longer ones are refused, not truncated,
/// because a truncated URL is a different address.
pub const MAX_URL_LEN: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Scheme {
    Https,
    Http,
}

impl Scheme {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Https => "https",
            Self::Http => "http",
        }
    }

    const fn default_port(self) -> u16 {
        match self {
            Self::Https => 443,
            Self::Http => 80,
        }
    }
}

/// A parsed absolute web address.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Url {
    scheme: Scheme,
    host: String,
    port: Option<u16>,
    /// Always starts with `/`.
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

/// Why a URL could not be used.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrlError {
    Empty,
    TooLong,
    /// `mailto:`, `javascript:`, `data:`, `ftp:` and anything else that is
    /// not a web page this browser can fetch.
    UnsupportedScheme,
    /// `user:password@` in the authority. Refused rather than stripped, as the
    /// runtime's HTTPS parser does.
    Credentials,
    InvalidHost,
    InvalidPort,
    /// A relative reference with nothing to resolve it against.
    NoBase,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "empty address",
            Self::TooLong => "address too long",
            Self::UnsupportedScheme => "not a web address",
            Self::Credentials => "address carries a password",
            Self::InvalidHost => "invalid host",
            Self::InvalidPort => "invalid port",
            Self::NoBase => "relative address with no page to resolve it against",
        })
    }
}

impl std::error::Error for UrlError {}

impl Url {
    /// Parses an absolute URL.
    ///
    /// # Errors
    ///
    /// Anything that is not an absolute `http` or `https` URL with a valid
    /// host.
    pub fn parse(input: &str) -> Result<Self, UrlError> {
        let input = clean(input)?;
        let (scheme, rest) = split_scheme(&input).ok_or(UrlError::NoBase)?;
        let scheme = match scheme.to_ascii_lowercase().as_str() {
            "https" => Scheme::Https,
            "http" => Scheme::Http,
            _ => return Err(UrlError::UnsupportedScheme),
        };
        let rest = rest.trim_start_matches(['/', '\\']);
        let authority_end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);
        let (host, port) = parse_authority(authority, scheme)?;
        let (path, query, fragment) = split_tail(tail);
        Ok(Self {
            scheme,
            host,
            port,
            path: normalize_path(if path.is_empty() { "/" } else { &path }),
            query,
            fragment,
        })
    }

    /// Resolves a reference found in a page against this URL.
    ///
    /// # Errors
    ///
    /// The reference names an unsupported scheme or is otherwise unusable.
    pub fn join(&self, reference: &str) -> Result<Self, UrlError> {
        let reference = match clean(reference) {
            // An empty reference is the page itself.
            Err(UrlError::Empty) => return Ok(self.without_fragment()),
            other => other?,
        };
        if let Some((scheme, rest)) = split_scheme(&reference) {
            // `http:g` against an http page is relative, as is `http:/g`:
            // only `http://` starts a new authority.
            if scheme.eq_ignore_ascii_case(self.scheme.as_str())
                && !rest.starts_with("//")
                && !rest.starts_with("\\\\")
                && !rest.starts_with("/\\")
                && !rest.starts_with("\\/")
            {
                return if rest.is_empty() {
                    Ok(self.without_fragment())
                } else {
                    self.join(rest)
                };
            }
            if scheme.eq_ignore_ascii_case(self.scheme.as_str())
                || scheme.eq_ignore_ascii_case("https")
                || scheme.eq_ignore_ascii_case("http")
            {
                return Self::parse(&reference);
            }
            return Err(UrlError::UnsupportedScheme);
        }
        if reference.starts_with("//") || reference.starts_with("\\\\") {
            return Self::parse(&format!("{}:{reference}", self.scheme.as_str()));
        }
        let (path, query, fragment) = split_tail(&reference);
        let mut next = self.clone();
        next.fragment = fragment;
        if path.is_empty() {
            if query.is_some() {
                next.query = query;
            }
            return Ok(next);
        }
        next.query = query;
        next.path = if path.starts_with('/') {
            normalize_path(&path)
        } else {
            let directory = &self.path[..=self.path.rfind('/').unwrap_or(0)];
            normalize_path(&format!("{directory}{path}"))
        };
        Ok(next)
    }

    #[must_use]
    pub const fn scheme(&self) -> Scheme {
        self.scheme
    }

    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        self.port
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    #[must_use]
    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    /// The same address without its `#fragment`, which is what is fetched.
    #[must_use]
    pub fn without_fragment(&self) -> Self {
        Self {
            fragment: None,
            ..self.clone()
        }
    }

    /// The same address with `https` in place of `http`.
    ///
    /// The runtime fetches over HTTPS only, so a plain `http` link is tried
    /// over HTTPS rather than refused. A server without TLS then fails as
    /// unreachable, which is the truth.
    #[must_use]
    pub fn upgraded(&self) -> Self {
        let port = match (self.scheme, self.port) {
            (Scheme::Http, Some(80)) => None,
            (_, port) => port,
        };
        Self {
            scheme: Scheme::Https,
            port,
            ..self.clone()
        }
    }

    /// Scheme, host and port, which is what "same site" means for sessions
    /// and redirects.
    #[must_use]
    pub fn origin(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme.as_str(), self.host),
            None => format!("{}://{}", self.scheme.as_str(), self.host),
        }
    }

    /// Whether two addresses name the same document, ignoring fragments.
    #[must_use]
    pub fn same_document(&self, other: &Self) -> bool {
        self.without_fragment() == other.without_fragment()
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.origin())?;
        f.write_str(&self.path)?;
        if let Some(query) = &self.query {
            write!(f, "?{query}")?;
        }
        if let Some(fragment) = &self.fragment {
            write!(f, "#{fragment}")?;
        }
        Ok(())
    }
}

/// Trims what browsers trim and refuses what they would not fetch.
///
/// Leading and trailing controls and spaces go, and tabs and newlines inside
/// are removed. Nothing is encoded yet: each part of the address has its own
/// rule, applied once the parts are known.
fn clean(input: &str) -> Result<String, UrlError> {
    let trimmed = input.trim_matches(|c: char| c <= ' ');
    if trimmed.is_empty() {
        return Err(UrlError::Empty);
    }
    if trimmed.len() > MAX_URL_LEN {
        return Err(UrlError::TooLong);
    }
    Ok(trimmed
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect())
}

/// Percent-encodes `text` as UTF-8: controls, everything outside ASCII, and
/// the ASCII characters `also` names. Escapes already present are kept.
fn encode(text: &str, also: &[char]) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() && !c.is_ascii_control() && !also.contains(&c) {
            out.push(c);
        } else {
            let mut buffer = [0; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// What the URL standard encodes in each part, beyond controls and
/// non-ASCII.
const FRAGMENT_SET: &[char] = &[' ', '"', '<', '>', '`'];
const QUERY_SET: &[char] = &[' ', '"', '#', '<', '>', '\''];
const PATH_SET: &[char] = &[' ', '"', '#', '<', '>', '?', '^', '`', '{', '}'];

/// Splits `scheme:rest` when the prefix is a valid scheme.
fn split_scheme(input: &str) -> Option<(&str, &str)> {
    let colon = input.find(':')?;
    let scheme = &input[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    Some((scheme, &input[colon + 1..]))
}

fn parse_authority(authority: &str, scheme: Scheme) -> Result<(String, Option<u16>), UrlError> {
    if authority.contains('@') {
        return Err(UrlError::Credentials);
    }
    let port_at = if authority.starts_with('[') {
        authority.find(']').map_or(authority.len(), |end| end + 1)
    } else {
        authority.find(':').unwrap_or(authority.len())
    };
    let (host, port) = authority.split_at(port_at);
    let host = crate::host::parse(host).ok_or(UrlError::InvalidHost)?;
    let host = host.trim_end_matches('.').to_owned();
    if host.is_empty() || host.len() > 253 {
        return Err(UrlError::InvalidHost);
    }
    let port = match port.strip_prefix(':') {
        None if port.is_empty() => None,
        None => return Err(UrlError::InvalidHost),
        Some("") => None,
        Some(digits) => {
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(UrlError::InvalidPort);
            }
            let port: u16 = digits.parse().map_err(|_| UrlError::InvalidPort)?;
            (port != scheme.default_port()).then_some(port)
        }
    };
    Ok((host, port))
}

/// Splits `path?query#fragment`.
fn split_tail(tail: &str) -> (String, Option<String>, Option<String>) {
    let (rest, fragment) = match tail.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment.to_owned())),
        None => (tail, None),
    };
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (rest, None),
    };
    (
        encode(&path.replace('\\', "/"), PATH_SET),
        query.map(|query| encode(&query, QUERY_SET)),
        fragment.map(|fragment| encode(&fragment, FRAGMENT_SET)),
    )
}

/// Removes `.` and `..` segments (RFC 3986 section 5.2.4).
fn normalize_path(path: &str) -> String {
    let mut output: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        match *segment {
            "." | "%2e" | "%2E" => {
                if index == last {
                    output.push("");
                }
            }
            ".." | ".%2e" | ".%2E" | "%2e." | "%2E." | "%2e%2e" | "%2E%2E" => {
                output.pop();
                if index == last {
                    output.push("");
                }
            }
            other => output.push(other),
        }
    }
    let mut result = String::with_capacity(path.len());
    for segment in output {
        result.push('/');
        result.push_str(segment);
    }
    if result.is_empty() {
        result.push('/');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://a.example/b/c/d;p?q").unwrap()
    }

    #[test]
    fn rfc3986_normal_examples() {
        let cases = [
            ("g", "https://a.example/b/c/g"),
            ("./g", "https://a.example/b/c/g"),
            ("g/", "https://a.example/b/c/g/"),
            ("/g", "https://a.example/g"),
            ("//g", "https://g/"),
            ("?y", "https://a.example/b/c/d;p?y"),
            ("g?y", "https://a.example/b/c/g?y"),
            ("#s", "https://a.example/b/c/d;p?q#s"),
            ("g#s", "https://a.example/b/c/g#s"),
            (";x", "https://a.example/b/c/;x"),
            ("", "https://a.example/b/c/d;p?q"),
            (".", "https://a.example/b/c/"),
            ("./", "https://a.example/b/c/"),
            ("..", "https://a.example/b/"),
            ("../", "https://a.example/b/"),
            ("../g", "https://a.example/b/g"),
            ("../..", "https://a.example/"),
            ("../../g", "https://a.example/g"),
            ("../../../g", "https://a.example/g"),
            ("/./g", "https://a.example/g"),
            ("/../g", "https://a.example/g"),
            ("g.", "https://a.example/b/c/g."),
            ("..g", "https://a.example/b/c/..g"),
            ("./../g", "https://a.example/b/g"),
            ("g/./h", "https://a.example/b/c/g/h"),
            ("g/../h", "https://a.example/b/c/h"),
        ];
        for (reference, expected) in cases {
            let joined = if reference.is_empty() {
                base()
            } else {
                base().join(reference).unwrap()
            };
            assert_eq!(joined.to_string(), expected, "{reference}");
        }
    }

    #[test]
    fn refuses_what_it_cannot_fetch() {
        for bad in [
            "javascript:alert(1)",
            "mailto:a@b.c",
            "data:text/html,x",
            "ftp://x/",
        ] {
            assert_eq!(base().join(bad), Err(UrlError::UnsupportedScheme), "{bad}");
        }
        assert_eq!(
            Url::parse("https://u:p@x.example/"),
            Err(UrlError::Credentials)
        );
        assert_eq!(
            Url::parse("https://x.example:99999/"),
            Err(UrlError::InvalidPort)
        );
        assert_eq!(Url::parse("https:///"), Err(UrlError::InvalidHost));
        assert_eq!(Url::parse("/relative"), Err(UrlError::NoBase));
        assert_eq!(Url::parse("   "), Err(UrlError::Empty));
    }

    #[test]
    fn normalizes_case_default_port_and_whitespace() {
        let url = Url::parse("  HTTPS://Example.COM:443/a b\n/c?x#y ").unwrap();
        assert_eq!(url.to_string(), "https://example.com/a%20b/c?x#y");
        assert_eq!(
            Url::parse("http://x.example:80").unwrap().to_string(),
            "http://x.example/"
        );
        assert_eq!(
            Url::parse("http://x.example:8080/")
                .unwrap()
                .upgraded()
                .to_string(),
            "https://x.example:8080/"
        );
        assert_eq!(
            Url::parse("http://x.example/")
                .unwrap()
                .upgraded()
                .to_string(),
            "https://x.example/"
        );
    }

    #[test]
    fn non_ascii_is_percent_encoded() {
        let url = Url::parse("https://x.example/café").unwrap();
        assert_eq!(url.path(), "/caf%C3%A9");
    }

    #[test]
    fn same_document_ignores_fragment() {
        let a = Url::parse("https://x.example/p#one").unwrap();
        let b = Url::parse("https://x.example/p#two").unwrap();
        assert!(a.same_document(&b));
        assert!(!a.same_document(&Url::parse("https://x.example/q").unwrap()));
    }
}
