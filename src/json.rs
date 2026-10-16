//! Minimal JSON reader shared by the settings file and the product web surface.
//!
//! The value model is deliberately small: objects keep their member order (so duplicate
//! keys stay visible and callers can apply last-wins), arrays keep item order, strings are
//! fully unescaped (including surrogate pairs), and numbers are integral only. Integral-only
//! numbers are not a general JSON restriction, but they are the contract the settings
//! deserializer this parser was extracted from enforces: a fractional or exponent form is a
//! parse error rather than a lossy conversion. Callers that need another numeric policy must
//! extend this module so the rule stays in one place.
//!
//! The reader never allocates based on an unvalidated length: strings grow one decoded
//! character at a time and are checked against [`JsonLimits::max_string_bytes`] on every
//! iteration, and container nesting is checked against [`JsonLimits::max_nesting_depth`]
//! before each descent.

use std::fmt;

/// A parsed JSON document.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Integer(i64),
    Str(String),
    Object(Vec<(String, JsonValue)>),
    Array(Vec<JsonValue>),
}

/// Bounds applied to a JSON document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JsonLimits {
    /// Maximum decoded bytes for any single string.
    pub max_string_bytes: usize,
    /// Maximum JSON nesting depth accepted by the reader.
    pub max_nesting_depth: usize,
}

impl Default for JsonLimits {
    fn default() -> Self {
        Self {
            max_string_bytes: 64 * 1024,
            max_nesting_depth: 32,
        }
    }
}

/// Error returned when a JSON document cannot be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonError {
    /// The document is not valid JSON at the reported path.
    InvalidJson { path: String, reason: &'static str },
    /// A decoded string exceeds the configured bound.
    StringTooLong { byte_count: usize, limit: usize },
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { path, reason } => {
                write!(formatter, "invalid JSON at {path}: {reason}")
            }
            Self::StringTooLong { byte_count, limit } => write!(
                formatter,
                "decoded string of {byte_count} bytes exceeds the {limit}-byte limit"
            ),
        }
    }
}

impl std::error::Error for JsonError {}

impl JsonValue {
    /// Borrow this value as an object, or report `path` as the failure location.
    pub fn as_object(&self, path: &str) -> Result<&[(String, JsonValue)], JsonError> {
        match self {
            Self::Object(members) => Ok(members),
            _ => Err(self.invalid(path, "expected an object")),
        }
    }

    /// Borrow this value as a boolean, or report `path` as the failure location.
    pub fn as_bool(&self, path: &str) -> Result<bool, JsonError> {
        match self {
            Self::Bool(flag) => Ok(*flag),
            _ => Err(self.invalid(path, "expected true or false")),
        }
    }

    /// Borrow this value as a string, or report `path` as the failure location.
    pub fn as_str(&self, path: &str) -> Result<&str, JsonError> {
        match self {
            Self::Str(text) => Ok(text),
            _ => Err(self.invalid(path, "expected a string")),
        }
    }

    /// Borrow this value as an array, or report `path` as the failure location.
    pub fn as_array(&self, path: &str) -> Result<&[JsonValue], JsonError> {
        match self {
            Self::Array(items) => Ok(items),
            _ => Err(self.invalid(path, "expected an array")),
        }
    }

    /// Return the first member named `name`, matching the reader's member order.
    pub fn member(&self, name: &str) -> Option<&JsonValue> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn invalid(&self, path: &str, reason: &'static str) -> JsonError {
        JsonError::InvalidJson {
            path: path.to_owned(),
            reason,
        }
    }
}

/// Parse a complete JSON document. Trailing non-whitespace content is rejected.
pub fn parse_json(text: &str, limits: JsonLimits) -> Result<JsonValue, JsonError> {
    let mut parser = JsonParser {
        bytes: text.as_bytes(),
        position: 0,
        depth: 0,
        limits,
    };
    let value = parser.parse_value("$")?;
    parser.skip_whitespace();
    if parser.position != parser.bytes.len() {
        return Err(parser.error("$", "trailing content after the document"));
    }
    Ok(value)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    position: usize,
    depth: usize,
    limits: JsonLimits,
}

impl JsonParser<'_> {
    /// One nesting level for objects and arrays against the depth bound, so both
    /// containers share a single decision point.
    fn enter_nesting(&mut self, path: &str) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > self.limits.max_nesting_depth {
            return Err(self.error(path, "nesting depth exceeds the limit"));
        }
        Ok(())
    }

    fn parse_value(&mut self, path: &str) -> Result<JsonValue, JsonError> {
        self.skip_whitespace();
        match self.bytes.get(self.position) {
            Some(b'{') => self.parse_object(path),
            Some(b'[') => self.parse_array(path),
            Some(b'"') => Ok(JsonValue::Str(self.parse_string(path)?)),
            Some(b't') => self.parse_literal(path, "true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal(path, "false", JsonValue::Bool(false)),
            Some(b'n') => self.parse_literal(path, "null", JsonValue::Null),
            Some(b'-' | b'0'..=b'9') => Ok(JsonValue::Integer(self.parse_integer(path)?)),
            _ => Err(self.error(path, "expected a JSON value")),
        }
    }

    fn parse_object(&mut self, path: &str) -> Result<JsonValue, JsonError> {
        self.enter_nesting(path)?;
        // `position` is at `{`.
        self.position += 1;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.position) == Some(&b'}') {
            self.position += 1;
            self.depth -= 1;
            return Ok(JsonValue::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b'"') {
                return Err(self.error(path, "expected a member name"));
            }
            let name = self.parse_string(path)?;
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b':') {
                return Err(self.error(path, "expected a colon"));
            }
            self.position += 1;
            let value = self.parse_value(path)?;
            members.push((name, value));
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b'}') => {
                    self.position += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Object(members));
                }
                _ => return Err(self.error(path, "expected a comma or closing brace")),
            }
        }
    }

    fn parse_array(&mut self, path: &str) -> Result<JsonValue, JsonError> {
        self.enter_nesting(path)?;
        // `position` is at `[`.
        self.position += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.position) == Some(&b']') {
            self.position += 1;
            self.depth -= 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            let value = self.parse_value(path)?;
            items.push(value);
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b']') => {
                    self.position += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(self.error(path, "expected a comma or closing bracket")),
            }
        }
    }

    fn parse_string(&mut self, path: &str) -> Result<String, JsonError> {
        // `position` is at the opening quote.
        self.position += 1;
        let mut output = String::new();
        loop {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or_else(|| self.error(path, "unterminated string"))?;
            match byte {
                b'"' => {
                    self.position += 1;
                    self.check_string_bound(output.len())?;
                    return Ok(output);
                }
                b'\\' => {
                    self.position += 1;
                    let escaped = *self
                        .bytes
                        .get(self.position)
                        .ok_or_else(|| self.error(path, "unterminated escape"))?;
                    self.position += 1;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{08}'),
                        b'f' => output.push('\u{0C}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let unit = self.parse_hex4(path)?;
                            if (0xD800..0xDC00).contains(&unit) {
                                if self.bytes.get(self.position) == Some(&b'\\')
                                    && self.bytes.get(self.position + 1) == Some(&b'u')
                                {
                                    self.position += 2;
                                    let low = self.parse_hex4(path)?;
                                    if (0xDC00..0xE000).contains(&low) {
                                        let scalar = 0x10000
                                            + ((u32::from(unit) - 0xD800) << 10)
                                            + (u32::from(low) - 0xDC00);
                                        output.push(char::from_u32(scalar).ok_or_else(|| {
                                            self.error(path, "invalid surrogate pair")
                                        })?);
                                    } else {
                                        return Err(self.error(path, "lone leading surrogate"));
                                    }
                                } else {
                                    return Err(self.error(path, "lone leading surrogate"));
                                }
                            } else if (0xDC00..0xE000).contains(&unit) {
                                return Err(self.error(path, "lone trailing surrogate"));
                            } else {
                                output.push(
                                    char::from_u32(u32::from(unit))
                                        .ok_or_else(|| self.error(path, "invalid escape unit"))?,
                                );
                            }
                        }
                        _ => {
                            return Err(self.error(path, "unknown escape"));
                        }
                    }
                }
                0x00..=0x1F => {
                    return Err(self.error(path, "unescaped control character"));
                }
                _ => {
                    let rest = &self.bytes[self.position..];
                    let text = core::str::from_utf8(rest)
                        .map_err(|_| self.error(path, "invalid UTF-8"))?;
                    let character = text
                        .chars()
                        .next()
                        .ok_or_else(|| self.error(path, "unterminated string"))?;
                    output.push(character);
                    self.position += character.len_utf8();
                }
            }
            self.check_string_bound(output.len())?;
        }
    }

    fn parse_hex4(&mut self, path: &str) -> Result<u16, JsonError> {
        if self.position + 4 > self.bytes.len() {
            return Err(self.error(path, "truncated escape"));
        }
        let mut unit: u16 = 0;
        for byte in &self.bytes[self.position..self.position + 4] {
            unit = unit.wrapping_mul(16).wrapping_add(match byte {
                b'0'..=b'9' => (byte - b'0') as u16,
                b'a'..=b'f' => (byte - b'a' + 10) as u16,
                b'A'..=b'F' => (byte - b'A' + 10) as u16,
                _ => return Err(self.error(path, "invalid escape unit")),
            });
        }
        self.position += 4;
        Ok(unit)
    }

    fn parse_literal(
        &mut self,
        path: &str,
        literal: &str,
        value: JsonValue,
    ) -> Result<JsonValue, JsonError> {
        if self.bytes.len() - self.position < literal.len()
            || &self.bytes[self.position..self.position + literal.len()] != literal.as_bytes()
        {
            return Err(self.error(path, "invalid literal"));
        }
        self.position += literal.len();
        Ok(value)
    }

    /// Strict JSON integers: optional minus, no leading zeros, no fractions or exponents.
    /// Non-integral numbers are rejected exactly as the source deserializer rejects them.
    fn parse_integer(&mut self, path: &str) -> Result<i64, JsonError> {
        let start = self.position;
        if self.bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        let digits = self.position;
        while matches!(self.bytes.get(self.position), Some(b'0'..=b'9')) {
            self.position += 1;
        }
        if self.position == digits {
            return Err(self.error(path, "expected digits"));
        }
        if self.bytes[digits] == b'0' && self.position - digits > 1 {
            return Err(self.error(path, "leading zeros are not valid JSON"));
        }
        if matches!(self.bytes.get(self.position), Some(b'.' | b'e' | b'E')) {
            return Err(self.error(path, "fractional numbers are not valid here"));
        }
        let text = core::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| self.error(path, "invalid number"))?;
        text.parse::<i64>()
            .map_err(|_| self.error(path, "integer out of range"))
    }

    fn skip_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b'\t' | b'\n' | b'\r')
        ) {
            self.position += 1;
        }
    }

    fn check_string_bound(&self, len: usize) -> Result<(), JsonError> {
        if len > self.limits.max_string_bytes {
            return Err(JsonError::StringTooLong {
                byte_count: len,
                limit: self.limits.max_string_bytes,
            });
        }
        Ok(())
    }

    fn error(&self, path: &str, reason: &'static str) -> JsonError {
        JsonError::InvalidJson {
            path: path.to_owned(),
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_json, JsonError, JsonLimits, JsonValue};

    fn parse(text: &str) -> JsonValue {
        parse_json(text, JsonLimits::default()).expect("document parses")
    }

    #[test]
    fn reader_preserves_member_order_and_duplicates() {
        let value = parse(r#"{"b": 1, "a": true, "b": null, "c": [1, [2, 3], {}]}"#);
        let members = value.as_object("$").unwrap();
        assert_eq!(members.len(), 4);
        assert_eq!(members[0].0, "b");
        assert_eq!(members[0].1, JsonValue::Integer(1));
        assert_eq!(members[1].1, JsonValue::Bool(true));
        // `member` returns the first occurrence; last-wins stays the caller's decision.
        assert_eq!(value.member("b"), Some(&JsonValue::Integer(1)));
        assert_eq!(value.member("c").unwrap().as_array("$.c").unwrap().len(), 3);
        assert_eq!(value.member("missing"), None);
    }

    #[test]
    fn reader_decodes_escapes_and_surrogate_pairs() {
        let value = parse(r#"{"s": "\"\\\/\b\f\n\r\t\u0041\u00e9\ud83d\ude00"}"#);
        assert_eq!(
            value.member("s").unwrap().as_str("$.s").unwrap(),
            "\"\\/\u{8}\u{c}\n\r\tA\u{e9}\u{1f600}"
        );
    }

    #[test]
    fn reader_rejects_malformed_documents_with_a_path() {
        for (text, reason) in [
            ("", "expected a JSON value"),
            ("{} trailing", "trailing content after the document"),
            (r#"{"a" 1}"#, "expected a colon"),
            (r#"{"a": 1,}"#, "expected a member name"),
            ("[1, 2", "expected a comma or closing bracket"),
            (r#""\x""#, "unknown escape"),
            (r#""\ud800""#, "lone leading surrogate"),
            (r#""\udc00""#, "lone trailing surrogate"),
            ("\"unterminated", "unterminated string"),
            ("\"\u{1}control\"", "unescaped control character"),
            ("01", "leading zeros are not valid JSON"),
            ("1.5", "fractional numbers are not valid here"),
            ("1e3", "fractional numbers are not valid here"),
            ("99999999999999999999", "integer out of range"),
        ] {
            let error = parse_json(text, JsonLimits::default()).unwrap_err();
            match error {
                JsonError::InvalidJson {
                    path,
                    reason: found,
                } => {
                    assert_eq!(found, reason, "for input {text:?}");
                    assert_eq!(path, "$");
                }
                other => panic!("unexpected error for {text:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn reader_enforces_depth_and_string_bounds() {
        let deep = "[".repeat(40) + &"]".repeat(40);
        let error = parse_json(&deep, JsonLimits::default()).unwrap_err();
        assert!(matches!(
            error,
            JsonError::InvalidJson {
                reason: "nesting depth exceeds the limit",
                ..
            }
        ));

        let limits = JsonLimits {
            max_string_bytes: 4,
            max_nesting_depth: 2,
        };
        assert_eq!(
            parse_json(r#""abcd""#, limits).unwrap(),
            JsonValue::Str("abcd".to_owned())
        );
        let error = parse_json(r#""abcde""#, limits).unwrap_err();
        assert_eq!(
            error,
            JsonError::StringTooLong {
                byte_count: 5,
                limit: 4
            }
        );
        // The bound counts decoded bytes and is enforced after every decoded character, so
        // the reported size is where the bound was first crossed (three two-byte
        // characters already exceed four bytes).
        let error = parse_json("\"\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\"", limits).unwrap_err();
        assert_eq!(
            error,
            JsonError::StringTooLong {
                byte_count: 6,
                limit: 4
            }
        );
    }

    #[test]
    fn accessors_report_the_caller_supplied_path() {
        let value = parse(r#"{"n": 3}"#);
        let number = value.member("n").unwrap();
        assert_eq!(
            number.as_object("$.n").unwrap_err(),
            JsonError::InvalidJson {
                path: "$.n".to_owned(),
                reason: "expected an object",
            }
        );
        assert_eq!(
            number.as_bool("$.n").unwrap_err(),
            JsonError::InvalidJson {
                path: "$.n".to_owned(),
                reason: "expected true or false",
            }
        );
        assert_eq!(
            JsonValue::Null.as_array("$.n").unwrap_err(),
            JsonError::InvalidJson {
                path: "$.n".to_owned(),
                reason: "expected an array",
            }
        );
        assert_eq!(
            JsonValue::Null.as_str("$.n").unwrap_err(),
            JsonError::InvalidJson {
                path: "$.n".to_owned(),
                reason: "expected a string",
            }
        );
    }
}
