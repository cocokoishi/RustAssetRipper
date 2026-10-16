//! Message tables for the product web surface (port of `AssetRipper.GUI.Localizations`).
//!
//! The frozen product embeds one locale file per language and generates a property per message
//! key from `en_US.json`; a language that does not translate a key falls back to the English
//! string. The port vendors the same files under `src/web/localizations` (MIT licensed, see the
//! `LICENSE` beside them), loads them once at startup with the shared JSON reader, and applies
//! the same `en_US` fallback.
//!
//! The language list is derived from the vendored file stems with `_` mapped to `-`, exactly
//! like `LanguageCodes.AsHyphenatedLanguageCode`; the display names come from the frozen
//! baseline's generated culture table (`.NET` `CultureInfo.NativeName`), which is not
//! reproducible from the locale files themselves.

use std::borrow::Cow;
use std::error::Error;
use std::fmt;

use assetripper::json::{parse_json, JsonLimits};

/// The vendored locale files, in `LANGUAGE_CODES` order.
const LOCALE_FILES: &[(&str, &str)] = &[
    ("ar", include_str!("localizations/ar.json")),
    ("de", include_str!("localizations/de.json")),
    ("el", include_str!("localizations/el.json")),
    ("en-US", include_str!("localizations/en_US.json")),
    ("es", include_str!("localizations/es.json")),
    ("fa", include_str!("localizations/fa.json")),
    ("fr", include_str!("localizations/fr.json")),
    ("id", include_str!("localizations/id.json")),
    ("it", include_str!("localizations/it.json")),
    ("ja", include_str!("localizations/ja.json")),
    ("ko", include_str!("localizations/ko.json")),
    ("lt", include_str!("localizations/lt.json")),
    ("nl", include_str!("localizations/nl.json")),
    ("pl", include_str!("localizations/pl.json")),
    ("pt-BR", include_str!("localizations/pt_BR.json")),
    ("ro", include_str!("localizations/ro.json")),
    ("ru", include_str!("localizations/ru.json")),
    ("th", include_str!("localizations/th.json")),
    ("tr", include_str!("localizations/tr.json")),
    ("uk", include_str!("localizations/uk.json")),
    ("vi", include_str!("localizations/vi.json")),
    ("zh-Hans", include_str!("localizations/zh_Hans.json")),
    ("zh-Hant", include_str!("localizations/zh_Hant.json")),
];

/// Language codes to display names, extracted from the frozen baseline's generated culture
/// table.
const LANGUAGE_NAMES: &str = include_str!("localizations/language_names.json");

/// The language whose strings every other language falls back to.
pub const FALLBACK_LANGUAGE: &str = "en-US";

/// Wraps a message key that is missing from every vendored locale file. Rendering tests assert
/// this never reaches a page, which is how a mistyped key is caught.
pub const MISSING_MARKER_PREFIX: &str = "\u{27e6}missing:";
/// Closes [`MISSING_MARKER_PREFIX`].
pub const MISSING_MARKER_SUFFIX: &str = "\u{27e7}";

/// Every message table the product can render, plus the display names of the languages.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Messages {
    english: Vec<(String, String)>,
    tables: Vec<(String, Vec<(String, String)>)>,
    names: Vec<(String, String)>,
}

/// A vendored locale file could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessagesError(String);

impl fmt::Display for MessagesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for MessagesError {}

impl Messages {
    /// Load every vendored table. The files are compiled into the binary, so a failure means
    /// the repository data itself is malformed and the server must not start silently.
    pub fn load() -> Result<Self, MessagesError> {
        let limits = JsonLimits::default();
        let names = collect_pairs(LANGUAGE_NAMES, "language_names.json", limits)?;
        let mut tables = Vec::with_capacity(LOCALE_FILES.len());
        for (code, text) in LOCALE_FILES {
            let entries = collect_pairs(text, code, limits)?;
            tables.push(((*code).to_owned(), entries));
        }
        let english = tables
            .iter()
            .find(|(code, _)| code == FALLBACK_LANGUAGE)
            .map(|(_, entries)| entries.clone())
            .ok_or_else(|| {
                MessagesError(format!("the vendored {FALLBACK_LANGUAGE} table is missing"))
            })?;
        Ok(Self {
            english,
            tables,
            names,
        })
    }

    /// The language codes with their display names, in code order.
    pub fn languages(&self) -> impl Iterator<Item = (&str, &str)> {
        self.names
            .iter()
            .map(|(code, name)| (code.as_str(), name.as_str()))
    }

    /// The number of message keys the fallback language defines.
    ///
    /// Only the tests read the count; pages render through [`Messages::text`].
    #[cfg(test)]
    pub fn key_count(&self) -> usize {
        self.english.len()
    }

    /// The text of `key` in `code`, falling back to English and then to the missing marker.
    pub fn text(&self, code: &str, key: &str) -> Cow<'_, str> {
        if code != FALLBACK_LANGUAGE {
            if let Some((_, entries)) = self.tables.iter().find(|(candidate, _)| candidate == code)
            {
                if let Some((_, value)) = entries.iter().find(|(candidate, _)| candidate == key) {
                    return Cow::Borrowed(value);
                }
            }
        }
        if let Some((_, value)) = self.english.iter().find(|(candidate, _)| candidate == key) {
            return Cow::Borrowed(value);
        }
        Cow::Owned(format!(
            "{MISSING_MARKER_PREFIX}{key}{MISSING_MARKER_SUFFIX}"
        ))
    }
}

fn collect_pairs(
    text: &str,
    source: &str,
    limits: JsonLimits,
) -> Result<Vec<(String, String)>, MessagesError> {
    let value = parse_json(text, limits).map_err(|error| {
        MessagesError(format!("the vendored {source} table is invalid: {error}"))
    })?;
    let members = value.as_object("$").map_err(|error| {
        MessagesError(format!(
            "the vendored {source} table is not a JSON object: {error}"
        ))
    })?;
    let mut pairs = Vec::with_capacity(members.len());
    for (key, value) in members {
        let text = value.as_str(key).map_err(|error| {
            MessagesError(format!(
                "the vendored {source} table entry '{key}' is not a string: {error}"
            ))
        })?;
        pairs.push((key.clone(), text.to_owned()));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::{Messages, FALLBACK_LANGUAGE, MISSING_MARKER_PREFIX, MISSING_MARKER_SUFFIX};
    use assetripper::localization::LANGUAGE_CODES;
    fn messages() -> Messages {
        Messages::load().expect("the vendored tables load")
    }

    #[test]
    fn every_language_code_has_a_vendored_file_and_a_name() {
        let messages = messages();
        let vendored: Vec<&str> = messages.languages().map(|(code, _)| code).collect();
        assert_eq!(vendored, LANGUAGE_CODES);
        for (code, name) in messages.languages() {
            assert!(!name.is_empty(), "{code} has no display name");
        }
    }

    #[test]
    fn english_defines_the_key_set_and_other_languages_fall_back_to_it() {
        let messages = messages();
        assert_eq!(messages.key_count(), 272);
        let english = messages.text(FALLBACK_LANGUAGE, "welcome").into_owned();
        assert!(!english.is_empty());
        assert!(!english.starts_with(MISSING_MARKER_PREFIX));
        // The Indonesian table is empty upstream, so every key resolves to English.
        assert_eq!(messages.text("id", "welcome"), english);
        assert_eq!(
            messages.text("id", "asset_ripper_free"),
            messages.text(FALLBACK_LANGUAGE, "asset_ripper_free")
        );
        // A translated table returns its own text.
        let german = messages.text("de", "welcome").into_owned();
        assert!(!german.starts_with(MISSING_MARKER_PREFIX));
    }

    #[test]
    fn an_unknown_key_is_reported_instead_of_rendering_empty_text() {
        let messages = messages();
        let missing = messages.text(FALLBACK_LANGUAGE, "definitely_not_a_key");
        assert_eq!(
            missing,
            format!("{MISSING_MARKER_PREFIX}definitely_not_a_key{MISSING_MARKER_SUFFIX}")
        );
    }

    #[test]
    fn every_key_resolves_in_every_language_without_a_marker() {
        use assetripper::json::{parse_json, JsonLimits};

        let messages = messages();
        let english = parse_json(
            include_str!("localizations/en_US.json"),
            JsonLimits::default(),
        )
        .expect("the English table parses");
        let keys: Vec<&str> = english
            .as_object("$")
            .expect("the English table is an object")
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys.len(), messages.key_count());
        for code in LANGUAGE_CODES {
            for key in &keys {
                let text = messages.text(code, key);
                assert!(
                    !text.starts_with(MISSING_MARKER_PREFIX),
                    "language {code} has no text for key {key}"
                );
            }
        }
    }

    #[test]
    fn language_display_names_are_the_baseline_culture_names() {
        let messages = messages();
        let name = |code: &str| {
            messages
                .languages()
                .find(|(candidate, _)| *candidate == code)
                .map(|(_, name)| name.to_owned())
                .expect("the language is registered")
        };
        assert_eq!(name("en-US"), "English (United States)");
        assert_eq!(name("de"), "Deutsch");
        assert_eq!(name("zh-Hans"), "中文（简体）");
    }
}
