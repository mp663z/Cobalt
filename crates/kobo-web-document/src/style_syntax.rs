//! Bounded CSS rule structure, before selector or property interpretation.
//!
//! This scanner does not decide whether a conditional group applies, nor
//! whether a selector or declaration is valid. In particular it never turns
//! an unknown `@media` or `@supports` body into unconditional rules.

const MAX_BYTES: usize = 256 * 1024;
const MAX_RULES: usize = 4096;
const MAX_DEPTH: usize = 16;

/// A qualified rule and its enclosing conditional at-rules, in source order.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Rule<'a> {
    pub prelude: &'a str,
    pub declarations: &'a str,
    pub conditions: Vec<Condition<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Condition<'a> {
    pub name: &'a str,
    pub prelude: &'a str,
}

#[derive(Default)]
pub(crate) struct Sheet<'a> {
    pub rules: Vec<Rule<'a>>,
    pub truncated: bool,
    pub malformed: bool,
    /// Unhandled at-rules (including semicolon at-rules) make a visual
    /// render incomplete even when later ordinary rules remain parseable.
    pub unsupported_at_rule: bool,
}

/// Scan a UTF-8 sheet without evaluating at-rules or splitting declarations.
/// Oversized input is explicitly marked; callers must not present it as a
/// complete CSS render. All slices borrow `input` and require no copied CSS.
pub(crate) fn scan(input: &str) -> Sheet<'_> {
    let mut sheet = Sheet::default();
    if input.len() > MAX_BYTES {
        sheet.truncated = true;
        return sheet;
    }
    walk(input, 0, input.len(), &mut Vec::new(), &mut sheet);
    sheet
}

fn walk<'a>(
    input: &'a str,
    mut pos: usize,
    end: usize,
    conditions: &mut Vec<Condition<'a>>,
    sheet: &mut Sheet<'a>,
) {
    if conditions.len() > MAX_DEPTH {
        sheet.truncated = true;
        return;
    }
    let bytes = input.as_bytes();
    while pos < end {
        skip_space_and_comments(bytes, &mut pos, end, sheet);
        if pos >= end {
            break;
        }
        let start = pos;
        let Some((delimiter, at)) = next_delimiter(bytes, pos, end) else {
            sheet.malformed = true;
            break;
        };
        if delimiter == b';' {
            if input[start..at].trim_start().starts_with('@') {
                sheet.unsupported_at_rule = true;
            } else {
                sheet.malformed = true;
            }
            pos = at + 1;
            continue;
        }
        let header = input[start..at].trim();
        // A selector prelude containing an at-rule is not a selector. For
        // example, an unterminated @import followed by a block must not be
        // reinterpreted as a harmless unmatched element selector.
        if header.contains('@') && !header.starts_with('@') {
            sheet.unsupported_at_rule = true;
        }
        let Some(close) = close_brace(bytes, at + 1, end) else {
            sheet.malformed = true;
            break;
        };
        if let Some(at_rule) = header.strip_prefix('@') {
            // No @font-face, @keyframes or unknown block can contribute to
            // this renderer's computed output. The scanner still skips the
            // whole block so its children never leak as unconditional CSS.
            let split = at_rule
                .find(|ch: char| ch.is_ascii_whitespace() || ch == '(')
                .unwrap_or(at_rule.len());
            let name = &at_rule[..split];
            let prelude = at_rule[split..].trim();
            // These are the only grouping at-rules admitted to the structure.
            // The later evaluator still has to accept each condition explicitly.
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "media" | "supports" | "layer"
            ) {
                if conditions.len() == MAX_DEPTH {
                    sheet.truncated = true;
                } else {
                    conditions.push(Condition { name, prelude });
                    walk(input, at + 1, close, conditions, sheet);
                    conditions.pop();
                }
            } else {
                sheet.unsupported_at_rule = true;
            }
        } else if !header.is_empty() {
            if sheet.rules.len() == MAX_RULES {
                sheet.truncated = true;
                return;
            }
            sheet.rules.push(Rule {
                prelude: header,
                declarations: input[at + 1..close].trim(),
                conditions: conditions.clone(),
            });
        }
        pos = close + 1;
    }
}

fn skip_space_and_comments(bytes: &[u8], pos: &mut usize, end: usize, sheet: &mut Sheet<'_>) {
    while *pos < end {
        if bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        } else if bytes[*pos..end].starts_with(b"/*") {
            let Some(stop) = bytes[*pos + 2..end].windows(2).position(|w| w == b"*/") else {
                sheet.malformed = true;
                *pos = end;
                return;
            };
            *pos += stop + 4;
        } else {
            break;
        }
    }
}

/// Find an unquoted top-level opening brace or semicolon. Braces inside
/// functions, strings or comments cannot start a qualified rule.
fn next_delimiter(bytes: &[u8], mut pos: usize, end: usize) -> Option<(u8, usize)> {
    let mut quote = 0;
    let (mut parens, mut brackets) = (0_usize, 0_usize);
    while pos < end {
        let ch = bytes[pos];
        if ch == 92 {
            pos = (pos + 2).min(end);
            continue;
        }
        if quote != 0 {
            if ch == quote {
                quote = 0;
            }
        } else if bytes[pos..end].starts_with(b"/*") {
            let stop = bytes[pos + 2..end].windows(2).position(|w| w == b"*/")?;
            pos += stop + 4;
            continue;
        } else {
            match ch {
                39 | b'"' => quote = ch,
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b'{' | b';' if parens == 0 && brackets == 0 => return Some((ch, pos)),
                _ => {}
            }
        }
        pos += 1;
    }
    None
}

/// Closing brace of a declaration or at-rule body. Other braces are paired,
/// including braces inside unsupported nested rules.
fn close_brace(bytes: &[u8], mut pos: usize, end: usize) -> Option<usize> {
    let mut depth = 1_usize;
    let mut quote = 0;
    while pos < end {
        let ch = bytes[pos];
        if ch == 92 {
            pos = (pos + 2).min(end);
            continue;
        }
        if quote != 0 {
            if ch == quote {
                quote = 0;
            }
        } else if bytes[pos..end].starts_with(b"/*") {
            let stop = bytes[pos + 2..end].windows(2).position(|w| w == b"*/")?;
            pos += stop + 4;
            continue;
        } else {
            match ch {
                39 | b'"' => quote = ch,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                _ => {}
            }
        }
        pos += 1;
    }
    None
}

/// Validate a complete declaration block before it becomes paint authority.
/// The legacy splitter intentionally returns only parseable pairs, which is
/// useful for reader visibility but unsafe for all-or-nothing CSS painting.
/// Reject malformed or dropped statements and any silent 256-declaration cap.
pub(crate) fn declarations_complete(input: &str) -> bool {
    let mut statements = 0_usize;
    let bytes = input.as_bytes();
    let (mut start, mut quote, mut depth) = (0, 0_u8, 0_usize);
    let mut cursor = 0_usize;
    while cursor <= bytes.len() {
        if cursor == bytes.len() || (bytes[cursor] == b';' && quote == 0 && depth == 0) {
            let piece = input[start..cursor].trim();
            if !piece.is_empty() {
                // A comment-only segment is valid CSS, not a malformed
                // declaration. The reader splitter ignores it as well.
                if !crate::css::strip_comments(piece).trim().is_empty() {
                    statements += 1;
                    if statements > 256 || split_declaration(piece).is_none() {
                        return false;
                    }
                }
            }
            start = cursor + 1;
        } else if bytes[cursor] == 92 {
            if cursor + 1 >= bytes.len() {
                return false;
            }
            cursor += 2;
            continue;
        } else if quote != 0 {
            if bytes[cursor] == quote {
                quote = 0;
            }
        } else if bytes[cursor..].starts_with(b"/*") {
            let Some(end) = bytes[cursor + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
            else {
                return false;
            };
            cursor += end + 4;
            continue;
        } else {
            match bytes[cursor] {
                39 | 34 => quote = bytes[cursor],
                b'(' | b'[' => depth += 1,
                b')' | b']' => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    quote == 0 && depth == 0
}

/// Split declaration statements on semicolons outside quoted strings and functions.
pub(crate) fn declarations(input: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let (mut start, mut quote, mut depth) = (0, 0_u8, 0_usize);
    let mut cursor = 0;
    while cursor <= bytes.len() && out.len() < 256 {
        if cursor == bytes.len() || (bytes[cursor] == b';' && quote == 0 && depth == 0) {
            let piece = input[start..cursor].trim();
            if let Some((name, value)) = split_declaration(piece) {
                out.push((name, value));
            }
            start = cursor + 1;
        } else if bytes[cursor] == 92 {
            cursor = (cursor + 2).min(bytes.len());
            continue;
        } else if quote != 0 {
            if bytes[cursor] == quote {
                quote = 0;
            }
        } else if bytes[cursor..].starts_with(b"/*") {
            let Some(end) = bytes[cursor + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
            else {
                break;
            };
            cursor += end + 4;
            continue;
        } else {
            match bytes[cursor] {
                39 | 34 => quote = bytes[cursor],
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        cursor += 1;
    }
    out
}

fn split_declaration(piece: &str) -> Option<(&str, &str)> {
    let bytes = piece.as_bytes();
    let (mut quote, mut depth, mut cursor) = (0_u8, 0_usize, 0_usize);
    while cursor < bytes.len() {
        if bytes[cursor] == 92 {
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if quote != 0 {
            if bytes[cursor] == quote {
                quote = 0;
            }
        } else if bytes[cursor..].starts_with(b"/*") {
            let end = bytes[cursor + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")?;
            cursor += end + 4;
            continue;
        } else {
            match bytes[cursor] {
                39 | 34 => quote = bytes[cursor],
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b':' if depth == 0 => {
                    let (name, value) = piece.split_at(cursor);
                    let value = value[1..].trim();
                    let name = name.trim();
                    if !name.is_empty() && !value.is_empty() {
                        return Some((name, value));
                    }
                    return None;
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_functions_and_comments_do_not_break_rules() {
        let css = r#"/* { */ a[href="}"] { content: "a}b"; background: url("data:image/svg+xml,<svg>{}</svg>") } p { color: black }"#;
        let parsed = scan(css);
        assert!(!parsed.malformed);
        assert_eq!(parsed.rules.len(), 2);
        assert_eq!(parsed.rules[0].prelude, "a[href=\"}\"]");
        assert!(parsed.rules[0].declarations.contains("a}b"));
        assert_eq!(parsed.rules[1].prelude, "p");
    }

    #[test]
    fn conditional_rules_keep_conditions_in_source_order() {
        let parsed = scan("a{display:block}@media screen {b{display:none}@supports (display:grid){c{display:grid}}}d{color:red}");
        assert_eq!(
            parsed.rules.iter().map(|r| r.prelude).collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        assert_eq!(parsed.rules[1].conditions[0].name, "media");
        assert_eq!(parsed.rules[2].conditions[1].name, "supports");
        assert!(parsed.rules[3].conditions.is_empty());
    }

    #[test]
    fn unsupported_at_rules_do_not_leak_their_contents() {
        let parsed =
            scan("@font-face{font-family:x}@unknown thing{.bad{display:none}}.good{display:block}");
        assert_eq!(parsed.rules.len(), 1);
        assert_eq!(parsed.rules[0].prelude, ".good");
        assert!(parsed.unsupported_at_rule);
        assert!(scan("@import url('styles.css');p{display:block}").unsupported_at_rule);
        assert!(scan("@unknown thing; p{display:block}").unsupported_at_rule);
        assert!(scan("p @unknown thing {display:block}").unsupported_at_rule);
    }

    #[test]
    fn complete_declarations_reject_silent_drops() {
        assert!(declarations_complete("color:red; padding:1px 2px;"));
        assert!(declarations_complete(
            "/* comment */; color:red; /* tail */"
        ));
        assert!(declarations_complete("/* comment */"));
        assert!(!declarations_complete("/* comment */; broken; color:red"));
        assert!(declarations_complete("content:'a;b'; color:blue"));
        assert!(!declarations_complete("color:red; broken; width:10px"));
        assert!(!declarations_complete("color:red; broken:"));
        assert!(!declarations_complete("color:'unclosed"));
        assert!(!declarations_complete("color:red; /* unclosed"));
        assert!(!declarations_complete(&"color:red;".repeat(257)));
    }

    #[test]
    fn malformed_and_oversized_inputs_are_explicit() {
        assert!(scan("@media screen {p { color:red }").malformed);
        assert!(scan(&"p{}".repeat(70000)).truncated);
        assert!(scan(&"p{}".repeat(5000)).truncated);
    }
}
