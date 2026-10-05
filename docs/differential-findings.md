# Source behavior and differential findings

This log records behavior differences found between the Rust port and the frozen local source. A source-code analysis is identified as such; it is not described as an executed C# differential.

## DIFF-BROTLI-METADATA-001 — metadata length off by one in source detection

| Field | Finding |
|---|---|
| Status | Rust detector corrected and regression-covered; C# execution and Unity-produced-file comparison remain unrun |
| Source location | `AssetRipper/Source/AssetRipper.IO.Files/CompressedFiles/Brotli/BrotliFile.cs`, `BrotliFile.IsBrotliFile` |
| Input evidence | `scheme::tests::dispatches_a_spec_encoded_unity_brotli_metadata_wrapper` constructs a Brotli stream with Unity's 36-byte marker in an empty metadata meta-block, followed by an uncompressed meta-block containing `webdata payload` and a final empty block. The fixture is assembled from RFC 7932 bit fields; the pinned decoder returns the exact payload for every WBITS value from 10 through 24. |
| Source-derived behavior | The source reads the encoded `MSKIPLEN - 1` value and compares it directly with `BrotliSignature.Length`. For the valid 36-byte marker, the encoded value is 35, so the signature comparison fails. This is derived from the inspected implementation; the C# method was not executed because `dotnet` is unavailable. |
| Root cause | RFC 7932 §9.2 stores metadata length minus one. The detector omitted the `+1` needed to obtain the number of metadata bytes before comparing the marker length. |
| Rust change | `has_brotli_file_signature` now parses the Brotli window header, validates the empty metadata block fields and zero alignment bits, adds one to the encoded length, bounds the marker read, and compares the metadata bytes. The recursive scheme reader then decodes the same input under the configured output limit. |
| Regression evidence | The constructed marker streams are recognized and decoded for WBITS 10–24. `docs/validation-log.md` records the full test run. |
| Remaining comparison | No C# execution, separate Brotli encoder run, AssetRipper-produced wrapper, or real-file differential has been performed. |

The format reference is [RFC 7932 §9.2](https://www.rfc-editor.org/rfc/rfc7932.html#section-9.2). Its metadata length field stores `MSKIPLEN - 1`, and the metadata bytes are excluded from decompressed output.
## DIFF-MULTIFILE-SEEK-END-001 — source uses the opposite End offset sign

| Field | Finding |
|---|---|
| Status | Rust follows standard End-relative seek semantics; source discrepancy is identified by static inspection; C# execution is unavailable |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/Streams/MultiFileStream.cs, MultiFileStream.Seek |
| Input evidence | io::stream::tests::multi_file_stream_reads_across_parts_and_seeks_logically seeks to End(-2) over a seven-byte logical stream and reads the final two bytes. |
| Source-derived behavior | The inspected source sets Position to Length - offset for SeekOrigin.End. With the conventional negative offset used to seek backward from end, this moves beyond the stream length. |
| Rust behavior | MultiFileStream implements Rust Seek semantics: the End-relative offset is added to the logical stream length. |
| Remaining comparison | This is a source-code discrepancy, not an executed C# differential. The local AssetRipper cannot be built here because dotnet is unavailable. |

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
