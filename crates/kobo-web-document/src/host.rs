//! Host names as the URL standard reads them for `http` and `https`.
//!
//! Percent-escapes are decoded, forbidden characters refused, IPv4 addresses
//! written in any of the forms browsers accept are normalised to dotted
//! decimal, IPv6 addresses to their compressed form, and non-ASCII names
//! mapped and Punycode-encoded.
//!
//! The mapping is the part of UTS 46 real links need: case folding, the
//! full-width forms, the ideographic full stops and the characters it says to
//! ignore. Names needing the rest of its tables (compatibility forms beyond
//! full width, normalisation) are encoded as they are.

/// Parses the host of a special URL, returning it serialised.
pub(crate) fn parse(input: &str) -> Option<String> {
    if let Some(inner) = input.strip_prefix('[') {
        let inner = inner.strip_suffix(']')?;
        return ipv6(inner).map(|pieces| format!("[{}]", serialize_ipv6(pieces)));
    }
    let decoded = percent_decode(input)?;
    let mapped = map(&decoded);
    if mapped.is_empty() {
        return None;
    }
    // Characters UTS 46 disallows: the replacement character and the
    // noncharacters.
    if mapped.chars().any(|c| {
        c == '\u{FFFD}' || ('\u{FDD0}'..='\u{FDEF}').contains(&c) || u32::from(c) & 0xFFFE == 0xFFFE
    }) {
        return None;
    }
    let mut labels = Vec::new();
    for label in mapped.split('.') {
        if label.is_ascii() {
            labels.push(label.to_owned());
        } else {
            labels.push(format!("xn--{}", punycode(label)?));
        }
    }
    let ascii = labels.join(".");
    if ascii.chars().any(forbidden) {
        return None;
    }
    if ends_in_number(&ascii) {
        return ipv4(&ascii).map(|address| {
            let [a, b, c, d] = address.to_be_bytes();
            format!("{a}.{b}.{c}.{d}")
        });
    }
    Some(ascii)
}

fn forbidden(c: char) -> bool {
    c.is_ascii_control()
        || matches!(
            c,
            ' ' | '#' | '%' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
        )
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(u8::try_from(high * 16 + low).ok()?);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// The UTS 46 mapping, for the characters real links carry.
fn map(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            // Ignored.
            '\u{AD}'
            | '\u{200B}'
            | '\u{2060}'
            | '\u{FEFF}'
            | '\u{34F}'
            | '\u{180B}'..='\u{180D}'
            | '\u{FE00}'..='\u{FE0F}' => {}
            // Spaces, which are then refused.
            '\u{A0}' | '\u{3000}' => out.push(' '),
            // Full stops that separate labels.
            '\u{3002}' | '\u{FF0E}' | '\u{FF61}' => out.push('.'),
            // Full-width ASCII.
            '\u{FF01}'..='\u{FF5E}' => {
                let ascii = char::from_u32(u32::from(c) - 0xFF01 + 0x21).unwrap_or(c);
                out.push(ascii.to_ascii_lowercase());
            }
            c if c.is_ascii() => out.push(c.to_ascii_lowercase()),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// RFC 3492 Punycode, for one label.
fn punycode(label: &str) -> Option<String> {
    const BASE: u32 = 36;
    const T_MIN: u32 = 1;
    const T_MAX: u32 = 26;
    fn adapt(mut delta: u32, points: u32, first: bool) -> u32 {
        delta /= if first { 700 } else { 2 };
        delta += delta / points;
        let mut step = 0;
        while delta > ((BASE - T_MIN) * T_MAX) / 2 {
            delta /= BASE - T_MIN;
            step += BASE;
        }
        step + (BASE - T_MIN + 1) * delta / (delta + 38)
    }
    fn digit(d: u32) -> char {
        char::from_u32(if d < 26 {
            d + u32::from(b'a')
        } else {
            d - 26 + u32::from(b'0')
        })
        .unwrap_or('a')
    }
    let input: Vec<u32> = label.chars().map(u32::from).collect();
    let mut out: String = label.chars().filter(char::is_ascii).collect();
    let basic = u32::try_from(out.len()).ok()?;
    if basic > 0 {
        out.push('-');
    }
    let mut code: u32 = 0x80;
    let mut delta: u32 = 0;
    let mut bias: u32 = 72;
    let mut handled = basic;
    let total = u32::try_from(input.len()).ok()?;
    while handled < total {
        let smallest = input.iter().copied().filter(|&point| point >= code).min()?;
        delta = delta.checked_add((smallest - code).checked_mul(handled + 1)?)?;
        code = smallest;
        for &point in &input {
            if point < code {
                delta = delta.checked_add(1)?;
            }
            if point == code {
                let mut rest = delta;
                let mut step = BASE;
                loop {
                    let threshold = if step <= bias {
                        T_MIN
                    } else if step >= bias + T_MAX {
                        T_MAX
                    } else {
                        step - bias
                    };
                    if rest < threshold {
                        break;
                    }
                    out.push(digit(threshold + (rest - threshold) % (BASE - threshold)));
                    rest = (rest - threshold) / (BASE - threshold);
                    step += BASE;
                }
                out.push(digit(rest));
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        code += 1;
    }
    Some(out)
}

/// Whether the last label (ignoring one trailing dot) is a number, which
/// makes the whole host an IPv4 address or nothing.
fn ends_in_number(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let last = host.rsplit('.').next().unwrap_or("");
    !last.is_empty() && (last.bytes().all(|b| b.is_ascii_digit()) || ipv4_number(last).is_some())
}

fn ipv4_number(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let (digits, radix) =
        if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
            (hex, 16)
        } else if part.len() > 1 && part.starts_with('0') {
            (&part[1..], 8)
        } else {
            (part, 10)
        };
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some(0);
    }
    if !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    // Longer than any u32 in any radix: too big, whatever it says.
    if digits.len() > 32 {
        return Some(u64::MAX);
    }
    u64::from_str_radix(digits, radix).ok().or(Some(u64::MAX))
}

fn ipv4(host: &str) -> Option<u32> {
    let host = host.strip_suffix('.').unwrap_or(host);
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let numbers: Vec<u64> = parts
        .iter()
        .map(|part| ipv4_number(part))
        .collect::<Option<_>>()?;
    let (last, rest) = numbers.split_last()?;
    if rest.iter().any(|&n| n > 255) {
        return None;
    }
    let room = 5 - u32::try_from(numbers.len()).ok()?;
    if *last >= 256_u64.pow(room) {
        return None;
    }
    let mut address = *last;
    for (index, &n) in rest.iter().enumerate() {
        address += n * 256_u64.pow(3 - u32::try_from(index).ok()?);
    }
    u32::try_from(address).ok()
}

fn ipv6(input: &str) -> Option<[u16; 8]> {
    let mut pieces = [0_u16; 8];
    let chars: Vec<char> = input.chars().collect();
    let mut index = 0;
    let mut compress: Option<usize> = None;
    let mut at = 0;
    if chars.first() == Some(&':') {
        if chars.get(1) != Some(&':') {
            return None;
        }
        at += 2;
        index += 1;
        compress = Some(index);
    }
    while at < chars.len() {
        if index == 8 {
            return None;
        }
        if chars[at] == ':' {
            if compress.is_some() {
                return None;
            }
            at += 1;
            index += 1;
            compress = Some(index);
            continue;
        }
        let mut value: u32 = 0;
        let mut length = 0;
        while length < 4 && at < chars.len() {
            let Some(d) = chars[at].to_digit(16) else {
                break;
            };
            value = value * 16 + d;
            at += 1;
            length += 1;
        }
        if at < chars.len() && chars[at] == '.' {
            if length == 0 || index > 6 {
                return None;
            }
            embedded_ipv4(&chars[at - length..], &mut pieces[index..index + 2])?;
            index += 2;
            break;
        }
        if at < chars.len() && chars[at] == ':' {
            at += 1;
            if at >= chars.len() {
                return None;
            }
        } else if at < chars.len() {
            return None;
        }
        pieces[index] = u16::try_from(value).ok()?;
        index += 1;
    }
    if let Some(start) = compress {
        let mut swaps = index - start;
        index = 7;
        while index != 0 && swaps > 0 {
            pieces.swap(index, start + swaps - 1);
            index -= 1;
            swaps -= 1;
        }
    } else if index != 8 {
        return None;
    }
    Some(pieces)
}

/// The dotted IPv4 address that may end an IPv6 one, into two pieces.
fn embedded_ipv4(chars: &[char], pieces: &mut [u16]) -> Option<()> {
    let text: String = chars.iter().collect();
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut bytes = [0_u8; 4];
    for (byte, part) in bytes.iter_mut().zip(parts) {
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *byte = part.parse().ok()?;
    }
    pieces[0] = u16::from_be_bytes([bytes[0], bytes[1]]);
    pieces[1] = u16::from_be_bytes([bytes[2], bytes[3]]);
    Some(())
}

fn serialize_ipv6(pieces: [u16; 8]) -> String {
    // The first longest run of two or more zero pieces is written as `::`.
    let mut best: Option<(usize, usize)> = None;
    let mut run: Option<(usize, usize)> = None;
    for (i, &piece) in pieces.iter().enumerate() {
        if piece == 0 {
            run = Some(run.map_or((i, 1), |(start, len)| (start, len + 1)));
            if let Some((start, len)) = run {
                if len >= 2 && best.is_none_or(|(_, best_len)| len > best_len) {
                    best = Some((start, len));
                }
            }
        } else {
            run = None;
        }
    }
    let mut out = String::new();
    let mut i = 0;
    while i < 8 {
        if let Some((start, len)) = best {
            if i == start {
                out.push_str(if i == 0 { "::" } else { ":" });
                i += len;
                continue;
            }
        }
        out.push_str(&format!("{:x}", pieces[i]));
        if i < 7 {
            out.push(':');
        }
        i += 1;
    }
    out
}
