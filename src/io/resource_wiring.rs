//! Production wiring between decoded object payloads and ResourceFile-backed exporters.
//!
//! Baseline chain this module completes, in source order:
//!
//! 1. `StreamedResourceExtensions.CheckIntegrity` / `GetContent`
//!    (`AssetRipper.SourceGenerated.Extensions/StreamedResourceExtensions.cs:9-83`) resolve a
//!    decoded `StreamingInfo` through `collection.Bundle.ResolveResource(path)` and then read the
//!    exact `offset`/`size` range, refusing empty paths, a zero size, signed-range overflow, a
//!    missing file, and a range past the file end.
//! 2. `YamlStreamedAssetExportCollection.Prepare` copies that range into the asset's inline
//!    byte-data field and clears the stream metadata before serialization.
//!
//! The lookup and range kernels already live in [`crate::resource`]; this module is the caller that
//! joins them to a decoded object value: it locates the version-specific streaming field in the
//! decoded payload, resolves the resource across the mounted bundle hierarchy, the explicit root
//! catalog, and the external platform/mixed provider, reads the validated range, and then hands the
//! bytes to the exporter staging pass. A missing resource stays a distinct, reported outcome, so an
//! exporter can neither silently drop the payload nor inline the wrong bytes.
//!
//! Wire the entry point [`stage_workspace_streamed_yaml_asset`] into the project export path; the
//! crate-level entry point is also re-exported from `crate::export::io_wiring`.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::export::streamed_assets::{
    prepare_streamed_yaml_asset, StreamedYamlAssetError, StreamedYamlAssetKind,
    StreamedYamlAssetLimits, StreamedYamlAssetPreparation,
};
use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::{
    PlatformResourceProvider, PlatformResourceSource, PlatformStructureLimits,
};
use crate::resource::{
    find_streamed_resource_infos, resolve_and_read_streamed_resource_info, ResolvedResourceFile,
    RootResourceFileCatalog, StreamedResourceContentError, StreamedResourceContentLimits,
    StreamedResourceContentResolution, StreamedResourceInfo, StreamedResourceScanError,
    StreamedResourceScanLimits,
};
use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::workspace::SerializedFileWorkspace;

/// The decoded StreamingInfo field name a baseline YAML collection reads for each asset kind.
///
/// `YamlStreamedAssetExportCollection.ExportMesh` reads the generated Mesh `StreamData` field.
/// Its serialized spelling is `m_StreamData`; the `IImageTexture` interface is implemented by
/// several generated classes whose generated member names are class-suffixed even though current
/// TypeTrees serialize the common field as `m_StreamData`.
pub const fn streamed_asset_stream_field_names(
    kind: StreamedYamlAssetKind,
) -> &'static [&'static str] {
    match kind {
        StreamedYamlAssetKind::Mesh => &["m_StreamData"],
        StreamedYamlAssetKind::ImageTexture => &[
            "m_StreamData_C189",
            "m_StreamData_C188",
            "m_StreamData_C187",
            "m_StreamData_C117",
            "m_StreamData_C89",
            "m_StreamData_C28",
            "m_StreamData",
        ],
    }
}

/// A decoded object's streaming field, its resolved resource, and the exporter staging result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceStreamedAssetReport {
    /// The prepared exporter result (the asset value has already been staged in place).
    pub preparation: StreamedYamlAssetPreparation,
    /// The decoded field path the wiring selected, when the payload owns a streaming field.
    pub stream_field_path: Option<String>,
    /// The VFS path the resource resolved to, when a resource was actually resolved.
    pub resolved_virtual_path: Option<String>,
    /// The external provider that supplied the resource, when it came from one.
    pub resolved_external_source: Option<PlatformResourceSource>,
    /// DataPaths the provider searched without finding the resource.
    pub unavailable_data_paths: Vec<String>,
}

/// Why a decoded object's streaming field could not be turned into exporter input.
#[derive(Debug)]
pub enum WorkspaceStreamedAssetError {
    /// The bounded nested StreamingInfo scan rejected the payload.
    Scan(StreamedResourceScanError),
    /// More than one field of the expected name exists in the decoded payload.
    AmbiguousStreamField {
        role: &'static str,
        first: String,
        second: String,
    },
    /// The resource was not found anywhere the bundle hierarchy or provider can see.
    ResourceMissing {
        path: String,
        unavailable_data_paths: Vec<String>,
    },
    /// The lookup or the bounded range read failed.
    Resource(StreamedResourceContentError),
    /// The exporter staging pass rejected the asset's streamed shape.
    Staging(StreamedYamlAssetError),
}

impl Display for WorkspaceStreamedAssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan(source) => write!(formatter, "streamed asset scan failed: {source}"),
            Self::AmbiguousStreamField {
                role,
                first,
                second,
            } => write!(
                formatter,
                "streamed asset has multiple {role} fields: '{first}' and '{second}'"
            ),
            Self::ResourceMissing {
                path,
                unavailable_data_paths,
            } => {
                write!(
                    formatter,
                    "streamed asset resource '{path}' was not found in the bundle hierarchy or external DataPaths"
                )?;
                if !unavailable_data_paths.is_empty() {
                    write!(
                        formatter,
                        " (unavailable DataPaths: {})",
                        unavailable_data_paths.join(", ")
                    )?;
                }
                Ok(())
            }
            Self::Resource(source) => {
                write!(
                    formatter,
                    "streamed asset resource resolution failed: {source}"
                )
            }
            Self::Staging(source) => write!(formatter, "streamed asset staging failed: {source}"),
        }
    }
}

impl Error for WorkspaceStreamedAssetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Scan(source) => Some(source),
            Self::Resource(source) => Some(source),
            Self::Staging(source) => Some(source),
            Self::AmbiguousStreamField { .. } | Self::ResourceMissing { .. } => None,
        }
    }
}

/// Selects the decoded streaming field of one asset kind from an object payload.
///
/// The payload is scanned once with the bounded nested scan, then filtered to the field name the
/// baseline collection reads for `kind`. Duplicate matches are reported instead of picking one,
/// matching the exporter's field-alias ambiguity handling.
pub fn select_streamed_asset_info(
    root: &SerializedValue,
    kind: StreamedYamlAssetKind,
    scan_limits: StreamedResourceScanLimits,
) -> Result<Option<(String, StreamedResourceInfo)>, WorkspaceStreamedAssetError> {
    let names = streamed_asset_stream_field_names(kind);
    let found = find_streamed_resource_infos(root, scan_limits)
        .map_err(WorkspaceStreamedAssetError::Scan)?;
    let mut selected: Option<(String, StreamedResourceInfo)> = None;
    for located in found {
        let leaf = located
            .field_path
            .rsplit('.')
            .next()
            .unwrap_or(located.field_path.as_str());
        if !names.contains(&leaf) {
            continue;
        }
        if let Some((first, _)) = &selected {
            return Err(WorkspaceStreamedAssetError::AmbiguousStreamField {
                role: "streaming",
                first: first.clone(),
                second: located.field_path,
            });
        }
        selected = Some((located.field_path, located.info));
    }
    Ok(selected)
}

/// Resolves a decoded streaming field to its bounded byte range through the production lookup.
///
/// This is the caller-facing form of `StreamedResourceExtensions.GetContent`: the bundle-hierarchy
/// lookup, the explicit root catalog, and the external platform/mixed provider are consulted in
/// source order, and the returned bytes are exactly the declared `offset`/`size` range.
pub fn read_object_streamed_resource_bytes<'a>(
    filesystem: &'a VirtualFileSystem,
    workspace: &'a SerializedFileWorkspace,
    source_bundle_index: Option<usize>,
    root_catalog: &'a RootResourceFileCatalog,
    provider: &PlatformResourceProvider<'_>,
    info: &StreamedResourceInfo,
    limits: StreamedResourceContentLimits,
) -> Result<StreamedResourceContentResolution<'a>, StreamedResourceContentError> {
    resolve_and_read_streamed_resource_info(
        filesystem,
        workspace,
        source_bundle_index,
        info,
        root_catalog,
        provider,
        limits,
    )
}

/// Resolves, reads, and stages one decoded object's version-specific streamed payload for export.
///
/// `root` must be an export-owned copy of the decoded object value; it is mutated exactly as the
/// baseline YAML collection mutates the asset before writing (inline bytes set, stream metadata
/// cleared). The returned report carries the resource identity used, and a payload whose declared
/// resource cannot be found fails with [`WorkspaceStreamedAssetError::ResourceMissing`] rather than
/// exporting the object with its stream silently dropped.
#[allow(clippy::too_many_arguments)]
pub fn stage_workspace_streamed_yaml_asset<'a>(
    root: &mut SerializedValue,
    kind: StreamedYamlAssetKind,
    filesystem: &'a VirtualFileSystem,
    workspace: &'a SerializedFileWorkspace,
    source_bundle_index: Option<usize>,
    root_catalog: &'a RootResourceFileCatalog,
    provider: &PlatformResourceProvider<'_>,
    scan_limits: StreamedResourceScanLimits,
    content_limits: StreamedResourceContentLimits,
    staging_limits: StreamedYamlAssetLimits,
) -> Result<WorkspaceStreamedAssetReport, WorkspaceStreamedAssetError> {
    let selected = select_streamed_asset_info(root, kind, scan_limits)?;
    let mut report = WorkspaceStreamedAssetReport {
        preparation: StreamedYamlAssetPreparation::NoStreamField,
        stream_field_path: selected.as_ref().map(|(path, _)| path.clone()),
        resolved_virtual_path: None,
        resolved_external_source: None,
        unavailable_data_paths: Vec::new(),
    };

    let Some((_, info)) = selected else {
        // The staging pass still owns the "no stream field" and "unset stream" outcomes.
        report.preparation = prepare_streamed_yaml_asset(root, kind, None, staging_limits)
            .map_err(WorkspaceStreamedAssetError::Staging)?;
        return Ok(report);
    };

    if !info.is_set() {
        report.preparation = prepare_streamed_yaml_asset(root, kind, None, staging_limits)
            .map_err(WorkspaceStreamedAssetError::Staging)?;
        return Ok(report);
    }

    // The baseline YAML exporter keeps an already-populated inline payload and clears the
    // stale StreamingInfo without resolving its path. Probe that branch before touching any
    // resource provider; the content-required result is the only arm that proceeds to lookup.
    match prepare_streamed_yaml_asset(root, kind, None, staging_limits) {
        Ok(preparation) => {
            report.preparation = preparation;
            return Ok(report);
        }
        Err(crate::export::streamed_assets::StreamedYamlAssetError::StreamContentRequired {
            ..
        }) => {}
        Err(error) => return Err(WorkspaceStreamedAssetError::Staging(error)),
    }

    let resolution = read_object_streamed_resource_bytes(
        filesystem,
        workspace,
        source_bundle_index,
        root_catalog,
        provider,
        &info,
        content_limits,
    )
    .map_err(WorkspaceStreamedAssetError::Resource)?;

    let bytes = match resolution {
        StreamedResourceContentResolution::Missing {
            unavailable_data_paths,
        } => {
            return Err(WorkspaceStreamedAssetError::ResourceMissing {
                path: info.path,
                unavailable_data_paths,
            });
        }
        StreamedResourceContentResolution::Unset => {
            report.preparation = prepare_streamed_yaml_asset(root, kind, None, staging_limits)
                .map_err(WorkspaceStreamedAssetError::Staging)?;
            return Ok(report);
        }
        StreamedResourceContentResolution::Resolved { resource, bytes } => {
            match &resource {
                ResolvedResourceFile::Bundled { virtual_path, .. }
                | ResolvedResourceFile::ExplicitRoot { virtual_path, .. } => {
                    report.resolved_virtual_path = Some((*virtual_path).to_owned());
                }
                ResolvedResourceFile::External {
                    virtual_path,
                    source,
                    ..
                } => {
                    report.resolved_virtual_path = Some(virtual_path.clone());
                    report.resolved_external_source = Some(*source);
                }
                ResolvedResourceFile::Missing {
                    unavailable_data_paths,
                } => {
                    return Err(WorkspaceStreamedAssetError::ResourceMissing {
                        path: info.path,
                        unavailable_data_paths: unavailable_data_paths.clone(),
                    });
                }
            }
            bytes
        }
    };

    report.preparation = prepare_streamed_yaml_asset(root, kind, Some(&bytes), staging_limits)
        .map_err(WorkspaceStreamedAssetError::Staging)?;
    Ok(report)
}

/// The caller limits used by the export wiring, grouped so the project path passes one value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceStreamedAssetLimits {
    pub lookup: PlatformStructureLimits,
    pub content: StreamedResourceContentLimits,
    pub scan: StreamedResourceScanLimits,
    pub staging: StreamedYamlAssetLimits,
}

#[cfg(test)]
mod tests {
    use super::{
        read_object_streamed_resource_bytes, select_streamed_asset_info,
        stage_workspace_streamed_yaml_asset, streamed_asset_stream_field_names,
        WorkspaceStreamedAssetError,
    };
    use crate::export::streamed_assets::{
        StreamedYamlAssetKind, StreamedYamlAssetLimits, StreamedYamlAssetPreparation,
    };
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::platform_structure::{PlatformResourceProvider, PlatformStructureLimits};
    use crate::resource::{
        ExplicitRootResourceInput, RootResourceCatalogLimits, RootResourceFileCatalog,
        StreamedResourceContentLimits, StreamedResourceInfo, StreamedResourceScanLimits,
    };
    use crate::scheme::{SchemeReader, SchemeReaderLimits};
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::workspace::SerializedFileWorkspace;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn stream_value(path: &str, offset: SerializedValue, size: u32) -> SerializedValue {
        SerializedValue::Struct(vec![
            field(
                "path",
                SerializedValue::StringBytes(path.as_bytes().to_vec()),
            ),
            field("offset", offset),
            field("size", SerializedValue::U32(size)),
        ])
    }

    fn mesh_payload(
        path: &str,
        offset: SerializedValue,
        size: u32,
        inline: &[u8],
    ) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_StreamData", stream_value(path, offset, size)),
            field(
                "m_VertexData",
                SerializedValue::Struct(vec![field(
                    "m_DataSize",
                    SerializedValue::Bytes(inline.to_vec()),
                )]),
            ),
        ])
    }

    fn image_texture_payload(
        path: &str,
        offset: SerializedValue,
        size: u32,
        inline: &[u8],
    ) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_StreamData_C189", stream_value(path, offset, size)),
            field("m_ImageData_C189", SerializedValue::Bytes(inline.to_vec())),
        ])
    }

    fn explicit_root_catalog(
        filesystem: &VirtualFileSystem,
        dispatched: &crate::scheme::DispatchedFile<'static>,
    ) -> RootResourceFileCatalog {
        let input = [ExplicitRootResourceInput {
            file_name: "shared.resS",
            virtual_path: "/explicit/shared.resS",
            dispatched_file: dispatched,
        }];
        RootResourceFileCatalog::from_explicit_resource_inputs(
            filesystem,
            &input,
            RootResourceCatalogLimits::default(),
        )
        .unwrap()
    }

    fn filesystem_with_resource(bytes: &[u8]) -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/explicit")).unwrap();
        filesystem
            .write_all_bytes("/explicit/shared.resS", bytes)
            .unwrap();
        filesystem
    }

    /// The version-specific field selector is the one the baseline YAML collections read.
    #[test]
    fn field_names_follow_the_baseline_streamed_collections() {
        assert_eq!(
            streamed_asset_stream_field_names(StreamedYamlAssetKind::Mesh),
            &["m_StreamData"]
        );
        assert_eq!(
            streamed_asset_stream_field_names(StreamedYamlAssetKind::ImageTexture),
            &[
                "m_StreamData_C189",
                "m_StreamData_C188",
                "m_StreamData_C187",
                "m_StreamData_C117",
                "m_StreamData_C89",
                "m_StreamData_C28",
                "m_StreamData",
            ]
        );
    }

    #[test]
    fn selection_reads_the_version_specific_field_and_reports_ambiguity() {
        let payload = image_texture_payload("a.resS", SerializedValue::U64(1), 2, &[]);
        let selected = select_streamed_asset_info(
            &payload,
            StreamedYamlAssetKind::ImageTexture,
            Default::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.0, "m_StreamData_C189");
        assert_eq!(
            selected.1,
            StreamedResourceInfo {
                path: "a.resS".to_owned(),
                offset: 1,
                size: 2,
            }
        );
        // The generated Texture2D property is class-suffixed, while its current
        // TypeTree serializes the common field without a suffix.
        let texture2d = SerializedValue::Struct(vec![field(
            "m_StreamData_C28",
            stream_value("b.resS", SerializedValue::U64(0), 1),
        )]);
        let selected = select_streamed_asset_info(
            &texture2d,
            StreamedYamlAssetKind::ImageTexture,
            Default::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.0, "m_StreamData_C28");

        let type_tree_texture = SerializedValue::Struct(vec![field(
            "m_StreamData",
            stream_value("c.resS", SerializedValue::U64(2), 3),
        )]);
        let selected = select_streamed_asset_info(
            &type_tree_texture,
            StreamedYamlAssetKind::ImageTexture,
            Default::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.0, "m_StreamData");

        // Duplicate owner fields are reported, not resolved by pick-one.
        let duplicated = SerializedValue::Struct(vec![
            field(
                "m_StreamData",
                stream_value("a.resS", SerializedValue::U64(0), 1),
            ),
            field(
                "m_StreamData",
                stream_value("b.resS", SerializedValue::U64(0), 1),
            ),
        ]);
        assert!(matches!(
            select_streamed_asset_info(&duplicated, StreamedYamlAssetKind::Mesh, Default::default()),
            Err(WorkspaceStreamedAssetError::AmbiguousStreamField {
                first,
                second,
                ..
            }) if first == "m_StreamData" && second == "m_StreamData"
        ));
    }

    /// End-to-end: a decoded object goes through lookup, a validated range read, and exporter
    /// staging, and the staged payload carries exactly the declared bytes.
    #[test]
    fn object_payload_is_staged_from_a_resolved_resource_range() {
        let filesystem = filesystem_with_resource(b"0123456789abcdef");
        let dispatched =
            SchemeReader::read_with_limits(b"resource", SchemeReaderLimits::default()).unwrap();
        let catalog = explicit_root_catalog(&filesystem, &dispatched);
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);

        let mut payload = mesh_payload("shared.resS", SerializedValue::U64(3), 5, &[]);
        let report = stage_workspace_streamed_yaml_asset(
            &mut payload,
            StreamedYamlAssetKind::Mesh,
            &filesystem,
            &workspace,
            None,
            &catalog,
            &provider,
            StreamedResourceScanLimits::default(),
            StreamedResourceContentLimits::default(),
            StreamedYamlAssetLimits::default(),
        )
        .unwrap();

        assert_eq!(
            report.preparation,
            StreamedYamlAssetPreparation::Materialized {
                resource: StreamedResourceInfo {
                    path: "shared.resS".to_owned(),
                    offset: 3,
                    size: 5,
                },
                byte_count: 5,
            }
        );
        assert_eq!(report.stream_field_path.as_deref(), Some("m_StreamData"));
        assert_eq!(
            report.resolved_virtual_path.as_deref(),
            Some("/explicit/shared.resS")
        );
        assert_eq!(
            payload
                .unique_field_in_base_chain("m_VertexData")
                .unwrap()
                .unwrap()
                .unique_field_in_base_chain("m_DataSize")
                .unwrap()
                .unwrap(),
            &SerializedValue::Bytes(b"34567".to_vec())
        );
    }

    /// A declared resource that the bundle hierarchy and provider cannot see is a reported
    /// missing-resource outcome, and the object value is left untouched.
    #[test]
    fn missing_declared_resource_is_reported_without_mutating_the_payload() {
        let filesystem = filesystem_with_resource(b"0123456789abcdef");
        let dispatched =
            SchemeReader::read_with_limits(b"resource", SchemeReaderLimits::default()).unwrap();
        let catalog = explicit_root_catalog(&filesystem, &dispatched);
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let source = image_texture_payload("absent.resS", SerializedValue::U64(0), 4, &[]);
        let mut payload = source.clone();

        let error = stage_workspace_streamed_yaml_asset(
            &mut payload,
            StreamedYamlAssetKind::ImageTexture,
            &filesystem,
            &workspace,
            None,
            &catalog,
            &provider,
            StreamedResourceScanLimits::default(),
            StreamedResourceContentLimits::default(),
            StreamedYamlAssetLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            WorkspaceStreamedAssetError::ResourceMissing { path, .. } if path == "absent.resS"
        ));
        assert_eq!(payload, source);
    }

    /// An existing inline payload keeps the baseline precedence: no resource read is requested.
    #[test]
    fn existing_inline_bytes_stage_without_a_resource_lookup() {
        let filesystem = filesystem_with_resource(b"0123456789abcdef");
        let dispatched =
            SchemeReader::read_with_limits(b"resource", SchemeReaderLimits::default()).unwrap();
        let catalog = explicit_root_catalog(&filesystem, &dispatched);
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let mut payload = image_texture_payload("absent.resS", SerializedValue::U64(0), 4, b"kept");

        let report = stage_workspace_streamed_yaml_asset(
            &mut payload,
            StreamedYamlAssetKind::ImageTexture,
            &filesystem,
            &workspace,
            None,
            &catalog,
            &provider,
            StreamedResourceScanLimits::default(),
            StreamedResourceContentLimits::default(),
            StreamedYamlAssetLimits::default(),
        )
        .unwrap();
        assert_eq!(
            report.preparation,
            StreamedYamlAssetPreparation::ExistingInlineData {
                resource: StreamedResourceInfo {
                    path: "absent.resS".to_owned(),
                    offset: 0,
                    size: 4,
                },
            }
        );
    }

    /// The bounded range read is the exported contract: a range past the resource end fails loudly.
    #[test]
    fn out_of_range_streamed_request_fails_loudly() {
        let filesystem = filesystem_with_resource(b"0123456789");
        let dispatched =
            SchemeReader::read_with_limits(b"resource", SchemeReaderLimits::default()).unwrap();
        let catalog = explicit_root_catalog(&filesystem, &dispatched);
        let workspace = SerializedFileWorkspace::default();
        let provider = PlatformResourceProvider::new(None, None);
        let info = StreamedResourceInfo {
            path: "shared.resS".to_owned(),
            offset: 8,
            size: 4,
        };

        let error = read_object_streamed_resource_bytes(
            &filesystem,
            &workspace,
            None,
            &catalog,
            &provider,
            &info,
            StreamedResourceContentLimits {
                resource_lookup: PlatformStructureLimits::default(),
                range_read: Default::default(),
            },
        )
        .unwrap_err();
        assert!(
            format!("{error}").contains("byte-range read failed"),
            "{error}"
        );
    }

    /// Reference-runtime differential for the resource lookup and range-read rows.
    ///
    /// The probe (`tools/probes/io-differential`) built a real root `GameBundle` with one child
    /// bundle and three `ResourceFile`s and ran the shipped `Bundle.ResolveResource` for nine
    /// queries plus the source `GetContent` range read for four ranges. The workspace below mirrors
    /// that hierarchy and every outcome must agree; the child-bundle, parent-scope, fixed-name, and
    /// missing/empty/extension-mismatch outcomes are all pinned by the reference rows.
    #[test]
    fn resource_lookup_and_ranges_match_the_reference_runtime_fixture() {
        use crate::resource::{
            read_streamed_resource_bytes, resolve_resource_file, ResolvedResourceFile,
            StreamedResourceLimits,
        };
        use crate::serialized_file::workspace::{WorkspaceResourceFile, WorkspaceSerializedBundle};

        fn fixture_field<'a>(fields: &'a [&'a str], key: &str) -> &'a str {
            let prefix = format!("{key}=");
            fields
                .iter()
                .find_map(|field| field.strip_prefix(&prefix))
                .unwrap_or_else(|| panic!("missing reference IO fixture field {key:?}"))
        }

        fn hex(bytes: &[u8]) -> String {
            use std::fmt::Write as _;
            let mut text = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                write!(text, "{byte:02X}").expect("string write");
            }
            text
        }

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem.create_directory(Some("/child")).unwrap();
        filesystem
            .write_all_bytes("/game/root.resource", b"ROOTRESOURCE")
            .unwrap();
        filesystem
            .write_all_bytes("/child/shared.resS", b"0123456789abcdef")
            .unwrap();
        filesystem
            .write_all_bytes("/child/sibling.resS", b"ABCDEFGH")
            .unwrap();

        let workspace = SerializedFileWorkspace::with_resource_facts_for_test(
            vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/game".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![0],
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/child".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: None,
                    direct_collection_file_indices: Vec::new(),
                    direct_resource_file_indices: vec![1, 2],
                },
            ],
            vec![
                WorkspaceResourceFile {
                    virtual_path: "/game/root.resource".to_owned(),
                    name: "root.resource".to_owned(),
                    name_fixed: "root.resource".to_owned(),
                    container_bundle_index: 0,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/child/shared.resS".to_owned(),
                    name: "archive:/textures/Shared.resS".to_owned(),
                    name_fixed: "shared.ress".to_owned(),
                    container_bundle_index: 1,
                    container_entry_index: 0,
                    container_resource_index: 0,
                },
                WorkspaceResourceFile {
                    virtual_path: "/child/sibling.resS".to_owned(),
                    name: "sibling.resS".to_owned(),
                    name_fixed: "sibling.ress".to_owned(),
                    container_bundle_index: 1,
                    container_entry_index: 1,
                    container_resource_index: 1,
                },
            ],
        );
        let provider = PlatformResourceProvider::new(None, None);

        let mut lookups = 0usize;
        for line in include_str!("io_differential_fixture.txt").lines() {
            let Some(rest) = line.strip_prefix("RESOURCE|lookup|") else {
                continue;
            };
            let fields: Vec<&str> = rest.split('|').collect();
            let case = fixture_field(&fields, "case");
            let query = fixture_field(&fields, "query");
            let expected = fixture_field(&fields, "outcome");
            let source_bundle = match case {
                "child_own_shared" | "child_parent_root" => Some(1),
                _ => Some(0),
            };
            let resolved = resolve_resource_file(
                &filesystem,
                &workspace,
                source_bundle,
                query,
                &provider,
                PlatformStructureLimits::default(),
            )
            .unwrap_or_else(|error| panic!("{case}: lookup failed: {error}"));

            let (outcome, length) = match resolved {
                ResolvedResourceFile::Missing { .. } => ("NULL".to_owned(), -1i64),
                ResolvedResourceFile::Bundled { resource_index, .. } => {
                    let resource = &workspace.resource_files()[resource_index];
                    let bytes = filesystem
                        .read_all_bytes(&resource.virtual_path, 4096)
                        .unwrap_or_else(|error| panic!("{case}: {error}"));
                    (format!("FILE:{}", resource.name_fixed), bytes.len() as i64)
                }
                ResolvedResourceFile::ExplicitRoot { resource_index, .. } => {
                    (format!("EXPLICIT:{resource_index}"), -1)
                }
                ResolvedResourceFile::External { virtual_path, .. } => {
                    let bytes = filesystem
                        .read_all_bytes(&virtual_path, 4096)
                        .unwrap_or_else(|error| panic!("{case}: {error}"));
                    (
                        format!("EXTERNAL:{}", virtual_path.rsplit('/').next().unwrap()),
                        bytes.len() as i64,
                    )
                }
            };
            assert_eq!(outcome, expected, "{case}: lookup outcome");
            assert_eq!(
                length.to_string(),
                fixture_field(&fields, "length"),
                "{case}: resource length"
            );
            lookups += 1;
        }
        assert_eq!(lookups, 9, "reference lookup row count");

        let mut ranges = 0usize;
        for line in include_str!("io_differential_fixture.txt").lines() {
            let Some(rest) = line.strip_prefix("RESOURCE|range|") else {
                continue;
            };
            let fields: Vec<&str> = rest.split('|').collect();
            let case = fixture_field(&fields, "case");
            let offset: u64 = fixture_field(&fields, "offset").parse().unwrap();
            let size: u32 = fixture_field(&fields, "size").parse().unwrap();
            let expected = fixture_field(&fields, "outcome");
            let read = read_streamed_resource_bytes(
                &filesystem,
                "/child/shared.resS",
                offset,
                u64::from(size),
                StreamedResourceLimits::default(),
            );
            if expected == "OUT_OF_RANGE" {
                assert!(
                    read.is_err(),
                    "{case}: the reference refuses a range past the file end"
                );
            } else {
                let bytes = read
                    .unwrap_or_else(|error| panic!("{case}: {error}"))
                    .unwrap_or_else(|| panic!("{case}: no bytes"));
                assert_eq!(hex(&bytes), expected, "{case}");
            }
            ranges += 1;
        }
        assert_eq!(ranges, 4, "reference range row count");
    }
}
