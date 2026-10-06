//! UI language codes ported from `AssetRipper.GUI.Localizations`.
//!
//! Ports the locale-independent rules: the default code, the hyphenated code
//! table derived from the `AssetRipper/Localizations/*.json` filenames, code
//! existence, the file-stem conversion tables, and the `LoadLanguage`
//! switch rule (null, current, and unknown codes leave the current language
//! alone; only a known different code switches and raises the change event).
//!
//! The 272-key string tables and their per-key en-US fallback stay with a future
//! UI surface: with no localized message consumer, vendoring six thousand strings
//! would be dead data. The settings-file `LanguageCode` field stays a validated-at-
//! startup string for the same reason (the source validates it in `LoadLanguage`,
//! not in the JSON reader). The `LanguageNameDictionary` native names stay out too:
//! the source computes them at build time from the builder's `CultureInfo` data,
//! which has no version-stable Rust equivalent, and they feed only the language
//! picker UI.

/// `LanguageCodes.English`, the default `CurrentLanguageCode`.
pub const ENGLISH_LANGUAGE_CODE: &str = "en-US";

/// The hyphenated language codes of `LanguageNameDictionary`, derived from the
/// locale file stems with `_` replaced by `-` (`GetFileNameWithoutExtension` plus
/// `Replace('_', '-')`), in filename order.
pub const LANGUAGE_CODES: &[&str] = &[
    "ar", "de", "el", "en-US", "es", "fa", "fr", "id", "it", "ja", "ko", "lt", "nl", "pl", "pt-BR",
    "ro", "ru", "th", "tr", "uk", "vi", "zh-Hans", "zh-Hant",
];

/// `LanguageCodes.AsHyphenatedLanguageCode`: the generated switch maps exactly the
/// locale file stems that contain an underscore; every other code is identity.
/// Only the four regional locales have underscored stems.
pub fn as_hyphenated_language_code(code: &str) -> &str {
    match code {
        "en_US" => "en-US",
        "pt_BR" => "pt-BR",
        "zh_Hans" => "zh-Hans",
        "zh_Hant" => "zh-Hant",
        _ => code,
    }
}

/// `LanguageCodes.AsUnderscoredLanguageCode`: the reverse table, used for the
/// locale file names; every other code is identity.
pub fn as_underscored_language_code(code: &str) -> &str {
    match code {
        "en-US" => "en_US",
        "pt-BR" => "pt_BR",
        "zh-Hans" => "zh_Hans",
        "zh-Hant" => "zh_Hant",
        _ => code,
    }
}

/// `LanguageCodes.Exists`: a non-null code the dictionary knows.
pub fn language_exists(code: &str) -> bool {
    LANGUAGE_CODES.contains(&code)
}

/// What `Localization.LoadLanguage` does with a requested code.
///
/// The change event fires exactly on a switch, so callers can observe it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum LoadLanguageOutcome {
    /// The current language is kept: the request was null, identical after
    /// normalization, or unknown. No change event fires.
    Unchanged,
    /// A known different code becomes current; the change event fires.
    Switched {
        /// The normalized code that becomes current.
        code: String,
    },
}

/// `Localization.LoadLanguage` as a pure decision over the current code and an
/// optional requested code: normalize, then switch only to a known different code.
///
/// The source never fails: null, identical, and unknown codes are all silent
/// no-ops, so this returns the outcome directly with no error half.
pub fn decide_load_language(current: &str, requested: Option<&str>) -> LoadLanguageOutcome {
    let Some(requested) = requested else {
        return LoadLanguageOutcome::Unchanged;
    };
    let normalized = as_hyphenated_language_code(requested);
    if normalized != current && language_exists(normalized) {
        LoadLanguageOutcome::Switched {
            code: normalized.to_owned(),
        }
    } else {
        LoadLanguageOutcome::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use super::{
        as_hyphenated_language_code, as_underscored_language_code, decide_load_language,
        language_exists, LoadLanguageOutcome, ENGLISH_LANGUAGE_CODE, LANGUAGE_CODES,
    };

    #[test]
    fn code_table_matches_the_locale_files() {
        assert_eq!(ENGLISH_LANGUAGE_CODE, "en-US");
        assert_eq!(LANGUAGE_CODES.len(), 23);
        assert_eq!(
            LANGUAGE_CODES,
            &[
                "ar", "de", "el", "en-US", "es", "fa", "fr", "id", "it", "ja", "ko", "lt", "nl",
                "pl", "pt-BR", "ro", "ru", "th", "tr", "uk", "vi", "zh-Hans", "zh-Hant",
            ]
        );
    }

    #[test]
    fn hyphenation_maps_file_stems_to_dictionary_codes() {
        assert_eq!(as_hyphenated_language_code("en_US"), "en-US");
        assert_eq!(as_hyphenated_language_code("pt_BR"), "pt-BR");
        assert_eq!(as_hyphenated_language_code("zh_Hans"), "zh-Hans");
        assert_eq!(as_hyphenated_language_code("zh_Hant"), "zh-Hant");
        assert_eq!(as_hyphenated_language_code("en-US"), "en-US");
        assert_eq!(as_hyphenated_language_code("fr"), "fr");
        assert_eq!(as_hyphenated_language_code(""), "");
        // Only real file stems map: unknown underscored codes are identity,
        // exactly like the generated switch default arm.
        assert_eq!(as_hyphenated_language_code("xx_YY"), "xx_YY");
    }

    #[test]
    fn underscoring_reverses_the_stem_table() {
        assert_eq!(as_underscored_language_code("en-US"), "en_US");
        assert_eq!(as_underscored_language_code("pt-BR"), "pt_BR");
        assert_eq!(as_underscored_language_code("zh-Hans"), "zh_Hans");
        assert_eq!(as_underscored_language_code("zh-Hant"), "zh_Hant");
        assert_eq!(as_underscored_language_code("fr"), "fr");
        assert_eq!(as_underscored_language_code("en_US"), "en_US");
        assert_eq!(as_underscored_language_code(""), "");
        // Both tables round-trip over every known code.
        for code in LANGUAGE_CODES {
            assert_eq!(
                as_hyphenated_language_code(as_underscored_language_code(code)),
                *code
            );
        }
    }

    #[test]
    fn existence_needs_a_known_hyphenated_code() {
        assert!(language_exists("en-US"));
        assert!(language_exists("zh-Hant"));
        // Every table code exists; the dictionary lookup is ordinally case-sensitive
        // like the source, so differently-cased codes do not.
        for code in LANGUAGE_CODES {
            assert!(language_exists(code), "{code}");
        }
        assert!(!language_exists("EN-US"));
        assert!(!language_exists("en_US"));
        assert!(!language_exists("pt_BR"));
        assert!(!language_exists("xx"));
        assert!(!language_exists(""));
    }

    #[test]
    fn load_language_switches_only_to_known_different_codes() {
        // Null and identical requests keep the language without an event.
        assert_eq!(
            decide_load_language("en-US", None),
            LoadLanguageOutcome::Unchanged
        );
        assert_eq!(
            decide_load_language("en-US", Some("en-US")),
            LoadLanguageOutcome::Unchanged
        );
        assert_eq!(
            decide_load_language("en-US", Some("en_US")),
            LoadLanguageOutcome::Unchanged
        );
        // A known different code switches.
        assert_eq!(
            decide_load_language("en-US", Some("fr")),
            LoadLanguageOutcome::Switched {
                code: "fr".to_owned()
            }
        );
        assert_eq!(
            decide_load_language("fr", Some("en_US")),
            LoadLanguageOutcome::Switched {
                code: "en-US".to_owned()
            }
        );
        // An unknown code is a silent no-op, exactly like the source.
        assert_eq!(
            decide_load_language("en-US", Some("xx")),
            LoadLanguageOutcome::Unchanged
        );
        assert_eq!(
            decide_load_language("fr", Some("xx")),
            LoadLanguageOutcome::Unchanged
        );
    }
}
