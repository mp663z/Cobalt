//! What came back from a fetch, and what to tell the reader when nothing did.
//!
//! The runtime hands an application the body of a response and nothing else:
//! no status line, no `Content-Type`, no final address after redirects. So
//! the kind of document is read from the bytes, the way browsers sniff a
//! response that arrived without a type, and relative links are resolved
//! against the address that was asked for.

/// The most of a page that is read. Larger pages are cut here and say so;
/// a page this size is hundreds of screens on a reader.
pub const MAX_PAGE_BYTES: u32 = 2 * 1024 * 1024;

/// Sent as `Accept`. Only what this browser can set as pages.
pub const ACCEPT: &str = "text/html,application/xhtml+xml,text/plain;q=0.9";

/// What a response body turned out to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// HTML or XHTML: parsed and paged.
    Html,
    /// Text with no markup: shown as it is, line by line.
    Plain,
    /// Something this browser cannot show, named for the reader.
    Unsupported(&'static str),
}

/// Reads the kind of a response from its first bytes.
#[must_use]
pub fn sniff(body: &[u8]) -> Kind {
    const MAGIC: &[(&[u8], &str)] = &[
        (b"%PDF-", "a PDF"),
        (b"\x89PNG", "an image"),
        (b"\xFF\xD8\xFF", "an image"),
        (b"GIF8", "an image"),
        (b"RIFF", "a media file"),
        (b"PK\x03\x04", "a ZIP archive, or a document inside one"),
        (b"\x1F\x8B", "a compressed file"),
        (b"ID3", "an audio file"),
        (b"OggS", "a media file"),
        (b"\x00\x00\x00", "a media file"),
    ];
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    for (magic, name) in MAGIC {
        if body.starts_with(magic) {
            return Kind::Unsupported(name);
        }
    }
    let head = &body[..body.len().min(1024)];
    if head.contains(&0) {
        return Kind::Unsupported("a binary file");
    }
    let start = head
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |at| &head[at..]);
    let lower: Vec<u8> = start.iter().take(512).map(u8::to_ascii_lowercase).collect();
    let markup = [
        &b"<!doctype html"[..],
        b"<html",
        b"<head",
        b"<body",
        b"<title",
        b"<meta",
        b"<p",
        b"<div",
        b"<h1",
        b"<!--",
        b"<?xml",
        b"<table",
        b"<a ",
        b"<br",
        b"<script",
        b"<style",
    ];
    if markup.iter().any(|tag| lower.starts_with(tag)) {
        return Kind::Html;
    }
    if lower
        .windows(5)
        .any(|window| window == b"<html" || window == b"<body")
    {
        return Kind::Html;
    }
    // Everything else, JSON included, reads as text.
    Kind::Plain
}

/// Why a page did not arrive, in the reader's terms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Offline,
    Unreachable,
    TimedOut,
    NotFound,
    TooLarge,
    Denied,
    Refused,
    Busy { retry_after_seconds: u32 },
}

impl Failure {
    /// A heading and a sentence for the error screen.
    #[must_use]
    pub fn explain(self) -> (&'static str, String) {
        match self {
            Self::Offline => (
                "No Wi-Fi",
                "The reader is not connected. Join a network and try again.".into(),
            ),
            Self::Unreachable => (
                "The site did not answer",
                "The address may be wrong, or the site may be down. The reader's own connection is working.".into(),
            ),
            Self::TimedOut => (
                "The site took too long",
                "It may be slow or busy. Trying again often works.".into(),
            ),
            Self::NotFound => (
                "Page not found",
                "The site answered, but there is no page at this address.".into(),
            ),
            Self::TooLarge => (
                "Page too large",
                "This page is larger than the browser will read.".into(),
            ),
            Self::Denied => (
                "Browse cannot use the network",
                "Network access for Browse is turned off in Settings.".into(),
            ),
            Self::Refused => (
                "The site wants you to sign in",
                "This page is only shown to signed-in visitors, and Browse cannot sign in yet.".into(),
            ),
            Self::Busy { retry_after_seconds } => (
                "The site asked to wait",
                format!("It will answer again in about {}.", wait(retry_after_seconds)),
            ),
        }
    }

    /// Whether a Retry button could help.
    #[must_use]
    pub const fn worth_retrying(self) -> bool {
        matches!(
            self,
            Self::Offline | Self::Unreachable | Self::TimedOut | Self::Busy { .. }
        )
    }
}

fn wait(seconds: u32) -> String {
    match seconds {
        0..=1 => "a second".into(),
        2..=59 => format!("{seconds} seconds"),
        60 => "a minute".into(),
        _ => format!("{} minutes", seconds.div_ceil(60)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_html_wherever_the_page_starts() {
        for body in [
            &b"<!DOCTYPE html><html><body>hi"[..],
            b"\xEF\xBB\xBF<!doctype html>",
            b"\n\n  <html lang=en>",
            b"<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\">",
            b"<!-- generated --><html>",
            b"<p>A fragment of a page",
            b"Server banner\n<html><body>late start",
        ] {
            assert_eq!(
                sniff(body),
                Kind::Html,
                "{:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn text_without_markup_is_plain() {
        assert_eq!(sniff(b"RFC 9110\n\nHTTP Semantics"), Kind::Plain);
        assert_eq!(sniff(b"{\"ok\": true}"), Kind::Plain);
        assert_eq!(sniff(b""), Kind::Plain);
    }

    #[test]
    fn files_that_are_not_pages_are_named() {
        assert_eq!(sniff(b"%PDF-1.7\n..."), Kind::Unsupported("a PDF"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Kind::Unsupported("an image"));
        assert_eq!(
            sniff(b"\x1F\x8B\x08\x00"),
            Kind::Unsupported("a compressed file")
        );
        assert_eq!(sniff(b"abc\x00def"), Kind::Unsupported("a binary file"));
    }

    #[test]
    fn every_failure_has_words_and_only_some_retry() {
        let all = [
            Failure::Offline,
            Failure::Unreachable,
            Failure::TimedOut,
            Failure::NotFound,
            Failure::TooLarge,
            Failure::Denied,
            Failure::Refused,
            Failure::Busy {
                retry_after_seconds: 90,
            },
        ];
        for failure in all {
            let (heading, sentence) = failure.explain();
            assert!(
                !heading.is_empty() && sentence.ends_with('.'),
                "{failure:?}"
            );
            assert!(!sentence.contains('\u{2014}'), "{failure:?}");
        }
        assert!(Failure::TimedOut.worth_retrying());
        assert!(!Failure::NotFound.worth_retrying());
        assert!(!Failure::Denied.worth_retrying());
        assert_eq!(
            Failure::Busy {
                retry_after_seconds: 90
            }
            .explain()
            .1,
            "It will answer again in about 2 minutes."
        );
    }
}
