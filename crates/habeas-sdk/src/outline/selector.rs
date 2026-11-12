//! Outline selector syntax: `/`-separated `kind[name]` or `kind@hash` segments.

use std::fmt;

/// Number of identity-hash hex digits in an anonymous segment.
pub const HASH_DIGITS: usize = 12;

/// Percent-encodes `%`, `[`, `]`, `/`, `#`, `@`, and whitespace as UTF-8 bytes.
pub fn encode_name(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for character in name.chars() {
        if matches!(character, '%' | '[' | ']' | '/' | '#' | '@') || character.is_whitespace() {
            let mut buffer = [0; 4];
            for byte in character.encode_utf8(&mut buffer).bytes() {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        } else {
            encoded.push(character);
        }
    }
    encoded
}

// {impl}`requirements.references.outline_selectors`
/// Reverses [`encode_name`], rejecting malformed escapes and unescaped
/// delimiters.
pub fn decode_name(encoded: &str) -> Result<String, SelectorError> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        match bytes[position] {
            b'%' => {
                let hex = encoded
                    .get(position + 1..position + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                    .ok_or_else(|| SelectorError::new(encoded, "malformed percent escape"))?;
                decoded.push(hex);
                position += 3;
            }
            b'[' | b']' | b'/' | b'#' | b'@' => {
                return Err(SelectorError::new(
                    encoded,
                    "delimiter characters in names must be percent-encoded",
                ));
            }
            byte => {
                decoded.push(byte);
                position += 1;
            }
        }
    }
    if encoded.chars().any(char::is_whitespace) {
        return Err(SelectorError::new(
            encoded,
            "whitespace in names must be percent-encoded",
        ));
    }
    String::from_utf8(decoded)
        .map_err(|_| SelectorError::new(encoded, "percent escapes are not UTF-8"))
}

// {impl}`requirements.references.outline_selectors`
/// A segment's label: a name, made of `/`-joined parts, or an identity hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Label {
    Named(Vec<String>),
    Hash(String),
}

/// One parsed selector segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub kind: String,
    pub label: Label,
    pub ordinal: Option<usize>,
}

impl Segment {
    /// The segment without its `#N` ordinal.
    pub fn base(kind: &str, label: &Label) -> String {
        match label {
            Label::Named(parts) => format!(
                "{kind}[{}]",
                parts
                    .iter()
                    .map(|part| encode_name(part))
                    .collect::<Vec<_>>()
                    .join("/")
            ),
            Label::Hash(hash) => format!("{kind}@{hash}"),
        }
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&Self::base(&self.kind, &self.label))?;
        if let Some(ordinal) = self.ordinal {
            write!(formatter, "#{ordinal}")?;
        }
        Ok(())
    }
}

// {cite}`habeas:util`
/// An invalid selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorError {
    pub selector: String,
    pub message: String,
}

impl SelectorError {
    // {cite}`habeas:util`
    pub fn new(selector: &str, message: impl Into<String>) -> Self {
        Self {
            selector: selector.to_owned(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SelectorError {
    // {cite}`habeas:util`
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid outline selector {:?}: {}",
            self.selector, self.message
        )
    }
}

impl std::error::Error for SelectorError {}

// {impl}`requirements.references.outline_selectors`
/// Splits a selector at `/` outside brackets.
fn split_segments(selector: &str) -> Result<Vec<&str>, SelectorError> {
    let mut segments = Vec::new();
    let mut depth = false;
    let mut start = 0;
    for (index, character) in selector.char_indices() {
        match character {
            '[' if depth => return Err(SelectorError::new(selector, "nested `[`")),
            '[' => depth = true,
            ']' if !depth => return Err(SelectorError::new(selector, "unmatched `]`")),
            ']' => depth = false,
            '/' if !depth => {
                segments.push(&selector[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth {
        return Err(SelectorError::new(selector, "unclosed `[`"));
    }
    segments.push(&selector[start..]);
    Ok(segments)
}

/// Parses one segment.
fn parse_segment(selector: &str, segment: &str) -> Result<Segment, SelectorError> {
    let kind_end = segment
        .find(|character: char| !(character.is_ascii_alphanumeric() || "_-".contains(character)))
        .unwrap_or(segment.len());
    let kind = &segment[..kind_end];
    if kind.is_empty() {
        return Err(SelectorError::new(
            selector,
            format!("segment {segment:?} has no kind"),
        ));
    }
    let rest = &segment[kind_end..];
    let (label, rest) = if let Some(rest) = rest.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| SelectorError::new(selector, "unclosed `[`"))?;
        let parts = rest[..close]
            .split('/')
            .map(decode_name)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| SelectorError::new(selector, error.message))?;
        (Label::Named(parts), &rest[close + 1..])
    } else if let Some(rest) = rest.strip_prefix('@') {
        let digits = rest
            .find(|character: char| !character.is_ascii_hexdigit())
            .unwrap_or(rest.len());
        if digits != HASH_DIGITS {
            return Err(SelectorError::new(
                selector,
                format!("segment {segment:?} must have {HASH_DIGITS} hex digits after `@`"),
            ));
        }
        (
            Label::Hash(rest[..digits].to_ascii_lowercase()),
            &rest[digits..],
        )
    } else {
        return Err(SelectorError::new(
            selector,
            format!("segment {segment:?} must be `kind[name]` or `kind@hash`"),
        ));
    };
    let ordinal = if rest.is_empty() {
        None
    } else {
        let ordinal = rest
            .strip_prefix('#')
            .and_then(|digits| digits.parse::<usize>().ok())
            .filter(|ordinal| *ordinal > 0)
            .ok_or_else(|| {
                SelectorError::new(
                    selector,
                    format!("segment {segment:?} has trailing text {rest:?}"),
                )
            })?;
        Some(ordinal)
    };
    Ok(Segment {
        kind: kind.to_owned(),
        label,
        ordinal,
    })
}

/// Parses a selector; the empty selector has no segments and selects the root.
pub fn parse_selector(selector: &str) -> Result<Vec<Segment>, SelectorError> {
    if selector.is_empty() {
        return Ok(Vec::new());
    }
    split_segments(selector)?
        .into_iter()
        .map(|segment| parse_segment(selector, segment))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Label, decode_name, encode_name, parse_selector};

    // {test}`requirements.references.outline_selectors`
    #[test]
    fn names_round_trip_through_percent_encoding() {
        let name = "a b/c[d]#e@f%g\té";
        let encoded = encode_name(name);
        assert_eq!(encoded, "a%20b%2Fc%5Bd%5D%23e%40f%25g%09é");
        assert_eq!(decode_name(&encoded).unwrap(), name);
        assert!(decode_name("a/b").is_err());
        assert!(decode_name("%zz").is_err());
        assert!(decode_name("a b").is_err());
    }

    // {test}`requirements.references.outline_selectors`
    #[test]
    fn parses_named_anonymous_and_ordinal_segments() {
        assert!(parse_selector("").unwrap().is_empty());
        let segments =
            parse_selector("impl[Thing/Trait%3CT%3E]/fn[run]#2/use@0123456789AB").unwrap();
        assert_eq!(
            segments[0].label,
            Label::Named(vec!["Thing".into(), "Trait<T>".into()])
        );
        assert_eq!(segments[1].ordinal, Some(2));
        assert_eq!(segments[2].label, Label::Hash("0123456789ab".into()));
        assert_eq!(segments[2].to_string(), "use@0123456789ab");
        for invalid in [
            "fn",
            "fn[run",
            "[run]",
            "fn[run]#0",
            "use@123",
            "fn[run]x",
            "a//b",
        ] {
            assert!(parse_selector(invalid).is_err(), "{invalid}");
        }
    }
}
