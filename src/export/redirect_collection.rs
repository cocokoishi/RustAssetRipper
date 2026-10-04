//! Redirect collection mechanics ported from `RedirectExportCollection`.
//!
//! Redirect collections never export files (`Export` throws, `Exportable` is false); they map
//! member assets to known pointers so references resolve to engine, script, or deleted-asset
//! targets. Two shapes exist:
//!
//! - `RedirectExportCollection` holds a redirection map plus a missing map. `Add` rejects a
//!   zero file ID and zero or missing-reference GUIDs; both maps reject duplicate members.
//!   `Assets` yields redirect members alone when nothing is missing, missing members alone
//!   when nothing redirects, otherwise redirect members followed by missing members. The
//!   display name is the single redirect member's best name, or the fixed
//!   `RedirectExportCollection` name otherwise (including the single-missing-member case).
//! - `SingleRedirectExportCollection` fixes one asset to one pointer with reference-identity
//!   membership: any other asset fails, and its file ID answers come straight from the
//!   pointer.
//!
//! Pointer resolution checks the missing map first (a class-ID missing reference with the
//! stored type), then the redirection map; local pointers always throw. `GetExportID` reads
//! the redirection map only, so missing-only members have no export ID.
//!
//! Members are caller-supplied keys (indices, path IDs): insertion order is preserved, and no
//! asset object model is required.

use super::export_type::ExportAssetType;

/// Which GUID class a redirect entry carries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RedirectGuid {
    /// `UnityGuid.Zero`: rejected by `Add`.
    Zero,
    /// `UnityGuid.MissingReference`: rejected by `Add`.
    MissingReference,
    /// Any real GUID, accepted by `Add`.
    Real,
}

/// One redirection-map entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RedirectEntry {
    pub member: usize,
    pub file_id: i64,
    pub guid: RedirectGuid,
    pub asset_type: ExportAssetType,
}

/// One missing-map entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MissingEntry {
    pub member: usize,
    pub asset_type: ExportAssetType,
}

/// Why a redirect operation fails.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RedirectError {
    /// `Add` with a zero file ID.
    ZeroFileId,
    /// `Add` with a zero or missing-reference GUID.
    ReservedGuid,
    /// The member is already present in either map.
    DuplicateMember,
    /// The member is in neither map.
    UnknownMember,
    /// Local pointers always throw.
    LocalReference,
}

impl core::fmt::Display for RedirectError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroFileId => write!(formatter, "redirect file IDs must be nonzero"),
            Self::ReservedGuid => write!(
                formatter,
                "redirect GUIDs must be neither zero nor the missing reference"
            ),
            Self::DuplicateMember => write!(formatter, "the member is already redirected"),
            Self::UnknownMember => write!(formatter, "the member is not redirected"),
            Self::LocalReference => write!(formatter, "redirect pointers are never local"),
        }
    }
}

impl std::error::Error for RedirectError {}

/// How a member pointer resolves.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RedirectPointer {
    /// The stored redirection-map pointer.
    Redirected {
        file_id: i64,
        asset_type: ExportAssetType,
    },
    /// A class-ID missing reference with the missing map's stored type.
    Missing { asset_type: ExportAssetType },
}

/// An insertion-ordered multi-redirect collection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RedirectCollection {
    redirect: Vec<RedirectEntry>,
    missing: Vec<MissingEntry>,
}

impl RedirectCollection {
    /// An empty redirect collection.
    pub fn new() -> Self {
        Self {
            redirect: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// Mirror `Contains`: either map counts.
    pub fn contains_member(&self, member: usize) -> bool {
        self.redirect.iter().any(|entry| entry.member == member)
            || self.missing.iter().any(|entry| entry.member == member)
    }

    /// Mirror `Add`: guard the file ID and GUID, then append to the redirection map.
    pub fn add(&mut self, entry: RedirectEntry) -> Result<(), RedirectError> {
        if entry.file_id == 0 {
            return Err(RedirectError::ZeroFileId);
        }
        if entry.guid != RedirectGuid::Real {
            return Err(RedirectError::ReservedGuid);
        }
        if self.contains_member(entry.member) {
            return Err(RedirectError::DuplicateMember);
        }
        self.redirect.push(entry);
        Ok(())
    }

    /// Mirror `AddMissing`: append to the missing map.
    pub fn add_missing(&mut self, entry: MissingEntry) -> Result<(), RedirectError> {
        if self.contains_member(entry.member) {
            return Err(RedirectError::DuplicateMember);
        }
        self.missing.push(entry);
        Ok(())
    }

    /// Mirror `Assets`: redirect members, then missing members, skipping the empty side.
    pub fn members(&self) -> impl Iterator<Item = usize> + '_ {
        self.redirect
            .iter()
            .map(|entry| entry.member)
            .chain(self.missing.iter().map(|entry| entry.member))
    }

    /// Mirror `GetExportID`: the redirection map's file ID only.
    pub fn export_id(&self, member: usize) -> Result<i64, RedirectError> {
        self.redirect
            .iter()
            .find(|entry| entry.member == member)
            .map(|entry| entry.file_id)
            .ok_or(RedirectError::UnknownMember)
    }

    /// Mirror `CreateExportPointer`: missing first, then redirection; never local.
    pub fn pointer(&self, member: usize, is_local: bool) -> Result<RedirectPointer, RedirectError> {
        if is_local {
            return Err(RedirectError::LocalReference);
        }
        if let Some(entry) = self.missing.iter().find(|entry| entry.member == member) {
            return Ok(RedirectPointer::Missing {
                asset_type: entry.asset_type,
            });
        }
        if let Some(entry) = self.redirect.iter().find(|entry| entry.member == member) {
            return Ok(RedirectPointer::Redirected {
                file_id: entry.file_id,
                asset_type: entry.asset_type,
            });
        }
        Err(RedirectError::UnknownMember)
    }
}

/// Mirror the `Name` rule: a single redirect member's best name, else the fixed name.
///
/// `single_best_name` carries the caller's rendered best name when exactly one redirect
/// member exists; `None` selects the fixed `RedirectExportCollection` name.
pub const fn redirect_collection_name(single_best_name: Option<&'static str>) -> &'static str {
    match single_best_name {
        Some(name) => name,
        None => "RedirectExportCollection",
    }
}

/// A fixed single-asset redirect with reference-identity membership.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SingleRedirect {
    pub member: usize,
    pub file_id: i64,
    pub asset_type: ExportAssetType,
}

impl SingleRedirect {
    /// Mirror `Contains`: reference identity with the fixed member.
    pub const fn contains(self, member: usize) -> bool {
        self.member == member
    }

    /// Mirror `GetExportID`: the pointer's file ID for the member, else an error.
    pub fn export_id(self, member: usize) -> Result<i64, RedirectError> {
        if self.contains(member) {
            Ok(self.file_id)
        } else {
            Err(RedirectError::UnknownMember)
        }
    }

    /// Mirror `CreateExportPointer`: the fixed pointer for the member, never local.
    pub fn pointer(self, member: usize, is_local: bool) -> Result<RedirectPointer, RedirectError> {
        if is_local {
            return Err(RedirectError::LocalReference);
        }
        if self.contains(member) {
            Ok(RedirectPointer::Redirected {
                file_id: self.file_id,
                asset_type: self.asset_type,
            })
        } else {
            Err(RedirectError::UnknownMember)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::export_type::ExportAssetType::{Internal, Meta};
    use super::{
        redirect_collection_name, RedirectCollection, RedirectEntry, RedirectError, RedirectGuid,
        RedirectPointer, SingleRedirect,
    };

    fn redirect(member: usize, file_id: i64) -> RedirectEntry {
        RedirectEntry {
            member,
            file_id,
            guid: RedirectGuid::Real,
            asset_type: Internal,
        }
    }

    #[test]
    fn add_guards_reject_reserved_ids_and_duplicates() {
        let mut collection = RedirectCollection::new();
        assert_eq!(
            collection.add(RedirectEntry {
                file_id: 0,
                ..redirect(1, 0)
            }),
            Err(RedirectError::ZeroFileId)
        );
        for guid in [RedirectGuid::Zero, RedirectGuid::MissingReference] {
            assert_eq!(
                collection.add(RedirectEntry {
                    guid,
                    ..redirect(1, 10)
                }),
                Err(RedirectError::ReservedGuid)
            );
        }
        assert_eq!(collection.add(redirect(1, 10)), Ok(()));
        assert_eq!(
            collection.add(redirect(1, 11)),
            Err(RedirectError::DuplicateMember)
        );
        assert!(collection.contains_member(1));
        assert!(!collection.contains_member(2));
    }

    #[test]
    fn members_follow_redirect_then_missing_order() {
        let mut both = RedirectCollection::new();
        both.add(redirect(1, 10)).unwrap();
        both.add_missing(super::MissingEntry {
            member: 2,
            asset_type: Meta,
        })
        .unwrap();
        both.add(redirect(3, 30)).unwrap();
        assert_eq!(both.members().collect::<Vec<_>>(), [1, 3, 2]);

        let mut redirect_only = RedirectCollection::new();
        redirect_only.add(redirect(5, 50)).unwrap();
        assert_eq!(redirect_only.members().collect::<Vec<_>>(), [5]);

        let mut missing_only = RedirectCollection::new();
        missing_only
            .add_missing(super::MissingEntry {
                member: 7,
                asset_type: Meta,
            })
            .unwrap();
        assert_eq!(missing_only.members().collect::<Vec<_>>(), [7]);

        // The name is the single redirect member's best name, else the fixed name.
        assert_eq!(redirect_collection_name(Some("Main Asset")), "Main Asset");
        assert_eq!(redirect_collection_name(None), "RedirectExportCollection");
    }

    #[test]
    fn pointers_check_missing_first_and_never_local() {
        let mut collection = RedirectCollection::new();
        collection.add(redirect(1, 10)).unwrap();
        collection
            .add_missing(super::MissingEntry {
                member: 2,
                asset_type: Meta,
            })
            .unwrap();
        assert_eq!(
            collection.pointer(1, false),
            Ok(RedirectPointer::Redirected {
                file_id: 10,
                asset_type: Internal,
            })
        );
        assert_eq!(
            collection.pointer(2, false),
            Ok(RedirectPointer::Missing { asset_type: Meta })
        );
        // GetExportID reads the redirection map only.
        assert_eq!(collection.export_id(1), Ok(10));
        assert_eq!(collection.export_id(2), Err(RedirectError::UnknownMember));
        assert_eq!(
            collection.pointer(9, false),
            Err(RedirectError::UnknownMember)
        );
        assert_eq!(
            collection.pointer(1, true),
            Err(RedirectError::LocalReference)
        );
    }

    #[test]
    fn single_redirect_pins_one_member_to_one_pointer() {
        let single = SingleRedirect {
            member: 4,
            file_id: 40,
            asset_type: Internal,
        };
        assert!(single.contains(4));
        assert!(!single.contains(5));
        assert_eq!(single.export_id(4), Ok(40));
        assert_eq!(single.export_id(5), Err(RedirectError::UnknownMember));
        assert_eq!(
            single.pointer(4, false),
            Ok(RedirectPointer::Redirected {
                file_id: 40,
                asset_type: Internal,
            })
        );
        assert_eq!(single.pointer(5, false), Err(RedirectError::UnknownMember));
        assert_eq!(single.pointer(4, true), Err(RedirectError::LocalReference));
    }
}
