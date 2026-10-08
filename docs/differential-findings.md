# Source behavior and differential findings

This log records behavior differences found between the Rust port and the frozen local source. A source-code analysis is identified as such; it is not described as an executed C# differential.

## DIFF-BROTLI-METADATA-001 — metadata length off by one in source detection

| Field | Finding |
|---|---|
| Status | Frozen-source runtime confirms the scheme detector false-negative; Rust follows RFC 7932 and the direct source decoder confirms the payload |
| Source location | `AssetRipper/Source/AssetRipper.IO.Files/CompressedFiles/Brotli/BrotliFile.cs`, `BrotliFile.IsBrotliFile` |
| Input evidence | `tools/probes/io-differential/Program.cs` creates an actual temporary file containing a valid RFC 7932 Brotli stream with Unity's 36-byte metadata marker and a .NET BrotliStream-compressed UnityFS payload. The source `SchemeReader.LoadFile` result and direct `BrotliFile.Read` result are frozen in `src/compression/wrapper_scheme_runtime_fixture.txt`; a direct Rust regression checks both plus file-backed wrapper import and mounted UnityFS entry bytes. |
| Source-runtime behavior | The source `SchemeReader.LoadFile` selects `ResourceFile` and leaves the marked bytes untouched. Calling the source `BrotliFile.Read` directly on the same bytes produces the original UnityFS bytes. The detector reads the encoded `MSKIPLEN - 1` value and compares it directly with `BrotliSignature.Length`; for the valid 36-byte marker, the encoded value is 35, so the signature comparison fails. |
| Root cause | RFC 7932 §9.2 stores metadata length minus one. The detector omitted the `+1` needed to obtain the number of metadata bytes before comparing the marker length. |
| Rust change | `has_brotli_file_signature` now parses the Brotli window header, validates the empty metadata block fields and zero alignment bits, adds one to the encoded length, bounds the marker read, and compares the metadata bytes. The recursive scheme reader then decodes the same input under the configured output limit. |
| Regression evidence | The bit-level marker tests recognize and decode WBITS 10–24. The frozen-source runtime probe confirms the scheme-level false-negative, while the direct source decoder and Rust's file-backed path both recover the UnityFS payload. `docs/validation-log.md` records exact commands and test results. |
| Remaining comparison | No Unity-produced Brotli wrapper was available in Phase 1. The runtime fixture is a generated standards-valid file and does not establish the prevalence of this source detector bug in shipped inputs. |

The format reference is [RFC 7932 §9.2](https://www.rfc-editor.org/rfc/rfc7932.html#section-9.2). Its metadata length field stores `MSKIPLEN - 1`, and the metadata bytes are excluded from decompressed output.
## DIFF-MULTIFILE-SEEK-END-001 — source uses the opposite End offset sign

| Field | Finding |
|---|---|
| Status | Resolved: Rust now reproduces the isolated C# source behavior and has a direct regression |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/Streams/MultiFileStream.cs, MultiFileStream.Seek |
| Input evidence | `tools/probes/multifile/MultiFileProbe.cs` runs against the isolated frozen `AssetRipper.IO.Files.dll` and records `SEEK_END|10|10|12|8` in `src/io/multifile_path_fixture.txt`. A fresh 16-row probe run on 2026-10-08 matched the committed fixture exactly. Rust test `multi_file_stream_seeks_from_end_with_source_offsets` checks the same offsets. |
| Source-runtime behavior | For length 10, the source returns 10 for End(0), 12 for End(-2), and 8 for End(+2), confirming `Position = Length - offset`. |
| Rust behavior | `MultiFileStream` now computes `Length - offset`, giving 10, 12, and 8 for End(0), End(-2), and End(+2). It also rejects negative and out-of-range positions rather than wrapping them into `u64`. |
| Resolution | The earlier deliberate hardening divergence was removed to restore source parity. `source_seek_target` applies this rule to both `MultiFileStream` and the file-backed `RandomAccessStream` view; `PartialStream` retains the source's separate standard-sign rule. |

## DIFF-MULTIFILE-PATH-001 — canonical split names and path-backed mounting

| Field | Finding |
|---|---|
| Status | Canonical path behavior now matches the frozen source's acceptance/rejection outcomes; source-runtime differential and mount regression pass |
| Source location | `AssetRipper.IO.Files/Streams/MultiFileStream.cs` (`IsMultiFile`, `OpenRead`); `SplitNameComparer.cs`; `AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`) |
| Input evidence | `tools/probes/multifile/MultiFileProbe.cs` runs only synthetic files under the Rust repository's ignored `target` directory against the isolated reference copy at `%LOCALAPPDATA%\Temp\opencode\ar-ref`. Its committed fixture records regular and split base/part names, `IsMultiFile` cases, part ordering through split10, boundary reads, missing-part rejection, and zero-padded-name rejection. |
| Source-runtime behavior | `SchemeReader.LoadFile` exposes the same logical base name for a regular path, split base, or supplied split part. Parts are numerically ordered; gaps and `.split00` plus `.split1` are rejected because the source requires exact `.split0` through `.splitN` names. |
| Rust change | `open_split_file` now checks the exact canonical names across the enumerated `.split*` candidate count before sorting/opening. This rejects `.split00` as a substitute for `.split0`; `SchemeReader::mount_path` carries the logical name into the VFS. |
| Regression evidence | `multi_file_paths_match_reference_runtime_fixture` compares path classification, bytes, names, numeric order, boundary reads, gap rejection, and padded-name rejection. `mount_path_assembles_split_inputs_and_uses_the_logical_base_name` and `mount_path_rejects_zero_padded_parts_the_reference_cannot_open` exercise the integrated mount path. |
| Remaining difference | Rust reports `NotFound` with the expected canonical path; the C# fixture records a rejected `Exception`. Both reject the malformed family. End-relative seek behavior now matches and is closed in `DIFF-MULTIFILE-SEEK-END-001`. |

## DIFF-LOCAL-FS-SHARING-001 — Rust's default Windows file sharing allowed input mutation

| Field | Finding |
|---|---|
| Status | Windows behavior is runtime-pinned. Unix read sharing is implemented from .NET 10 source and cross-target checked; non-Windows runtime validation is externally blocked. |
| Source location | `AssetRipper.IO.Files/LocalFileSystem.g.cs` (`LocalFileImplementation.OpenRead`); `LocalFileSystem.cs`; `Streams/MultiFileStream.cs`; `SchemeReader.LoadFile`; [.NET 10 SafeFileHandle.Unix.cs](https://github.com/dotnet/runtime/blob/release/10.0/src/libraries/System.Private.CoreLib/src/Microsoft/Win32/SafeHandles/SafeFileHandle.Unix.cs); [.NET 10 File.cs](https://github.com/dotnet/runtime/blob/release/10.0/src/libraries/System.Private.CoreLib/src/System/IO/File.cs); [.NET 10 FileSystem.Unix.cs](https://github.com/dotnet/runtime/blob/release/10.0/src/libraries/System.Private.CoreLib/src/System/IO/FileSystem.Unix.cs) |
| Input evidence | `tools/probes/local-filesystem/Program.cs` ran against the isolated frozen AssetRipper IO.Files project using synthetic files under the Rust repository's ignored `target` directory. `src/io/local_filesystem_fixture.txt` records that writes and deletes are rejected while ordinary and split input streams are open, and that their original bytes remain readable. |
| Source-runtime behavior | On Windows, `LocalFileSystem.File.OpenRead` uses .NET `File.OpenRead` (`FileShare.Read`), and writes/deletes fail with `IOException` while the handle is retained. In .NET 10 Unix source, `FileShare.Read` maps to a nonblocking shared `flock`; only `EWOULDBLOCK` rejects the open and other lock errors are best-effort. `File.WriteAllBytes` also requests `FileShare.Read`, so compatible shared locks allow it to write on supported local filesystems. Unix delete uses `unlink`, which the advisory lock does not prevent. [.NET documents `Directory.GetFiles` order as unspecified](https://learn.microsoft.com/en-us/dotnet/api/system.io.directory.getfiles?view=net-10.0). |
| Root cause | Rust initially matched Windows by setting a narrow share mode but left Unix at plain `File::open`; the earlier source note incorrectly stated that .NET Unix did not lock `FileShare.Read`. |
| Rust change | `open_os_read` uses Windows `FILE_SHARE_READ`; on Rustix-supported Unix targets it takes a nonblocking shared `flock` and rejects only `WOULDBLOCK`, matching .NET's best-effort treatment of other lock errors. Other targets retain native read-only opens. Discovery sorts results for deterministic output; this is compatible with the source's unspecified order contract. |
| Regression evidence | The fresh frozen-source probe exited 0 with 76 lines. After filtering four supplemental `SCHEME` rows and one direct `BROTLI` row covered by their own fixtures, the remaining 71 lines match `src/io/io_differential_fixture.txt` exactly; all three `DIR|*` rows match. Windows test `physical_file_read_sharing_matches_local_filesystem_runtime_fixture` and `mount_directory_preserving_containers_keeps_split_container_identity` cover retained inputs. Linux and Windows test targets pass `cargo check --workspace --tests`; the Unix lock regression was cross-compiled, not run. |
| Remaining comparison | No Linux runtime, WSL distribution, or container runtime is available here, so the non-Windows .NET and Rust retained-reader cases and their integrated directory-import replay have not run. This row remains blocked and cannot be marked verified until a supported non-Windows runtime is available. |

## DIFF-LOCAL-FS-ADAPTER-001 — local filesystem wrappers and temporary storage

| Field | Finding |
|---|---|
| Status | The Windows reference probe matches the implemented LocalFileSystem file, directory, text, enumeration, path, and temporary-storage operations. Remaining text-encoding and cross-platform runtime gaps keep the aggregate adapter open. |
| Source location | `AssetRipper.IO.Files/FileSystem.cs`; `LocalFileSystem.cs`; generated `LocalFileSystem.g.cs`; `Streams/Smart/SmartStream.cs` |
| Input evidence | `tools/probes/local-filesystem/LocalFileSystemProbe.csproj` ran against the shipped frozen `AssetRipper.IO.Files.dll` with scratch files beneath `target/`. Its 37 normalized rows are committed in `src/io/local_filesystem_fixture.txt`; random GUIDs and absolute paths are reduced to shape and existence facts. |
| Source-runtime behavior | The default temp path first probes `<AppContext.BaseDirectory>/temp/<4-hex>` and falls back to `<Path.GetTempPath()>/AssetRipper/<4-hex>` on IO/authorization failure. `File.CreateTemporary` creates an empty full-GUID file; `Directory.CreateTemporary` creates an 8-hex child. The setter ignores blank values and stores a full path. File and directory Get/Enumerate forms are exercised for top-level and recursive wildcard searches, including modern *.* matching of extensionless files. Default text writes are UTF-8 without a preamble; reads detect UTF-8/16 BOMs; explicit UTF-16 writes include its preamble. Path Join, name, extension, directory, rooted, full-path, and relative-path results are source-probed. `File.OpenWrite` preserves existing tail bytes and starts at position zero; `WriteAllBytes` is blocked while a Windows `File.OpenRead` is retained. `DeleteTemporaryDirectory` recursively removes its tree and is a no-op once absent. |
| Rust change | New `src/io/local_filesystem.rs` centralizes source-style shared/exclusive file opens, byte and text I/O, create/delete/exists, lazy directory enumeration, path helpers, temp file/directory methods, and local-first/system-fallback temp selection. `SmartStream::create_temp` now uses this adapter and deletes the temporary file after its last shared reference drops. |
| Regression evidence | Windows test `local_filesystem_operations_match_the_frozen_runtime_fixture` compares all 37 normalized source rows. Additional tests cover local temp selection, fallback when the executable root is not a directory, blank setters, file sharing, and GUID shape. `default_temporary_smart_stream_uses_local_filesystem_policy_and_lifetime` checks the production stream connection and reference lifetime. |
| Remaining comparison | TextEncoding currently covers UTF-8, UTF-16LE/BE, and UTF-32LE/BE; arbitrary .NET Encoding instances and custom fallback policies remain open. Enumerate* result sets are source-probed, but deferred error timing is not compared. The Unix FileShare runtime probe is separately blocked by the lack of a non-Windows runtime. Aggregate status stays `IO-STREAM-001` in progress. |

## DIFF-REFERENCE-TEST-NEWLINE-001 — 20 reference test failures are a checkout artifact, and they confirm LF is the intended YAML newline

| Field | Finding |
|---|---|
| Status | Root cause identified from source plus executed reference test output. Not a product defect and not a Rust defect. |
| Source location | `AssetRipper/Source/AssetRipper.Tests/Traversal/DefaultYamlWalkerTests.cs`; `AssetRipper/Source/AssetRipper.Yaml/YAMLWriter.cs` |
| Input evidence | The reference suite was executed in an isolated copy. `AssetRipper.Tests` reported `Failed: 20, Passed: 151, Total: 171`. Every failure message has the same shape, for example `Expected: "%YAML 1.1\r\n%TAG !u!..."` versus `But was: "%YAML 1.1\n%TAG !u!..."` with `Strings differ at index 9`, which is the first newline. The seven other test projects passed completely: AssemblyDumper 9/9, Assets 57/57, GUI.Web 6/6, IO.Files 137/137, Numerics 65/65, SerializationLogic 54/54, Yaml 11/11. Reference total: 339 passed, 20 failed. |
| Source-derived behavior | `DefaultYamlWalkerTests.GenerateYaml` constructs `StringWriter { NewLine = "\n" }`, so the generated document uses LF. The expected values are C# raw string literals, whose line endings come from the `.cs` file itself. |
| Root cause | The frozen checkout is CRLF. `DefaultYamlWalkerTests.cs` was measured as 354 LF of which 354 are CRLF and 0 are lone LF, so every raw string literal contains `\r\n` while the writer deliberately emits `\n`. A CRLF assertion cannot match an LF writer. |
| Reference build status | `dotnet 10.0.401` is available on this workstation, so `dotnet restore AssetRipper.slnx` and `dotnet build AssetRipper.slnx -c Release` both exit 0 across all 51 projects. The earlier handoff statement that `dotnet` was unavailable no longer holds. |
| Rust behavior | `src/export/unity_yaml.rs` already emits `\n` for the YAML head, node line breaks, and the document separator, which matches the source writer's configured newline and not the CRLF checkout. No Rust change was required. |
| Remaining comparison | This finding documents a test-fixture/checkout mismatch, not a behavior difference. A LF checkout of the same commit would make the 20 assertions pass. Unity Editor import checks and the reference GUI/web runtime remain unrun. |


## DIFF-YAML-DASH-WHITESPACE-001 — `-` followed by non-space Unicode whitespace

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper/Source/AssetRipper.Yaml/YamlScalarNode.StringNode.cs`, `GetStringStyle` / `IllegalStringsRegex` (`(^-\s)`) |
| Input evidence | Isolated probe (`Temp/opencode/yamlemitprobe`) referencing the built net10.0 `AssetRipper.Yaml.dll`: `YamlScalarNode.Create("-\u00A0foo")` and `"-\u2003foo"` emit `'-\u00A0foo'` and `'-\u2003foo'` (single-quoted), while `"-\u200Bfoo"` stays plain. |
| Source-derived behavior | The regex alternative `(^-\s)` uses .NET `\s` = `[ \f\n\r\t\v\x85\p{Z}]`, so a `-` followed by any whitespace starts a quoted scalar. Control whitespace is already DoubleQuoted earlier, so only non-control Unicode whitespace (for example U+00A0, U+2003) reaches this branch. |
| Root cause | The Rust predicate used `value.starts_with("- ")`, which only recognizes an ASCII space after the dash. |
| Rust change | `string_needs_quotes` now tests `value.starts_with('-')` plus `char::is_whitespace` on the following character. |
| Regression evidence | `unity_yaml::tests::scalar_quoting_matches_the_reference_yaml_runtime` embeds the probe's exact document bytes for `-\u00A0foo`, `-\u2003foo`, and `-\u200Bfoo`. |
| Remaining comparison | Only the probe's 48 cases were executed; a broader value sweep and Unity-produced YAML remain unrun. |

## DIFF-YAML-EMPTY-WHITESPACE-001 — empty plain value drops the pending space

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper/Source/AssetRipper.Yaml/Emitter.cs`, `Write(ReadOnlySpan<char>)` |
| Input evidence | The same isolated probe wrote `{k: <empty string>}`; the reference runtime emitted `k:\n` (no space after the colon), while the Rust emitter produced `k: \n`. |
| Source-derived behavior | `Emitter.Write(ReadOnlySpan<char>)` calls `WriteDelayed()` only when the span is non-empty. An empty plain scalar therefore lets the newline from `EndTransition` cancel the deferred whitespace instead of flushing it. |
| Root cause | The Rust `write_str` unconditionally called `write_delayed()` before appending, flushing the deferred space. |
| Rust change | `write_str` now returns early for an empty value, matching `Write(ReadOnlySpan<char>)`. |
| Regression evidence | The differential table's first case (`""` -> `k:\n`) plus the empty-string cases in `EXP-YAML-QUOTING-001`. |
| Remaining comparison | The rule is pinned for scalar values; whether an empty mapping/sequence node produces the same cancellation in every layout was not separately probed. |


## DIFF-YAML-FLOAT-FORMAT-001 — float scalars must follow .NET shortest-round-trip formatting

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper/Source/AssetRipper.Yaml/YamlScalarNode.NumericNode.cs`; `YamlScalarNode.FloatingPointHexNode.cs`; `Emitter.cs` `Write(float)`/`Write(double)`; `AssetRipper/Source/AssetRipper.IO.Files/InvariantStreamWriter.cs` |
| Input evidence | Isolated net10.0 probe (`Temp/opencode/numericprobe/numeric_probe.json`, 203 cases) capturing `Single`/`Double.ToString(CultureInfo.InvariantCulture)` for curated boundaries plus 60+60 deterministic pseudo-random finite bit patterns. |
| Source-derived behavior | Floats pass through `TextWriter.Write` under the baseline's invariant culture, so Unity YAML floats use .NET's default shortest-round-trip "G" format: `NaN` -> `NaN`; `+Inf` -> `Infinity`; `-Inf` -> `-Infinity`; zero keeps its sign (`0`/`-0`). Scientific notation is used when the decimal exponent `e` is below -4 or at least 9 (`Single`) / 17 (`Double`); the exponent is always signed and zero-padded to two digits. `FloatingPointHexNode` wraps the same text as `0x{bits:x8}({value})` / `0x{bits:x16}({value})`. |
| Root cause | Rust emitted `f32`/`f64` through `Display`, which never uses exponent notation and spells infinities `inf`/`-inf`; for example `1E-05` became `0.00001` and `3.4028235E+38` became a 39-digit integer. |
| Rust change | Added `format_dotnet_f32`/`format_dotnet_f64`, which parse Rust's shortest-round-trip `LowerExp` digits and apply the measured notation thresholds; both the plain and hex scalar arms now use them. |
| Regression evidence | `unity_yaml::tests::float_formatting_matches_the_reference_runtime` asserts all 203 captured values; the quoting differential next to it also exercises `\u0000` and control characters. |
| Remaining comparison | The probe covers finite values and the special forms; no Unity-produced YAML float comparison has run, and the earlier `EXP-YAML-NUMERIC-001` row previously relied on Rust formatting. |


## DIFF-NORMALIZE-DIVISION-001 — `Vector3.Normalize` divides instead of using a reciprocal

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `System.Numerics.Vector3.Normalize` used by `CompressedMeshExtensions.GetNormals`/`GetTangents`/`MakeFloatAndSignArrays` |
| Input evidence | Isolated probe (`Temp/opencode/meshprobe`) called `Vector3.Normalize` on `(1.2, 0.1, 0)`, `(-3, 4, 0)`, and `(1, 1, 1)` and recorded the exact bit patterns; the Rust port using a reciprocal gave `1065295263` for the first component of the first vector where .NET gives `1065295264`. |
| Source-derived behavior | `Vector3.Normalize` computes `value.X / length`, `value.Y / length`, `value.Z / length` — a per-component division, not a `1.0 / length` reciprocal multiply. |
| Root cause | The first Rust implementation multiplied by `1.0f32 / length`, which rounds differently in the last ULP. |
| Rust change | `resources::compressed_mesh::normalize` now divides each component by the length. |
| Regression evidence | `resources::compressed_mesh::tests::normalize_matches_the_reference_runtime` plus the normals/tangents differential in `geometry_matches_the_reference_runtime`. |
| Remaining comparison | Only three normalize inputs were probed; the fix is the source's actual formula, so broader inputs follow from the same expression. |


## DIFF-VALUECONSTANT-TYPEID-001 — `ValueConstant_5_5`'s `TypeID` is inert

| Field | Finding |
|---|---|
| Status | Confirmed source behavior recorded; the Rust port models an explicit `type_id` and notes the constraint |
| Source location | `AssetRipper.SourceGenerated.Subclasses.ValueConstant.ValueConstant_5_5` (generated); consumed by `AssetRipper.SourceGenerated.Extensions/ClipExtensions.cs` `ConvertValueArrayToGenericBinding` |
| Input evidence | Isolated probe (`Temp/opencode/vcread`, `vcfields`, `vctype`) created `ValueConstant.Create(version)` for 4.3–2017. `ValueConstant_4` has a private `m_TypeID` field and round-trips `TypeID`; `ValueConstant_5_5` has only `m_ID`, `m_Type`, `m_Index`, its `TypeID` getter always returns 0, and setting `TypeID` or `Type` leaves it 0. |
| Source-derived behavior | `ClipExtensions.ConvertValueArrayToGenericBinding` reads `values.Data.ValueArray[i].TypeID`. On the `_5_5` layout (Unity 5.5+) that value is always 0, so every entry takes the `else` branch: an Animator binding with `Attribute = ID` and stride 1, instead of the Transform position/rotation/scale classification. |
| Root cause | The generated `_5_5` subtype dropped the `m_TypeID` serialized field (the CRC now lives in `Type`), but the extension was not updated to read `Type`. This is a baseline quirk, not a Rust defect. |
| Rust change | `resources::clip::ClipView::convert_value_array_to_generic_binding` reproduces the source algorithm over an explicit `ValueArrayEntry::type_id`. The serialization layer must mirror the generated class: for the `_5_5` layout leave `type_id` at 0 (do not substitute `Type`), so 5.5+ conversion degenerates identically. |
| Regression evidence | `resources::clip::tests::clip_matches_the_reference_runtime` asserts the four-binding result for the `_4` layout (4.3/5.0/5.4) and the empty result when a clip has no binding; the fixture is committed as `src/resources/clip_binding_fixture.txt`. |
| Remaining comparison | No real 5.5+ asset with a legacy `Binding` array has been converted through both tools, and the Unity re-import effect of the degenerated bindings has not been observed. |


## DIFF-GUID-TOSTRING-001 — `UnityGuid.ToString` nibble order

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | External `AssetRipper.Primitives.UnityGuid.ToString`; the generated `GUID` subclass override in `AssetRipper.AssemblyDumper/Passes/Pass502_FixGuidAndHashYaml.cs`; consumed by `Meta.ExportYamlDocument` (`root.Add(GuidName, GUID.ToString())`) and every project-pointer YAML GUID |
| Input evidence | Isolated probe (`Temp/opencode/guidprobe2`) constructed `UnityGuid` from word tuples and byte spans and captured `ToString`/`ToByteArray`. For `Data0=0x01020304, Data1=0x11121314, Data2=0x21222324, Data3=0x31323334` the reference yields `40302010413121114232221243332313`, the per-word reversal of the big-endian hex; `ToByteArray` round-trips the four little-endian words. |
| Source-derived behavior | `UnityGuid.ToString` emits each 32-bit word's nibbles from least significant to most significant, which is the canonical Unity layout: the well-known `unity default resources` GUID is `0000000000000000e000000000000000` for the word tuple `(0, 0, 0xE, 0)`. |
| Root cause | The Rust `format_unity_guid` read the four little-endian words but emitted each word's nibbles most-significant first, producing the reversed string (`00000000000000000000000e00000000` for the same tuple). Every `.meta` GUID and every YAML GUID reference was therefore reversed relative to the baseline. |
| Rust change | `format_unity_guid` now emits nibbles least-significant-first. Ten self-authored test expectations that encoded the reversed convention were corrected to the reference-derived values (GUID map keys, `m_Guid`/`m_Guids`, `Meta`/pointer YAML, sidecar bytes, workspace pointer GUIDs). |
| Regression evidence | `export::unity_yaml::tests::unity_guid_string_matches_the_reference_runtime` asserts the four committed probe rows (`src/export/guid_fixture.txt`) plus the canonical engine GUID; the previously failing Meta/pointer/value/workspace tests now pass unchanged in behavior. |
| Remaining comparison | The `Hash128.ToString` helper's exact byte layout is in the external package and was not probed; no Unity-import run has confirmed the exported GUIDs. |


## DIFF-GUID-TOSTRING-002 — `FileIdentifier.GetFilePath` Meta GUID

| Field | Finding |
|---|---|
| Status | Fixed in Rust; shares the `DIFF-GUID-TOSTRING-001` reference rule |
| Source location | `AssetRipper.IO.Files/SerializedFiles/Parser/FileIdentifier.cs` `GetFilePath` (and `ToString`): for `AssetType.Meta`, `return Guid.ToString();` |
| Input evidence | The same `Temp/opencode/guidprobe2` probe established the `UnityGuid.ToString` nibble order. The Rust `serialized_file::metadata::FileIdentifier::get_file_path` was formatting the four words with `{data0:08x}{data1:08x}{data2:08x}{data3:08x}` (most-significant nibble first). |
| Source-derived behavior | Meta external identifiers resolve to the canonical Unity GUID string, e.g. the words `(0x01234567, 0x89abcdef, 0, 0xffffffff)` become `76543210fedcba9800000000ffffffff`. |
| Root cause | The same reversal as `format_unity_guid`: the four little-endian words were emitted most-significant nibble first, so the dependency identity string disagreed with every GUID the exporter writes. |
| Rust change | `FileIdentifier::get_file_path` now emits each word's nibbles least-significant first, matching the fixed `export::unity_yaml::format_unity_guid`. The `io::dependency` Meta-identity test's registered name and expected string were updated to the corrected value. |
| Regression evidence | `io::dependency::tests::meta_bundle_identity_uses_guid_but_platform_dependency_uses_path_name` asserts the corrected GUID string; the non-Meta path identity and missing-GUID branches are unchanged. |
| Remaining comparison | No real external-dependency resolution through a Meta identifier has been exercised against a Unity project. |


## DIFF-PACKEDFLOAT-NAN-001 — cross-platform NaN sign in packed float unpack

| Field | Finding |
|---|---|
| Status | Fixed in Rust; found by GitHub Actions on Linux |
| Source location | `AssetRipper.SourceGenerated.Extensions/PackedFloatVectorExtensions.cs` reader: `return (x / halfMaxValue) + Start;`; Rust `resources::packed_float_vector::PackedBitReader::read` |
| Input evidence | The `float_vectors_match_the_reference_runtime` case `start=10.0, range=2.0, bitSize=32, data=0102030405060708` unpacks via the source quirk `mask = 0` so `halfMaxValue = 0`, giving `0.0 / 0.0`. The Windows reference (`Temp/opencode/floatprobe`) and the Windows Rust build both produced the positive quiet NaN `0x7FC00000`; the Linux x86-64 CI build produced the negative quiet NaN `0xFFC00000` (left `[4290772992, 4290772992]` vs right `[2143289344, 2143289344]`). |
| Source-derived behavior | The baseline targets Windows .NET, whose `0.0 / 0.0` yields `0x7FC00000`. The raw hardware default NaN sign is platform-dependent. |
| Root cause | Rust left the division result as the hardware default NaN, whose sign differs between the Windows and Linux x86-64 code paths. |
| Rust change | `PackedBitReader::read` canonicalizes a NaN result to `f32::from_bits(0x7FC00000)`, matching the Windows baseline deterministically. |
| Regression evidence | The same differential test now passes on both `ubuntu-latest` and `windows-latest` in GitHub Actions run `37247323096`. |
| Remaining comparison | Only the `mask = 0` `0/0` NaN path is canonicalized; a NaN produced from a NaN input payload is not separately probed. |


## DIFF-HASH128-OVERFLOW-001 — `HashHelper.ToString` checked-overflow quirk

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | Generated `AssetRipper.SourceGenerated.Helpers.HashHelper.ToString`, called by the `Hash128` `ToString` override injected in `Pass502_FixGuidAndHashYaml.cs` |
| Input evidence | Probe `Temp/opencode/hashprobe2` set the generated `Hash128` byte fields and captured `ToString` for seven patterns. Bytes `00..0f` yields `03020100070605040b0a09080f0e0d0c`; `0000007f...` yields `7f000000000000000000000000000000`; any word whose most significant byte is `>= 0x80` (`abababab...`, `...ff`, `00000080...`) throws `OverflowException`. |
| Source-derived behavior | The shipped helper builds each 32-bit word with checked `int` arithmetic, so the top byte's high bit overflows. The success format is each little-endian word's big-endian hex, the opposite nibble order from `UnityGuid.ToString`. |
| Root cause | The injected helper source casts to `uint` (which cannot overflow), but the shipped helper evidently uses checked `int`; the Rust port had no overflow path. |
| Rust change | Added `export::unity_yaml::format_hash128`, which returns `Hash128Error::Overflow` when any word's most significant byte is `>= 0x80` and otherwise emits the big-endian-per-word hex; `value_yaml`'s Hash128 conversion now propagates `UnityYamlObjectConversionError::Hash128Overflow` instead of always emitting a string. |
| Regression evidence | `export::unity_yaml::tests::hash128_string_matches_the_reference_runtime` asserts all seven committed rows (`src/export/hash128_fixture.txt`), including the 0x7F/0x80 boundary. |
| Remaining comparison | No real asset with a high-bit-top-byte hash has been exported through both tools; the per-asset failure handling of the baseline was not observed. |


## DIFF-TEXDEC-EXPANSION-001 — DXT bit-replication vs RGB565 bare-shift expansion, and the three-color DXT opaque-black index

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `DxtDecoder` and `RgbConverter`, dispatched by `AssetRipper.Export.Modules.Textures/TextureConverter.cs` |
| Input evidence | Probe `Temp/opencode/texdecprobe` decoded the shipped package. The DXT endpoint `0xF800` becomes `255` (`(31 << 3) \| (31 >> 2)`), while the standalone `RGB565` converter maps the same word to `248` (`31 << 3`). A DXT1/3/5 block with `color0 = 0x0000 <= color1 = 0xFFFF` and all index 3 yields opaque black `(0,0,0,255)`, not transparent black. |
| Source-derived behavior | The two code paths intentionally use different channel expansion. The three-color DXT index 3 is decoded as black with the default alpha `255` because the color routine does not carry transparency. |
| Root cause | A naive port would share one RGB565 expansion and would emit transparent black for the three-color DXT index. |
| Rust change | Added `resources::texture_decode` with a DXT-specific `dxt_rgb565_to_rgba` (bit replication) and a separate `rgb565_to_rgba` (bare shift), and a shared `write_color_block` whose three-color table ends in `[0, 0, 0, 255]`. |
| Regression evidence | `resources::texture_decode::tests::texture_decoders_match_the_reference_runtime` asserts all 22 committed rows (`src/resources/texture_decode_fixture.txt`), including the non-extreme `0x1860` endpoint and the `color0 <= color1` DXT3/DXT5 blocks. |
| Remaining comparison | Only 4×4 and 2×2 single-block inputs were probed; multi-block clipping is asserted structurally but not differentially. |


## DIFF-BC45-ROUNDING-001 — `Bc` decoders round the alpha interpolation; the `Dxt` decoder truncates

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Bc/BcHelpers.SmoothAlphaBlock` (used by `Bc4`/`Bc5`) versus `Dxt/DxtDecoder.DecompressDXT5` |
| Input evidence | Probe `Temp/opencode/texdecprobe` decoded the same alpha endpoints `a0 = 16, a1 = 32` through both paths. `Bc4` yields the table `16, 32, 19, 22, 25, 29, 0, 255`; `DxtDecoder.DecompressDXT5` yields `16, 32, 19, 22, 25, 28, 0, 255`. The `Bc` values match `(weight0 * a0 + weight1 * a1 + 1) / divisor`; the `Dxt` values match the same expression without the `+ 1`. |
| Source-derived behavior | The two decoder families are separate forks (bcdec vs uTinyRipper) with different rounding. `Bc4`/`Bc5` round up; `DxtDecoder` truncates. |
| Root cause | A shared alpha table would make one family disagree with the reference. |
| Rust change | Added `resources::texture_decode::bc4_alpha_table` (with `+ 1`) and kept `dxt5_alpha_table` (without), and pointed `decode_bc4`/`decode_bc5` at the rounded table. |
| Regression evidence | The committed fixture `src/resources/texture_decode_fixture.txt` includes six direct table-probe rows (`BC4T`) whose `pixel i` index is `i % 8`, so every table entry is asserted for six endpoint pairs. |
| Remaining comparison | The DXT decoder source confirms the truncating formula; only the sampled endpoints were probed. |


## DIFF-ETC-ALPHAORDER-001 — ETC2A8 alpha and color use opposite block permutations

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Etc/EtcDecoder.cs`: `DecodeEtc2Block` writes `output[WriteOrderTable[i]]`; `DecodeEac11Block` writes `output[WriteOrderTableRev[i] * 4]` |
| Input evidence | Probe `Temp/opencode/etcprobe` decoded an ETC2A8 block whose alpha indices ramp and whose color block is fixed; the resulting alpha channel is the reverse permutation of the color channel, which a naive shared loop would not reproduce. |
| Source-derived behavior | `WriteOrderTableRev[i] = WriteOrderTable[15 - i]`, so the alpha channel of pixel `p` is filled from index `15 - p`. |
| Root cause | A single shared write order would swap the alpha of symmetric-looking blocks. |
| Rust change | `resources::etc_decode` keeps `WRITE_ORDER_TABLE` for color and `WRITE_ORDER_TABLE_REV` for the EAC/alpha channel, and `decode_eac11_constant`/`decode_eac11_table` iterate the reversed table. |
| Regression evidence | `resources::etc_decode::tests::etc_decoders_match_the_reference_runtime` asserts all 23 committed rows, including the ETC2A8 rows and the EAC R/RG rows that exercise the reversed order. |
| Remaining comparison | Only 4×4 single-block inputs were probed; multi-block clipping is asserted structurally but not differentially. |


## DIFF-ATC-COLOR-001 — ATC endpoint interpolation and explicit-alpha branch differ from DXT

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Atc/AtcDecoder.cs` `DecodeColors`/`Extend` |
| Input evidence | Probe `Temp/opencode/texprobe2` decoded `c0 = 0x1CE7, c1 = 0x1CE7` (opaque) and `c0 = 0x9CE7, c1 = 0x001F` (explicit alpha). The opaque endpoints expand through `Extend(v,5,8) = (v<<3)|(v>>2)` and interpolate with `5/8` and `3/8` weights, not the DXT `1/3`/`2/3` weights. |
| Source-derived behavior | ATC's explicit-alpha branch sets color 0 to zero and derives color 1 as `max(0, c2 - c3/4)`; the alpha interpolation reuses the truncating DXT5-style table. |
| Root cause | Reusing the DXT color routine or the `+1`-rounded `Bc` alpha table would disagree with the reference. |
| Rust change | Added `resources::atc_yuy2` with ATC-specific `decode_colors`/`extend` and the truncating `dxt5_alpha_table` for alpha. |
| Regression evidence | `resources::atc_yuy2::tests::atc_and_yuy2_match_the_reference_runtime` asserts all five committed rows. |
| Remaining comparison | Only two ATC blocks were probed. |


## DIFF-BC6H-HALF-001 — BC6H 16-bit values are reinterpreted as half-floats for output conversion

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Bc/Bc6h.cs`: the decoded buffer is typed `ColorRGB<Half>` and `CopyBufferToOutput<ColorRGB<Half>, Half, TOutputColor, TOutputChannelValue>` is used |
| Input evidence | Probe `Temp/opencode/bc6hprobe` decoded the mode-0 block `0030557a...`. The reference output `1700ffff...` (R=23, G=0, B=255) is impossible if the 16-bit values are converted as integers (`(v*255+32895)>>16`), because `FinishUnquantize` caps at `31743` (which maps to `124`). Interpreting the same 16-bit values as binary16 floats reproduces the output. |
| Source-derived behavior | `FinishUnquantize` returns a `ushort` that is stored into a `ColorRGB<Half>` buffer, so each channel is a half-float bit pattern. The conversion to `byte` is `clamp((float)half * 255)`. |
| Root cause | A first port converted the 16-bit values as integers, which disagreed on every channel. |
| Rust change | `resources::bc6h` converts through `half_bits_to_f32` + `float_to_byte`, and the half/float helpers in `texture_decode` were made crate-visible. |
| Regression evidence | `resources::bc6h::tests::bc6h_matches_the_reference_runtime` asserts all 19 committed rows across the fourteen modes. |
| Remaining comparison | Only the unsigned path (`isSigned = false`) is exercised, matching the `TextureConverter` call site. |


## DIFF-PVRTC-BLUE-001 — PVRTC second-colour blue expands into the wrong slot

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Pvrtc/PvrtcDecoder.cs` `Unpack5554Colour`, non-opaque branch |
| Input evidence | Probe `Temp/opencode/pvrtcprobe` decoded a PVRTC4 8×8 image whose colours exercise the translucent branch. The reference writes `abColors[i * 4 + 2]` but then expands `abColors[0 * 4 + 2]` for both colours, so the second colour's blue (slot 6) is left un-replicated. A "corrected" port that expands slot 6 disagrees on the affected pixels. |
| Source-derived behavior | Colour A's blue is expanded twice (once with `>> 3` and once with `>> 4`); colour B's blue is never expanded. |
| Root cause | A literal translation of the reference is required; "fixing" the index changes the output. |
| Rust change | `resources::pvrtc::unpack_5554_colour` reproduces the slot-2 write for both branches. |
| Regression evidence | `resources::pvrtc::tests::pvrtc_matches_the_reference_runtime` asserts all four committed rows for PVRTC2 and PVRTC4. |
| Remaining comparison | Only 8×8 power-of-two images were probed. |

## DIFF-ASTC-BITTRANSFER-001 — `BitTransferSigned` argument order

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Astc/AstcDecoder.cs`, `DecodeEndpoints` cases 5, 9, and 13 |
| Input evidence | Probe `Temp/opencode/astcprobe` decoded 4×4 and 8×8 blocks. A port that treated the first `BitTransferSigned` argument as `v[0]` disagreed on every multi-partition block, for example block 0 of the first row: reference `136,178,42,128` versus the swapped port `33,25,0,50`. |
| Source-derived behavior | The reference calls `BitTransferSigned(ref v[1], ref v[0])`, so the first parameter `a` is `v[1]` and `b` is `v[0]`. Inside the helper `b = (b >> 1) | (a & 0x80)` and `a = (a >> 1) & 0x3f` (signed). |
| Root cause | The initial Rust port named the locals `a0 = v[0]`, `b0 = v[1]` and passed them in that order, mirroring the reference's register roles instead of its argument order. |
| Rust change | `resources::astc::decode_endpoints` now passes `(&mut v1, &mut v0)`, `(&mut v3, &mut v2)`, `(&mut v5, &mut v4)`, and `(&mut v7, &mut v6)` for cases 5, 9, and 13. |
| Regression evidence | `resources::astc::tests::astc_matches_the_reference_runtime` asserts all eight committed rows. |
| Remaining comparison | Only 4×4, 6×6, and 8×8 block footprints were probed. |

## DIFF-ASTC-SHIFT-001 — C# masks `ulong` shift counts

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by the same differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Astc/AstcDecoder.cs`, `GetBits64` and `BitReverseU8` |
| Input evidence | The first Rust run panicked with "attempt to shift left with overflow" at `GetBits64`'s `bit <= 0` branch and at `BitReverseU8`'s `>> (8 - bits)` when `bits == 0`. |
| Source-derived behavior | C# shifts a `ulong` by `count & 0x3f` and a promoted `int` (from `byte`) by `count & 0x1f`; `BitReverseTable[c] >> 8` is `0`, not an error. |
| Root cause | Rust's `<<`/`>>` on fixed-width integers panic (debug) when the shift count is not less than the width; the source relies on C#'s masking. |
| Rust change | `resources::astc::get_bits64` uses `wrapping_shr`/`wrapping_shl` for every branch, and `bit_reverse_u8` promotes the table byte to `i32` before shifting. |
| Regression evidence | The committed `astc_fixture.txt` rows include the reversed-intseq blocks that reach the masked shifts. |
| Remaining comparison | The masking behavior is asserted through decoded output, not by a direct helper probe. |

## DIFF-MESH-GENERATED-API-001 — serialized mesh field names from the generated API

| Field | Finding |
|---|---|
| Status | Source-derived evidence recorded; not an executed serialization differential |
| Source location | Shipped `AssetRipper.SourceGenerated.dll` (`IMesh`, `ISubMesh`, `IBoneWeights4`, `IStreamInfo`, `IChannelInfo`, `ICompressedMesh`); `AssetRipper/Source/AssetRipper.AssemblyDumper/Passes/Pass002_RenameSubnodes.cs` (`data[0]` becomes `data_0_`); `ValidNameGenerator.GetValidFieldName` |
| Input evidence | Probe `Temp/opencode/meshprops` dumped the generated property lists, committed as `src/resources/mesh_generated_api_fixture.txt` (105 lines). `ISubMesh` exposes `BaseVertex`/`FirstByte`/`FirstVertex`/`IndexCount`/`IsTriStrip`/`LocalAABB`/`Topology`/`TriangleCount`/`VertexCount`; `IBoneWeights4` exposes `BoneIndex_0_`..`BoneIndex_3_` and `Weight_0_`..`Weight_3_`; `IStreamInfo` exposes `ChannelMask`/`Offset`/`Stride_Byte`/`Stride_UInt32`/`DividerOp`/`Frequency`. |
| Source-derived behavior | The naming rule replaces invalid characters (`[`/`]`) with underscores, prefixes `m_`, and capitalizes, so `BoneIndex_0_` inverts to the TypeTree name `boneIndex[0]` and `Stride_Byte`/`Stride_UInt32` invert to a `stride` field with two storage widths. |
| Root cause | The frozen snapshot does not contain the generated `SubMesh`/`BoneWeights4` classes, so the TypeTree spellings cannot be read directly from it. |
| Rust change | `resources::mesh::read_sub_meshes`/`read_skin`/`read_streams` use the inverted names; a regression test pins the fixture property sets the mapping relies on. |
| Regression evidence | `resources::mesh::tests::generated_api_fixture_supports_the_field_name_mapping` asserts all relied-upon properties. |
| Remaining comparison | A residual `m_`-prefix ambiguity remains for sub-struct children, and no serialized `Mesh` payload or C# serialization round-trip has been executed. |

## DIFF-MESH-TYPETREE-GATES-001 - real class-43 TypeTrees correct the reader's version gates

| Field | Finding |
|---|---|
| Status | Source-derived evidence recorded; not an executed serialization differential |
| Source location | Reference `SourceTpk` (`AssetRipper.SourceGenerated.SourceTpk`) via `AssetRipper.Tpk` 2.0.0.0: class-43 release TypeTrees for 3.5.0a0, 5.0.0a0, 2019.0.0a0, 6000.6.0a5 |
| Input evidence | Probe `Temp/opencode/tpkprobe` walked the reference Tpk recursion and dumped every release node; the 909-line dump (185/223/238/255 nodes) is committed as `src/resources/mesh_typetree_fixture.txt`. |
| Source-derived behavior | Unity 3.5 `VertexData` has four fixed `m_Streams[0..3]` (`channelMask`/`offset`/`stride`/`align`, all `UInt32`) and no `m_Channels`; Unity 5.0+ uses `m_Channels` (`stream`/`offset`/`format`/`dimension`, all `UInt8`); `m_IndexFormat` exists only on 2019+. Unity 3.5 `CompressedMesh` carries `m_BindPoses`/`m_Colors` but no `m_FloatColors`/`m_UVInfo`; Unity 5.0+ is the reverse. Unity 3.5 `SubMesh` carries `isTriStrip` (`UInt32`) + `triangleCount` instead of `topology`, and has no `baseVertex`. `m_MeshMetrics[0/1]` exist only on 2019+. |
| Root cause | The reader previously required `m_BindPoses`/`m_FloatColors`/`m_Colors` unconditionally, read `m_Streams` only as a list, and asserted `m_IndexFormat` on all versions, so a real Unity 3.5 or 5.0 payload would fail or misread. |
| Rust change | `resources::mesh::read_compressed_mesh` treats the four gated fields as optional with presence-derived gates; `read_vertex_data` falls back to the four fixed `m_Streams[N]` structs; `m_IndexFormat` absence keeps the 16-bit default; the `SubMesh` `topology`/`baseVertex`/`triangleCount`/`isTriStrip` branches were already optional and are now pinned per version. |
| Regression evidence | `resources::mesh::tests::real_class43_typetrees_match_the_reader_field_names` asserts the exact child lists, gates, and element types for all four versions; `unity_3_5_fixed_streams_are_read_without_channels`, `a_unity_5_compressed_mesh_reads_with_the_pre_five_gates_off`, and `a_pre_unity_5_compressed_mesh_reads_with_the_unity_5_gates_off` pin the reader behavior. |
| Remaining comparison | No serialized `Mesh` payload from Unity and no C# serialization round-trip has been executed; the fixture is the reference's own TypeTree database, so a Unity/AssetRipper divergence inside the Tpk itself would not be caught. `m_CurrentChannels` (3.5/5.0) is not captured because the read path never consumes it. |

## DIFF-MESH-METRIC-001 - `CalculateMeshMetric` value and error behavior

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.SourceGenerated.Extensions/MeshExtensions.cs` (`CalculateMeshMetric`, `TriangleArea`); `TriangleEnumerable.cs` |
| Input evidence | Probe `Temp/opencode/meshread` invoked the private static directly through an expression-tree delegate (five value cases), committed as `src/resources/mesh_metric_fixture.txt`: square `1`, scaled `0.25`, flat `1`, empty `1`, strip `0.5`. A second probe section filled versioned meshes and read back `MeshMetrics_0_/1_`: `1`/`1` on `Mesh_2019` (`Has_MeshMetrics_0_/1_` true), `0`/`0` on `Mesh_5` (both gates false), matching the `m_MeshMetrics` TypeTree fields which exist only on 2019+. |
| Source-derived behavior | The metric is left-associative float division (`vertexAreaSum / n / uvAreaSum`); a single unit triangle yields `1`, not `0.5`. Short strips yield nothing without error; partial triangle/quad groups and out-of-range indexes throw in the reference. The `FillWithCompressedMeshData` stamp persists only when the generated class carries the fields. |
| Root cause | N/A (new port; the hand computation in the first draft said `0.5` for one triangle and the probe corrected it before the test was written). |
| Rust change | `resources::mesh_metric::{calculate_mesh_metric, triangle_triples, triangle_area_v2, triangle_area_v3}` with the source operation order; throws become `MeshMetricError` values. |
| Regression evidence | `resources::mesh_metric::tests::metric_matches_the_reference_runtime` asserts all five probed words exactly, `metric_stamp_follows_the_generated_class_gates` pins the version-gated stamp, plus winding/degenerate/partial-group/unsupported/out-of-range tests. |
| Remaining comparison | Only five value cases were probed. The stamp write path itself is not ported; any future port must gate it on field presence. |

## DIFF-SHADER-DUMMY-001 - shader script/parsed-form version gates and unparsed dummy text

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.SourceGenerated` class-48 `Shader_*`/`IShader` (`Script`, `ParsedForm`, `Has_Script`, `Has_ParsedForm`); `AssetRipper.Export.UnityProjects/Shaders/DummyShaderTextExporter.cs` (`ExportShader`); `SimpleShaderExporter.cs` (`TryCreateCollection`); `ProjectExporter.Overrides.cs` (shader registration order) |
| Input evidence | Probe `Temp/opencode/shaderexport` created real versioned shaders (`Shader.Create`, SDK 10.0.401), set `Script`, and called the shipped `DummyShaderTextExporter.ExportShader` plus both exporters' `TryCreateCollection`. Gates: `Shader_3_5`/`Shader_5` report `Has_Script` true / `Has_ParsedForm` false; `Shader_2019_3_0_b0`/`Shader_2022_2_0_b10` report the reverse. Six unparsed-path rows are committed as `src/export/shader_dummy_fixture.txt` (escaped `SEXPORT` rows: script with/without `SubShader`, with `Program`, on both script-carrying versions). |
| Source-derived behavior | The unparsed dummy output is the script header up to (not including) the first `SubShader`, then `\t//DummyShaderTextExporter\n`, one tab, the fallback body, and `}`; a missing `SubShader` returns false with no output. The header slice retains the script's own whitespace before `SubShader` (hence the doubled tab in the probed rows). `Simple` admits exactly scripts with decompiled text and is tried first under every mode, so a decompiled script never reaches the dummy exporter through the stack; `Decompile` mode falls back to the dummy exporter (no decompiler exporter exists in this snapshot). |
| Root cause | N/A (new port; the stacked-vs-direct distinction was caught while writing the test: the first draft asserted the dummy text through the stack for an admitting script, which the reference would route to `Simple` instead). |
| Rust change | `resources::shader::{ShaderView, script_has_decompiled_text}` (byte-oriented; presence-driven gates) and `export::shader_export::{export_shader_text, render_unparsed_dummy_bytes}` reusing `select_shader_exporter`; `Yaml` and parsed-form dummy are explicit errors, never silent fallbacks. |
| Regression evidence | `export::shader_export::tests::dummy_text_matches_the_reference_runtime` asserts all six probed rows byte-for-byte, separating direct-dummy assembly from stack dispatch; `byte_and_str_dummy_assembly_agree_on_valid_utf8` pins the byte/str equivalence; four `export-shader` CLI tests cover a synthetic class-48 SerializedFile end to end. |
| Remaining comparison | Only three script shapes on two versions were probed. The parsed-form dummy path (property block plus `TemplateList` selection over generated objects) is not ported; 2019+ shaders with a parsed form fail explicitly. No Unity-produced shader payload has been exported. |

## DIFF-CUSTOM-CURVE-REGEX-001 - `UnknownPathRegex` without a regex dependency

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs` (`UnknownPathRegex`: `^path_0x[0-9A-F]{1,10}_[H-Wh-w]{6}[HJLN]$`) |
| Input evidence | Probe `Temp/opencode/rpathprobe` evaluated the real .NET regex (same pattern literal, `RegexOptions.Compiled`, SDK 10.0.401) on 21 paths; rows committed as `src/resources/custom_curve_rpath_fixture.txt`. The four manufactured shapes match; lowercase hex, wrong prefixes, short/long tails, out-of-range tail letters (`G`, `Q`, `X`), and embedded separators do not. |
| Source-derived behavior | The matcher is a pure classifier over ASCII: literal `path_0x`, one to ten uppercase hex digits, one underscore, six `[H-W]`/`[h-w]` characters, one `[HJLN]` character. No regex crate is added; the manual matcher implements exactly this shape. |
| Root cause | N/A (new port). |
| Rust change | `resources::custom_curve::is_unknown_path` plus the surrounding data-driven `ToAttributeName` branches (constraint tables, renderer/material/sprite rules, converter prefixes). |
| Regression evidence | `resources::custom_curve::tests::unknown_path_matches_the_dotnet_regex` asserts all 21 probed rows; the shape test pins the boundary literals independently of the fixture. |
| Remaining comparison | The regex itself has no version branches. Scene- and checksum-dependent `ToAttributeName` branches are not ported. |

## DIFF-STREAMED-SLOPES-001 - streamed-curve slopes and frame decoding

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.Processing/AnimationClips/Editor/StreamedCurveKey.cs` (`Read`, `CalculateSlopes`, `SetRegularSlopes`, `SetNonNegSpecialSlopes`, `CalculateSpecialSlopes`, `SetNegInfSlope`, `SetPosInfSlope`, `HasDifferentLimits`); `StreamedFrame.cs` (`Read`, duplicate collapse, 2017.1+ `Align`) |
| Input evidence | Probe `Temp/opencode/sclipprobe` drove real `StreamedCurveKey` instances through `CalculateSlopes` (eight cases: regular, negative-infinity with arithmetic next-limit, a chained inexact-limit second calculation, equal/step limits with and without negative-infinity support, fractional times) and real `StreamedFrame.Read` over crafted little/big-endian buffers (duplicate collapse, pre/post-2017.1 alignment). Eleven rows committed as `src/resources/streamed_curve_fixture.txt`, with slopes, values, and next-key limits as exact bit words. |
| Source-derived behavior | Regular slopes use double-precision deltas with single-precision casts at the source positions; a coefficient-free next key under negative-infinity support gets an arithmetic left limit marked inexact. Zero-coefficient keys yield zero, positive, or negative infinity by the limit comparison; discontinuous estimates use the `5e-7f`-as-double threshold against `max(|LSL|, |RSL|)`. Frames collapse consecutive same-index curves keeping the last and align to four bytes on 2017.1+. The first comparison in `HasDifferentLimits` promotes the right limit to double (not a float truncation), which the port reproduces. |
| Root cause | N/A (new port; one test-replay bug on the chained row was caught before committing: the replay middle key initially carried the wrong coefficients and produced negative instead of positive-zero-third-slope output, fixed by reconstructing the exact probe sequence). |
| Rust change | `resources::streamed_curve::{StreamedCurveKey, StreamedFrame}` with bounded reads; truncation, negative counts, and oversized counts are explicit `StreamedCurveError` values. |
| Regression evidence | `resources::streamed_curve::tests::slope_calculation_matches_the_reference_runtime` asserts all eight probed slope rows bit-for-bit (replaying the chained arithmetic limit through the same two-call sequence); `frame_read_matches_the_reference_runtime` asserts all three frame rows; truncation/negative-count tests cover the error paths. |
| Remaining comparison | Eight slope shapes and three frame layouts were probed. `GenerateFramesFromStreamedClip` buffer assembly and the converter orchestration (`ProcessStreams`, bindings cache) are not ported. No Unity-produced clip has been decoded. |

## DIFF-CLIP-CONVERT-001 - next-frame search and banker's rounding

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.Processing/AnimationClips/AnimationClipConverter.cs` (`TryGetNextFrame`, `CalculateLastConstantTime` frame math via `float.Round`) |
| Input evidence | Probe `Temp/opencode/sclipprobe` invoked the private static `TryGetNextFrame` via reflection over frames with curve indexes `[3,7]`, `[7,3]`, `[7]` (six search rows) and evaluated `float.Round` on nine inputs; rows appended to `src/resources/streamed_curve_fixture.txt` (`SNEXT`, `ROUND`). The rounding rows confirm to-even behavior with sign preservation (`-0.5` rounds to `-0.0`). |
| Source-derived behavior | The search scans later frames in order and returns the first frame/position holding the curve ID, or misses. Frame-count math uses banker's rounding before truncation. |
| Root cause | N/A (new port). |
| Rust change | `resources::streamed_curve::try_get_next_frame` and `resources::clip_convert::{bankers_round, truncate_frame_count, calculate_last_constant_time}` plus the bindings cache, first-frame fix, dense/constant rules, and default weight. |
| Regression evidence | `next_frame_search_matches_the_reference_runtime` asserts all six search rows; `bankers_round_matches_the_reference_runtime` asserts all nine rounding words including the negative-zero case; branch tests cover every `CalculateLastConstantTime` path with synthetic frames. |
| Remaining comparison | The rounding probe covers nine inputs; the stop-time branches are synthetic-only. Curve emission stays open. |

## DIFF-AUDIO-DECODE-001 - decoder dispatch, diagnostic fill, and clip version gates

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values, plus one source-derived correction |
| Source location | `AssetRipper.Export.Modules.Audio/AudioClipDecoder.cs` (`TryDecode`, `CheckMagic`, `CopyPrintable`, `CopyHex`); `AssetRipper.SourceGenerated.Extensions/AudioClipExtensions.cs` (`GetAudioData`, `CheckAssetIntegrity`); `FmodSoundTypeExtensions.cs` (`ToRawExtension`); generated class-83 `AudioClip_*` gates |
| Input evidence | Probe `Temp/opencode/audioprobe` drove real `AudioClip.Create` instances with `AudioData`/`Type` set through the shipped `TryDecode` (SDK 10.0.401): the empty early message, typed `wav`/`ogg` passthrough with raw bytes, and the unmapped-type `audioClip` default — four rows committed as `src/export/audio_decode_fixture.txt`. Probe `Temp/opencode/audioprops` pinned the `IAudioClip` properties, the per-version gates (3.5 inline+typed, 5.0+ resource), and the `FmodSoundType`/`FmodSoundFormat` discriminants (`src/resources/audio_generated_api_fixture.txt`). The magic/diagnostic branches are reachable only through resource-backed 5.0+ clips, so they stay source-read. |
| Source-derived behavior | On 3.5 layouts `Has_Type` is version-true, so `TryDecode` always takes the typed path there; the magic checks apply to untyped (resource-backed) data. The unknown-byte diagnostic fills with U+00D8 (`'Ø'`), not ASCII `'O'`: the prior Rust port used the wrong fill and its test encoded it. Fixed to the source literal with the test corrected. |
| Root cause | The fill-character deviation predates this increment (wrong by one codepoint in `diagnostic_printable`); caught while tracing the diagnostic path for the CLI error. |
| Rust change | `resources::audio::{AudioClipView, fmod_sound_type_raw_extension}` (presence-driven gates, source-order resolution, lazy integrity check); `export::audio_export::try_decode_audio` composing the reference order with exact messages; `export-audio` CLI with inline and streamed-resource resolution. `FSB5`/Vorbis stay explicit errors. |
| Regression evidence | `export::audio_export::tests::try_decode_matches_the_reference_runtime` asserts all four probed rows; the corrected fill test pins U+00D8; seven `export-audio` CLI tests cover typed/tracker passthrough, streamed resolution, empty/unknown/FSB5 failures, the class-83 guard, and overwrite refusal. |
| Remaining comparison | Only four decode shapes were probed; `OggToWav` could not load in the probe sandbox (NAudio closure mismatch) and stays source-read. No Unity-produced clip has been exported. |

## DIFF-SHADER-PARSED-001 - parsed-form dummy property rendering and template selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows after one spacing fix |
| Source location | `AssetRipper.Export.UnityProjects/Shaders/DummyShaderTextExporter.cs` (`Export(IShader)`, `Export(ISerializedProperties)`, `Export(ISerializedProperty)`); `AssetRipper.SourceGenerated.Extensions/SerializedPropertyExtensions.cs` (`GetTypeString`, `GetDefaultValue`, `GetAttributes`); `TemplateList.cs`/`TemplateShader.cs`/`RequiredProperty.cs`/`PropertyType.cs`; `SerializedPropertyType.cs`/`SerializedPropertyFlag.cs` |
| Input evidence | Probe `Temp/opencode/shaderexport` built real 2019.4 `Shader.Create` instances with a `ParsedForm` through the generated API (lit, flags, single) and called the shipped `ExportShader` (SDK 10.0.401); rows committed as `src/export/shader_parsed_fixture.txt`. Probe `Temp/opencode/floatprobe` pinned the 28 `float.ToString(InvariantCulture)` values behind default rendering (`src/export/float_tostring_fixture.txt`). |
| Source-derived behavior | Property lines emit stored attributes then flag keywords in fixed order, `Vector` for color/vector, `Range(min, max)` with a space, texture keywords by `TexDim`, vector defaults `(v0,v1,v2,v3)` without spaces, and scalar defaults from `DefValue_0_`; the header/properties/marker/template/fallback/custom-editor assembly matches `render_parsed_dummy`. The shipped-DLL templates agree with the frozen snapshot sources (spaced `Default`, unspaced others). |
| Root cause | The first Rust port rendered `Range(min,max)` without the reference space; the differential caught it. |
| Rust change | `resources::shader::{ParsedShaderProperty, ParsedShaderForm, ShaderView::from_value}` decoding; `export::shader_export::{render_parsed_property, render_parsed_properties, parsed_dummy_body, export_parsed_dummy}` wired into the `Dummy`/`Decompile` stack. Serialized `m_*` names follow the class-48 convention without a local class-48 TypeTree dump. |
| Regression evidence | `export::shader_export::tests::parsed_dummy_text_matches_the_reference_runtime` matches all three probed outputs byte-for-byte through direct assembly and the mode stack; `float_tostring_matches_the_reference_runtime` pins all 28 formatter rows. |
| Remaining comparison | Only three property shapes were probed; `any`/`3D`/`2DArray`/`CubeArray` dimensions and `NoScaleOffset`/`Normal` flags are asserted directly. No Unity-produced shader has been exported. |

## DIFF-BLENDSHAPE-001 - blend-shape CRC core and lookup precedence

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded for the CRC core; branch dispatch asserted directly |
| Source location | `AssetRipper.SourceGenerated.Extensions/MeshBlendShapeExtensions.cs` (`IsCRCMatch`); `BlendShapeDataExtensions.cs` (`FindShapeNameByCRC`); `MeshExtensions.cs` (`FindBlendShapeNameByCRC`); `MeshBlendShapeChannelExtensions.cs` (`SetValues`); `AssetRipper.Checksum.Crc32Algorithm` (external package) |
| Input evidence | The 12 `Crc32Algorithm.HashUTF8` words plus the `MatchUTF8` accept/reject columns committed in `src/resources/crc28_fixture.txt` (probe `Temp/opencode/crcprobe`, shipped `AssetRipper.Checksum.dll`, SDK 10.0.401) are re-asserted through the new lookup path. Probe `Temp/opencode/blendprops` confirmed the generated property surface (`IBlendShapeData`, `IMeshBlendShapeChannel`, `IMeshBlendShape`, `IMesh.Shapes/ShapesList`). Serialized field names come from the committed 5.0 class-43 TypeTree dump (`mesh_typetree_fixture.txt`). |
| Source-derived behavior | `IsCRCMatch` is a null-guarded CRC equality; the channel scan returns the first stored-hash match; the mesh dispatch returns the channel-table result whenever `m_Shapes` is present and only otherwise scans the legacy list. `SetValues` writes `Name`, `NameHash = HashUTF8(name)`, `FrameIndex`, `FrameCount` on a mutable generated channel. |
| Root cause | No deviation found; this increment ports previously uncovered lookup behavior. |
| Rust change | `resources::blend_shape::{crc32_ieee, is_crc_match, find_channel_name_by_crc, BlendShapeLookupView}` with presence-driven decoding. `SetValues` stays caller-side (needs mutable generated objects). |
| Regression evidence | `crc32_and_match_reproduce_the_shipped_checksum_words` matches all 12 shipped words plus accept/reject; seven further tests pin first-match order, the no-fallthrough precedence, legacy scanning, decoding, and error paths. |
| Remaining comparison | No Unity-produced mesh with blend shapes has been looked up; the 4.x `m_ShapesList` layout is asserted synthetically (no 4.x TypeTree dump is committed). |

## DIFF-SCRIPT-IDENTITY-001 - assembly names and script identity

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.IO.Files/SpecialFileNames.cs` (`FixAssemblyName`, `RemoveAssemblyFileExtension`, `AddAssemblyFileExtension`, `IsProjectAssembly`); `AssetRipper.Import.Structure.Assembly/ScriptIdentifier.cs`; `ScriptingBackend.cs`; `MonoScriptExtensions.cs` (`GetFullName`, `GetValidAssemblyName`) |
| Input evidence | Probe `Temp/opencode/scriptidprobe` (SDK 10.0.401) drove the shipped `AssetRipper.IO.Files.dll` and `AssetRipper.Import.dll`; all 49 rows committed as `src/io/script_identity_fixture.txt`. The probe pinned the case-sensitive edges (`csharp`/`CSHARP.DLL`/`A.DLL` untouched), the prefix-before-strip order (`Boo.dll` → `Boo`, unprefixed), the `Assembly-`/`Assembly - ` prefix pair, the `[assembly]name` unique-name shapes, the default `ToString` type-name fallback, and the `ArgumentException`/`ArgumentNullException` rejections. |
| Source-derived behavior | Legacy module names gain `Assembly - ` before `.dll` stripping; `GetValidAssemblyName` falls back to `Assembly-CSharp` on empty; `GetFullName` qualifies only with a non-empty namespace. |
| Root cause | No deviation found; this increment ports previously uncovered identity behavior. |
| Rust change | `io::special_names::{fix_assembly_name, remove/add_assembly_file_extension, is_project_assembly}`; `processing::script_identity::{ScriptIdentifier, ScriptingBackend, get_full_name, get_valid_assembly_name}`. |
| Regression evidence | `assembly_name_helpers_match_the_reference_runtime` matches all 30 name rows; four identity tests match all 19 remaining rows. The null-namespace rejection is documented as unrepresentable in Rust. |
| Remaining comparison | The `IMonoScript`/assembly-manager resolutions (`GetBehaviourType`, `GetScriptID`, `IsScriptPresents`, `GetPropertiesHash`) need live managed assemblies and stay unported. |

## DIFF-EXPORT-SETTINGS-001 - configuration enum discriminants and record defaults

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.Export/Configuration/*.cs` (all 14 files); `AssetRipper.Import/Configuration/ImportSettings.cs`; `AssetRipper.Processing/Configuration/ProcessingSettings.cs` |
| Input evidence | Probe `Temp/opencode/settingprobe` (SDK 10.0.401) dumped all 59 enum discriminants plus 24 record defaults from the shipped assemblies; rows committed as `src/export/settings_fixture.txt`. The probe closed a real gap (the full 21-member `ScriptLanguageVersion`, including `Latest = 2147483647`) and confirmed every existing Rust default. |
| Source-derived behavior | Enum orders/discriminants are declaration order except `ScriptLanguageVersion` (explicit values); record defaults are the documented initializers with `DefaultVersion`/`TargetVersion` at `0.0.0a0`. |
| Root cause | No deviation found; this increment ports previously unverified discriminants and one missing enum. |
| Rust change | `export::settings::{discriminant consts, ScriptLanguageVersion (full), ExportDefaults/ImportDefaults/ProcessingDefaults}` asserted against the fixture, including the capitalized .NET boolean rendering. |
| Regression evidence | `enum_discriminants_and_record_defaults_match_the_reference_runtime` matches all 59 discriminants and 24 defaults. |
| Remaining comparison | Automatic language selection and version-record defaults need external version ordering and stay unported. |

## DIFF-SCRIPT-RESOLVER-001 - TypeTreeResolver match and failure reasons

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.Import.Structure.Assembly.Serializable/TypeTreeResolver.cs` |
| Input evidence | Extended probe `Temp/opencode/scriptidprobe` (SDK 10.0.401) invoked the internal shipped resolver through reflection: `RESOLVE|empty|0|<not-found>` and `RESOLVE|broken-tree|0|<node-build-failed>` appended to `src/io/script_identity_fixture.txt`. The broken-tree row proves the three-way match fired (a miss would report not-found). |
| Source-derived behavior | Ordinal class/namespace match plus extension-stripped assembly match, first-match scan order, and the two verbatim failure reasons. A default (null-name) identifier can never match. |
| Root cause | No deviation found; this increment ports previously uncovered resolver behavior. |
| Rust change | `processing::script_identity::{type_reference_matches_script, find_type_reference_index, TYPE_REFERENCE_NOT_FOUND, TYPE_TREE_NODE_BUILD_FAILED}` over the existing `SerializedTypeReference` metadata model. |
| Regression evidence | `type_reference_scan_matches_the_reference_runtime` replays the probed match and pins both literals; `type_reference_match_covers_assembly_stripping_case_and_absence` pins stripping, ordinality, absence, the default-identifier rule, and first-match order. |
| Remaining comparison | Building nodes from the matched `OldType` needs the TypeTree reader and stays unported. |

## DIFF-FILE-NAME-001 - export file-name planning and splitter edges

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows after two fixes |
| Source location | `AssetRipper.IO.Files/FileSystem.cs` (`RemoveCloneSuffixes`, `RemoveInstanceSuffixes`, `FixInvalidFileNameCharacters`, `FixInvalidPathCharacters`, `IsReservedName`, `GetUniqueName`, `MaxFileNameLength`); `Utf8Truncation.cs`; `ExportCollection.cs` (`GetUniqueFileName`) |
| Input evidence | Probe `Temp/opencode/filenameprobe` (SDK 10.0.401) drove the shipped `AssetRipper.IO.Files.dll`, including `LocalFileSystem.GetUniqueName` against real temporary directories; 86 rows committed as `src/io/file_names_fixture.txt` (pure ASCII via probe `\u` escapes after a cp437 pipeline mojibake was found and fixed). |
| Source-derived behavior | Host-dependent invalid sets and the Windows-only reserved gate (mirrored, not normalized); the `.`/bare-dot extension asymmetry (`a.` stems to `a` with no extension); truncation backs off split sequences; the remembered counter only skips checked-taken names. |
| Root cause | The first Rust port used one symmetric splitter (wrong on `a.`/`.`) and naive pipe splitting (wrong on pipe-bearing names); both fixed and regression-covered. |
| Rust change | `io::file_names::{UniqueNamePlanner, unique_export_file_name, ExportFileNameFacts, ...}` over caller-supplied existence facts. |
| Regression evidence | Five fixture-driven tests match all 86 probed rows plus the composition steps; OS-dependent rows degrade exactly like the source on non-Windows hosts. |
| Remaining comparison | Writing the asset bytes and the `{file}.meta` sidecar stays with the export pipeline; the add-only sidecar refusal (vs source overwrite) is the documented deviation. |

## DIFF-MONO-UTILS-001 - managed type classification and engine-asset names

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.SerializationLogic/MonoUtils.cs`; `AssetRipper.Import.AssetCreation/GameAssetFactory.cs` (`CreateEngineAsset`) |
| Input evidence | Probe `Temp/opencode/monoutilsprobe` (SDK 10.0.401) drove the shipped assemblies: 469 predicate cells, 268 `ToPrimitiveType` cells, 17 `PrimitiveType` discriminants, and 23 `CreateEngineAsset` outcomes at 2019.4.0f1; rows committed as `src/processing/mono_utils_fixture.txt`. |
| Source-derived behavior | Namespace-gated tables with exact const spellings; only long `System` names map to primitives (aliases fall to `Complex`); 21 engine names resolve to versioned subclasses (6 version-taking), anything else throws `NotSupportedException`. |
| Root cause | No deviation found; this increment ports previously uncovered classification behavior. |
| Rust change | `processing::mono_utils::{predicates, PrimitiveType, to_primitive_type, EngineAssetKind, engine_asset_kind}`. |
| Regression evidence | Three fixture-driven tests match all 777 probed rows. |
| Remaining comparison | Assembly loading, type resolution, and object construction need live managed assemblies and stay unported. |

## DIFF-ASSET-FACTORY-001 - asset-creation dispatch and engine-asset names

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded for the engine table; dispatch rules source-read and directly asserted |
| Source location | `AssetRipper.Import.AssetCreation/GameAssetFactory.cs` (`ReadAsset`, `ReadNormalObject`, `TryReadNormalObject`, `CreateAsset`, `CreateEngineAsset`) |
| Input evidence | The 23 `ENGINE` rows of `src/processing/mono_utils_fixture.txt` (probe `Temp/opencode/monoutilsprobe` at 2019.4.0f1, SDK 10.0.401) are re-asserted through `engine_asset_kind`; the version gate, fallback chain, tail rules, and bump arithmetic follow the inspected dispatch exactly. |
| Source-derived behavior | Pre-3.5 versions are unreadable before any read; MonoBehaviour takes the script path; engine collections tolerate failures (info for shaders); Patch versions retry once with a wrapping build bump; exact/zero-pad reads pass while the 24-byte Texture2D tail takes the Chinese-fields path; generated classes win over TPK trees over unknown objects. |
| Root cause | No deviation found; this increment ports previously uncovered factory behavior. |
| Rust change | `processing::asset_factory::{classify_read_path, classify_object_tail, decide_normal_object, bump_patch_build, classify_created_asset, engine_asset_kind}`. |
| Regression evidence | Six tests pin the dispatch precedence, tail branches, fallback order with log levels, bump wraparound, creation order, and all 23 engine arms. |
| Remaining comparison | Byte reading, node building, and assembly-backed MonoBehaviour structures need generated objects and live assemblies and stay unported. |

## DIFF-SCRIPT-GUID-001 - script and assembly GUID byte layout

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; layout ported and pinned row-for-row |
| Source location | `AssetRipper.Export.UnityProjects/Scripts/ScriptHashing.cs` (`CalculateScriptGuid`, `CalculateAssemblyGuid`) via `AssetRipper.Primitives/UnityGuid.cs` (`Md5Hash`, `ConvertSystemOrUnityBytes`, `ToString`) |
| Input evidence | 16 committed rows in `src/export/script_guid_fixture.txt` (probe `Temp/opencode/guidprobe`, shipped `AssetRipper.Export.UnityProjects.dll` plus `AssetRipper.Primitives.dll`, SDK 10.0.401): 4 `GUID3` string rows, 4 `GUID1` string rows (including the `MyLib.dll` extension-strip proof), and 8 `WORDS` word rows. |
| Source-derived behavior | `Md5Hash` MD5s the concatenated UTF-8 bytes; `ConvertSystemOrUnityBytes` reverses the first four bytes, swaps each of the next two pairs, leaves the tail, then swaps nibbles per byte; words are the converted buffer read little-endian; `ToString` renders each word least-significant nibble first. A prior hand-derived "nibble swap only" description was wrong about the field flips; the machine-checked `MATCH concat` rows plus the upstream source settle it. |
| Root cause | No deviation found; this increment ports the previously open GUID companions of the FileID hash. |
| Rust change | `export::script_hashing::{UnityGuid, unity_guid_md5_hash, unity_guid_md5_hash_three, calculate_script_guid, calculate_script_guid_str, calculate_assembly_guid, remove_assembly_file_extension, fix_assembly_name}` plus a from-scratch RFC 1321 MD5; `export::guid_policy::byte_layout_in_snapshot` now reports the deterministic sources closed. |
| Regression evidence | The RFC 1321 section A.5 suite pins MD5; the fixture test asserts all 4 script strings, all 4 assembly strings, and all 8 word rows, plus `str`/bytes equivalence, `MissingReference`/zero formatting, and GUID input bounds. |
| Remaining comparison | The `IMonoScript` overloads (fixed assembly names, namespace/class bytes) need live generated script objects and stay unported. |

## DIFF-SETTINGS-FILE-001 - settings-file JSON shape

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; shape ported and pinned byte-for-byte |
| Source location | `AssetRipper.Export/Configuration/SerializedSettings.cs` (`Save`, `DefaultFileName`); `SerializedSettingsContext.cs` (`WriteIndented`); `AssetRipper.Import/Configuration/UnityVersionJsonConverter.cs` |
| Input evidence | Two committed probe outputs (`src/export/settings_file_defaults.json`, `src/export/settings_file_modified.json`; probe `Temp/opencode/settingprobe2`, shipped `AssetRipper.Export.dll`, SDK 10.0.401): fresh-record defaults plus an all-fields-non-default record. Byte-level facts verified outside the text reader: no BOM, CRLF newlines, no trailing newline. |
| Source-derived behavior | Root `Import`/`Processing`/`Export` in declaration order; enums as numbers (including `-1`); versions as `ToString()` renderings (`0.0.0a0` default); `LanguageCode` string-or-null; `IgnoreStreamingAssets` absent via `[JsonIgnore]`; two-space indentation. |
| Root cause | No deviation found; this increment ports the previously open persistence round-trip. |
| Rust change | `export::settings_file::{write_settings_file, parse_settings_file, SerializedSettingsFile, VersionText}` plus a schema-scoped JSON reader; `from_discriminant` const maps for all eleven file enums in `export::settings`. |
| Regression evidence | Both writer outputs asserted byte-for-byte against CRLF literals; both fixtures round-trip; tolerance (unknown/missing/duplicate/escapes/surrogates) and rejection (paths, bounds) tests pin the load behavior. |
| Remaining comparison | Exe-adjacent default paths, the save/delete gate, and startup application need product wiring and stay unported. |

## DIFF-GLB-ENCODING-001 - GLB accessor encodings and skin-lane rules

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; encodings ported and pinned |
| Source location | `AssetRipper.Export.Modules.Models` (`GlbSubMeshBuilder`, `MeshDataExtensions`, `VertexColor1Texture8`) via SharpGLTF 1.0.2 (`VertexJoints4`, `SparseWeight8`, `IndexWeight`) |
| Input evidence | Probe `Temp/opencode/gltfprobe` (shipped `SharpGLTF.Core/Runtime/Toolkit.dll` 1.0.2, SDK 10.0.401) dumping accessor tables and buffer lanes: `POSITION/NORMAL/TANGENT/TEXCOORD 5126`, `COLOR_0 5121 normalized`, `JOINTS_0 5121` (all joints ≤ 255) vs whole-accessor `5123` with a 300 joint, `WEIGHTS_0 5126`, small-mesh indices `5123`, 65535-vertex indices `5123`, 65538-vertex indices `5125`; color lanes `FF0000FF,00FF007F,0000FF3F` (0.5→127 truncation); skin lanes proving dupe-sum, zero-drop, renormalization (0.3→0.5), and an `ArgumentOutOfRangeException` on all-zero weights. Upstream sources read for the exact rules: `VertexSkinning.cs` (`VertexJoints4` ctor, `GetEncodingAttributes`), `SparseWeight8.cs` (`Create`, `OrderedByWeight`, sum-0/1 renormalization skip), `IndexWeight.cs` (`InsertUnsorted` merge, `IsGreaterThan`, `BubbleSortByWeight`). |
| Source-derived behavior | Colors quantize per channel with truncation, but any non-finite channel zeroes the whole color (probed with infinite/NaN lanes). Skin: negatives are rejected by the port (unrepresentable); zero-weight lanes drop; duplicate indices merge by summation; sums scale to one except exact 0/1; lanes sort by magnitude desc with smaller indices on ties; missing lanes pad joint zero; joints wider than a byte widen the whole accessor; indices wider than 65534 widen the whole accessor (65535 is reserved). |
| Root cause | Three deviations found and fixed: `COLOR_0` was FLOAT (now normalized UBYTE), `JOINTS_0` was always USHORT (now value-width), indices were always UINT (now width-switched). Two prior test expectations encoded the old assumptions and were updated with the differential as authority. |
| Rust change | `resources::glb::{quantize_color, quantize_color_channel, process_skin_bindings, push_color_accessor, push_ubyte_view}`; width-switching `push_joints_accessor`/`push_index_accessor`; new `GlbError::BoneIndexTooLarge`. |
| Regression evidence | New tests pin truncation (incl. 0.999→254), vector poisoning, merge/drop/renormalize/pad rules, both error variants, and all four accessor width switches; full lib suite passes 965/965 locally. |
| Remaining comparison | `GlbLevelBuilder` hierarchy export and `GlbTerrainBuilder` have no CLI path and stay unported; morph targets are absent on both sides (confirmed in `GlbSubMeshBuilder`). |

## DIFF-HDR-001 - Radiance HDR writer rules

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; writer ported and pinned byte-for-byte |
| Source location | StbImageWriteSharp 1.16.7 `ImageWriter.WriteHdr` (shipped `StbImageWriteSharp.dll`), called by `AssetRipper.Export.Modules.Textures/DirectBitmap\`1.cs` (`SaveAsHdr`) with `Width`, `Height * Depth`, and the `GetDataAndComponentsForSaving` bytes |
| Input evidence | Probe `Temp/opencode/hdrprobe` (SDK 10.0.401, RGBA input): 22 committed `VEC` rows in `src/resources/hdr_fixture.txt` plus two over-size boundary facts (32767-wide solid is RLE at 2191 bytes with marker `02 02 7F FF`; 32768-wide solid is flat at 131187 bytes with an `C8643280` tail) |
| Source-derived behavior | Fixed 101-byte header (`#?RADIANCE`, `# Written by stb_image_write.h`, `FORMAT=32-bit_rle_rgbe`, `EXPOSURE=` with 10 spaces plus `1.0000000000000`, blank line) then `-Y {h} +X {w}`; bytes scale by `1/255` into a shared-exponent RGBE pixel (zero maps to all-zero, alpha ignored); scanlines encode independently with new-style RLE iff `8 <= width < 0x8000`, else flat; a match of 3+ encodes as 127-capped runs where an already-classified run emits even a 1-2 tail chunk, while shorter matches accumulate 128-capped literals that stop at the next run |
| Root cause | No deviation found; this increment ports the previously open HDR writer. The run-tail rule (e.g. a 128-run emits `R127 R1`, not `R127` plus a 3-literal) and the `0x8000` flat boundary were established purely from probe bytes, not from library sources |
| Rust change | `resources::image_export::{save_hdr, rgb8_to_rgbe, encode_hdr_scanline_rle}` with integer-`frexp` conversion and bounded chunk emission; `save()` dispatches `Hdr` to the new writer |
| Regression evidence | All 22 fixture vectors reproduce byte-for-byte (including all 256 gray conversions, every probed run length 1-130 plus 300, the 300-literal chunking, the mixed literal/run row, and the two-row marker repetition); targeted tests pin both width boundaries and alpha indifference; full lib suite passes 1003/1003 locally with warnings-denied Clippy |
| Remaining comparison | EXR (`ExrWriter`) and JPEG (stb `WriteJpg`) stay `Unsupported`; the zero-width degenerate case writes header only by construction and was not probed |

## DIFF-TEXTURE-ARRAY-001 - array/3D texture selection and size rules

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; views and rules ported and pinned |
| Source location | `AssetRipper.SourceGenerated.Extensions/Texture2DArrayExtensions.cs`, `Texture3DExtensions.cs`, `CubemapArrayExtensions.cs`; `AssetRipper.Export.Modules.Textures/TextureConverter.cs` (the three `GraphicsFormat` overloads) |
| Input evidence | Probe `Temp/opencode/arrayprobe` driving live 2022.3 assets over versioned `ProcessedAssetCollection`s (SDK 10.0.401): 6 committed rows in `src/resources/texture_array_fixture.txt`, plus `Has_*` gate outcomes on 5.6/2019.4/2022.3 and the newest-class property dumps |
| Source-derived behavior | Inline bytes win with no swap or unswizzle on any of the three classes; `Texture2DArray` size divides by depth with depths below two keeping the whole size; cubemap height equals width and depth is `CubemapCount * 6` (kept verbatim with the source `Not sure about this`); cubemap size is zero with no faces; the two `Texture3D` unions share one field each (setting `Format_UInt32` reads back through `Format`); the lightmap gate is off everywhere probed so the usage mode backs it; missing mip counts derive `round(log2(maxSide))`; unconvertible graphics formats fall back to `RGBA32` |
| Root cause | No deviation found; this increment ports the previously open array/3D field layer. The probe corrected one design assumption (separate `Texture3D` format spellings are one shared field, so the view keeps both read spellings but one value) |
| Rust change | `resources::texture_array::{Texture3DView, Texture2DArrayView, CubemapArrayView, resolve_array_image_data, texture2d_array_complete_image_size, cubemap_array_height/depth/complete_image_size, Texture3DView::{format_value, texture_format_or_default, lightmap_value, complete_image_size_value, mip_count_value}}` with `wrapping_div` matching the source `(int)` casts and an explicit error for the unreachable face-count overflow |
| Regression evidence | All 6 fixture rows reproduce byte-for-byte with their derived sizes; union precedence, rounding fallback (including the zero-side refusal where the source conversion throws), size branches, overflow refusal, and view readers pinned; full lib suite passes 1018/1018 locally with warnings-denied Clippy |
| Remaining comparison | Bitmap export wiring and CLI classes 117/187/188 stay open |

## DIFF-TEXTURE-GETIMAGEDATA-001 - GetImageData platform orchestration

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; orchestration ported and pinned |
| Source location | `AssetRipper.SourceGenerated.Extensions/Texture2DExtensions.cs` (`GetImageData` lines 79-113, `IsSwapBytes` lines 135-152); `AssetRipper.IO.Files/BuildTarget.cs` (Xbox360 = 11, Switch = 38, Switch2 = 48) |
| Input evidence | Probe `Temp/opencode/texplatprobe` driving live 2022.3 `Texture2D` assets over versioned `ProcessedAssetCollection`s (SDK 10.0.401): 12 committed rows in `src/resources/texture_platform_fixture.txt`. Supporting facts from the same probe: `Has_PlatformBlob_C28` is false on 2020.1 and older, true on 2020.2+; `m_PlatformBlob` confirmed as the single UTF-16 serialized-name literal in the shipped `AssetRipper.SourceGenerated.dll` |
| Source-derived behavior | Inline bytes win and are copied before a swap (input never mutated, output is a new array); non-swap formats return the input array by reference; streamed bytes swap in place on the fresh buffer; the unswizzle gate is blob length >= 12 AND exact `Switch` equality (Switch2 passes through); odd swap counts throw `IndexOutOfRangeException`; empty input returns empty |
| Root cause | Two probe-design corrections, no source deviations: RGBA32/RGB24 are not in the swap list (first probe vectors were no-op passthroughs by mistake), and pre-2020.2 assets have no blob store (the setter silently drops, so Switch vectors need 2020.2+ assets). The Rust port maps the odd-count throw to `SwapBytesError::OddByteCount` and borrows instead of aliasing (observably identical) |
| Rust change | `resources::texture::{Texture2DView::platform_blob, resolved_image_data}` with `Cow` aliasing; `SWITCH_BUILD_TARGET`/`SWITCH2_BUILD_TARGET` consts; `TextureExportError::Swap`/`Unswizzle`; CLI threads `metadata.prefix.target_platform`/`body_endian` through `export_texture2d_image`/`export_texture2d_value` |
| Regression evidence | All 12 fixture rows reproduce byte-for-byte, including both Switch vectors through the full orchestration; view-immutability, borrow-vs-own, streamed-swap, and blob-spelling tests; full lib suite passes 1008/1008 locally with warnings-denied Clippy |
| Remaining comparison | Streamed fetching stays caller-side; unknown platforms (discriminant 0) transform nothing by construction |

## DIFF-TYPETREE-READER-001 - TypeTree old/new reader parity

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; both rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/TypeTrees/TypeTreeNode.cs (IsFormat5, HasRefTypeHash, Read, Write); TypeTree.cs (Read, SetNamesFromBuffer, GetTypeName, ReadTreeNode); CommonString.cs (113-entry StringBuffer) |
| Input evidence | Probe Temp/opencode/typetreeprobe (shipped AssetRipper.IO.Files.dll plus AssetRipper.IO.Endian.dll, SDK 10.0.401) dumping 21 generation predicates, all 113 common entries, plus C#-constructed nested payloads: legacy BE gen8 and LE gen11 (GameObject/int/Vector3f/float, 4 nodes, levels 0,1,1,2); modern LE gen10 (custom MyCustomType plus common int, no hash), BE gen19 (same with hashes), LE gen22 (common Vector3f/float with hashes); negative-count refusal. Committed as src/serialized_file/typetree_reader_fixture.txt (156 lines). |
| Source-derived behavior | Legacy form is null-terminated type/name plus size/index/flags/version/meta plus child count with depth-assigned levels; modern form is count plus buffer size plus fixed 24-byte nodes (19+ adds 8-byte hash for 32 bytes) plus string buffer with custom offsets versus 0x80000000-masked common offsets; negative counts throw InvalidDataException. |
| Root cause | No deviation found; this increment ports the previously open nested and full-table coverage. Rust names IsFormat5 as is_format5 and HasRefTypeHash as has_ref_type_hash and wires all three call sites behavior-identically. |
| Rust change | src/serialized_file/metadata.rs (is_format5, has_ref_type_hash, read_type_tree, read_modern_type_tree_node, read_legacy_type_tree_node) plus test typetree_reader_matches_reference_runtime; fixture src/serialized_file/typetree_reader_fixture.txt. |
| Regression evidence | All 21 predicates, all 113 common offsets plus gap refusals, all 5 payloads (nodes, buffer bytes, offsets, levels, hashes, trailing checks) and the fail-loud negative-count case reproduce the reference dumps; full lib suite 1351 plus 60 CLI pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced SerializedFile TypeTrees and managed-reference payload decoding stay outside these two reader rows (tracked by SER-TT-ABSENT-001, SER-MANAGED-REFS-001 and related object rows). |

## DIFF-BUNDLE-HEADER-001 - UnityFS header fields/alignment plus Archive unsupported parity

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; both rows ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/BundleHeader.cs (IsBundleHeader 0x20 minimum); BundleVersion.cs (BF_LargeFilesSupport 7); FileStreamBundleHeader.cs plus FileStreamBundleFile.cs (header fields, v7+ 16-byte alignment); Archive/ArchiveBundleHeader.cs plus ArchiveBundle.cs plus ArchiveBundleScheme.cs (detection plus NotSupported Read/Write) |
| Input evidence | Probe Temp/opencode/bundleheaderprobe (shipped IO.Files plus IO.Endian plus Primitives DLLs, SDK 10.0.401) dumping the BundleVersion enum, Archive/UnityFS detection vectors (full/short/other-magic/empty) with total-length semantics, Archive Read/Write exception types, plus reference-written minimal empty-table uncompressed combined bundles for v5/v6/v7/v8 with read-back dumps. Committed as src/container/bundle_header_fixture.txt (27 lines). |
| Source-derived behavior | Bundle detection requires total stream length 0x20 or more; Archive detection is UnityArchive magic while its Read/Write always throw NotSupportedException; UnityFS header carries signature, version, two engine strings, Size, Comp, Uncomp and Flags with no per-version field branch beyond the v7+ post-header 16-byte alignment; v5 parses with shorter engine strings; v8 behaves like v7. |
| Root cause | No deviation found; this increment ports the previously open older-version and reference-generated coverage. Rust keeps the 0x20 total-length rule, the ArchiveReadUnsupported fail-loud error, the BUNDLE_VERSION_LARGE_FILES_SUPPORT 7 constant and the v7+ alignment. |
| Rust change | src/container/unityfs.rs test bundle_headers_match_reference_generated_bundles; src/scheme.rs test archive_detection_and_unsupported_read_match_reference; fixture src/container/bundle_header_fixture.txt. |
| Regression evidence | All detection vectors, both Archive throw types, all four reference bundles (fields, Total/HeadLen/Pad, zero blocks/nodes/hash, metadata/data offsets, v7+ alignment, v5 older proof) reproduce; full lib suite 1353 plus 60 CLI pass with warnings-denied Clippy. |
| Remaining comparison | UnityFS metadata compression and directory/entry lanes stay with IO-BUNDLE-UNITYFS-META-001 and IO-BUNDLE-UNITYFS-ENTRY-001; no Unity-produced bundle file has been compared. |

## DIFF-FILE-HEADER-001 - SerializedFile header generation branches

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; row ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedFileHeader.cs (Read, Write, HasEndianess, HasLargeFilesSupport, IsSerializedFileHeader); FormatVersion.cs |
| Input evidence | Probe Temp/opencode/fileheaderprobe (shipped IO.Files plus IO.Endian DLLs, SDK 10.0.401) dumping the FormatVersion enum, 21 generation predicates, plus reference-written headers for gens 5,8,9,21,22 with read-back dumps. Committed as src/serialized_file/file_header_fixture.txt (56 lines). |
| Source-derived behavior | Pre-9 headers are 16-byte legacy with no endian marker; 9+ adds the marker plus 4-byte alignment; 22+ writes zeros at legacy positions and real values at the 0x14 extended block; MetadataSize 0 or below throws; undefined generations throw. |
| Root cause | No deviation found; this increment ports the previously open pre-9 plus reference-generated coverage. Rust keeps the same branches with stricter fail-closed bounds (documented hardening over Debug-only asserts). |
| Rust change | src/serialized_file/header.rs test headers_match_reference_written_fixtures; fixture src/serialized_file/file_header_fixture.txt. |
| Regression evidence | All 21 predicates, all five reference headers (version, sizes, offsets, marker, reserved, header size) reproduce; full suite 1355 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Full SerializedFile metadata/object integration and container extraction stay with SER-META and SER-FILE-ENTRY rows; no Unity-produced file compared. |

## DIFF-GZIP-001 - GZip wrapper decode parity with .NET

| Field | Finding |
|---|---|
| Status | .NET interop differential recorded; decode direction pinned |
| Source location | AssetRipper.IO.Files/CompressedFiles/GZip/GZipFile.cs; GZipFileScheme.CanRead |
| Input evidence | Probe Temp/opencode/gzipprobe (System.IO.Compression.GZipStream Optimal, SDK 10.0.401) emitting hello/sequence/concat vectors plus .NET round-trip proof; empty is degenerate with zero bytes. Committed as src/compression/gzip_dotnet_fixture.txt (7 lines, independently encoded). |
| Source-derived behavior | Any valid GZip member with 1F 8B is accepted; concatenated members decode as one stream; encoder settings affect bytes but not validity. |
| Root cause | No deviation found in decode; encoder bytes intentionally differ by settings (documented valid-member difference, round-trip proven on both sides for the decode direction). |
| Rust change | src/compression/wrappers.rs test dotnet_encoded_members_decode_to_the_same_bytes; fixture src/compression/gzip_dotnet_fixture.txt. |
| Regression evidence | All three .NET vectors plus concat reproduce byte-for-byte through Rust decompress_gzip with magic checks; full suite 1355 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Real wrapped Unity file comparison and recursive dispatch over user content stay open; encoder byte-identity is explicitly not owed. |

## DIFF-LZMA-001 - LZMA unity and size-prefix lanes

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; both rows ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/LzmaCompression.cs (DecompressLzmaStream, DecompressLzmaSizeStream, CompressLzmaStream, CompressLzmaSizeStream); FileStreamBundleFile.cs (LZMA metadata write throws NotImplementedException) |
| Input evidence | Probe Temp/opencode/lzmaprobe (shipped IO.Files DLL, SDK 10.0.401) decoding existing SharpCompress vectors through the reference entry points, generating new unity-gen/size-gen vectors via the reference Compress methods, proving the write refusal and the -1 silent-empty. Committed as src/compression/lzma_reference_fixture.txt (13 lines). |
| Source-derived behavior | Unity lane is five property bytes plus raw stream; size lane adds an eight-byte LE decoded length; short streams are left short by the source loop for downstream rejection while Rust fails closed; long streams truncate to the requested count on both sides; LZMA metadata cannot be written by the baseline. |
| Root cause | No deviation found in decode; short/negative hardening and the missing write unit are documented deliberate dispositions (fail-closed, no success, no silent drop). |
| Rust change | src/compression/lzma.rs test reference_runtime_outputs_match_the_rust_decoder; fixture src/compression/lzma_reference_fixture.txt. |
| Regression evidence | All reference outputs match Rust byte-for-byte; write refusal and negative fail-closed pinned; full suite 1356 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | UnityFS metadata/entry integration over LZMA and real UnityWeb file comparison stay open as noted; encoder byte-identity is not owed. |

## DIFF-UNITYFS-META-001 - UnityFS metadata all lanes

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; row ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/FileStream/FileStreamBundleFile.cs (ReadFileStreamMetadata, ReadMetadata); BlocksInfo.cs; DirectoryInfo.cs; StorageBlock.cs; FileStreamNode.cs |
| Input evidence | Probe Temp/opencode/unityfsmetaprobe (shipped IO.Files DLL, SDK 10.0.401) reading eight C#-constructed full bundles with 1-block/1-node tables (None/LZ4/LZMA, combined/separate, start/end, v6/v7) plus LZHAM and truncated refusals. Committed as src/container/unityfs_meta_fixture.txt (20 lines). |
| Source-derived behavior | Metadata compression None/LZ4/LZMA decode via the same payload path; LZHAM throws UnsupportedBundleDecompression before reading; placement at end seeks to Size minus compressed length with v7 16-byte alignment; separate directories leave nodes empty; truncated metadata throws loudly. |
| Root cause | No deviation found; this increment ports the previously synthetic-only lanes. LZHAM is unsupported-by-the-baseline (proven refusal). Truncated input fails loud on both sides with different messages (no silent success). |
| Rust change | src/container/unityfs.rs test metadata_tables_match_reference_constructed_bundles; fixture src/container/unityfs_meta_fixture.txt. |
| Regression evidence | All six decodable lanes reproduce blocks/nodes/header/offsets/table values; LZHAM and truncated pinned fail-loud; full suite 1357 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Entry extraction over storage blocks (Zstd Huffman/FSE, non-combined variants) stays with IO-BUNDLE-UNITYFS-ENTRY-001; real-file comparison stays open as noted. |

## DIFF-UNITYFS-ENTRY-001 - UnityFS entry all lanes

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; row ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/BundleFileBlockReader.cs (block dispatch with ZstdCompression.IsZstd fallback); FileStreamBundleFile.cs (ReadFileStreamData with AddFailedFile); ZstdCompression.cs; ResourceFile.cs |
| Input evidence | Probe Temp/opencode/unityfsentryprobe (shipped IO.Files DLL, SDK 10.0.401) reading seven C#-constructed bundles with entry payloads across None/LZ4/LZMA/Zstd-Huffman-FSE blocks, two-block reassembly, non-combined empty, and LZHAM refusal. Committed as src/container/unityfs_entry_fixture.txt (18 lines). |
| Source-derived behavior | Entry bytes reassemble across storage blocks with per-block compression; Zstd blocks decode by frame-signature fallback; non-combined directories yield zero entries; LZHAM entries record FailedFile loudly instead of succeeding. |
| Root cause | No deviation found; this increment ports the previously synthetic-only lanes including Zstd Huffman/FSE (previously RFC-built only). LZHAM is unsupported-by-the-baseline with loud proof on both sides. |
| Rust change | src/container/unityfs.rs test entry_bytes_match_reference_constructed_bundles; fixture src/container/unityfs_entry_fixture.txt. |
| Regression evidence | All five decodable entries reproduce byte-for-byte; non-combined empty and LZHAM loud pinned; full suite 1358 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Real-file comparison stays open as noted; the UnityFS header/meta/entry vertical is now fully reference-differentially covered. |

## DIFF-SER-SCALAR-001 - SerializedFile metadata scalar fields

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedFileMetadata.cs (HasSignature, HasPlatform, HasLongFileID, HasUserInformation, stripped-version fallback); SerializedFileHeader.cs (pre-9 metadata-at-end) |
| Input evidence | Probe Temp/opencode/sermetascalarprobe (shipped IO.Files DLL, SDK 10.0.401) reading three C#-constructed files with empty tables across gen8 parseable/unparseable and gen14. Committed as src/serialized_file/metadata_scalar_fixture.txt (10 lines). |
| Source-derived behavior | Version text gates the layout branch with stripped fallback; LongFileID exists only for generations 7-13; user information is a trailing zero-term string; pre-9 metadata sits at the end with DataOffset 0 while gen14 data follows metadata. |
| Root cause | No deviation found except representation: unparseable versions normalize to 0.0.0a0 on the reference while Rust preserves the raw text (both signal stripped, documented). |
| Rust change | src/serialized_file/metadata.rs test scalar_fields_match_reference_constructed_files; fixture src/serialized_file/metadata_scalar_fixture.txt. |
| Regression evidence | All three files reproduce version/platform/LongFileID/user/tables on both sides; bounds for both layouts pinned; full suite 1359 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Type/object/external/script tables plus TypeTree payloads stay with SER-META-TYPES/OBJECTS/EXTERNALS rows; full UnityVersion grammar stays out by design. |

## DIFF-SER-TABLE-001 - SerializedFile object/external/script tables

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/ObjectInfo.cs (IsLongID, HasClassID, HasScriptTypeIndex, HasIsDestroyed, HasSerializedTypeIndex); FileIdentifier.cs; LocalSerializedObjectIdentifier.cs |
| Input evidence | Probe Temp/opencode/sermetatableprobe (shipped IO.Files DLL, SDK 10.0.401) reading three C#-constructed files with one object, one external, and zero/one script records across gen8/12/20. Committed as src/serialized_file/metadata_table_fixture.txt (10 lines). |
| Source-derived behavior | PathID is 32-bit before generation 14 and 64-bit with 4-byte alignment after; pre-17 files carry raw type IDs with ClassID/ScriptTypeIndex/IsDestroyed branches while gen20 carries a type-table index; script identifiers mirror the PathID width; externals carry asset path, GUID, type, and origin. |
| Root cause | No deviation found; this increment ports the previously synthetic-only table reads. Zero types keep type matching out of scope by construction on both sides. |
| Rust change | src/serialized_file/metadata.rs test table_records_match_reference_constructed_files; fixture src/serialized_file/metadata_table_fixture.txt. |
| Regression evidence | All records reproduce FileID/TypeID/ClassID/scripts/externals on both sides; full suite 1360 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Type records with TypeTree payloads stay with SER-META-TYPES-001; identifier-to-object integration and script recovery stay downstream. |

## DIFF-LEGACY-001 - Legacy Raw/Web/WebData containers

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned; one real Rust bug fixed |
| Source location | AssetRipper.IO.Files/BundleFiles/BundleHeader.cs; RawWeb/RawWebBundleHeader.cs (HasHash/HasCompleteFileSize/HasUncompressedBlocksInfoSize, Write throws); RawWeb/RawWebBundleFile.cs (header-size check, last-chunk LZMA, metadata-size check); WebFiles/WebFile.cs; WebFiles/WebFileEntry.cs (Offset/Size plus four-byte LE name) |
| Input evidence | Probe Temp/opencode/legacybundleprobe (shipped IO.Files DLL, SDK 10.0.401) over C#-constructed Raw/Web bundles plus reference-written WebFiles with align on/off and a 200-char name, plus corrupt-header and bad-offset refusals. Committed as src/container/legacy_bundle_fixture.txt (30 lines). |
| Source-derived behavior | Version gates 1-8 select hash/CRC, complete size, and uncompressed size with 4-byte alignment; Raw reads post-header bytes while Web decompresses the last LZMA size-prefixed scene; entry names are four-byte LE (not 7-bit); corrupt sizes and bad ranges fail loud. |
| Root cause | One real deviation found and fixed: Rust used 7-bit name lengths while the reference uses four-byte LE (bytes proven from reference-written files). All other lanes match; malformed inputs fail loud on both sides with different messages (no silent success). Raw/Web Write is unsupported-by-the-baseline (proven refusal). |
| Rust change | src/container/raw_web.rs test headers_and_entries_match_reference_constructed_bundles; src/container/web_file.rs test directory_and_entries_match_reference_written_files plus the 7-bit to four-byte fix across read/write/tests/fixtures; fixture src/container/legacy_bundle_fixture.txt. |
| Regression evidence | All version gates, both entry lanes, both WebFile alignments, and both refusals reproduce; the name-codec fix is regression-covered by D1/D2; full suite 1362 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Real-file comparison stays open as noted; legacy container lanes are now fully reference-differentially covered. |

## DIFF-STREAM-001 - Stream wrappers Smart/MultiFile/Partial/RandomAccess

| Field | Finding |
|---|---|
| Status | Source-runtime wrapper differences reconciled; wrapper regressions pass |
| Source location | AssetRipper.IO.Files/Streams/Smart/SmartStream.cs (null fails loud); MultiFileStream.cs (splitN OpenRead); PartialStream.cs (no construction positioning, Seek adds base offset); RandomAccessStream.cs (internal, bounded file range) |
| Input evidence | Frozen reference probes `tools/probes/io-differential` and `tools/probes/multifile` execute lengths, reads, seeks, positions, EOF, and `ReadByte` over memory, temp, null, split parts, ranges, and file ranges. Fresh 2026-10-08 runs produced six `STREAM` rows and 16 MultiFile path rows matching their committed fixtures exactly. |
| Source-derived behavior | `PartialStream` begins at the wrapped cursor, clamps `Read` by `Length - Position`, delegates `ReadByte` without clamping, and maps seek offsets through its base. `RandomAccessStream` and `MultiFileStream` use `Length - offset` for End seeks. Shared range reads leave the parent cursor unchanged; split reads advance part-by-part; null SmartStream operations fail loudly. |
| Root cause | Rust had normalized three source quirks to conventional behavior: first `PartialStream.Read` was repositioned, `ReadByte` was range-bounded, and RandomAccess/MultiFile End offsets were standard-signed. Compatibility required preserving the source operations. |
| Rust change | `src/io/stream.rs` now retains the source parent cursor for PartialStream reads, exposes the source's delegated `read_byte`, and uses source-sign End seeks for `FileRangeStream` and `MultiFileStream`. Unix file ranges use `read_at`; Windows uses `seek_read` and restores the cursor while the shared handle remains locked. |
| Regression evidence | `stream_observables_match_the_reference_runtime_fixture`, `partial_stream_matches_source_cursor_and_bounded_read_behavior`, `partial_stream_read_byte_matches_source_unbounded_delegation_and_borrowed_lifetime`, `partial_and_random_access_views_keep_their_distinct_end_seek_rules`, and `multi_file_stream_seeks_from_end_with_source_offsets`; all 29 `io::stream::tests` passed. |
| Remaining comparison | No difference remains in the tested wrapper operations. The full LocalFileSystem adapter surface and the bounded contiguous-buffer requirement of downstream container parsers remain open in aggregate `IO-STREAM-001`; Unix retained-reader runtime validation remains under the externally blocked directory-import row. |

## DIFF-SER-TYPE-001 - SerializedFile type and managed-reference records

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; two rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedTypeBase.cs (RefactoredClassId 16, RefactorTypeData 17, HasTypeTreeHashes 13, StoresTypeDependencies 21); SerializedType.cs; SerializedTypeReference.cs (gen21-only name strings); TypeTree.cs (legacy recursive versus modern count-plus-buffer) |
| Input evidence | Probe Temp/opencode/sermetatypeprobe (shipped IO.Files DLL, SDK 10.0.401) reading five C#-constructed files spanning gen8/12/20/21 with one type or one managed ref and zero objects. Committed as src/serialized_file/metadata_type_fixture.txt (15 lines). |
| Source-derived behavior | RawTypeID always; stripped from 16; script index from 17; ScriptID only for -1/114/positive-index scripts with OldTypeHash from 13; TypeTree legacy 1-node versus modern empty; dependencies from 21; managed names only from 21. All reproduce. |
| Root cause | No deviation found; this increment ports the previously synthetic-only type lanes. Hash values pin to construction facts with reference alignment proof. |
| Rust change | src/serialized_file/metadata.rs test type_records_match_reference_constructed_files; fixture src/serialized_file/metadata_type_fixture.txt. |
| Regression evidence | All five records reproduce IDs, flags, indices, hashes, trees, dependencies, and managed names on both sides; full suite 1364 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Type-to-object matching and script recovery stay with their downstream rows; managed field resolution stays downstream. |

## DIFF-COMP-ENC-001 - Independently-encoded Brotli/LZ4/Zstd

| Field | Finding |
|---|---|
| Status | Independent-encoder differential recorded; three rows ported and pinned |
| Source location | AssetRipper.IO.Files/CompressedFiles/Brotli/BrotliFile.cs (BrotliStream decode, Write NotImplemented); BundleFileBlockReader.cs with K4os raw blocks; BundleFiles/ZstdCompression.cs (PartialStream-wrapped DecompressStream, pre-sized output) |
| Input evidence | Probe Temp/opencode/compencodeprobe (SDK 10.0.401) with .NET/K4os/ZstdSharp encoders and reference round-trip oracles. Committed as src/compression/reference_encoded_fixture.txt (25 lines). |
| Source-derived behavior | Brotli text/binary/empty, LZ4 match-heavy and literal blocks, and Zstd entropy-coded frames all decode byte-exact in Rust. |
| Root cause | No deviation found; this increment replaces hand-only evidence with independently encoded vectors (Zstd is the first Huffman/FSE evidence). |
| Rust change | Three per-codec tests in wrappers.rs, lz4_block.rs, zstd.rs; shared fixture src/compression/reference_encoded_fixture.txt. |
| Regression evidence | All seven vectors decode byte-exact on both sides; full suite 1367 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Broad fuzz stays future work; Unity Zstd uses standard framing covered here. |

## DIFF-COMP-WRITE-INTEROP-001 — Rust LZMA, LZ4, and GZip outputs decoded by the source runtime

| Field | Finding |
|---|---|
| Status | Both codec directions are source-runtime verified for the baseline's supported write helpers |
| Source location | `BundleFiles/LzmaCompression.cs`; `CompressedFiles/GZip/GZipFile.cs`; `FileStream/FileStreamBundleFile.cs` and its K4os LZ4 call |
| Input evidence | Rust-authored raw LZMA, size-prefixed LZMA, GZip, and raw LZ4 blocks are frozen in `src/compression/rust_encoder_reference_fixture.txt`. `tools/probes/io-differential/Program.cs` reads those files through the source `LzmaCompression` methods, .NET `GZipStream`, and K4os `LZ4Codec.Decode`. Each source output is byte-identical to the input payload. |
| Source-derived behavior | The baseline offers raw and size-prefixed LZMA compression helpers, GZip file write, and LZ4 block encoding used by the UnityFS metadata writer. Brotli has no writer, Zstandard exposes a decoder only, UnityFS metadata writing rejects LZMA, and non-empty UnityFS entry data writing throws `NotImplementedException`. |
| Rust change | `src/compression/lzma.rs` now exposes bounded raw and size-prefixed LZMA writers. Existing bounded LZ4 and GZip writers are checked against the source decoders. No full UnityFS bundle writer is added: the only product callsite found for `FileStreamBundleFile.Write` creates an empty uncompressed lighting bundle; non-empty entry data is not implemented in the frozen source. |
| Regression evidence | `rust_lzma_encoder_outputs_match_reference_runtime_decoding`, `rust_lz4_encoder_output_decodes_in_k4os_reference_runtime`, and `rust_gzip_encoder_output_decodes_in_the_reference_runtime` compare the product outputs to the frozen fixture and also decode them with the Rust readers. Existing independent source-encoded vectors cover the opposite direction. |
| Remaining comparison | Unity-produced compression outputs and broad fuzz coverage remain unrun. This finding covers codec formats and the source-supported codec write helpers; it does not claim an AssetBundle creation workflow. |

## DIFF-IO-STREAM-INPUT-001 - bounded seekable SchemeReader path input

| Field | Finding |
|---|---|
| Status | Closed for the 45 source-probed file-backed scheme lanes, numbered split input, and configured wrapper/input bounds; no dispatch or mounted-byte deviation found |
| Source location | `AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`, `ReadFile`); `AssetRipper.IO.Files/Streams/MultiFileStream.cs` (`OpenReadMulti`) |
| Input evidence | `tools/probes/io-differential/Program.cs` creates 45 synthetic inputs under the ignored Rust `target` directory and records the frozen `SchemeReader.LoadFile` outcomes in `src/io/io_differential_fixture.txt` and `src/compression/wrapper_scheme_runtime_fixture.txt`. The split-path source observation is recorded in `src/io/multifile_path_fixture.txt`. The standalone SerializedFile row is recorded in src/io/serialized_file_scheme_fixture.txt. No user AssetBundle bytes are involved. |
| Source behavior | `LoadFile` opens a seekable `SmartStream` over a regular file or numbered split parts and passes it through the registered `IScheme` order. `ReadFile` returns the first matching scheme or a `ResourceFile` fallback. |
| Rust behavior | `SchemeReader::read_path_streaming` opens the same logical regular/split input, probes only a bounded prefix, and streams outer GZip/Brotli decoding under per-layer and aggregate limits. It materializes the final decoded container/leaf under `max_input_bytes`, because downstream parsers currently require contiguous bytes. |
| Regression evidence | `scheme_dispatch_matches_the_reference_runtime_fixture` compares file-backed dispatch, recursive inventories, mounts, and failure continuation over the 41 baseline lanes. Separate regressions exercise file-backed Brotli, WebData with a GZip entry, and unknown-code/non-Zstandard storage failure. A standalone SerializedFile regression also checks direct dispatch, parsing, mounting, and byte preservation against src/io/serialized_file_scheme_fixture.txt. `mount_path_assembles_split_inputs_and_uses_the_logical_base_name` replays the source split result. `streaming_dispatch_enforces_wrapper_and_input_limits` pins input size, wrapper depth, per-layer, and aggregate decompression failures. |
| Remaining comparison | No Unity-produced input is claimed. Directory/batch discovery remains under `IO-FS-IMPORT-DIRECTORY-001`, and lower-level stream lifetime/seek semantics remain open under `IO-STREAM-001`. |

## DIFF-SER-ENV-001 - SerializedFile envelopes and file entry

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; two rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedFileMetadata.cs (HasEnableTypeTree 13, SupportsRefObject 20, scripts 11); SerializedFileHeader.cs (HasEndianess 9-plus, LargeFilesSupport 22) |
| Input evidence | Probe Temp/opencode/sermetaenvprobe (shipped IO.Files DLL, SDK 10.0.401) reading eleven minimal envelopes plus one gen10 full file. Committed as src/serialized_file/metadata_envelope_fixture.txt (28 lines). |
| Source-derived behavior | All per-generation presence gates reproduce with empty tables; gen10 entry carries identity plus payload bytes. |
| Root cause | No deviation found; one probe-construction correction made (gen22 needs its endian marker plus 48-byte large header). |
| Rust change | src/serialized_file/metadata.rs test metadata_envelopes_match_reference_constructed_files; fixture src/serialized_file/metadata_envelope_fixture.txt. |
| Regression evidence | All twelve files reproduce envelope plus payload on both sides; full suite 1368 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Object field semantics stay with the object-value rows; container extraction stays with its row. |

## DIFF-SER-OBJF-001 - Object field reading (values and arrays)

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; two rows ported and pinned |
| Source location | AssetRipper.Import/Structure/Assembly/Serializable/SerializableStructure.cs; TypeTrees/SerializableTreeType.cs (ToPrimititeType, IsString, AlignBytes); SerializableValue.cs (ReadUtf8StringAligned, per-field Align) |
| Input evidence | Probe Temp/opencode/objfieldprobe (shipped Import DLL, SDK 10.0.401) reading one VALUES file plus one ARRAYS file. Committed as src/serialized_file/object_field_fixture.txt (9 lines). |
| Source-derived behavior | All scalar, string, nested, and array values reproduce; string nodes need Array children; strings self-align; byte vectors align with AlignBytes plus pad. |
| Root cause | No value deviation found. One latent rule difference noted: Rust aligns some arrays by Unity-version rule where the source uses the per-field AlignBytes flag; realistic files (flag plus pad together, or neither) agree on both sides. |
| Rust change | src/serialized_file/object.rs tests object_scalar_string_nested_fields_match_reference and object_string_nested_empty_arrays_match_reference; fixture src/serialized_file/object_field_fixture.txt. No production change. |
| Regression evidence | Both files reproduce every field on both sides; full suite 1370 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Managed-reference payloads, enum/generated semantics, fixed buffers, and unusual engine layouts stay with their rows; the full version/flag alignment matrix stays open. |

## DIFF-SER-OBJW-001 - Object encode and file rewrite

| Field | Finding |
|---|---|
| Status | Reference-constructed-file evidence recorded; two rows ported and pinned |
| Source location | AssetRipper.Import/Structure/Assembly/Serializable/SerializableValue.cs; AssetRipper.IO.Files/SerializedFiles/Parser/ObjectInfo.cs (gen22 i64 byteStart); SerializedTypeBase.cs (21-plus post-tree deps); SerializedFileMetadata.cs (pre-9 end-minus-size metadata) |
| Input evidence | Probe Temp/opencode/objrewriteprobe (shipped IO.Files plus Import DLLs, SDK 10.0.401) reading four decodeable files R8/R12/R20/R22; F1 payload from DIFF-SER-OBJF-001 reused as the encode oracle. Committed as src/serialized_file/object_rewrite_fixture.txt (12 lines). |
| Source-derived behavior | Encode reproduces reference-authored bytes exactly; rewrite with growth reparses on all four generations with correct sizes, values, alignment, and retention. |
| Root cause | No Rust value deviation found. One layout mismatch corrected on the probe side (gen8 must be header-data-metadata, not metadata-middle). |
| Rust change | src/serialized_file/object.rs test object_encode_reproduces_reference_authored_payload; src/serialized_file/file.rs test rewrites_reference_constructed_files_across_generations; fixture src/serialized_file/object_rewrite_fixture.txt. No production change. |
| Regression evidence | Both new tests pass; full suite 1372 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Managed-reference payloads stay explicit errors; add/remove objects and table changes stay out of scope by design; exact pad bytes are Rust policy with no baseline counterpart. |

## DIFF-SET-SAVEDEL-001 - Settings save/delete product surface

| Field | Finding |
|---|---|
| Status | Source-behavior parity recorded; settings-file row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export/Configuration/SerializedSettings.cs (SaveToDefaultPath, DeleteDefaultPath, TryLoadFromDefaultPath) |
| Input evidence | Source read (no runtime probe needed: semantics are file overwrite and existence-checked delete on the executable-adjacent path); hermetic temp-directory round-trip tests in src/main.rs. |
| Source-derived behavior | Save overwrites unconditionally; delete is a no-op success when absent; load keeps built-in defaults when absent and fails loudly on corrupt content. |
| Root cause | No deviation; the CLI commands are the product equivalent of the GUI save/delete triggers. |
| Rust change | src/main.rs (save_serialized_settings_to_dir, delete_serialized_settings_in_dir, save/delete_default_settings_from_arguments, CLI dispatch, usage, four tests plus classifier pin). |
| Regression evidence | Four new CLI tests pass; full suite 1373 plus 64 pass with warnings-denied Clippy. |
| Remaining comparison | Unknown enum discriminants stay explicit errors per the row disposition. |

## DIFF-ANIM-CACHE-001 - PathChecksumCache add/get/reset

| Field | Finding |
|---|---|
| Status | Reference-method evidence recorded; cache row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Processing/AnimationClips/PathChecksumCache.cs |
| Input evidence | Probe Temp/opencode/animcacheprobe (shipped Processing/Checksum DLLs, SDK 10.0.401) calling the real Add/TryGetPath/Reset via reflection. Committed as src/resources/anim_cache_fixture.txt (24 lines). |
| Source-derived behavior | Seed present; eight Add words, idempotent re-adds, all reverse lookups, miss, and reset-clears-seed plus re-add all reproduce exactly. |
| Root cause | No Rust deviation found. Two probe-side construction facts: parameterless construction leaves field-initializer dictionaries null, and Reset touches processedAssets (both restored via reflection). |
| Rust change | src/resources/path_checksum.rs test cache_reproduces_the_reference_add_get_reset_sequence; fixture src/resources/anim_cache_fixture.txt. No production change. |
| Regression evidence | New test passes (module 7/7); full suite 1374 plus 64 pass with warnings-denied Clippy. |
| Remaining comparison | BuildPathsCache population and Add(IMonoScript) stay caller-side per the row disposition. |

## DIFF-IO-DISCOVERY-001 - Bundle discovery end to end

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; three discovery rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs; AssetRipper/Source/AssetRipper.Assets/Bundles/GameBundle.FromPaths.cs |
| Input evidence | Probe Temp/opencode/bundleprobe2 (shipped Import/Assets/IO DLLs, SDK 10.0.401) running GameStructure.Load over two synthesized UnityFS bundles. Committed as src/io/bundle_discovery_fixture.txt (bundle hexes plus reference dump). |
| Source-derived behavior | Both entry namings (extension and CAB-) discover, parse, and materialize without assemblies; collections are entry-named; unknown ClassIDs yield zero collections; engine collections are injected. |
| Root cause | No Rust deviation on the shared inventory (files, entries, collections, ClassIDs, PathIDs, decode counts). Object models differ by design (generated vs TypeTree). |
| Rust change | src/main.rs test directory_inspection_reproduces_the_reference_bundle_inventory; inspect_directories_to refactor (dead printer removed); fixture src/io/bundle_discovery_fixture.txt. |
| Regression evidence | New test passes; full suite 1374 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Engine injection and Unity-produced files stay out per the row dispositions. |

## DIFF-IO-PLATFORM-001 - Platform detection and registration

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; twelve platform rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Platforms/; AssetRipper/Source/AssetRipper.Import/Structure/Platforms/ |
| Input evidence | Probe Temp/opencode/platformprobe (shipped Import/IO DLLs, SDK 10.0.401) running CheckPlatform plus CollectFiles over 21 constructed trees. Committed as src/io/platform_probe_fixture.txt (specs plus reference dump plus readable version/container bytes). |
| Source-derived behavior | All 21 cases reproduce on kind, name, backend, DataPaths, files, and assemblies, including OBB ordering, dual Mac DataPaths, PS4 null version, and WinPhone shadowing. |
| Root cause | No Rust deviation found. Presence-only decisions plus content reads limited to version files and bundle/SerializedFile header checks (documented per platform). |
| Rust change | src/io/platform_structure.rs test detection_and_registration_reproduce_the_reference_platform_dump; fixture src/io/platform_probe_fixture.txt. No production change. |
| Regression evidence | New test passes (21/21 cases); full suite 1375 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced builds and version-field scope stay out per the row dispositions. |

## DIFF-IO-MIXED-SELECTION-001 - Mixed-input selection

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; three file-selection rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Structure/Platforms/; AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs |
| Input evidence | Probe Temp/opencode/mixedprobe (shipped Import/IO DLLs, SDK 10.0.401) running GameStructure.Load over a mixed directory, an explicit file list, and an explicit split-part selection. Committed as src/io/mixed_probe_fixture.txt. |
| Source-derived behavior | Directory, explicit, and split-part selections reproduce on names and paths, including split folding, short-versus-full key conventions, nested traversal, and the unknown-.resS negative. |
| Root cause | Two Rust deviations found and fixed: directory walk dropped split groups (now folded via scan_folded_kind); explicit selections kept the part path (now the logical base path). |
| Rust change | src/io/game_structure.rs production (split folding plus base-path registration); test mixed_selection_reproduces_the_reference_mixed_dump; fixture src/io/mixed_probe_fixture.txt. |
| Regression evidence | New test passes (3/3 selections); full suite 1376 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced inputs and external-file dependency loading stay out per the row dispositions. |

## DIFF-SER-PPTR-001 - PPtr values, external table, and join

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; two file-reference rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/Parser/; AssetRipper/Source/AssetRipper.Import/Structure/Assembly/Serializable/; AssetRipper/Source/AssetRipper.SerializationLogic/SerializablePointerType.cs |
| Input evidence | Probe Temp/opencode/pptrprobe (shipped Import/IO DLLs, SDK 10.0.401) building two gen12 SerializedFiles with PPtr payloads and one external entry. Committed as src/serialized_file/pptr_probe_fixture.txt. |
| Source-derived behavior | Per-file graphs expose the same edges, the external table matches on GUID words/type/path, and the multi-file join resolves every edge with a complete graph. |
| Root cause | No Rust deviation found. Four source facts (expanded PPtr nodes, CommonString typenames, zero-terminated FileIdentifier strings, gen12 LongFileID) proven mid-batch. |
| Rust change | Test pptr_values_external_table_and_join_reproduce_the_reference_dump; fixture src/serialized_file/pptr_probe_fixture.txt. No production change. |
| Regression evidence | New test passes; full suite 1377 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Meta-type GUID-name matching and Unity-produced inputs stay out per the row dispositions. |

## DIFF-EXP-SETTINGS-VERSION-001 - automatic C# language selection

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export/Configuration/ScriptLanguageVersion.cs; AssetRipper.Primitives dependency rank order |
| Input evidence | Probe Temp/opencode/versionprobe (shipped Export + Primitives DLLs, SDK 10.0.401) resolving both automatic modes over 17 boundary Unity versions. Committed as src/export/settings_version_fixture.txt. |
| Source-derived behavior | Both automatic modes match the reference on every version; the HasCSharp9Support gate lands exactly on 2021.2.0b6 and 2022.1.0a3. |
| Root cause | No Rust deviation found. Release-channel rank order plus the source threshold table ported and pinned mid-batch. |
| Rust change | UnityReleaseVersion/UnityReleaseType plus language_version_facts in src/export/settings.rs; fixture src/export/settings_version_fixture.txt; test automatic_language_selection_reproduces_the_reference_version_dump. |
| Regression evidence | New test passes; full suite 1378 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced inputs stay out per the row disposition. |

## DIFF-IO-VFS-001 - VirtualFileSystem operations and enumeration

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/VirtualFileSystem.cs; AssetRipper/Source/AssetRipper.IO.Files.Tests/VirtualFileSystemTests.cs |
| Input evidence | Probe Temp/opencode/vfsprobe (shipped IO.Files DLL, SDK 10.0.401) replaying 26 scenarios. Committed as src/io/vfs_probe_fixture.txt. |
| Source-derived behavior | Every operation outcome matches; enumeration membership matches on all shapes while raw order differs (insertion vs lexical). |
| Root cause | No Rust deviation beyond the documented order hardening. Insertion order proven by reverse-creation and nested-shape discriminators. |
| Rust change | Test virtual_file_system_reproduces_the_reference_probe_dump; fixture src/io/vfs_probe_fixture.txt. No production change. |
| Regression evidence | New test passes; full suite 1379 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | GetUniqueName counter loop, local adapter (IO-STREAM-001), and Unity-produced inputs stay out per the row disposition. |

## DIFF-ANIM-MUSCLE-001 - humanoid muscle binding table

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/Enums/AnimationClip/HumanoidMuscleType.cs |
| Input evidence | Probe Temp/opencode/muscleprobe (shipped SourceGenerated.Extensions DLL, SDK 10.0.401) dumping all ids plus versioned updates. Committed as src/resources/muscle_probe_fixture.txt. |
| Source-derived behavior | Every attribute string and every versioned update matches; the throw boundary lands exactly at Last=200. |
| Root cause | One real deviation found and fixed: out-of-range None replaced by HumanoidMuscleError (source throws ArgumentException). No silent drop remains on this path. |
| Rust change | HumanoidMuscleError plus Result return in avatar.rs and clip_convert.rs; fixture src/resources/muscle_probe_fixture.txt; test humanoid_muscles_reproduce_the_reference_probe_dump. |
| Regression evidence | New test passes; full suite 1380 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Binding-to-muscle cast, curve emission, and Unity-produced inputs stay out per the row disposition. |

## DIFF-SCRIPT-RESOLVER-001 - resolver node building

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Structure/Assembly/Serializable/TypeTreeResolver.cs; .../TypeTrees/TypeTreeNodeStruct.cs; .../TypeTrees/SerializableTreeType.cs; AssetRipper.SerializationLogic (PrimitiveType, ManagedReferenceTypes, SerializablePointerType) |
| Input evidence | Extended probe Temp/opencode/scriptidprobe (shipped Import/IO.Files DLLs, SDK 10.0.401) with six hand-built OldType node lists. Committed as NODESRC/NODETREE/NTTYPE/NTFIELD rows in src/io/script_identity_fixture.txt. |
| Source-derived behavior | Every built type and field matches in exact order. Malformed level steps fail the build where the source throws (explicit None, no silent tree). |
| Root cause | One real deviation found and fixed: ReferencedManagedType flow mapping (true, per its shared constructor). No silent drop remains on this path. |
| Rust change | build_type_nodes, unwrap_primitive, SerializableTreeType model with shared substitutions, and resolve_serializable_tree in processing/script_identity.rs; test serializable_trees_reproduce_the_reference_node_dump. |
| Regression evidence | New test passes; full suite 1381 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Live assembly loading and Cecil type resolution stay TYPE-SCRIPT-001; MonoBehaviour-structure root mode and Tpk trees stay outside this row. |

## DIFF-CUBEMAP-CONVERT-001 - live-cubemap conversion (flip gate and UnpackNormal)

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Textures/TextureConverter.cs (`TryConvertToBitmap(ITexture2D, ...)`, `UnpackNormal`); AssetRipper/Source/AssetRipper.SourceGenerated/Classes/ClassID_89 (27 versioned `Cubemap_*` classes) |
| Input evidence | Probe Temp/opencode/cubeconvprobe (shipped Export.Modules.Textures + SourceGenerated DLLs, SDK 10.0.401) converting a real class-89 Cubemap (6 DXT5 faces, hand-crafted flip-asymmetric blocks) with and without the `NormalmapDXT5nm` lightmap plus a class-28 Texture2D over the same face bytes. Committed as src/resources/cubemap_convert_fixture.txt (15 rows). |
| Source-derived behavior | Cubemap output stacks six 8x8 faces with no FlipY; the plain texture flips; `UnpackNormal` swaps alpha and red (the `GetChannels` out-parameter order is `(r, g, b, a)`, so the caller locals named `(a, g, _, r)` receive input red in `a` and input alpha in `r`) and recomputes blue with truncating conversion. |
| Root cause | One real deviation found and fixed: `Bitmap::unpack_normal` kept red/alpha in place and only recomputed blue. The old synthetic-only tests encoded the bug (e.g. `(128,128,0,255)` expected `(128,128,254,255)`); the reference produces `(255,128,127,128)`. Two stale expectations corrected alongside the fix. |
| Rust change | Swapped red/alpha lanes in `Bitmap::unpack_normal` (src/resources/texture_converter.rs); fixture src/resources/cubemap_convert_fixture.txt; test live_cubemap_conversion_matches_the_reference_runtime. |
| Regression evidence | All three probed conversions reproduce every output pixel plus dimensions; full suite 1384 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Live-sprite/atlas conversion and Unity-produced textures stay with their own rows; external decoders (Crunch) stay unsupported like the 2D path. |

## DIFF-SPRITE-ATLAS-001 - live-sprite atlas coordinates

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; six rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/SpriteExtensions.cs (`GetSpriteCoordinatesInAtlas`); AssetRipper/Source/AssetRipper.Assets/Generics/AssetDictionary.cs + AssetPair.cs (map mechanics) |
| Input evidence | Probe Temp/opencode/spriteprobe (shipped SourceGenerated.Extensions + SourceGenerated DLLs, SDK 10.0.401) driving real Sprite objects (2022.3, 5.6, 4.5) with `m_` backing fields and a real SpriteAtlas with an empty or reference-hit RenderDataMap entry. Committed as src/resources/sprite_atlas_fixture.txt (3 gate + 30 case rows). |
| Source-derived behavior | RD path, stored and stored-zero pivots, atlas miss with zero-border preservation, atlas hit with border shift, key-less 5.6 sprite, and derived-pivot 4.5 sprite all reproduce bit-for-bit; `Has_Pivot` is false on 4.5 (true on 5.6/2022.3), `Has_RenderDataKey` false before 2022, `Has_Border` true throughout. |
| Root cause | No Rust deviation found; this increment replaces synthetic-only atlas evidence with live-object vectors. Map population bypasses the pair property setters (reference types rejected) via backing fields, like file readers do. |
| Rust change | Test atlas_coordinates_match_the_reference_runtime in src/resources/sprite.rs; fixture src/resources/sprite_atlas_fixture.txt. No production change. |
| Regression evidence | All six probed cases reproduce rect, pivot, and border; full suite 1385 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | `RD.Texture` pointer resolution and exporter wiring stay with RES-SPRITE-CONVERT-001; Unity-produced sprites stay out per the row disposition. |

## DIFF-SPRITE-CONVERT-001 - live-sprite conversion (pointer resolution and delegation)

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Textures/SpriteConverter.cs (`Supported`, `TryConvertToBitmap`); AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/PPtrExtensions.cs (FileID 0 = same collection) |
| Input evidence | Probe Temp/opencode/spriteprobe (shipped Export.Modules.Textures + Assets DLLs, SDK 10.0.401) with a real 4x4 RGBA32 texture registered via protected `AddAsset` plus real sprites whose `RD.Texture` m_ fields link (FileID 0, PathID 44), null (PathID 0), or dangle (PathID 999). Committed as src/resources/sprite_convert_fixture.txt (13 rows). |
| Source-derived behavior | Linked sprite reports Supported with a flipped 4x4 bitmap; nulled and dangling sprites report false with empty output. FileID 0 resolves inside the sprite's own collection; only the registered PathID is present. |
| Root cause | No Rust deviation found; this increment proves cross-object pointer resolution plus gated delegation end-to-end. |
| Rust change | Test sprite_conversion_matches_the_reference_runtime in src/resources/sprite.rs; fixture src/resources/sprite_convert_fixture.txt. No production change. |
| Regression evidence | All three probed conversions reproduce both decisions plus every pixel; full suite 1386 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Exporter wiring stays with the exporter rows; Unity-produced sprites stay out per the row disposition. |

## DIFF-TEXTURE-ARRAY-CONVERT-001 - array/3D conversion pixels and the 117 unpack gate

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; five rows ported and pinned; one real bug fixed |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Textures/TextureConverter.cs (ITexture2DArray lines 86-113, ITexture3D lines 51-84 with the FlipY plus stored-lightmap UnpackNormal, ICubemapArray lines 115-142 with the FlipY kept despite `Maybe not needed?`); AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/Texture3DExtensions.cs (`GetLightmapFormat` lines 88-98, usage-backed) |
| Input evidence | Probe Temp/opencode/arrayconvprobe (shipped Export.Modules.Textures + Assets DLLs, SDK 10.0.401) driving real class-187 (2x2 depth 2), class-117 (2x2x2 plain and UsageMode-5), and class-188 (width 2 count 1) assets with full inline R8G8B8A8_UNorm bytes. Committed as src/resources/texture_array_convert_fixture.txt (15 rows). |
| Source-derived behavior | All three overloads FlipY per layer (array rows land [r1,r0,r3,r2], cubemap faces flip pairwise); empty buffers refuse; UsageMode 5 with stored LightmapFormat 0 does NOT unpack because the converter reads the stored `LightmapFormatE`, not `GetLightmapFormat`. |
| Root cause | One real Rust deviation found and fixed: `export_texture3d_image` unpacked on the derived (usage-backed) lightmap value, so a normal-mapped usage with a default stored field unpacked where the reference does not. The gate now reads the stored field only. An early probe scare (ArgumentOutOfRange on every conversion) traced to short probe buffers (one pixel per stacked row); full buffers convert. |
| Rust change | `resources::texture::export_texture3d_image` unpack gate plus doc comment; test `array_conversions_match_the_reference_runtime` in `resources::texture`; fixture `src/resources/texture_array_convert_fixture.txt`. No other production change (`flip_y` already flips per layer). |
| Regression evidence | All five probed conversions reproduce ok/size/pixels through the export functions with PNG wiring asserted; the `t3-normal` row fails under the old derived gate. Full suite 1387 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Crunch stays unsupported on these paths like the 2D one; streamed fetching stays caller-side; Unity-produced array textures stay out per the row disposition. |

## DIFF-TEXTURE-EXPORTER-001 - exporter admission and format selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; fourteen rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/TextureAssetExporter.cs (`TryCreateCollection` lines 26-38, `Export` lines 40-60); AssetRipper/Source/AssetRipper.Export/Configuration/ImageExportFormat.cs (`GetTextureExportFormat` lines 93-100, `TryGetFromExtension` lines 61-75); AssetRipper/Source/AssetRipper.Processing/AssetGroup.cs (`SetMainAsset`); AssetRipper/Source/AssetRipper.Assets/IUnityObjectBase.cs (`GetBestExtension`, Override ?? Original) |
| Input evidence | Probe Temp/opencode/texexpprobe (shipped UnityProjects/Processing/Export/Modules DLLs, SDK 10.0.401) with a real texture plus sprite grouped in a SpriteInformationObject (both MainAssets verified), a lone texture, and a GameObject, driving shipped `TryCreateCollection` per sprite mode, `CheckAssetIntegrity`, `TryConvertToBitmap`, and the full selection matrix. Committed as src/export/texture_exporter_fixture.txt (18 rows). |
| Source-derived behavior | Grouped texture admits under all three modes (non-sprite bypasses the Yaml gate); grouped sprite admits everywhere except Yaml; lone texture and GameObject decline; integrity and conversion pass on the live texture; `jpg` selects Jpeg, `??` lets an unrecognized override shadow a recognized original, uppercase `JPEG` falls back to configured. |
| Root cause | No Rust deviation found; this increment proves exporter branch selection plus format selection end-to-end through real objects. |
| Rust change | Test texture_exporter_matches_the_reference_runtime in export/export_extension.rs; fixture src/export/texture_exporter_fixture.txt. No production change. |
| Regression evidence | All 8 admissions and all 6 selections reproduce through the decision kernels with the probed integrity/conversion facts; export suite 18/18. |
| Remaining comparison | Bitmap encode and file IO stay caller-side; Unity-produced projects stay out per the row disposition. |

## DIFF-SHADER-STATE-001 - render-state predicates and pass selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; 43 behavior rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/SerializedShaderFloatValueExtensions.cs, SerializedShaderVectorValueExtensions.cs, SerializedShaderStateExtensions.cs, SerializedStencilOpExtensions.cs, SerializedShaderRTBlendStateExtensions.cs, SerializedPassExtensions.cs; Enums/Shader/ShaderType.cs, Enums/Shader/SerializedShader/StencilOp.cs, StencilComp.cs |
| Input evidence | Probe Temp/opencode/shstateprobe (shipped SourceGenerated.Extensions + SourceGenerated DLLs, SDK 10.0.401) with real SerializedShaderFloatValue_5_5, SerializedShaderVectorValue_5_5, SerializedShaderState_2017_2, SerializedStencilOp_5_5, SerializedPass_2017, and SerializedPass_2019_3_0_a7 instances plus `m_` backing-field writes, calling the shipped predicates. Committed as src/resources/shader_state_fixture.txt (49 rows). |
| Source-derived behavior | Exact float equality (-0.0 is zero, denormals are not), 255-max, `<noninit>`/empty namelessness, postfix-free search order, Keep/Keep/Keep/Always stencil default, alpha-nonzero/conservative-present-nonzero/On-Off words, the six-way stencil conjunction, mask-bit program membership with source order, the 2019 ray-tracing gate, live ShaderType/SerializedPassType tables, and C# `(int)`-cast agreement with Rust on NaN/out-of-range magnitudes. |
| Root cause | No Rust deviation found; this increment replaces source-derived expectations with live-object vectors and upgrades the `IsDefault` truncation scope note from assumed to probe-backed. A whole-tree caller audit over the frozen snapshot finds zero external call sites for any of these members, so the kernels pin decision shapes with no observable product behavior behind them. |
| Rust change | Test render_state_predicates_match_the_reference_runtime in resources/shader.rs; fixture src/resources/shader_state_fixture.txt; `IsDefault` doc now cites probe evidence. No production change. |
| Regression evidence | All 43 behavior rows replay through the kernels; shader suite 13/13. |
| Remaining comparison | Name backing selection, generic enum-cast machinery, name-or-value emission strings, remaining typed casts, and MaxShaderModelVersion stay caller-side; Unity-produced shaders stay out per the row disposition. |

## DIFF-SPRITE-METADATA-001 - sprite sheet entry fill

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three fill cases ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/SpriteMetaDataExtensions.cs (`FillSpriteMetaData` lines 26-71); AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/TextureExportCollection.cs (`AddSpriteSheet` lines 140-165, `SpriteMetaData.Create` per version) |
| Input evidence | Probe Temp/opencode/smetaprobe (shipped SourceGenerated DLL, SDK 10.0.401) with real 2022.3 sprites (plain, boned, atlas-hit, atlas-miss) plus `SpriteMetaData.Create` instances through the shipped fill. Committed as src/resources/sprite_metadata_fixture.txt (19 rows). |
| Source-derived behavior | Verbatim name carry, atlas-hit rect/pivot/border (with the border shift), miss-path RD rect/pivot/border, `Custom` alignment, zero tessellation, gate decisions, PPU bone scaling with the sprite-Rect (not atlas) root offset, and SpriteID assignment on version gates alone (even with an empty bone list). The vertex-less reference leaves outline/physics/vertices/indices/weights empty. |
| Root cause | No Rust deviation found; this increment replaces synthetic-only gate coverage with live-object vectors over the exact exporter-called factory and entry point. |
| Rust change | Test sprite_metadata_fill_matches_the_reference_runtime in resources/sprite.rs; fixture src/resources/sprite_metadata_fixture.txt. No production change. |
| Regression evidence | All three fill cases replay field-for-field through the kernel; sprite suite 19/19. |
| Remaining comparison | Mesh-vertex extraction bodies (`SetBoneGeometry`, `VertexDataBlob` reads, `GenerateOutline`/`GeneratePhysicsShape`, `MeshOutlineGenerator`) stay open by baseline size; real skinned-sprite vertex payloads stay out per the row disposition. |

## DIFF-MATERIAL-VIEW-001 - material property surface

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; five material cases ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/MaterialExtensions.cs (`FindPropertyNameByCRC28`, `TryGetTextureProperty`, `GetTextureProperties`); UnityPropertySheetExtensions.cs; AssetRipper/Source/AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs (line 107 live caller); AssetRipper/Source/AssetRipper.Export.Modules.Models/GlbLevelBuilder.cs (line 244 live caller) |
| Input evidence | Probe Temp/opencode/matprobe (shipped SourceGenerated/Checksum DLLs, SDK 10.0.401) with real `Material.Create` sheets on 3.5, 5.6, 2019.4, and 2022.3 (empty and populated) through the shipped property surface with shipped CRC28 words. Committed as src/resources/material_view_fixture.txt (62 rows). |
| Source-derived behavior | Exactly one tex-env/float/color gate per version (`Has_Ints` off on 3.5 and 5.6, on from 2019.4); serialized texture order; hit/miss lookups; CRC searches (postfix hits, plain float hit, misses); empty-but-gated sheets return null without throwing on all four versions. |
| Root cause | No Rust deviation found; this increment replaces source-derived expectations with live-object vectors. The `NotSupportedException` arm is bounded: every shipped version matches exactly one tex-env gate and the unversioned base sheet is abstract, so it fires only for file-reader-produced unknown layouts. |
| Rust change | Test material_view_matches_the_reference_runtime in resources/material.rs; fixture src/resources/material_view_fixture.txt. No production change. |
| Regression evidence | All five material cases replay through the view; material suite 26/26. |
| Remaining comparison | FastPropertyName shapes stay synthetic (cross-version dict writes are invisible to the version-gated extension); Material `.mat` YAML export and the shader exporters remain open. |

## DIFF-ANIM-HIERARCHY-001 - animator editor-hierarchy walk

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; 27 yields plus slot order ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/AnimatorControllerExtensions.cs (`GetStateBehaviours` lines 44-72, `FetchEditorHierarchy` lines 74-299); live callers AssetRipper/Source/AssetRipper.Processing/AnimatorControllers/AnimatorControllerProcessor.cs and AssetRipper/Source/AssetRipper.Export.UnityProjects/AnimatorControllers/AnimatorControllerExportCollection.cs |
| Input evidence | Probe Temp/opencode/animhierprobe (shipped SourceGenerated DLL, SDK 10.0.401) with a real versioned controller graph (2022.3 layers/pairs/machines plus 5.0 and 4.0 machines/states, FileID-0 PPtrs) through the shipped walk and behaviour lookup. Committed as src/resources/animator_hierarchy_fixture.txt (194 rows). |
| Source-derived behavior | Exact 27-yield claim order: layer behaviour/motion pairs, nested blend-tree recursion, the modern machine order, and the 4.0 legacy else-branch order (child machines, local, ordered, states); animation clips never yield; no deduplication on double-claimed states; slot order follows the index array ([1,0]); missing keys yield empty; resolved behaviours get `HideInHierarchy` (0->1). |
| Root cause | No Rust deviation found; this increment adds the fetch-order kernels and replaces synthetic-only slot/gate coverage with live-object vectors. Probe mechanics of note: same-version PPtrs resolve through the composite `PathID` while the _4 family needs the `m_PathID_` int field (the split slot silently mis-resolves); PPtrAccessList views reject AddNew over null backings, so file-reader-style backing fills are required; the 5.0 machine is modern-layout, only 4.0 takes the legacy branch. |
| Rust change | Fetch-order kernels (`fetch_editor_hierarchy` plus `FetchController/Layer/Pair/StateMachine/State/BlendTree` facts) and test editor_hierarchy_matches_the_reference_runtime in resources/animator_controller.rs; fixture src/resources/animator_hierarchy_fixture.txt. |
| Regression evidence | All 27 yields plus the live slot vector replay through the kernels; animator suite 6/6. |
| Remaining comparison | Pointer resolution and the flag mutation stay caller-side per the row disposition; transition-destination context rebuilds remain with their own coverage. |

## DIFF-GLB-MATERIAL-001 - GLB main/normal texture selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; five selection cases ported and pinned, one production gap closed |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Models/GlbLevelBuilder.cs (`GetTextures` lines 239-261, `IsMainTexture`, `IsNormalTexture`, private static on the nested `BuildParameters`); `MakeMaterialBuilder` lines 224-237 |
| Input evidence | Probe Temp/opencode/glbmatprobe (shipped Export.Modules.Models DLL, SDK 10.0.401) with real 2022.3 materials plus registered RGBA32 textures through the shipped selection. Committed as src/resources/glb_material_fixture.txt (25 rows). |
| Source-derived behavior | First main/normal spelling wins; any other property is the replacement that fills an absent main; the `??=` applies after resolution, so a null-texture main entry keeps looking (null-skips to `_Texture`). |
| Root cause | Real deviation found and fixed: the name-only kernel blocked later main spellings behind a null-texture first pick, while the reference skips unresolvable picks. Fixed with the resolution-aware `glb_material_textures_resolved` taking a caller predicate; the name-first kernel is unchanged. |
| Rust change | New `glb_material_textures_resolved` in resources/material.rs plus test glb_material_textures_match_the_reference_runtime; fixture src/resources/glb_material_fixture.txt. |
| Regression evidence | All five selection cases replay at resolved-path level; material suite 27/27; existing name-first tests unchanged. |
| Remaining comparison | Image decoding and SharpGLTF material building remain open. |

## DIFF-ANIM-CUSTOM-CURVE-001 — custom-curve resolver behavior and ReverseAscii salt

| Field | Finding |
|---|---|
| Status | Resolver kernels match the captured reference-runtime vectors; production animation integration remains open |
| Source location | `AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs` (`ToAttributeName`); `AssetRipper.Checksum` `Crc32Algorithm.ReverseAscii`; generated component `TryGetPath` dictionaries |
| Input evidence | Isolated `Temp/opencode/curveresolve` probe using the shipped .NET 10.0.401 reference assemblies and generated 2022.3 objects for an Animation, hierarchy, skinned mesh, and renderer/material. `Temp/opencode/ildump` decoded the shipped checksum method. Captured values are committed in `src/resources/custom_curve_resolve_fixture.txt`; the 11 path tables are committed in `src/resources/custom_curve_tables.rs`. These are live managed-object probes and source-derived table dumps, not Unity-produced files. |
| Source behavior | The fixture pins 39 actual `ToAttributeName` cases, including table hits/misses, scene-dependent blend-shape/material outcomes, manufactured fallbacks, and exact unsupported-enum exceptions. `ReverseAscii` passes `HashAscii(prefix, "HHHHHHH")`, so the seven-H marker is appended to the prefix before the CRC is computed. The 11 component dictionaries contain 2,551 entries. |
| Root cause | The initial Rust port hashed only the prefix before reversing the CRC, omitting the source's seven-H marker. The first fixture expectations also had incorrect CRC words and did not reproduce the resolver's real scene predicate. The live probe exposed these mismatches; Rust now streams the marker into the same CRC state and the fixture records the correct predicates and values. |
| Rust change | `src/resources/blend_shape.rs` now exposes a crate-private incremental IEEE CRC update; `src/resources/custom_curve.rs` ports the `ReverseAscii` transform, resolver dispatch, table hits, and source-compatible errors for unsupported binding enum values. |
| Regression evidence | `cargo test --workspace --locked custom_curve::tests` passes 12/12. The fixture includes 14 `ReverseAscii` outputs and 2 direct `HashAscii` marker hashes. The full Rust workspace passes 1,397 library tests and 65 CLI tests; formatting, warnings-denied Clippy, and the 1,449-file baseline verifier pass. |
| Remaining comparison | `AnimationClipConverter` does not call the Rust `to_attribute_name`; production code still lacks root/child traversal and asset-backed blend-shape/material CRC lookup to populate `CustomCurveSceneFacts`. Full clip conversion, independent Unity-file evidence, and dynamic Unity animation sampling remain unrun. Keep `PROC-ANIM-CUSTOM-CURVE-001` open. |

## DIFF-MANAGED-REFS-002 - nested managed-reference registries

| Field | Finding |
|---|---|
| Status | Closed for the source reader paths exercised by independent modern and legacy nested-registry fixtures; no Unity-produced or dynamic Unity result is claimed |
| Source location | `AssetRipper.Import/Structure/Assembly/Serializable/ManagedReferenceRegistryReader.cs` (`Read`, `ReadManagedType`, `ReadReferencedObject`, `ReadReferencedObjectData`); `SerializableStructure.cs` (`Read`); `TypeTreeResolver.cs`; `TypeTrees/SerializableTreeType.cs` (`FromReferencedObjectNode`, `AddNode`); `AssetRipper.SerializationLogic/ManagedReferenceTypes.cs` |
| Source evidence | The frozen source's `FromReferencedObjectNode` excludes only a direct root Registry marker. `AddNode` retains and constructs a Registry node below a nested structure, and `SerializableStructure.Read` dispatches that field through `ManagedReferenceRegistryReader.Read`. The checked-in probe builds those TypeTrees and runs the shipped reader on the same fixture bytes used by Rust. No UABEA or user AssetBundle data was used. |
| Rust change | `decode_typetree_payload` now recursively decodes nested registries with inherited object-stream offsets, allocation limits, and a configurable recursion bound. The writer uses the same recursive TypeTree path. The reference graph assigns stable preorder scopes to structs owning a Registry field; entries and ID edges resolve by `(scope, id)`, so duplicate IDs in different scopes remain distinct. Scope count, object count, edge count, and path bounds produce explicit errors. |
| Regression evidence | `tools/probes/managed-nested/Program.cs` consumes counted modern `rid` and terminated legacy indexed `id` fixtures to exactly 152/152 and 200/200 bytes. Rust's `nested_registry_decodes_writes_and_resolves_equal_ids_in_distinct_scopes` decodes and re-encodes both fixtures byte-for-byte, resolves equal IDs to the proper local entries, and checks depth/scope bounds. Existing root tests retain shared/cyclic IDs, nested PPtrs, YAML, and framing errors. |
| Remaining difference | No discrepancy remains in the source paths exercised. The source reader does not publish a separate managed-ID resolution graph; Rust's graph scope mapping follows the nested `SerializableStructure` ownership and is checked with equal IDs on both sides of the scope boundary. Unity-produced fixture and dynamic Unity validation remain unrun. `SER-MANAGED-REFS-001` is verified; the Phase 1 gate remains open. |

## DIFF-PREFAB-SCENE-BUNDLE-001 - scene hierarchy bundle-name assignment is ignored

| Field | Finding |
|---|---|
| Status | Closed for candidate selection and effective property behavior in the isolated AssetRipper runtime; no Unity-produced or Editor import/runtime result is claimed |
| Source location | `AssetRipper/Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs` (`Process` scene pass, lines 28-48); `AssetRipper/Source/AssetRipper.Processing/GeneratedObjectBase.cs` (`AssetBundleName` no-op setter); `Bundle.FetchAssets`; `AssetBundleExtensions.GetAssetBundleName`; `SerializedBundle.Name` |
| Source behavior | In `GameBundle.Scenes` order, the first scene collection whose owner is a `SerializedBundle` supplies the candidate. The first `IAssetBundle` in `FetchAssets()` order contributes `GetAssetBundleName()` (nonempty serialized `AssetBundleName_R`, else owning bundle name); absent an asset, the candidate is `bundle.Name`; absent a `SerializedBundle`, there is no candidate. `PrefabProcessor` assigns that candidate to `SceneHierarchyObject.AssetBundleName`, but the generated hierarchy inherits `GeneratedObjectBase.AssetBundleName`, whose setter does nothing and whose getter returns null. Thus the effective property is always null. The streamed-scene `Debug.Assert` has no release behavior. |
| Rust change | The bundle planner now keeps the selected candidate distinct from `effective_asset_bundle_name`, which remains `None`. Materialized scene hierarchies retain the candidate for diagnostics but expose an unset effective property; `export-project` writes blank DefaultImporter bundle-name fields, and `inspect` reports both values instead of presenting the candidate as the stored property. |
| Regression evidence | `tools/probes/scene-processor/Program.cs` places full 2019.4 class-ID-141 and class-ID-142 fixtures inside a `SerializedBundle` and runs the shipped processors. The runtime reports `BUNDLE-COMPUTED|streamed-scene-bundle` and `HIER-SET|<null>|...`; the C# source base confirms the no-op setter. Rust source-output replay checks `bundle=null`, and the project export test verifies blank `assetBundleName`/`assetBundleVariant` with a `DefaultImporter`. `tools/probes/scene-importer` records `DefaultImporter.Create` default fields in `src/export/default_importer_probe_fixture.txt`. Focused tests pass: 21 PrefabProcessor tests, the streamed-scene export integration, and four scene-summary CLI tests. |
| Remaining difference | Generated importer defaults and Rust sidecar fields are covered, but a complete source `SceneExportCollection` write and Unity Editor import remain unrun. `Has_IsStreamedSceneAssetBundle` version gates remain caller facts because the frozen snapshot does not expose their generated thresholds. Phase 1 remains open. |

## DIFF-SINGLEFILE-IMPORT-001 - single-path import against reference-written containers

| Field | Finding |
|---|---|
| Status | Closed for the one-path mount contract exercised with reference-written WebData, split paths, truncation, and gap inputs; no Unity-produced or dynamic Unity result is claimed |
| Source location | `AssetRipper/Source/AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`); `AssetRipper/Source/AssetRipper.IO.Files/WebFiles/WebFile.cs` (`Write`, `Read`); `AssetRipper/Source/AssetRipper.IO.Files/Streams/MultiFileStream.cs`; `AssetRipper/Source/AssetRipper.IO.Files/FileContainer.cs` |
| Source behavior | `WebFile.Write` with entry alignment on and off produces distinct bytes that both read back with identical entries (`alpha.bin` 13 bytes, `beta.bin` 5 bytes). An 8-byte truncation does not throw: dispatch falls through signature recognition and the input loads as a raw `ResourceFile` leaf. Split base and split-part paths resolve to the same entries. A missing middle part (`split0` + `split2` without `split1`) is rejected with an `Exception`. |
| Rust change | No production change was needed: `SchemeReader::mount_path` already implements this contract. The new test pins it against the reference bytes. |
| Regression evidence | Isolated probe `tools/probes/singlefile/Program.cs` (frozen `AssetRipper.IO.Files`, SDK 10.0.401) wrote both containers and recorded every outcome in `src/scheme/singlefile_probe_fixture.txt`. Rust test `mount_path_matches_reference_written_webdata_and_fails_loud_on_gap` replays all rows through production `mount_path`: both reference variants mount 2 entries with byte-identical payloads, base and part paths agree, the gap fails naming the missing part with no partial mount left behind, and the truncation mounts verbatim as a raw leaf matching the source fallback. Full workspace: 1416 library and 68 CLI tests; warnings-denied Clippy, format check, and `git diff --check` passed; baseline verifier still exits 1 on the 188 pre-existing unlisted build outputs with all 1,449 listed files unchanged. |
| Remaining difference | No discrepancy remains in the source paths exercised. Mounted VFS entries feed the same workspace ingestion and dependency-report pipeline as directory imports; missing/ambiguous/resource dependency outcomes stay explicit in that shared layer. Unity-produced single-file inputs and dynamic Unity import checks remain unrun. `IO-FS-IMPORT-PATH-001` is verified; the Phase 1 gate remains open with 35 non-verified rows. |

## DIFF-BUILDSETTINGS-REAL-LAYOUT-001 - BuildSettings real TPK layouts and reference materialization

| Field | Finding |
|---|---|
| Status | Closed for the class-141 scene-path layouts exercised across the dumped versions; the struct-entry branch is newly bounded as defensive-only. |
| Source location | `AssetRipper/Source/AssetRipper.Import/Structure/Assembly/TypeTrees/TypeTreeNodeStruct.cs` (`TryMakeFromTpk`); `AssetRipper/Source/AssetRipper.AssemblyDumper/Passes/Pass002_RenameSubnodes.cs` (line 473: `m_Levels` to `m_Scenes` earlier than 5.2); `AssetRipper/Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs` (fixed-layout `ReadNormalObject`); `AssetRipper/Source/AssetRipper.Tools.DependenceGrapher/Program.cs` (bundle loading pattern) |
| Source behavior | The real class-141 release trees use plain string vectors for scenes on every dumped version (4.7.2f1 `levels`, 5.0.0f1 `levels`, 5.2.0f1 through 6000.0.23f1 `scenes`): `vector` to `Array` to `int size` plus `string data` to `Array` to `int size` plus `char data`. No struct-with-`path` entry shape exists in any dumped layout. The rename rule proves real serialized trees carry `m_`-prefixed names. The last `bool` before the strings carries `AlignBytes` (0x4000) so the version string starts on a four-byte boundary. `GameAssetFactory` reads the fixed generated layout and yields `UnreadableObject` unless the payload matches byte-exactly. |
| Rust change | No production change was needed: `decode_build_settings_scene_paths` already handles both `m_Scenes` and `m_Levels` string vectors, and the object decoder already honors the `AlignBytes` flag (0x4000). The full-layout test fixtures now mirror the dumped TPK order, shapes, and bool-run alignment. The struct-entry branch stays as synthetic-tested defensive code with no version provenance in class 141 (likely confusion with `EditorBuildSettings` scenes, a different settings object). |
| Regression evidence | Isolated probe `tools/probes/buildsettings/Program.cs` (frozen runtime, SDK 10.0.401) dumped the real TPK node lists for 4.7.2f1, 5.0.0f1, 5.2.0f1, 2019.4.40f1, 2021.3.0f1, 2022.3.0f1, and 6000.0.23f1 (recorded in `src/processing/buildsettings_tpk_fixture.txt`). Rust-built full-layout payloads for 2019.4.40f1 (`m_Scenes`) and 5.0.0f1 (`m_Levels`) were parsed by the reference and materialized as REAL generated objects (`BuildSettings_2018_3_0_b4`, `BuildSettings_5`) with agreeing scene paths, version strings, GUID words, and flags (recorded in `src/processing/buildsettings_probe_fixture.txt`). Rust test `full_layout_build_settings_match_the_reference_materialization` replays every row through production `decode_build_settings_scene_paths` plus field reads. Full workspace: 1419 library and 69 CLI tests; warnings-denied Clippy, format check, and `git diff --check` passed; baseline verifier still exits 1 on the 188 pre-existing unlisted build outputs with all 1,449 listed files unchanged. |
| Remaining difference | No discrepancy remains in the source paths exercised. A Unity-produced `globalgamemanagers` file and dynamic Unity import checks remain unrun. `PROC-SCENE-BUILDSETTINGS-001` is verified; the Phase 1 gate remains open with 32 non-verified rows.

## DIFF-EXPORT-ROOT-GUARD-001 - Export root guard, deletion consent, and the consent-declined default

| Field | Finding |
|---|---|
| Status | Closed for the guard contract exercised by the reference-runtime probe, with one intentional and documented deviation in how a refused export is surfaced. |
| Source location | `AssetRipper/Source/AssetRipper.GUI.Web/GameFileLoader.cs` (`ExportUnityProject` 59-76, `ExportPrimaryContent` 78-99, `IsValidExportDirectory` 108-122, `IsNonEmptyDirectory` 124-127, `UserConsentsToDeletion` 129-142) |
| Source behavior | Both export entry points run the same guard. An empty path, or a path whose `Path.GetFileName` is exactly `Desktop`, `Documents`, or `Downloads`, is invalid and the method returns without exporting. When loaded and valid: if `Directory.Exists(path)` and `EnumerateFileSystemEntries(path).Any()` then consent is requested; on refusal the source logs `User declined to delete existing export directory. Aborting export.` and again returns without exporting. Otherwise it runs `Directory.Delete(path, true)`, `Directory.CreateDirectory(path)`, then exports. `UserConsentsToDeletion` returns `true` whenever `Headless` is set, and otherwise a dialog result that defaults to `false`. The reflection probe recorded 22 directory-gate rows, 9 non-empty-directory rows (2 symlink rows emitted `skip`), 48 `Path.GetFileName` rows, and 1 headless-consent row (`true`). |
| Rust change | New `src/export/export_root.rs` ports `source_file_name`, the protected-name gate, `is_non_empty_directory`, and `prepare_export_root(path, delete_existing)` (symlink-aware delete of a non-empty root, then create). `export_workspace_yaml_project` now calls it as its first statement, replacing the older `ensure_fresh_output_root`, so the guard runs before any project write and an existing non-empty root can never be written over. `export-project` accepts `--delete-existing` before or after the inputs and never consumes it as the output root; the flag selects the source's delete-and-recreate arm. Without the flag the product keeps the source's consent-declined arm, so a default export stays non-destructive. |
| Regression evidence | Replay test `reference_export_root_guard_rows_match_the_rust_port` in `src/export/export_root.rs` substitutes the probe work root into `src/export/project_guard_fixture.txt` and asserts every row through production code, pinning Windows counts `[22, 7, 48, 1]` (the 2 symlink rows are recorded `skip`) and the consented delete; `validate_export_directory_rejects_empty_and_system_directory_names`, `is_non_empty_directory_requires_a_readable_directory_with_entries`, and 4 `prepare_export_root_*` tests cover the arms directly. CLI: `export_project_refuses_a_nonempty_output_directory_without_touching_it`, `export_project_replaces_a_nonempty_output_directory_when_delete_existing_leads`, `export_project_accepts_delete_existing_after_the_inputs`, `export_project_treats_delete_existing_as_a_flag_not_as_the_output_directory`, and the end-to-end `export_project_writes_a_cross_file_yaml_workspace` are green. Full workspace: 1432 library and 72 CLI tests passed; warnings-denied Clippy on Windows and on `--target x86_64-unknown-linux-gnu`, format check, and `git diff --check` passed; `tools/verify-baseline.ps1` exits 0 (1,449 files, no missing, changed, or unlisted). |
| Remaining difference | The source silently returns (no export, no error) on an invalid path or refused consent; the Rust port returns a typed `ExportRootError` and the CLI exits non-zero with a message containing the source's refusal sentence. That surfacing difference is intentional: a skipped export must not be reported as a successful one. The probe emitted `skip` for the two symlink rows, so the symlink-aware delete is implemented and reviewed but not demonstrated at runtime. The headless auto-consent arm is deliberately not the product default. |

## DIFF-EXPORT-DIR-GATE-UNIX-001 - Export directory gate and `Path.GetFileName` extraction on Unix

| Field | Finding |
|---|---|
| Status | Closed for the Windows-runtime rows and for the divergence found in the verified product path; the Unix arm is source-derived only. |
| Source location | `AssetRipper/Source/AssetRipper.GUI.Web/GameFileLoader.cs` (`IsValidExportDirectory`); authoritative .NET sources under `target/ref-src/`: `Path.cs` (`GetFileName(ReadOnlySpan<char>)`, 236-248), `Path.Windows.cs`, `Path.Unix.cs`, `PathInternal.Windows.cs` (`GetRootLength`, device/UNC/extended prefixes, drive-letter check) |
| Source behavior | `Path.GetFileName` starts at `max(root length, last separator index + 1)`. Windows root rules produce `""` for `C:`, `C:\`, `\\server\share`, `//root`; `file` for `\\?\C:\x\y`, `\\?\UNC\server\share\file`, and `\\\\.\C:\x`; `a` for `C:a`; `b:c` for `a\b:c`. Unix treats only `/` as a separator with a single-slash root, so `//root` yields `root` (Windows `""`), `C:` yields `C:` (Windows `""`), and `C:\Users\me\Desktop` yields `Desktop` rather than `""`. The gate itself only compares that basename against `Desktop`/`Documents`/`Downloads`, so the platform difference is observable on Unix for Windows-style absolute paths. |
| Rust change | `export_root::source_file_name` implements the shared rule over raw platform units (`cfg`-split `encode_wide`/`from_wide` and `as_bytes`/`from_vec`) with a `#[cfg(windows)]` device/UNC/drive-letter root length and a `#[cfg(unix)]` single-slash root. `product::is_valid_export_directory` now delegates to it, and `product.rs` re-exports the single `PROTECTED_EXPORT_DIRECTORY_NAMES` constant instead of keeping a duplicate. Previously the product gate used `path.rsplit(['/','\\']).next()`, which treated `\` as a separator on every host, so on Unix it rejected `C:\Users\me\Desktop` where the source accepts it. |
| Regression evidence | `source_file_name` is pinned by a 12-case cross-platform table plus a `#[cfg(windows)]` 23-case table and a `#[cfg(unix)]` 9-case table derived from the authoritative .NET sources; `product.rs` gained `export_directory_gate_agrees_with_the_shared_export_root_guard`, which asserts over 13 paths that `is_valid_export_directory(path)` equals `validate_export_directory(Path::new(path)).is_ok()`, so the product gate and the shared guard cannot drift again, alongside a `cfg`-split test that keeps `C:\Users\me\Desktop` rejected on Windows and accepted on Unix. The 48 `Path.GetFileName` fixture rows replay the Windows runtime. Full workspace: 1432 library and 72 CLI tests passed; warnings-denied Clippy on Windows and on `--target x86_64-unknown-linux-gnu`, format check, and `git diff --check` passed. |
| Remaining difference | No Unix runtime is available on this host, so the Unix root-length table, the Unix arm of the gate, and the Unix invalid-character set of `src/io/file_names.rs` (`DIFF-FILE-NAME-001`) rest on the authoritative .NET sources rather than a runtime differential, and the non-portable fixture rows are skipped by design. Cross-target Clippy proves the Unix arm compiles warning-free, not that it behaves correctly at runtime. | |

## DIFF-PREFAB-IMPORTER-DEFAULTS-001 - PrefabImporter factory defaults

| Field | Finding |
|---|---|
| Status | Closed for the default object returned by PrefabImporter.Create with a null AssetCollection on the probed Unity version. RootGameObject AssetBundleName application and the full source collection export remain open under EXPORT-UNITY-001. |
| Source location | Source/AssetRipper.Export.UnityProjects/Project/PrefabExportCollection.cs (CreateImporter); AssetRipper.SourceGenerated ClassID_468431735.PrefabImporter.Create |
| Source behavior | The isolated runtime probe on Unity 2022.3.22f1 returned AssetBundleName=null, blank AssetBundleName_R, AssetBundleVariant, Name, and UserData, with ExternalObjects.Count=0. PrefabExportCollection only assigns AssetBundleName_R when RootGameObject.AssetBundleName is non-null. |
| Rust result | The modern Prefab project route emits PrefabImporter with empty externalObjects, userData, assetBundleName, and assetBundleVariant for the synthetic root with no bundle metadata. |
| Regression evidence | tools/probes/scene-importer/Program.cs and src/export/prefab_importer_probe_fixture.txt record the reference-runtime result. project_export_writes_a_processed_prefab_collection_with_generated_transform verifies the resulting .prefab and .meta through export-project. The probe ran against the isolated ar-ref build, not the read-only reference source directory. |
| Remaining difference | The workspace route does not yet derive RootGameObject.AssetBundleName from the source AssetBundle Container. No full source export comparison or Unity Editor import was run; generated legacy Prefab and m_PrefabInternal behavior remain unsupported. |

## DIFF-PREFAB-INSTANCE-ROUTE-001 - Existing modern PrefabInstance processing and export

| Field | Finding |
|---|---|
| Status | Closed for the synthetic Unity 2019.4 modern `PrefabInstance` branch through AssetRipper source processing and the Rust `export-project` route. This does not close full collection-export parity or `EXPORT-UNITY-001`. |
| Source location | `AssetRipper.Processing.Prefabs.PrefabProcessor.Process` existing-PrefabInstance loop; `PrefabHierarchyObject.Create`; `GameObjectHierarchyObject.ExportableAssets`; `AssetRipper.Export.UnityProjects.Project.PrefabExportCollection` |
| Input evidence | Rust test helper `make_existing_prefab_instance_file` builds generation-10 bytes for Unity 2019.4.40f1 with `BuildTarget.NoTarget`, GameObject PathID 10, Transform PathID 11, and PrefabInstance PathID 12 whose `m_RootGameObject` points to 10. The identical file was passed to the reference runtime under a `.unity` path so its collection flags selected the Editor TypeTree layout. SHA-256: `AFF86571CB3C94DDB2130F2B718BB5DC9F286A0971531D247535E504928E984D`. |
| Reference behavior | The isolated `SceneProcessorProbe` run materializes all three input classes as generated objects and the real `PrefabProcessor` creates one `PrefabHierarchyObject`. Root GameObject 10 and Transform 11 are members and exportable; PrefabInstance 12 is retained as the hierarchy source and hidden from YAML output. Exact rows are in `src/processing/prefab_instance_runtime_fixture.txt`. |
| Rust result | `project_export_routes_an_existing_modern_prefab_instance_collection` routes the same bytes through the Rust workspace and project exporter. It writes one `Root.prefab` and one `PrefabImporter` sidecar with exactly the GameObject and Transform documents, reciprocal mapped PPtrs, and no visible PrefabInstance document or duplicate generic `.asset` output. |
| Regression evidence | Focused command `cargo test --workspace --locked project_export_routes_an_existing_modern_prefab_instance_collection -- --nocapture` passed (1 library test; 73 CLI tests filtered). Reference command `dotnet run --project tools/probes/scene-processor/SceneProcessorProbe.csproj --configuration Release -- process target/prefab-instance-source-runtime.txt target/prefab-instance-2019.4.unity` exited 0 against the isolated `ar-ref` build. Probe output confirms `GameObject_2018_3`, `Transform_2018_3`, `PrefabInstance_2018_3`, one Prefab Hierarchies collection, and the expected exportable/hidden sets. |
| Remaining difference | This is a source processor differential plus a Rust product-route fixture, not a comparison against AssetRipper's full Unity project exporter. Unity Editor import was not run because the installed Editor is unlicensed. Generated legacy Prefabs, legacy `m_PrefabInternal` overlays, Container-derived importer bundle metadata, and broader export parity remain open. |

## DIFF-YAML-STREAMED-IMAGE-CLASS-ROUTE-001 - Streamed image-texture YAML routing

| Field | Finding |
|---|---|
| Status | Closed for the streamed YAML preparation branch and generated `IImageTexture` class dispatch; texture decoding and full project-export parity remain open. |
| Source location | `AssetRipper.Export.UnityProjects.ProjectExporter` registrations; `YamlStreamedAssetExporter.TryCreateCollection`; `YamlStreamedAssetExportCollection.ExportTexture`; `IImageTexture`; `StreamingInfoExtensions`. |
| Source behavior | The source registers the YAML streamed exporter for the `IImageTexture` interface. Reflection of the frozen generated assembly reports class IDs 28, 89, 117, 187, 188, and 189. For class 28, the isolated source project exporter resolved `texture.resS` offset 2 size 3, emitted bytes `32 33 34` as Texture2D TypelessData, cleared the serialized stream fields for YAML, then restored the original object values after export. Current TPK layouts for 28/89/117/187/188 serialize `m_StreamData` and `image data`; class 189 uses its generated class fields. |
| Rust result | `workspace_yaml_export_handles_every_generated_image_texture_interface_class` sends six generated TypeTree-shaped SerializedFiles through the Rust collection YAML route, checking bytes, cleared stream fields, and unchanged source objects. `export_project_materializes_streamed_texture2d_and_fails_closed_when_resource_is_missing` checks the class-28 root-resource project path, `.texture2D.meta`, input preservation, and no output on a missing declared resource. |
| Regression evidence | Source probe: `dotnet run --project tools/probes/streamed-image-project/StreamedImageProjectProbe.csproj --configuration Release -p:ReferenceRoot=C:\Users\koishi\AppData\Local\Temp\opencode\ar-ref -- target/source-streamed-image-runtime` exited 0 and asserted the emitted YAML payload/cleared stream plus restored object values. Rust full workspace: 1,485 library passed, 1 ignored; 74 CLI passed; 0 doc tests. |
| Remaining difference | The direct source exporter probe constructs a generated class-28 object and ResourceFile; the compact Rust SerializedFile fixture does not encode the full generated class-28 release payload, so this is a source-behavior differential, not byte-identical source parser replay. The Rust six-class integration is based on the frozen interface union and TPK field probe, not per-version generated dispatch. Source declines empty/unavailable streamed data; the Rust project route reports a missing declared resource as an error instead of silently omitting it. Unity Editor import is unavailable on this host. |
## DIFF-TT-ABSENT-GENERATED-TEXTASSET-001 - Bounded generated TextAsset and MonoScript recovery

| Field | Finding |
|---|---|
| Status | Closed for source-probed Unity 2019.4.40f1 generated TextAsset and MonoScript cases through Rust object workspace and reference graph; the TextAsset also reaches export-project. SER-TT-ABSENT-001 and SER-ASSET-FACTORY-001 remain open. |
| Source location | AssetRipper.Import.AssetCreation.GameAssetFactory.ReadAsset / CreateAsset; generated TextAsset_2018_3; TypeTreeNodeStruct.TryMakeFromTpk; SerializedFile.ObjectReadFlags. |
| Input evidence | The source probe authors TextAsset class ID 49 and MonoScript class ID 115, each at PathID 42, Unity 2019.4.40f1, platform 19 (StandaloneWin64Player), with an empty serialized file TypeTree. Their SF_HEX and SF_READ rows are frozen as textasset_notree and monoscript_notree in src/processing/asset_factory_fixture.txt; a fresh isolated probe replay matched all four rows for each case byte-for-byte. |
| Reference behavior | The shipped factory creates TextAsset_2018_3 named Hero and MonoScript_2018_3 named HeroController from their source TPK layouts. The MonoScript rows also pin m_ClassName=HeroController, m_Namespace=VRC.SDK, m_AssemblyName=Assembly-CSharp, and m_ExecutionOrder=200. The isolated SerFactoryProbe author command exited 0 and wrote only to the ar-ref copy and Rust target scratch/output; its exact invocation and location are recorded in docs/validation-log.md. |
| Rust result | ObjectWorkspace recovers both payloads and retains each recovered TypeTree; the reference graph reports a complete scan for both. The MonoScript workspace regression checks its class, name, namespace, assembly, and execution-order fields. export-project emits one class-49 YAML document for the TextAsset with m_Name: Hero and m_Script: print(1). Both inputs remain unchanged. An unsupported unknown class remains unresolved and fails project export before creating the output root. |
| Regression evidence | Focused command cargo test --workspace --locked tree -- --nocapture passed (41 library tests and 2 CLI tests matched); cargo test --workspace --locked monoscript -- --nocapture passed the MonoScript case. Full suite and build gates are in docs/validation-log.md. |
| Remaining difference | Rust recovery is restricted to generated classes with source-probed 2019.4.40f1 TPK layouts and selects release/editor data using file object-read flags. The project integration exercised the player/release layout only; editor-file fallback selection remains unverified. Other generated classes or versions, TPK TypeTreeObject, MonoBehaviour assembly types, and all GameAssetFactory retry/tail/engine-collection branches also remain open. The project-export route is only checked end to end for TextAsset. Unity Editor import was not run. |

## DIFF-IO-BUNDLE-SCHEME-001 - Bundle dispatch, layouts, entries, and nested reads

| Field | Finding |
|---|---|
| Status | Closed for the source-probed UnityFS entry-failure reporting and continuation behavior across direct, recursive, batch, workspace, inspection, and export-refusal paths. External resource/dependency resolution remains open under `IO-RESOURCE-001`; full Phase 1 parity remains incomplete. |
| Source location | `AssetRipper.IO.Files.SchemeReader.LoadFile` / `ReadFile`; `FileStreamBundleFile.ReadFileStreamMetadata` / `ReadFileStreamData`; `BundleFileBlockReader.ReadEntry`; `RawWebBundleHeader.Read`; `RawWebBundleFile.Read`; `WebFile.Read`; `ArchiveBundleFile.Read`. |
| Fixture evidence | `tools/probes/io-differential/Program.cs` independently builds container bytes and calls the frozen source runtime's `SchemeReader.LoadFile` plus `ReadContentsRecursively`. `src/io/io_differential_fixture.txt` now contains 41 source `SCHEME` cases (the other 30 stream rows were byte-identical to the prior fixture). The source run writes only to the isolated Rust `target/` directory and references the isolated build at `%LOCALAPPDATA%\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release`. |
| Reference behavior | Cases cover UnityFS versions 1/6/7/8; metadata compression None/LZMA/LZ4/LZ4HC plus rejected LZHAM and unknown discriminants; storage None/LZMA/LZ4/LZ4HC, source-rejected LZHAM, and Zstandard frames on the default branch; entries crossing mixed-compression blocks; combined and uncombined directory metadata; front/end metadata placement; v7+ header alignment and padding-at-start; UnityRaw and UnityWeb header versions 1–8; Web's final scene record; nested UnityFS/Raw/Web/WebData and GZip paths; WebData entries; plain resources; and UnityArchive's `NotSupportedException`. A UnityFS LZHAM block becomes one `FailedFile` while a valid sibling remains in the reference container. |
| Rust result | Production `SchemeReader` matches every reference scheme and recursive leaf inventory. For the 36 source-readable container cases without failed entries, the test calls `mount_container_tree_with_source_name`, checks resource counts/names, reads each VFS-mounted file, and compares its bytes with the reference inventory. For the LZHAM entry case, direct and recursive product mounts now retain one typed failed-entry record and continue to mount the later `after.bin` sibling with the reference bytes. Batch aggregation preserves the originating input path; ObjectWorkspace preserves the container owner. CLI inspection reports the failed entry, and project export rejects the incomplete batch/workspace before creating output. Archive remains explicit `ArchiveReadUnsupported`; LZHAM/unknown metadata remain explicit parse errors. |
| Regression evidence | Isolated source command (exit 0): `dotnet run --project tools/probes/io-differential/IoDifferentialProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- target/io-differential-final.txt`; it wrote 71 total lines, including 41 `SCHEME` rows. The fixture includes a valid later sibling in a separate uncompressed block after the LZHAM failure. Rust focused tests passed for the full scheme fixture, workspace failure ownership, and `inspection_reports_failed_bundle_entries_and_project_export_refuses_partial_mounts`; the full suite and static gates are recorded in `docs/validation-log.md`. The source probe emits an existing CA2022 warning in the unrelated `SmartStream.Read` probe. |
| Remaining difference | No difference remains in the source-probed bundle dispatch, recursive mount, and per-entry UnityFS failure-reporting behavior. Failure count and diagnostic metadata limits are explicit fail-closed bounds. External resource-name/dependency resolution remains open under `IO-RESOURCE-001`; no Unity Editor import/runtime result is claimed. |

## DIFF-PRODUCT-CLI-EMPTY-DIRECTORY-001 - LoadAndProcess on an empty input directory

| Field | Finding |
|---|---|
| Status | Closed for the empty-directory load/process gate. The broader directory-inspection workflow remains open under `PRODUCT-CLI-INSPECT-001`. |
| Source location | `GameStructure.Load`; `GameStructure` constructor; `ExportHandler.LoadAndProcess`; `GameFileLoader.LoadAndProcess`. |
| Input evidence | `tools/probes/cli-inspect/CliInspectProbe.csproj` calls the frozen `ExportHandler.LoadAndProcess` with one empty directory using the isolated reference build. The probe input is created under Rust `target/`; no source or user data is read. |
| Reference behavior | Source returns a GameData whose GameBundle has zero asset collections. The LoadAndProcess collection gate skips `Process`, so the overridden processor-sequence factory is never requested. The probe emits `EMPTY_DIRECTORY|collections=0|processors_requested=0`. `GameStructure` logs its existing no-valid-assets warning. |
| Rust result | `mount_input_directories` now creates the selected VFS root even when no files were discovered. `inspect_directories` returns successfully with a no-valid-assets message and skips workspace loading/processing, matching the source empty-bundle outcome. |
| Regression evidence | Isolated source command (exit 0): `dotnet run --project tools/probes/cli-inspect/CliInspectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- target/cli-inspect-empty-probe-input`. Rust test `empty_directory_inspection_reports_unprocessed_empty_bundle` replays the frozen output row in `src/io/cli_inspect_fixture.txt` and asserts the CLI warning. Product command `cargo run --locked -- inspect target/cli-inspect-empty-probe-input` exits 0 with the expected message. |
| Remaining difference | This closes only the empty-directory outcome. Platform and mixed directory inventory, dependency loading, and all baseline GUI/export/settings workflows retain their own open or completed rows; full source product parity is not claimed. |

## DIFF-PRODUCT-CLI-PLATFORM-MIXED-001 - Platform and mixed directory union in CLI inspection

| Field | Finding |
|---|---|
| Status | Closed for preserving selected mixed-directory files alongside a detected platform structure and resolving a cross-file reference in their common workspace. Remaining CLI error/summary behavior and full product workflows stay open under `PRODUCT-CLI-INSPECT-001`. |
| Source location | `AssetRipper/Source/AssetRipper.Import/Platforms/PlatformChecker.cs`; `AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs` (`InitializeGameCollection`); `AssetRipper/Source/AssetRipper.Export.UnityProjects/ExportHandler.cs` (`LoadAndProcess`). |
| Input evidence | `tools/probes/cli-inspect/CliInspectProbe.csproj` builds a synthetic Windows platform root containing `game.exe` and `game_Data/globalgamemanagers`, plus a separately selected mixed directory containing `main.assets`. Both SerializedFiles use the source-probed `SFHEX` bytes from `src/io/platform_probe_fixture.txt`; scratch inputs are created beneath Rust `target/`. |
| Reference behavior | The isolated source runtime retains `globalgamemanagers` and `main.assets` in `GameBundle.FetchAssetCollections()` and requests the processor sequence once. Its result is frozen as `PLATFORM_MIXED|input_names=globalgamemanagers,main.assets|processors_requested=1`. |
| Rust result | `inspect_directories_to` now scans selected paths left unconsumed by platform detection. `PlatformStructure::load_primary_serialized_workspace_with_additional_inputs` merges mixed SerializedFile inputs, registrations, and DataPaths with platform initial files and pre-expanded containers before one dependency walk. The regression gives the platform primary file an external reference to mixed `main.assets`; the report resolves that request as `AlreadyLoaded` in the same workspace. `inspect` reports both registration sets and all three collections (platform primary, platform container child, and mixed file). |
| Regression evidence | Isolated source command (exit 0): `dotnet run --project tools/probes/cli-inspect/CliInspectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- --platform-mixed src/io/platform_probe_fixture.txt target/cli-inspect-platform-mixed-replay`. Rust command `cargo test --workspace --locked platform_directory_inspection_unions_mixed_inputs_and_resolves_their_references -- --nocapture` passes; it checks the common-workspace dependency outcome and CLI summary against the frozen source row in `src/io/cli_inspect_fixture.txt`. Product command `cargo run --locked -- inspect target/cli-inspect-platform-mixed-replay/platform target/cli-inspect-platform-mixed-replay/mixed` exits 0 and reports both registrations and two SerializedFiles. |
| Remaining difference | This closes one Windows platform root plus one mixed directory and a resolved external reference. Other platform layouts, multiple platform selections, and malformed mixed inputs remain under their platform and directory-import rows; malformed container behavior remains under IO-BUNDLE-001/IO-STREAM-INPUT-001. Full export/settings/cancellation and GUI workflows remain under their product rows. No Unity Editor import/runtime behavior is claimed. |

## DIFF-PRODUCT-CLI-INPUT-GATES-001 - directory inspection input errors and empty-load behavior

| Field | Finding |
|---|---|
| Status | Closed for the Rust directory-inspection input gate, no-asset result, platform/mixed union, dependency summary, and structural inventory route. This does not close baseline processor mutations, project export, settings, or GUI behavior. |
| Source location | AssetRipper.GUI.Web.GameFileLoader.LoadAndProcess; AssetRipper.Export.UnityProjects.ExportHandler.LoadAndProcess; AssetRipper.Import.Structure.GameStructure.Load; AssetRipper.Import.Platforms.PlatformChecker.CheckPlatform; MixedGameStructure; GameBundle.FromPaths. |
| Input evidence | The isolated frozen-source probe tools/probes/cli-inspect generated only scratch directories under Rust target. Its --input-cases mode checks an empty path list, a missing directory, an empty directory, and a resource-only directory; its --platform-mixed mode creates a Windows platform root plus a selected mixed directory from src/io/platform_probe_fixture.txt. The stable LOAD and PLATFORM_MIXED rows are in src/io/cli_inspect_fixture.txt. No user bundle or reference source file was used as input. |
| Source behavior | ExportHandler.LoadAndProcess throws ArgumentException for no paths and fails on a missing directory. Empty and resource-only directories return a GameBundle with zero asset collections and never request the processor sequence. A Windows platform directory plus a mixed directory retains globalgamemanagers and main.assets and requests one processor pass. |
| Rust behavior | The inspect CLI rejects no arguments and nonexistent directories with nonzero diagnostics. Empty and resource-only directories return successfully with the no-valid-assets message and skip workspace processing. For valid directories, inspection mounts bundle inventory, discovers SerializedFiles, resolves reachable external references, merges selected mixed files with a recognized platform workspace, and prints registrations, collection, scene, and dependency summaries. Failed bundle entries are reported by the existing mount/inspection path. |
| Regression evidence | Fresh isolated source command: dotnet run --project tools/probes/cli-inspect/CliInspectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- --input-cases target/cli-inspect-input-cases-replay. The four selected LOAD rows matched src/io/cli_inspect_fixture.txt byte-for-byte. Rust tests: inspect_cli_rejects_empty_and_missing_directory_inputs; resource_only_directory_matches_the_source_no_asset_process_gate; empty_directory_inspection_reports_unprocessed_empty_bundle; directory_inspection_reproduces_the_reference_bundle_inventory; platform_directory_inspection_preserves_and_expands_registered_containers; platform_directory_inspection_unions_mixed_inputs_and_resolves_their_references. Actual cargo run --locked --quiet -- inspect invocations exit 1 for no arguments and missing directories, and exit 0 with the expected no-assets message for empty/resource-only directories. A fresh source platform/mixed probe and Rust product command both report both input registrations and two SerializedFiles. |
| Remaining comparison | The Rust CLI summary is a diagnostic surface with no baseline stdout equivalent; its structural values are checked against frozen source collections, input inventories, and dependency outcomes. The source processor sequence is probed only for its gate, not for processor mutations; those mutations remain under PROC-SCENE-001 and PROC-ANIM-001. Full exporter/settings/cancellation and browser workflows remain under EXPORT-UNITY-001, EXP-SETTINGS-APPLY-001, and PRODUCT-GUI-001. |

## DIFF-IO-RESOURCE-EXPORT-001 - Streamed resource resolution through project export

| Field | Finding |
|---|---|
| Status | Closed for the generic streamed-resource lookup and bounded-range path exercised through project export, including selected root ResourceFiles and an external provider/DataPath. Texture decoding and complete project-export parity remain open in their asset-family and product rows. |
| Source location | AssetRipper.Export.UnityProjects ProjectExporter and YamlStreamedAssetExporter; StreamingInfoExtensions; GameBundle.ResolveExternalResource; IResourceProvider; Rust src/export/workspace_yaml.rs, src/export/project_workflow.rs, src/resource.rs, and src/io/platform_structure.rs. |
| Input evidence | Synthetic Unity 2022.3.22f1 Texture2D class 28 with stream path texture.resS, offset 2, size 3; the resource bytes are 30 31 32 33 34 35. Rust product input uses a generated Windows player layout with the resource under game_Data. The isolated reference probe tests both a GameBundle root ResourceFile and a custom IResourceProvider backed by a generated scratch file. |
| Source behavior | The frozen source project exporter reads bytes 32 33 34 into Texture2D YAML, clears the path/offset/size fields in the export view, and restores the original generated object after serialization. In external mode, GameBundle.ResourceProvider receives exactly one request for texture.resS and returns the resource file; the output YAML has empty path and zero offset/size. |
| Rust behavior | Production export-project builds the platform resource provider and passes it to the workspace exporter. The streamed YAML route resolves class-28 bytes at offset 2/size 3, emits data 323334, clears stream metadata in the export-owned view, writes the Texture2D asset, NativeFormatImporter sidecar, and detected Unity project version, and leaves both input files unchanged. The same YAML preparation route covers Mesh and generated IImageTexture class IDs 28, 89, 117, 187, 188, and 189. The root-catalog project-export test and missing-resource fail-closed test remain passing. |
| Regression evidence | Source commands exited 0 using the isolated reference copy: streamed-image-project probe in default root-resource mode and with --external target/.../source-export target/.../input/texture.resS. External output rows are frozen in src/export/streamed_image_resource_fixture.txt. Rust command cargo test --workspace --locked export_project_resolves_streamed_texture2d_from_platform_data_path -- --nocapture passed; existing export_project_materializes_streamed_texture2d_and_fails_closed_when_resource_is_missing covers root input and missing-resource behavior. Full gates are recorded in docs/validation-log.md. |
| Remaining difference | No difference remains in the tested root/provider lookup, byte range, YAML stream clearing, and source-object restoration route. Other class-specific decoders/exporters, complete settings and product workflows, and Unity Editor import/runtime validation remain open under their dedicated rows; the installed Editor has no license. |

## DIFF-SHADER-PROJECT-001 - Shader and Material project collection integration

| Field | Finding |
|---|---|
| Status | Open. The covered source collection paths now have a Rust `export-project` route and source-runtime importer comparisons; complete Material fields and Unity import remain outstanding. |
| Source location | `ProjectExporter.Overrides` registration order; `SimpleShaderExporter`, `DummyShaderTextExporter`, `YamlShaderExporter`, `YamlShaderExportCollection`; `ShaderExportCollection.CreateImporter`; base `AssetExportCollection.GetExportID`; `ExportCollection.GetUniqueFileName`. |
| Source behavior | An isolated AssetRipper 2.0.0 runtime probe exported generated Shader objects for Dummy and YAML modes at Unity 5.0, 2017.1, 2017.2, 2017.4, 2018.1/2/3, 2019.4, and 2022.3. Simple decompiled text is written verbatim to `.shader` and uses `ShaderImporter`; YAML mode writes `.asset`, uses `NativeFormatImporter`, and writes `AvoidSavingYamlShaders.cs` from Unity 3.5 plus `YamlShaderPostprocessor.cs` from 2018.1. `name` is present through 2018.2; `mainObjectFileID` is present by 2017.1; `externalObjects` appears from 2017.2; and `nonModifiableTextures` appears from 2018.1. Base asset IDs are class-prefixed (`Shader` 4,800,000; `Material` 2,100,000); Windows-invalid slash and backslash in `Test/Simple` become underscores. Unset string importer fields serialize as empty YAML values rather than the token `null`. The Rust comparison fixture contains 18 normalized importer rows (nine Unity versions for each of Dummy and YAML). |
| Rust behavior | `project_workflow` passes decoded Shader state through the source registration stack and routes Simple/Dummy text to `.shader` with versioned ShaderImporter metadata, and YAML mode to `.asset` with versioned NativeFormatImporter metadata and deduplicated editor patch files. Parsed-form dummy rendering is wired into the project route. Asset and Shader YAML anchors, pointers, and importer IDs use class-prefixed IDs. `m_NonModifiableTextures` map entries resolve their PPtrs through the workspace graph and project export pointer map; null entries remain null, and unresolved or unmatched edges fail closed. Material YAML participates in an integrated Material→Shader GUID/fileID regression. |
| Regression evidence | `tools/probes/shader-project/Program.cs` ran against the isolated read-only-source copy at `%LOCALAPPDATA%\Temp\opencode\ar-ref`; build exited 0 with 0 warnings and 0 errors. Eighteen normalized `.meta` rows and source asset/support-file output rows are recorded in `src/export/shader_project_fixture.txt`. The 2018.2.21f1 class-48 TPK confirms the `map` / `pair(first:string, second:PPtr<Texture>)` serialized shape; the isolated source runtime emitted the null `_MainTex` importer row committed in `src/export/shader_nonmodifiable_fixture.txt`. `shader_and_yaml_shader_importers_match_reference_project_probe` compares every base importer sidecar row; `export_project_shader_importer_matches_the_reference_nonmodifiable_texture_map` compares the non-modifiable null-map row; `export_project_shader_importer_maps_nonmodifiable_texture_pointers` checks a resolved cross-file texture PPtr against its emitted GUID/fileID. The project probe build and focused tests are recorded in `docs/validation-log.md`. |
| Remaining difference | Material serialization is not complete for all versioned fields; source-runtime output was generated rather than Unity-produced; Unity Editor import/runtime was not run because the installed editor is unlicensed. Serialized class-48 TypeTree coverage remains partial. `RES-MATERIAL-EXPORT-001` and `EXPORT-UNITY-001` remain open. |

## DIFF-MATERIAL-PROJECT-2018-001 - Unity 2018.2 Material project YAML

| Field | Finding |
|---|---|
| Status | Matched for the 2018.2.21f1 Material release layout with both empty saved-property maps and a populated case containing one entry in each texture/float/color map. This bounded comparison does not close `RES-MATERIAL-EXPORT-001` or the broader Shader/Material project row. |
| Source location | `AssetRipper.Export.UnityProjects/Project/DefaultYamlExporter.cs`; the generated class-21 Material and `UnityPropertySheet` TypeTrees; project pointer conversion for the Material `m_Shader` field. |
| Input evidence | `SerFactoryProbe.dll tpk 21 2018.2.21f1` produced release and editor TypeTrees committed in `src/export/material_tpk_2018_fixture.txt`. Rust regressions build synthetic SerializedFiles from the release tree: one has empty maps, and the populated case has `_MainTex` with non-default scale/offset, `_Glossiness: 0.375`, and `_Color` RGBA `(0.125, 0.25, 0.5, 0.75)`, plus the same cross-file Shader PPtr and string tag. No user data is used. |
| Source behavior | The isolated AssetRipper runtime probe creates generated `Material_2018_2` and `Shader_2018_2` objects, exports into a fresh target directory, and emits two normalized `.mat` fixtures. The populated output in `src/export/material_project_properties_fixture.yaml` confirms dictionary-key rendering and all non-default values; the base output remains in `src/export/material_project_fixture.yaml`. Both contain root `serializedVersion: 6`, the three editor defaults, all release fields, nested `m_SavedProperties.serializedVersion: 3`, and a Shader pointer with class-prefixed fileID and `type: 3`. |
| Rust behavior | The TypeTree writer preserves node versions; Unity YAML conversion emits versioned nested-struct `serializedVersion`; the 2018.2 project Material body adds the source-runtime editor defaults; and the Material `m_Shader` export pointer uses Meta pointer type 3 while resolving its fileID/GUID from the Shader collection. The populated regression matches texture scale/offset, float and RGBA values, and all three string-keyed dictionaries. |
| Regression evidence | Fresh source runtime exports exited 0 and both normalized YAML fixtures matched exactly; the fresh class-21 TPK probe matched its committed TypeTree fixture after newline normalization. `export_project_material_yaml_matches_the_reference_2018_2_project_probe` and `export_project_material_saved_property_maps_match_the_reference_2018_2_project_probe` compare the emitted `.mat` against the source fixtures after normalizing only the generated Shader GUID. `export_project_dispatches_collections_and_writes_a_complete_unity_project` also passes with source-confirmed `type: 3`. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | The populated case has one entry per saved-property map. Multiple-entry ordering/duplicate-key behavior, other Material version layouts and fields, Unity-produced SerializedFiles, C# serialization round-trip, and Unity Editor import/runtime remain unverified. The Material row stays `已实现但未验证`. |

## DIFF-MATERIAL-PROJECT-2018-3-001 - Unity 2018.3 Material project YAML boundary

| Field | Finding |
|---|---|
| Status | Matched for the 2018.3.0f1 release layout with one populated entry in each saved-property map. This differential records the editor-field boundary from 2018.2 and does not close `RES-MATERIAL-EXPORT-001`. |
| Source location | `AssetRipper.Export.UnityProjects/Project/DefaultYamlExporter.cs`; generated class-21 `Material_2018_3`, class-48 `Shader_2018_3`, and their release TypeTrees; project Material editor-default serialization. |
| Input evidence | `SerFactoryProbe.dll tpk 21 2018.3.0f1` produced the release/editor trees committed in `src/export/material_tpk_2018_3_fixture.txt`. The Rust regression constructs a synthetic SerializedFile from the release tree with the same scalar/tag fields and one `_MainTex`, `_Glossiness`, and `_Color` entry used by the 2018.3 source runtime. The Material Shader PPtr resolves across files. No user data is used. |
| Source behavior | An isolated source runtime probe creates generated 2018.3 Material/Shader objects and exports `src/export/material_project_2018_3_fixture.yaml`. Relative to the 2018.2 output, the source editor block includes `m_PrefabInstance` and `m_PrefabAsset` instead of `m_PrefabInternal`; the serialized release fields and populated saved-property values match the 2018.2 case. |
| Rust behavior | Project Material export selects the source-observed 2018.3 editor fields by Unity version and emits their null PPtrs in source order. It preserves the 2018.2 branch separately. The TypeTree decoder and saved-property YAML conversion produce the same scalar, string-tag, texture-environment, float, and RGBA values as the source runtime. |
| Regression evidence | `export_project_material_yaml_matches_the_reference_2018_3_project_probe` passes against the source fixture after normalizing only the generated Shader GUID. Fresh source runtime runs matched all three Material fixtures (2018.2 empty, 2018.2 populated, 2018.3 populated), and a fresh TPK probe matched the committed 2018.3 TypeTree fixture. The isolated probe project build succeeded with 0 warnings and 0 errors. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | This is one 2018.3 layout with one entry per map. Other Unity-version editor fields/layouts, multiple-entry ordering/duplicate behavior, Unity-produced SerializedFiles, C# serialization round-trip, and Unity Editor import/runtime remain unverified. `RES-MATERIAL-EXPORT-001` remains `已实现但未验证`. |
