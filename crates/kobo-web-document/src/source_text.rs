//! Text normalization shared with semantic conversion and an opt-in source
//! recorder for native colour work. Handles belong to the caller's same DOM.
//! This is not an authored-colour or semantic Inline representation yet.
use crate::dom::Handle;
use std::ops::Range;

pub(crate) fn collapse_keep_edges(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceSpan {
    pub(crate) bytes: Range<usize>,
    /// None for browser-generated text, never guessed from matching words.
    pub(crate) source: Option<Handle>,
}

/// A bounded paragraph-local text buffer. Byte ranges address normalized
/// UTF-8, not the original input. A collapsed boundary space belongs to the
/// first source that emitted it. A refused append leaves the buffer unchanged.
#[allow(dead_code)] // Opt-in foundation, not yet semantic Inline plumbing.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SourceText {
    pub(crate) text: String,
    pub(crate) spans: Vec<SourceSpan>,
}

#[allow(dead_code)]
impl SourceText {
    pub(crate) fn push(
        &mut self,
        input: &str,
        source: Option<Handle>,
        max_bytes: usize,
        max_spans: usize,
    ) -> bool {
        let collapsed = collapse_keep_edges(input);
        let collapsed = if self.text.ends_with(' ') && collapsed.starts_with(' ') {
            &collapsed[1..]
        } else {
            &collapsed
        };
        if collapsed.is_empty() {
            return true;
        }
        let Some(end) = self.text.len().checked_add(collapsed.len()) else {
            return false;
        };
        let merge = self.spans.last().is_some_and(|s| s.source == source);
        if end > max_bytes || (!merge && self.spans.len() >= max_spans) {
            return false;
        }
        let start = self.text.len();
        self.text.push_str(collapsed);
        if merge {
            self.spans.last_mut().unwrap().bytes.end = end;
        } else {
            self.spans.push(SourceSpan {
                bytes: start..end,
                source,
            });
        }
        true
    }

    /// Same Unicode trim rules as the existing semantic edge trim. Ranges
    /// crossing an edge are clipped; fully trimmed spans disappear.
    pub(crate) fn trim(&mut self) {
        let start = self.text.len() - self.text.trim_start().len();
        let end = self.text.trim_end().len().max(start);
        self.spans.retain_mut(|span| {
            let left = span.bytes.start.max(start);
            let right = span.bytes.end.min(end);
            if left >= right {
                return false;
            }
            span.bytes = left - start..right - start;
            true
        });
        self.text = self.text[start..end].to_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_small_partition_has_complete_nonoverlapping_utf8_provenance() {
        for left in ["", " ", "a", "a ", " é", "है", "\u{a0}", "\n\t"] {
            for right in ["", " ", "a", " a", "é ", "है", "\u{a0}", "\r\n"] {
                let mut text = SourceText::default();
                assert!(text.push(left, Some(1), 100, 8));
                assert!(text.push(right, Some(2), 100, 8));
                let mut expected = collapse_keep_edges(left);
                let next = collapse_keep_edges(right);
                expected.push_str(if expected.ends_with(' ') && next.starts_with(' ') {
                    &next[1..]
                } else {
                    &next
                });
                text.trim();
                assert_eq!(text.text, expected.trim());
                let mut at = 0;
                for span in &text.spans {
                    assert_eq!(span.bytes.start, at);
                    assert!(text.text.is_char_boundary(span.bytes.start));
                    assert!(text.text.is_char_boundary(span.bytes.end));
                    assert!(span.bytes.end > span.bytes.start);
                    at = span.bytes.end;
                }
                assert_eq!(at, text.text.len());
            }
        }
    }

    #[test]
    fn duplicate_text_and_collapsed_boundary_space_keep_distinct_sources() {
        let mut text = SourceText::default();
        assert!(text.push("  same \n", Some(17), 100, 8));
        assert!(text.push("\t same  ", Some(23), 100, 8));
        text.trim();
        assert_eq!(text.text, "same same");
        assert_eq!(
            text.spans,
            vec![
                SourceSpan {
                    bytes: 0..5,
                    source: Some(17)
                },
                SourceSpan {
                    bytes: 5..9,
                    source: Some(23)
                },
            ]
        );
    }

    #[test]
    fn unicode_offsets_nbsp_and_synthetic_text_are_not_guessed() {
        let mut text = SourceText::default();
        assert!(text.push("\tहै\u{a0}é ", Some(5), 100, 8));
        assert!(text.push(" Note ", None, 100, 8));
        text.trim();
        assert_eq!(text.text, "है\u{a0}é Note");
        assert_eq!(&text.text[text.spans[0].bytes.clone()], "है\u{a0}é ");
        assert_eq!(&text.text[text.spans[1].bytes.clone()], "Note");
        assert_eq!(text.spans[1].source, None);
    }

    #[test]
    fn limits_are_atomic_and_adjacent_same_source_merges() {
        let mut text = SourceText::default();
        assert!(text.push("one", Some(1), 6, 1));
        let before = text.clone();
        assert!(!text.push(" four", Some(1), 6, 1));
        assert_eq!(text, before);
        assert!(!text.push("two", Some(2), 6, 1));
        assert_eq!(text, before);
        assert!(text.push("two", Some(1), 6, 1));
        assert_eq!(text.text, "onetwo");
        assert_eq!(text.spans.len(), 1);
        assert_eq!(text.spans[0].bytes, 0..6);
    }

    #[test]
    fn empty_and_whitespace_only_have_no_dangling_ranges() {
        let mut text = SourceText::default();
        assert!(text.push("", Some(1), 0, 0));
        assert!(text.push(" \t\n", Some(1), 1, 1));
        text.trim();
        assert!(text.text.is_empty());
        assert!(text.spans.is_empty());
        assert!(text.push("x", Some(2), 1, 1));
        assert_eq!(text.spans[0].bytes, 0..1);
    }
}
