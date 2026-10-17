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

/// The line break a `TextWriter` uses when the caller leaves `NewLine` at its default.
///
/// `DefaultJsonWalker` writes whatever `TextWriter.NewLine` holds, and the reference's two callers
/// differ:
///
/// * the **file export** wraps a `StreamWriter` and never sets `NewLine`, so it gets
///   `Environment.NewLine` - `\r\n` on Windows, `\n` elsewhere;
/// * the **browser `/Assets/Json` route** explicitly sets `NewLine = "\n"`
///   (`AssetRipper.GUI.Web/Pages/Assets/AssetAPI.cs`), so it is `\n` on every platform.
///
/// The line break is therefore a property of the writer the caller supplies, not a constant of this
/// module: [`JsonWalker::new`] uses this host default and [`JsonWalker::with_line_break`] takes an
/// explicit one. Hard-coding either value would be wrong for one of the two callers.
const fn default_line_break() -> &'static str {
    if cfg!(windows) {
        "\r\n"
    } else {
        "\n"
    }
}

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
    /// The line break to write, mirroring `TextWriter.NewLine`.
    ///
    /// `&'static str` because both of the reference's callers use a compile-time constant: the host
    /// default and a literal `"\n"`.
    line_break: &'static str,
}

impl JsonWalker {
    /// A walker writing into a fresh buffer with the host's default line break.
    pub fn new() -> Self {
        Self {
            line_break: default_line_break(),
            ..Self::default()
        }
    }

    /// A walker writing an explicit line break, for a caller that sets `TextWriter.NewLine`.
    pub fn with_line_break(line_break: &'static str) -> Self {
        Self {
            line_break,
            ..Self::default()
        }
    }

    /// The line break this walker writes.
    pub fn line_break(&self) -> &'static str {
        self.line_break
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
        self.output.push_str(self.line_break);
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
        self.output.push_str(self.line_break);
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
        self.output.push_str(self.line_break);
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
        self.output.push_str(self.line_break);
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
        self.output.push_str(self.line_break);
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
        self.output.push_str(self.line_break);
        self.indent();
        self.output.push_str("\"Key\": ");
        true
    }

    /// `ExitPair`: close a pair object.
    pub fn exit_pair(&mut self) {
        self.output.push_str(self.line_break);
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

    /// Position the next entry in the current container without writing a leaf.
    ///
    /// Containers need this because a leaf's positioning lives in [`Self::visit_value`], while a
    /// nested object or list must be positioned *before* its opening brace is written.
    pub fn position_entry(&mut self) {
        self.separator();
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

/// Write one decoded value as the reference's indented JSON body.
///
/// This is the function the browser `/Assets/Json` route and the file export both need: the
/// reference renders an asset by walking it and writing as it goes, so its output is tab-indented
/// and line-broken rather than compact. [`write_json_value`] produces the compact form and is kept
/// for callers that want one line.
///
/// The traversal mirrors the reference's callback order:
///
/// * a `Struct` is an object body - `EnterAsset`, then `EnterField` plus the field's value for each
///   field, then `ExitAsset`. The reference calls `DivideAsset` between fields, which is the same
///   position [`JsonWalker::enter_field`] already handles, so no explicit divide is issued;
/// * an `Array` is a list - `EnterList`, each element as the next entry, then `ExitList`;
/// * everything else is a leaf written through `VisitPrimitive`, either inline when the cursor
///   already sits after a `"name": ` prefix or positioned as the next entry otherwise.
pub fn write_json_body(walker: &mut JsonWalker, value: &SerializedValue) {
    write_decoded_value(walker, value, false);
}

/// Write `value` at the cursor, optionally already positioned by the caller.
///
/// `positioned` is true when a `"name": ` prefix (or an equivalent indent) has just been written,
/// so a leaf must not emit a separator of its own. Containers position themselves when they are
/// entries, because the reference positions an object element *before* `EnterAsset` writes its
/// brace.
fn write_decoded_value(walker: &mut JsonWalker, value: &SerializedValue, positioned: bool) {
    match value {
        SerializedValue::Struct(fields) => {
            if !positioned {
                walker.position_entry();
            }
            walker.enter_asset();
            for field in fields {
                walker.enter_field(&field.name);
                write_decoded_value(walker, &field.value, true);
            }
            walker.exit_asset();
        }
        SerializedValue::Array(items) => {
            if !positioned {
                walker.position_entry();
            }
            if items.is_empty() {
                walker.enter_list(0);
            } else {
                walker.enter_list(items.len());
                for item in items {
                    write_decoded_value(walker, item, false);
                }
                walker.exit_list();
            }
        }
        leaf => {
            if positioned {
                walker.visit_value_inline(leaf);
            } else {
                walker.visit_value(leaf);
            }
        }
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
        SerializedValue::F32(value) => {
            write_invariant_float(output, f64::from(*value), FloatKind::F32)
        }
        SerializedValue::F64(value) => write_invariant_float(output, *value, FloatKind::F64),
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

/// Write a float the way `float.ToString()` / `double.ToString()` does under the invariant culture.
///
/// **Rust's `Display` is not equivalent, and using it produces byte-different output.** The
/// reference passes no format string, so the value reaches .NET's shortest-round-trip default,
/// whose behaviour was captured into `float_format_fixture.txt`. It differs from `{}` in three
/// ways:
///
/// - **Exponent threshold.** `Display` never uses exponent notation. .NET switches to it when the
///   decimal exponent is below `-5` or at/above `9` for `f32` and `17` for `f64`, so `1e-5f` prints
///   `1E-05` rather than `0.00001`, and `1e9f` prints `1E+09` while `1e8f` still prints `100000000`.
/// - **Exponent spelling.** .NET writes an uppercase `E`, an explicit sign for positive exponents,
///   and at least two exponent digits (`E+09`, `E-05`, `E+38`).
/// - **Non-finite values.** .NET writes bare `NaN`, `Infinity` and `-Infinity` - not valid JSON, but
///   that is what the reference emits - whereas `Display` writes `inf` and `-inf`.
///
/// Integral values print without a trailing `.0` in both, and the sign of negative zero is kept.
///
/// `kind` decides both the exponent threshold **and** the precision the shortest digit string is
/// taken at, so the two can never disagree.
fn write_invariant_float(output: &mut String, value: f64, kind: FloatKind) {
    if value.is_nan() {
        output.push_str("NaN");
        return;
    }
    if value.is_infinite() {
        output.push_str(if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        });
        return;
    }
    // The shortest round-trip digits must be taken at the **originating** type's precision, which is
    // why this parameter carries the kind rather than just a threshold pair. Formatting an `f32`
    // widened to `f64` with `{}` prints `0.000009999999747378752`, because widening preserves the
    // binary value exactly and `f64` needs all those digits to round-trip; .NET formats the `float`
    // itself and prints `1E-05`. Both the digit string and the exponent threshold therefore come
    // from the same `kind`.
    let plain = match kind {
        FloatKind::F32 => format!("{}", value as f32),
        FloatKind::F64 => format!("{value}"),
    };
    let (negative, digits) = match plain.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, plain.as_str()),
    };
    // Split the shortest representation into significant digits and a decimal exponent, where the
    // exponent is the position of the first significant digit relative to the decimal point.
    let (integer, fraction) = match digits.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (digits, ""),
    };
    let mut significant = String::with_capacity(integer.len() + fraction.len());
    significant.push_str(integer);
    significant.push_str(fraction);
    // Strip leading zeros, tracking how many were removed so the exponent stays correct.
    let leading_zeros = significant.len() - significant.trim_start_matches('0').len();
    let significant = significant.trim_start_matches('0');
    // Strip trailing zeros: the shortest form never relies on them, but `{}` can emit `.0`-free
    // integers such as `100`, where the expansion below would otherwise carry a zero exponent.
    let significant = significant.trim_end_matches('0');
    if significant.is_empty() {
        // The value is zero; `-0` keeps its sign and neither form uses an exponent.
        output.push_str(if negative { "-0" } else { "0" });
        return;
    }
    let integral_length = integer.len() as i64 - leading_zeros as i64;
    let exponent = integral_length - 1;
    let (low, high) = kind.scientific_thresholds();
    let use_scientific = exponent < low || exponent >= high;

    if negative {
        output.push('-');
    }
    if use_scientific {
        // One digit before the point, the rest after, then `E` and a signed two-digit minimum
        // exponent.
        let mut characters = significant.chars();
        let first = characters.next().expect("significant digits are non-empty");
        output.push(first);
        let rest: String = characters.collect();
        if !rest.is_empty() {
            output.push('.');
            output.push_str(&rest);
        }
        output.push('E');
        if exponent < 0 {
            output.push('-');
            let magnitude = exponent.unsigned_abs();
            let _ = write!(output, "{magnitude:02}");
        } else {
            output.push('+');
            let _ = write!(output, "{exponent:02}");
        }
        return;
    }

    // Fixed notation. The digit string is `significant`; the decimal point goes after
    // `exponent + 1` digits, padding with zeros on either side as needed.
    let point = exponent + 1;
    if point <= 0 {
        output.push_str("0.");
        for _ in 0..(-point) {
            output.push('0');
        }
        output.push_str(significant);
    } else if point as usize >= significant.len() {
        output.push_str(significant);
        for _ in significant.len()..point as usize {
            output.push('0');
        }
    } else {
        let (head, tail) = significant.split_at(point as usize);
        output.push_str(head);
        output.push('.');
        output.push_str(tail);
    }
}

/// Which threshold set applies, i.e. whether the value came from a `float` or a `double`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FloatKind {
    /// `float`: exponent form below -5 or at/above 9.
    F32,
    /// `double`: exponent form below -5 or at/above 17.
    F64,
}

impl FloatKind {
    /// The fixed-notation exponent range: fixed while `low <= exponent < high`.
    ///
    /// The lower bound is inclusive because .NET prints `1e-4f` as `0.0001` but `1e-5f` as
    /// `1E-05`, so scientific begins **at** exponent -5.
    const fn scientific_thresholds(self) -> (i64, i64) {
        match self {
            Self::F32 => (-4, 9),
            Self::F64 => (-4, 17),
        }
    }
}

/// JavaScript-string-encode `text` and wrap it in quotes (`HttpUtility.JavaScriptStringEncode`).
///
/// **This is not ordinary JSON escaping, and using the usual rules produces byte-different
/// output.** The reference's behaviour was captured from the runtime into
/// `json_escape_fixture.txt`; it differs from naive JSON escaping in four ways:
///
/// - every control character is written as `\uXXXX` with **uppercase** hex, so there is no `\n`,
///   `\r`, `\t`, `\b` or `\f` shorthand anywhere in the output;
/// - `<`, `>`, `&` and `'` are escaped as `\u003c`, `\u003e`, `\u0026` and `\u0027` even though
///   JSON does not require it;
/// - the JavaScript line separators `U+2028` and `U+2029` are escaped;
/// - non-ASCII text is left as-is rather than escaped.
///
/// `"` and `\` keep their short forms and `/` is never escaped. The fixture's `QUOTED` rows show
/// that the quoting overload escapes identically apart from adding the quotes, which is why both
/// call sites share this function.
fn write_json_string(output: &mut String, text: &str) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            // HTML-sensitive characters the encoder guards, then the JavaScript line separators.
            '<' => output.push_str("\\u003c"),
            '>' => output.push_str("\\u003e"),
            '&' => output.push_str("\\u0026"),
            '\'' => output.push_str("\\u0027"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            character if (character as u32) < 0x20 => {
                let _ = write!(output, "\\u{:04X}", character as u32);
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
        // The reference's encoder uses `\uXXXX` for control characters - there is no `\n` or
        // `\t` shorthand in its output. See the fixture for the full captured behaviour.
        assert_eq!(output, "\"a\\\"b\\\\c\\u000Ad\\u0009e\"");

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

    /// Replay the escaping the reference runtime actually performs.
    ///
    /// The previous version of this test asserted `\n` and `\t` because it had been written from
    /// this file's implementation rather than from the reference - the assertion and the code
    /// shared one mistake. These rows come from `tools/probes/json-escape`, which calls the real
    /// `HttpUtility.JavaScriptStringEncode`, so the expectation is independent of the port.
    #[test]
    fn json_escape_matches_the_reference_runtime_rows() {
        let fixture = include_str!("json_escape_fixture.txt");
        let mut checked = 0usize;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // `|` can appear inside the encoded output (it is never escaped), so split on the
            // first and last separators only.
            let first = line.find('|').expect("overload separator");
            let last = line.rfind('|').expect("description separator");
            let (tag, encoded, described) =
                (&line[..first], &line[first + 1..last], &line[last + 1..]);
            let input = unescape_description(described);
            match tag {
                // The walker always wraps in quotes, so it matches the QUOTED rows directly.
                "QUOTED" => {
                    let mut output = String::new();
                    write_json_string(&mut output, &input);
                    assert_eq!(output, encoded, "input {described}");
                }
                // The NOQUOTE rows are the same escaping without the surrounding quotes.
                "NOQUOTE" => {
                    let mut output = String::new();
                    write_json_string(&mut output, &input);
                    let inner = output
                        .strip_prefix('"')
                        .and_then(|rest| rest.strip_suffix('"'))
                        .expect("the walker always quotes");
                    assert_eq!(inner, encoded, "input {described}");
                }
                other => panic!("unexpected row tag {other}"),
            }
            checked += 1;
        }
        assert_eq!(checked, 34, "every recorded row is replayed");
    }

    /// Replay the float/double formatting the reference runtime performs.
    ///
    /// The port used Rust's `Display` and a comment asserted the two agree; they do not. These rows
    /// come from `tools/probes/float-format`, which calls the real
    /// `ToString(CultureInfo.InvariantCulture)` - the exact call the walker reaches - so the
    /// expectation is independent of the formatter under test.
    #[test]
    fn floats_match_the_reference_runtime_rows() {
        let fixture = include_str!("float_format_fixture.txt");
        let mut checked = 0usize;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            let mut output = String::new();
            match fields[0] {
                // The boundary rows name the intended exponent; the bits are not given, so the
                // value is rebuilt from the description.
                "BOUNDARY" => {
                    let kind = match fields[1] {
                        "f32" => FloatKind::F32,
                        "f64" => FloatKind::F64,
                        other => panic!("unexpected kind {other}"),
                    };
                    let exponent: i32 = fields[2]
                        .trim_start_matches("1e")
                        .parse()
                        .expect("boundary exponent");
                    let value = 10_f64.powi(exponent);
                    let value = match kind {
                        FloatKind::F32 => f64::from(value as f32),
                        FloatKind::F64 => value,
                    };
                    write_invariant_float(&mut output, value, kind);
                }
                "VALUE" => {
                    let (kind, value) = match fields[1] {
                        "f32" => (
                            FloatKind::F32,
                            f64::from(f32::from_bits(
                                u32::from_str_radix(fields[2], 16).expect("f32 bits"),
                            )),
                        ),
                        "f64" => (
                            FloatKind::F64,
                            f64::from_bits(u64::from_str_radix(fields[2], 16).expect("f64 bits")),
                        ),
                        other => panic!("unexpected kind {other}"),
                    };
                    write_invariant_float(&mut output, value, kind);
                }
                other => panic!("unexpected row tag {other}"),
            }
            assert_eq!(output, fields[3], "row {line}");
            checked += 1;
        }
        assert!(
            checked >= 75,
            "every recorded row is replayed, saw {checked}"
        );
    }

    /// Replay the reference's own callback output.
    ///
    /// `tools/probes/json-walker` instantiates the reference `DefaultJsonWalker` over a
    /// `StringWriter` and drives its public callbacks directly, so the expected text in
    /// `json_walker_shape_fixture.txt` comes from the reference rather than from this module. Every
    /// scenario below issues the same callback sequence the probe issued.
    ///
    /// The comparison normalises `\r\n` to `\n` on both sides, because the reference's line break is
    /// `Environment.NewLine` and the fixture was captured on Windows; `walker_uses_the_host_line_break`
    /// asserts the host-dependent half separately so this test stays valid on either platform.
    #[test]
    fn walker_shape_matches_the_reference_runtime_rows() {
        let fixture = include_str!("json_walker_shape_fixture.txt");
        let mut checked = 0usize;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let rest = line.strip_prefix("TEXT|").expect("TEXT row");
            let (name, escaped) = rest.split_once('|').expect("scenario and text");
            let expected = unescape_walker_text(escaped);
            assert!(
                !expected.starts_with("<threw"),
                "{name} is recorded as a probe failure: {expected}"
            );
            let actual = walker_scenario(name);
            assert_eq!(
                actual.replace("\r\n", "\n"),
                expected.replace("\r\n", "\n"),
                "scenario {name}"
            );
            checked += 1;
        }
        assert_eq!(checked, 13, "every recorded scenario is replayed");
    }

    /// The default line break is the host's, and an explicit one overrides it.
    ///
    /// Asserted separately from the fixture comparison because the fixture can only record the
    /// platform it was captured on. A hard-coded `\n` would pass the normalised comparison above
    /// while producing output that differs from the reference in every line on Windows; conversely
    /// a hard-coded `\r\n` would be wrong for the browser route, which sets `NewLine = "\n"`.
    #[test]
    fn walker_line_break_is_the_writers() {
        let host = if cfg!(windows) { "\r\n" } else { "\n" };
        assert_eq!(JsonWalker::new().line_break(), host);
        let mut walker = JsonWalker::new();
        walker.enter_asset();
        walker.enter_field("m_Name");
        walker.visit_value_inline(&SerializedValue::StringBytes(b"Cube".to_vec()));
        walker.exit_asset();
        assert_eq!(
            walker.finish(),
            format!("{{{host}\t\"m_Name\": \"Cube\"{host}}}")
        );

        // The browser route's configuration: `AssetAPI.GetJson` sets `NewLine = "\n"`.
        let mut walker = JsonWalker::with_line_break("\n");
        walker.enter_asset();
        walker.enter_field("m_Name");
        walker.visit_value_inline(&SerializedValue::StringBytes(b"Cube".to_vec()));
        walker.exit_asset();
        assert_eq!(walker.finish(), "{\n\t\"m_Name\": \"Cube\"\n}");
    }

    /// The indented traversal that gives `JsonWalker` its production caller.
    ///
    /// The expected text is built from the fixture-verified shape rules - one tab per level, a
    /// newline before every closing brace, and a comma at the end of the previous line - and written
    /// with `\n` because that is the line break the browser route configures. What this test does
    /// **not** establish is that the reference's `WalkStandard` visits an asset's fields in the same
    /// order the port's decoded `Struct` holds them; that remains the row's open question.
    #[test]
    fn decoded_values_render_as_the_reference_indented_body() {
        use SerializedFieldValue as Field;
        let value = SerializedValue::Struct(vec![
            Field {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"hero".to_vec()),
            },
            Field {
                name: "m_Data".to_owned(),
                value: SerializedValue::Array(vec![SerializedValue::U8(1), SerializedValue::U8(2)]),
            },
        ]);
        let mut walker = JsonWalker::with_line_break("\n");
        write_json_body(&mut walker, &value);
        assert_eq!(
            walker.finish(),
            "{\n\t\"m_Name\": \"hero\",\n\t\"m_Data\": [\n\t\t1,\n\t\t2\n\t]\n}"
        );

        // An empty array collapses onto one line and needs no exit, so it does not gain the
        // newline an empty *string* body would end with.
        let empty = SerializedValue::Struct(vec![Field {
            name: "m_Items".to_owned(),
            value: SerializedValue::Array(Vec::new()),
        }]);
        let mut walker = JsonWalker::with_line_break("\n");
        write_json_body(&mut walker, &empty);
        assert_eq!(walker.finish(), "{\n\t\"m_Items\": []\n}");

        // A nested object gains a level of indentation, and its closing brace sits at its own level.
        let nested = SerializedValue::Struct(vec![Field {
            name: "m_Inner".to_owned(),
            value: SerializedValue::Struct(vec![Field {
                name: "m_Leaf".to_owned(),
                value: SerializedValue::I32(7),
            }]),
        }]);
        let mut walker = JsonWalker::with_line_break("\n");
        write_json_body(&mut walker, &nested);
        assert_eq!(
            walker.finish(),
            "{\n\t\"m_Inner\": {\n\t\t\"m_Leaf\": 7\n\t}\n}"
        );
    }

    /// The port's callback sequence for one probe scenario.
    ///
    /// Note the shape of this API: positioning and value writing are separate, so a field value is
    /// written with [`JsonWalker::visit_value_inline`] after [`JsonWalker::enter_field`] has already
    /// positioned the entry. A list element or a top-level value uses [`JsonWalker::visit_value`],
    /// which performs the positioning itself. The reference splits the same work across its
    /// `Divide*` and `VisitPrimitive` callbacks.
    fn walker_scenario(name: &str) -> String {
        use SerializedValue::{Bytes, Char, StringBytes, F32, I32};
        let mut walker = JsonWalker::new();
        match name {
            // No callbacks at all: the probe created the walker and read it back.
            "empty-asset" => {}
            "one-field-string" => {
                walker.enter_asset();
                walker.enter_field("m_Name");
                walker.visit_value_inline(&StringBytes(b"Cube".to_vec()));
                walker.exit_asset();
            }
            "two-fields-divided" => {
                walker.enter_asset();
                walker.enter_field("m_A");
                walker.visit_value_inline(&I32(1));
                // `enter_field` positions the next entry, so no explicit divide is needed.
                walker.enter_field("m_B");
                walker.visit_value_inline(&SerializedValue::Bool(true));
                walker.exit_asset();
            }
            "empty-list" => {
                assert!(!walker.enter_list(0), "an empty list needs no exit");
            }
            "list-of-three" => {
                if walker.enter_list(3) {
                    walker.visit_value(&I32(1));
                    walker.visit_value(&I32(2));
                    walker.visit_value(&I32(3));
                    walker.exit_list();
                }
            }
            "empty-string-map" => {
                assert!(
                    !walker.enter_dictionary(0, true),
                    "an empty string-keyed dictionary needs no exit"
                );
            }
            "empty-int-map" => {
                assert!(
                    !walker.enter_dictionary(0, false),
                    "an empty non-string-keyed dictionary needs no exit"
                );
            }
            "string-map-one" => {
                if walker.enter_dictionary(1, true) {
                    walker.enter_dictionary_pair(true);
                    walker.visit_value_inline(&StringBytes(b"key".to_vec()));
                    walker.divide_dictionary_pair(true);
                    walker.visit_value_inline(&I32(5));
                    walker.exit_dictionary_pair(true);
                    walker.exit_dictionary(true);
                }
            }
            "int-map-one" => {
                if walker.enter_dictionary(1, false) {
                    if walker.enter_dictionary_pair(false) {
                        walker.visit_value_inline(&I32(7));
                        walker.divide_dictionary_pair(false);
                        walker.visit_value_inline(&I32(5));
                        walker.exit_dictionary_pair(false);
                    }
                    walker.exit_dictionary(false);
                }
            }
            "nested-list-in-field" => {
                walker.enter_asset();
                walker.enter_field("m_Values");
                if walker.enter_list(2) {
                    walker.visit_value(&I32(1));
                    walker.visit_value(&I32(2));
                    walker.exit_list();
                }
                walker.exit_asset();
            }
            "primitive-byte-array" => walker.visit_value_inline(&Bytes(vec![1, 2, 3, 4])),
            "primitive-float" => walker.visit_value_inline(&F32(1.5)),
            "primitive-char" => walker.visit_value_inline(&Char(0x41)),
            other => panic!("unexpected scenario {other}"),
        }
        walker.finish()
    }

    /// Reverse the fixture's `\r`, `\n`, `\t` and `\\` escaping.
    fn unescape_walker_text(text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut characters = text.chars();
        while let Some(character) = characters.next() {
            if character != '\\' {
                result.push(character);
                continue;
            }
            match characters.next() {
                Some('r') => result.push('\r'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                other => panic!("unexpected escape {other:?}"),
            }
        }
        result
    }

    /// Expand the fixture's `\uXXXX` prose form back into the character it describes.
    fn unescape_description(text: &str) -> String {
        let mut result = String::new();
        let mut rest = text;
        while let Some(index) = rest.find("\\u") {
            result.push_str(&rest[..index]);
            let hex = &rest[index + 2..index + 6];
            let code = u32::from_str_radix(hex, 16).expect("fixture hex");
            result.push(char::from_u32(code).expect("fixture code point"));
            rest = &rest[index + 6..];
        }
        result.push_str(rest);
        result
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
        let nl = default_line_break();
        assert_eq!(walker.finish(), format!("{{{nl}\t\"m_Width\": 2{nl}}}"));
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
        let nl = default_line_break();
        assert_eq!(
            walker.finish(),
            format!("{{{nl}\t\"m_Empty\": [],{nl}\t\"m_EmptyMap\": {{}}{nl}}}")
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
        let nl = default_line_break();
        assert_eq!(
            walker.finish(),
            format!("[{nl}\t{{{nl}\t\t\"Key\": 5,{nl}\t\t\"Value\": 6{nl}\t}}{nl}]")
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
