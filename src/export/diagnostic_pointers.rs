//! Reference semantics of the diagnostic export collections.
//!
//! `SkipExportCollection`, `FailExportCollection`, and `EmptyExportCollection` never produce
//! asset files, but they still participate in reference resolution, and the baseline keeps
//! those rules explicit rather than nulling references:
//!
//! - `FailExportCollection.Export` logs a warning naming the asset and returns `false`; it
//!   never throws and never writes. `Skip` and `Empty` throw `NotSupportedException` from
//!   `Export`, but both report `Exportable == false`, so the project loop never calls it.
//! - A non-local reference to a skipped or failed asset becomes an explicit missing-reference
//!   pointer: the main export ID plus `UnityGuid.MissingReference` plus the collection
//!   exporter's `ToExportType`. References are never silently dropped or nulled.
//! - A local reference to a skipped or failed asset throws (`ArgumentException`); every
//!   `Empty` member access throws `NotSupportedException`, including `Contains`, which always
//!   answers false while `Skip`/`Fail` match by asset identity.
//! - Display names differ: `Skip` uses the asset type name, `Fail` uses the best asset name,
//!   and `Empty` is the fixed `EmptyExportCollection` singleton name.
//!
//! Exportability, `.meta` emission, and destination rules live in `collection_kind.rs`; this
//! module covers only the export outcomes, pointer mapping, names, and membership above. The
//! GUID and type values flow in from `export_type.rs` and `ids.rs` through the caller.

use super::export_type::ExportAssetType;

/// The three diagnostic collection families.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticFamily {
    /// `SkipExportCollection`: skipped assets with missing-reference pointers.
    Skip,
    /// `FailExportCollection`: warning plus `false` on export, same pointer mapping.
    Fail,
    /// `EmptyExportCollection`: the throwing singleton.
    Empty,
}

/// What calling `Export` on a diagnostic collection does.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticExportOutcome {
    /// Throws `NotSupportedException` (`Skip`, `Empty`).
    ThrowsNotSupported,
    /// Logs a warning naming the asset and returns `false` (`Fail`).
    WarnsAndReturnsFalse,
}

impl DiagnosticFamily {
    /// The `Export` outcome for this family.
    pub const fn export_outcome(self) -> DiagnosticExportOutcome {
        match self {
            Self::Skip | Self::Empty => DiagnosticExportOutcome::ThrowsNotSupported,
            Self::Fail => DiagnosticExportOutcome::WarnsAndReturnsFalse,
        }
    }

    /// The display `Name` for this family.
    ///
    /// `Skip` reports the asset type name, `Fail` the best asset name, and `Empty` its fixed
    /// singleton name; both names arrive rendered by the caller.
    pub const fn display_name(
        self,
        asset_type_name: &'static str,
        best_asset_name: &'static str,
    ) -> &'static str {
        match self {
            Self::Skip => asset_type_name,
            Self::Fail => best_asset_name,
            Self::Empty => "EmptyExportCollection",
        }
    }

    /// The `Contains` answer for a same-asset probe.
    ///
    /// `Skip` and `Fail` match by asset identity; `Empty` always answers false.
    pub const fn contains_same_asset(self) -> bool {
        match self {
            Self::Skip | Self::Fail => true,
            Self::Empty => false,
        }
    }
}

/// A reference that cannot be resolved to exported bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticPointer {
    /// A non-local reference maps to the main export ID plus the missing-reference GUID
    /// plus the collection exporter's pointer type.
    MissingReference {
        main_export_id: i64,
        asset_type: ExportAssetType,
    },
}

/// Why a diagnostic pointer cannot be produced.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiagnosticPointerError {
    /// Local references to skipped or failed assets throw `ArgumentException`.
    LocalReference,
    /// Every `Empty` member access throws `NotSupportedException`.
    EmptyCollection,
}

impl core::fmt::Display for DiagnosticPointerError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LocalReference => write!(
                formatter,
                "local references to skipped or failed assets are not supported"
            ),
            Self::EmptyCollection => {
                write!(formatter, "the empty export collection has no members")
            }
        }
    }
}

impl std::error::Error for DiagnosticPointerError {}

/// Mirror `Skip`/`Fail`/`Empty` `CreateExportPointer`.
///
/// `is_local` selects the throwing local path; otherwise the caller-supplied main export ID
/// (from `ids.rs`) and exporter pointer type (from `export_type.rs`) form the explicit
/// missing-reference pointer.
pub fn diagnostic_pointer(
    family: DiagnosticFamily,
    is_local: bool,
    main_export_id: i64,
    asset_type: ExportAssetType,
) -> Result<DiagnosticPointer, DiagnosticPointerError> {
    match family {
        DiagnosticFamily::Empty => Err(DiagnosticPointerError::EmptyCollection),
        DiagnosticFamily::Skip | DiagnosticFamily::Fail => {
            if is_local {
                Err(DiagnosticPointerError::LocalReference)
            } else {
                Ok(DiagnosticPointer::MissingReference {
                    main_export_id,
                    asset_type,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::export_type::ExportAssetType;
    use super::{
        diagnostic_pointer, DiagnosticExportOutcome, DiagnosticFamily, DiagnosticPointer,
        DiagnosticPointerError,
    };

    #[test]
    fn export_outcomes_match_throw_warn_and_false_rules() {
        assert_eq!(
            DiagnosticFamily::Skip.export_outcome(),
            DiagnosticExportOutcome::ThrowsNotSupported
        );
        assert_eq!(
            DiagnosticFamily::Empty.export_outcome(),
            DiagnosticExportOutcome::ThrowsNotSupported
        );
        assert_eq!(
            DiagnosticFamily::Fail.export_outcome(),
            DiagnosticExportOutcome::WarnsAndReturnsFalse
        );
    }

    #[test]
    fn non_local_references_become_explicit_missing_references() {
        // Skipped and failed assets keep their main export ID and exporter type; only the
        // GUID becomes the missing-reference sentinel, never null.
        for family in [DiagnosticFamily::Skip, DiagnosticFamily::Fail] {
            assert_eq!(
                diagnostic_pointer(family, false, 2_100_000, ExportAssetType::Serialized),
                Ok(DiagnosticPointer::MissingReference {
                    main_export_id: 2_100_000,
                    asset_type: ExportAssetType::Serialized,
                })
            );
            assert_eq!(
                diagnostic_pointer(family, false, 3_000_003, ExportAssetType::Meta),
                Ok(DiagnosticPointer::MissingReference {
                    main_export_id: 3_000_003,
                    asset_type: ExportAssetType::Meta,
                })
            );
            // Local references throw instead of resolving.
            assert_eq!(
                diagnostic_pointer(family, true, 2_100_000, ExportAssetType::Serialized),
                Err(DiagnosticPointerError::LocalReference)
            );
        }
        // The empty singleton throws on every pointer access.
        assert_eq!(
            diagnostic_pointer(DiagnosticFamily::Empty, false, 0, ExportAssetType::Meta),
            Err(DiagnosticPointerError::EmptyCollection)
        );
        assert_eq!(
            diagnostic_pointer(DiagnosticFamily::Empty, true, 0, ExportAssetType::Meta),
            Err(DiagnosticPointerError::EmptyCollection)
        );
    }

    #[test]
    fn names_and_membership_follow_family_rules() {
        assert_eq!(
            DiagnosticFamily::Skip.display_name("Texture2D", "Brick"),
            "Texture2D"
        );
        assert_eq!(
            DiagnosticFamily::Fail.display_name("Texture2D", "Brick"),
            "Brick"
        );
        assert_eq!(
            DiagnosticFamily::Empty.display_name("Texture2D", "Brick"),
            "EmptyExportCollection"
        );
        assert!(DiagnosticFamily::Skip.contains_same_asset());
        assert!(DiagnosticFamily::Fail.contains_same_asset());
        assert!(!DiagnosticFamily::Empty.contains_same_asset());
    }
}
