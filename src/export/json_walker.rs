//! JSON walker ported from `AssetRipper.Export.PrimaryContent.DefaultJsonWalker`.
//!
//! The reference walks a decoded asset with `AssetWalker` and writes JSON as it goes, rather
//! than building a value tree first. This port keeps that structure: [`JsonWalker`] exposes the
//! same enter/divide/exit/visit callbacks, so a caller drives it exactly where the reference's
//! traversal would, and the writer state (indent depth, line breaks) lives in one place.
//!
//! Shape rules that matter for byte-identical output:
//!
//! - Indentation is one tab per level and the reference writes a newline *before* every closing
//!   brace, so a container never ends on the same line as its last entry.
//! - An empty list is `[]` and an empty string-keyed dictionary is `{}` on one line; the
//!   reference returns `false` from `Enter*` in that case so the caller skips the matching exit.
//! - A non-string-keyed dictionary renders as a list of `{"Key": .., "Value": ..}` objects.
//! - `byte[]` is Base64, strings and field names are JavaScript-string-encoded, and a `PPtr`
//!   is the fixed `{ "m_FileID": n, "m_PathID": m }` object with spaces.
//! - Floats use the invariant shortest round-trip form (`float`/`double` `ToString`), which the
//!   port matches with `Display`, and integral types print without a decimal point.

use std::fmt::Write as _;

use crate::serialized_file::object::SerializedValue;

/// Which container the walker is currently inside, so a closing brace can be written correctly
/// and a separator can be chosen from context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Container {
    /// An asset or pair body: `{ .. }`.
    Object,
    /// A list: `[ .. ]`.
    List,
    /// A string-keyed dictionary: `{ "key": value, .. }`.
    StringMap,
}

/// JSON writer mirroring `DefaultJsonWalker`.
///
/// Callers drive the traversal; this type only formats. Every `enter_*` returns whether the
/// matching `exit_*` is expected, exactly like the reference's `bool` returns, so an empty
/// container can be written on one line without a trailing newline.
#[derive(Debug, Default)]
pub struct JsonWalker {
    output: String,
    indent: usize,
    stack: Vec<Container>,
    /// Whether the current container already holds an entry, so a separator is due.
    needs_separator: Vec<bool>,
}

impl JsonWalker {
    /// A walker writing into a fresh buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The JSON text written so far.
    pub fn finish(self) -> String {
        self.output
    }

    /// The JSON text written so far, borrowing the buffer.
    pub fn as_str(&self) -> &str {
        &self.output
    }

    fn indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push('\t');
        }
    }

    /// Write the separator due before an entry in the current container, if any.
    ///
    /// Every entry starts on its own indented line: the first entry in a container just gets a
    /// newline and an indent, and later entries are preceded by a comma on the previous line.
    fn separator(&mut self) {
        let Some(pending) = self.needs_separator.last_mut() else {
            return;
        };
        if *pending {
            self.output.push(',');
        }
        *pending = true;
        self.output.push('\n');
        self.indent();
    }

    /// `EnterAsset`: open an object body.
    pub fn enter_asset(&mut self) -> bool {
        self.output.push('{');
        self.indent += 1;
        self.stack.push(Container::Object);
        self.needs_separator.push(false);
        true
    }

    /// `ExitAsset`: close an object body.
    pub fn exit_asset(&mut self) {
        self.output.push('\n');
        self.indent = self.indent.saturating_sub(1);
        self.indent();
        self.output.push('}');
        self.stack.pop();
        self.needs_separator.pop();
    }

    /// `EnterField`: write `"name": ` and prepare for the value.
    pub fn enter_field(&mut self, name: &str) -> bool {
        self.separator();
        write_json_string(&mut self.output, name);
        self.output.push_str(": ");
        true
    }

    /// `EnterList`: open a list, or write `[]` and report that no exit follows.
    pub fn enter_list(&mut self, count: usize) -> bool {
        if count == 0 {
            self.output.push_str("[]");
            return false;
        }
        self.output.push('[');
        self.indent += 1;
        self.stack.push(Container::List);
        self.needs_separator.push(false);
        true
    }

    /// `ExitList`: close a list.
    pub fn exit_list(&mut self) {
        self.output.push('\n');
        self.indent = self.indent.saturating_sub(1);
        self.indent();
        self.output.push(']');
        self.stack.pop();
        self.needs_separator.pop();
    }

    /// Start the next list or dictionary entry.
    pub fn divide(&mut self) {
        let Some(pending) = self.needs_separator.last_mut() else {
            return;
        };
        *pending = true;
        self.output.push(',');
        self.output.push('\n');
    }

    /// `EnterDictionary`: open `{ .. }` for string keys or `[ .. ]` otherwise.
    pub fn enter_dictionary(&mut self, count: usize, string_keys: bool) -> bool {
        if count == 0 {
            self.output.push_str(if string_keys { "{}" } else { "[]" });
            return false;
        }
        self.output.push(if string_keys { '{' } else { '[' });
        self.indent += 1;
        self.stack.push(if string_keys {
            Container::StringMap
        } else {
            Container::List
        });
        self.needs_separator.push(false);
        true
    }

    /// `ExitDictionary`: close a dictionary opened by [`Self::enter_dictionary`].
    pub fn exit_dictionary(&mut self, string_keys: bool) {
        self.output.push('\n');
        self.indent = self.indent.saturating_sub(1);
        self.indent();
        self.output.push(if string_keys { '}' } else { ']' });
        self.stack.pop();
        self.needs_separator.pop();
    }

    /// `EnterDictionaryPair`: a string-keyed pair writes its key through the caller, so the
    /// entry is positioned here; a non-string pair opens its own object at the current position.
    pub fn enter_dictionary_pair(&mut self, string_keys: bool) -> bool {
        self.separator();
        if !string_keys {
            self.enter_pair();
        }
        true
    }

    /// `DivideDictionaryPair`: the `": "` between a string key and its value, or the separator
    /// plus `"Value": ` for a `{"Key": .., "Value": ..}` object.
    pub fn divide_dictionary_pair(&mut self, string_keys: bool) {
        if string_keys {
            self.output.push_str(": ");
        } else {
            self.separator();
            self.output.push_str("\"Value\": ");
        }
    }

    /// `ExitDictionaryPair`: only non-string pairs close an object.
    pub fn exit_dictionary_pair(&mut self, string_keys: bool) {
        if !string_keys {
            self.exit_pair();
        }
    }

    /// `EnterPair`: open a `{"Key": .., "Value": ..}` object for a non-string key.
    ///
    /// The caller positions this entry (a list separator or an explicit indent), so the pair
    /// only opens the brace and writes the `"Key": ` prefix. The `"Key"` entry counts as
    /// written, so the following divide emits the comma before `"Value"`.
    pub fn enter_pair(&mut self) -> bool {
        self.output.push('{');
        self.indent += 1;
        self.stack.push(Container::Object);
        self.needs_separator.push(true);
        self.output.push('\n');
        self.indent();
        self.output.push_str("\"Key\": ");
        true
    }

    /// `ExitPair`: close a pair object.
    pub fn exit_pair(&mut self) {
        self.output.push('\n');
        self.indent = self.indent.saturating_sub(1);
        self.indent();
        self.output.push('}');
        self.stack.pop();
        self.needs_separator.pop();
    }

    /// `VisitPPtr`: the fixed `{ "m_FileID": n, "m_PathID": m }` shape.
    pub fn visit_pptr(&mut self, file_id: i32, path_id: i64) {
        self.separator();
        let _ = write!(
            self.output,
            "{{ \"m_FileID\": {file_id}, \"m_PathID\": {path_id} }}"
        );
    }

    /// `VisitPrimitive` over one decoded value, positioned as the next entry in the current
    /// container.
    pub fn visit_value(&mut self, value: &SerializedValue) {
        self.separator();
        write_json_value(&mut self.output, value);
    }

    /// `VisitPrimitive` for a value that follows an explicit prefix such as `"Key": `.
    ///
    /// The caller already wrote the prefix, so no separator or indentation is emitted.
    pub fn visit_value_inline(&mut self, value: &SerializedValue) {
        write_json_value(&mut self.output, value);
    }

    /// `VisitPrimitive` for a bare string, used where the caller already knows the text.
    pub fn visit_string(&mut self, value: &str) {
        self.separator();
        write_json_string(&mut self.output, value);
    }

    /// `VisitPrimitive` for a boolean, positioned as the next entry.
    pub fn visit_bool(&mut self, value: bool) {
        self.separator();
        self.output.push_str(if value { "true" } else { "false" });
    }
}

/// Write one decoded value as JSON (`VisitPrimitive` plus the reference's value mapping).
///
/// An asset-level value is an object, so this recurses through the same shape rules the walker
/// applies; the reference reaches the same result because its walker visits the same fields.
pub fn write_json_value(output: &mut String, value: &SerializedValue) {
    match value {
        SerializedValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        SerializedValue::I8(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::U8(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::I16(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::U16(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::I32(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::U32(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::I64(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::U64(value) => {
            let _ = write!(output, "{value}");
        }
        SerializedValue::F32(value) => write_invariant_float(output, f64::from(*value)),
        SerializedValue::F64(value) => write_invariant_float(output, *value),
        SerializedValue::Char(value) => {
            // A `char` leaf is a UTF-16 code unit in Unity, so it renders as a character and
            // not as a number.
            let character = char::from_u32(u32::from(*value)).unwrap_or('\u{FFFD}');
            write_json_string(output, &character.to_string());
        }
        SerializedValue::StringBytes(bytes) => {
            write_json_string(output, &String::from_utf8_lossy(bytes))
        }
        SerializedValue::Bytes(bytes) => {
            output.push('"');
            output.push_str(&base64_encode(bytes));
            output.push('"');
        }
        SerializedValue::PPtr { file_id, path_id } => {
            let _ = write!(
                output,
                "{{ \"m_FileID\": {file_id}, \"m_PathID\": {path_id} }}"
            );
        }
        SerializedValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json_value(output, value);
            }
            output.push(']');
        }
        SerializedValue::Struct(fields) => {
            output.push('{');
            for (index, field) in fields.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json_string(output, &field.name);
                output.push_str(": ");
                write_json_value(output, &field.value);
            }
            output.push('}');
        }
        other => {
            // Anything else is quoted through its debug form, mirroring the reference's
            // `ToString` fallback rather than dropping the leaf.
            write_json_string(output, &format!("{other:?}"));
        }
    }
}

/// Write a float the way `float`/`double` `ToString` does under the invariant culture.
///
/// Rust's `Display` already gives the shortest round-trip form without an exponent for the
/// magnitudes Unity stores, so this only adds the reference's integral spelling (`1` rather
/// than `1.0`) and keeps non-finite values out of the output.
fn write_invariant_float(output: &mut String, value: f64) {
    if value.is_nan() {
        output.push_str("\"NaN\"");
        return;
    }
    if value.is_infinite() {
        output.push_str(if value.is_sign_negative() {
            "\"-Infinity\""
        } else {
            "\"Infinity\""
        });
        return;
    }
    let _ = write!(output, "{value}");
}

/// JavaScript-string-encode `text` and wrap it in quotes (`HttpUtility.JavaScriptStringEncode`).
///
/// The reference leaves non-ASCII characters as-is and escapes only the characters JSON
/// requires plus the HTML-sensitive ones it guards.
fn write_json_string(output: &mut String, text: &str) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            character if (character as u32) < 0x20 => {
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

/// Standard Base64 with padding and no line breaks (`Convert.ToBase64String`).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[(((second & 0x0F) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(third & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    #[test]
    fn primitives_print_in_the_reference_forms() {
        let mut output = String::new();
        write_json_value(&mut output, &SerializedValue::Bool(true));
        assert_eq!(output, "true");

        output.clear();
        write_json_value(&mut output, &SerializedValue::I32(-7));
        assert_eq!(output, "-7");

        output.clear();
        write_json_value(
            &mut output,
            &SerializedValue::U64(18_446_744_073_709_551_615),
        );
        assert_eq!(output, "18446744073709551615");

        // `float`/`double` `ToString` omits a trailing `.0`.
        output.clear();
        write_json_value(&mut output, &SerializedValue::F32(1.0));
        assert_eq!(output, "1");

        output.clear();
        write_json_value(&mut output, &SerializedValue::F32(0.5));
        assert_eq!(output, "0.5");

        output.clear();
        write_json_value(&mut output, &SerializedValue::F64(-2.25));
        assert_eq!(output, "-2.25");
    }

    #[test]
    fn strings_escape_and_bytes_become_base64() {
        let mut output = String::new();
        write_json_value(
            &mut output,
            &SerializedValue::StringBytes(b"a\"b\\c\nd\te".to_vec()),
        );
        assert_eq!(output, "\"a\\\"b\\\\c\\nd\\te\"");

        // Non-ASCII is left as-is, like `JavaScriptStringEncode`.
        output.clear();
        write_json_value(
            &mut output,
            &SerializedValue::StringBytes("青岚".as_bytes().to_vec()),
        );
        assert_eq!(output, "\"青岚\"");

        output.clear();
        write_json_value(&mut output, &SerializedValue::Bytes(vec![1, 2, 3, 4]));
        assert_eq!(output, "\"AQIDBA==\"");

        // A `char` leaf is a UTF-16 code unit and renders as a character.
        output.clear();
        write_json_value(&mut output, &SerializedValue::Char(0x4E2D));
        assert_eq!(output, "\"中\"");
    }

    #[test]
    fn pptr_uses_the_reference_spacing() {
        let mut output = String::new();
        write_json_value(
            &mut output,
            &SerializedValue::PPtr {
                file_id: 2,
                path_id: -42,
            },
        );
        assert_eq!(output, "{ \"m_FileID\": 2, \"m_PathID\": -42 }");
    }

    #[test]
    fn walker_writes_an_asset_body_with_tab_indentation() {
        let mut walker = JsonWalker::new();
        assert!(walker.enter_asset());
        assert!(walker.enter_field("m_Width"));
        walker.visit_value_inline(&SerializedValue::I32(2));
        walker.exit_asset();
        assert_eq!(walker.finish(), "{\n\t\"m_Width\": 2\n}");
    }

    #[test]
    fn empty_containers_collapse_onto_one_line() {
        let mut walker = JsonWalker::new();
        assert!(walker.enter_asset());
        assert!(walker.enter_field("m_Empty"));
        assert!(!walker.enter_list(0));
        assert!(walker.enter_field("m_EmptyMap"));
        assert!(!walker.enter_dictionary(0, true));
        walker.exit_asset();
        assert_eq!(
            walker.finish(),
            "{\n\t\"m_Empty\": [],\n\t\"m_EmptyMap\": {}\n}"
        );
    }

    #[test]
    fn non_string_dictionary_keys_become_key_value_objects() {
        let mut walker = JsonWalker::new();
        assert!(walker.enter_dictionary(1, false));
        assert!(walker.enter_dictionary_pair(false));
        walker.visit_value_inline(&SerializedValue::I32(5));
        walker.divide_dictionary_pair(false);
        walker.visit_value_inline(&SerializedValue::I32(6));
        walker.exit_dictionary_pair(false);
        walker.exit_dictionary(false);
        assert_eq!(
            walker.finish(),
            "[\n\t{\n\t\t\"Key\": 5,\n\t\t\"Value\": 6\n\t}\n]"
        );
    }

    #[test]
    fn struct_fields_recurse_through_the_same_shape_rules() {
        let value = SerializedValue::Struct(vec![
            field("m_Name", SerializedValue::StringBytes(b"hero".to_vec())),
            field(
                "m_Data",
                SerializedValue::Array(vec![SerializedValue::U8(1), SerializedValue::U8(2)]),
            ),
        ]);
        let mut output = String::new();
        write_json_value(&mut output, &value);
        assert_eq!(output, "{\"m_Name\": \"hero\",\"m_Data\": [1,2]}");
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xFF, 0xFE, 0xFD]), "//79");
    }
}
