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
