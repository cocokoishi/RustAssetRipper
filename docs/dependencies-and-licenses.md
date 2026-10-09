# Rust dependencies and license record

## Direct runtime dependencies

`lzma-rs` provides raw LZMA decoding. Unity's five-byte properties header and expected output length are parsed by `src/compression/lzma.rs`; the Rust wrapper bounds output allocation and decoder history memory. Only its `raw_decoder` feature is enabled.

`ruzstd` provides Zstandard frame decoding in pure Rust. It is built with `std` and `hash` for streaming I/O and frame checksum validation. Its decoder window is bounded by `UnityFsLimits::max_zstd_window_bytes`. The implementation uses Zstandard only for UnityFS storage blocks whose compression type is unknown to the source enum and whose data has the Zstandard frame signature. LZHAM type 4 remains explicitly unsupported, and metadata compression does not use this fallback.

`brotli-decompressor` provides a pure-Rust Brotli streaming reader. `flate2` provides GZip decoding and encoding with only its `rust_backend` feature enabled, using the pure-Rust `miniz_oxide` backend. Wrapper decoders cap decoded output; `flate2::read::MultiGzDecoder` reads concatenated GZip members. GZip encoding caps the compressed bytes written to memory. Unity's Brotli metadata marker is parsed with a bounded RFC 7932 bit reader, then `SchemeReader` recursively applies the decoder. The Rust parser accounts for the metadata length field's `MSKIPLEN - 1` representation; the source discrepancy is tracked in `docs/differential-findings.md`.

`rustix` provides safe Unix system-call bindings for the shared advisory file lock used by `File.OpenRead` parity. The `fs` feature is enabled; Linux uses `linux-raw-sys`, while other supported Unix targets use the crate's platform bindings.

`dotnetdll` reads and writes PE/CLI and ECMA-335 CLR metadata for the Mono assembly path. The project vendors upstream 0.3.0 at `vendor/dotnetdll-0.3.0` and locally fixes two assembly-round-trip defects: the `ManagedMask` reader mapping and TypeDef method-list starts when methods are stored as property/event accessors. The patch record is `vendor/dotnetdll-0.3.0/UPSTREAM-PATCHES.md`; the upstream `LICENSE` is retained. This is a Rust metadata library and does not invoke a .NET process. It does not parse Unity containers or SerializedFiles; those remain Rust core responsibilities. Its GPL-3.0+ license is compatible with this GPL-3.0-only project when distributed under GPL-3.0; retain the crate attribution and applicable license notice in distribution materials.

| Crate | Locked version | Use | Declared license | Rust version metadata |
|---|---:|---|---|---|
| [lzma-rs](https://github.com/gendx/lzma-rs) | 0.3.0 | Pure Rust raw LZMA decoder | MIT | Not declared in the published crate manifest |
| [ruzstd](https://github.com/KillingSpark/zstd-rs) | 0.9.0 | Pure Rust Zstandard frame decoder | MIT | 1.87 |
| [brotli-decompressor](https://github.com/dropbox/rust-brotli-decompressor) | 5.0.3 | Pure Rust Brotli streaming decoder | BSD-3-Clause / MIT | Not declared in the published crate manifest |
| [flate2](https://github.com/rust-lang/flate2-rs) | 1.1.10 | GZip stream decoder and encoder using `rust_backend` | MIT OR Apache-2.0 | 1.67 |
| [rustix](https://github.com/bytecodealliance/rustix) | 1.1.5 | Safe Unix `flock` bindings for retained input handles | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 1.65 |
| [dotnetdll](https://github.com/nickbclifford/dotnetdll) | 0.3.0, locally patched | Rust PE/CLI reader and writer for CLR assembly metadata used by Mono type lookup and manager-backed script DLL export | GPL-3.0+ | 1.85.1 |
| [byteorder](https://github.com/BurntSushi/byteorder) | 1.5.0 | Little-endian parsing used by `lzma-rs` | Unlicense OR MIT | 1.60 |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.2 | Flag types used by `rustix` | MIT OR Apache-2.0 | 1.56 |
| [crc](https://github.com/mrhooray/crc-rs) | 3.4.0 | CRC support compiled as an `lzma-rs` dependency | MIT OR Apache-2.0 | 1.83 |
| [crc-catalog](https://github.com/akhilles/crc-catalog) | 2.5.0 | CRC algorithm definitions used by `crc` | MIT OR Apache-2.0 | Not declared in the published crate manifest |
| [twox-hash](https://github.com/shepmaster/twox-hash) | 2.1.4 | XXH64 checksums used by `ruzstd` | MIT | 1.81 |
| alloc-no-stdlib | 2.0.4 | Allocation traits used by `brotli-decompressor` | BSD-3-Clause | Not declared in the published crate manifest |
| alloc-stdlib | 0.2.4 | Standard allocator adapter used by `brotli-decompressor` | BSD-3-Clause | Not declared in the published crate manifest |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | 0.9.1 | Pure Rust DEFLATE backend used by `flate2` | MIT OR Zlib OR Apache-2.0 | Not declared in the published crate manifest |
| adler2 | 2.0.1 | Adler checksum used by `miniz_oxide` | 0BSD OR MIT OR Apache-2.0 | Not declared in the published crate manifest |
| simd-adler32 | 0.3.10 | Adler checksum implementation used by `miniz_oxide` | MIT | Not declared in the published crate manifest |
| [crc32fast](https://github.com/srijs/rust-crc32fast) | 1.5.2 | GZip CRC validation used by `flate2` | MIT OR Apache-2.0 | 1.63 |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.5 | Target configuration used by `crc32fast` | MIT OR Apache-2.0 | 1.32 |
| [errno](https://github.com/lambda-fairy/rust-errno) | 0.3.14 | Unix error codes used by `rustix` on non-Linux Unix targets | MIT OR Apache-2.0 | 1.56 |
| [libc](https://github.com/rust-lang/libc) | 0.2.190 | Platform bindings used by `rustix` on non-Linux Unix targets | MIT OR Apache-2.0 | 1.65 |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.12.1 | Linux syscall bindings used by `rustix` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 1.63 |

`Cargo.lock` records the exact selected dependency graph. The package minimum is Rust 1.87 to satisfy `ruzstd` 0.9.0. Rust 1.87 is not installed in the current environment, so only Rust 1.98.1 has been used to build and test this repository. The selected crates are pure Rust and do not require a native C codec toolchain.

The AssetRipper port is GPL-3.0-only. Before distributing binaries, package the applicable third-party license texts and notices alongside the product; this record is not a substitute for those distribution artifacts.

## Verification scope

The raw LZMA fixture is encoded with `lzma-rs`'s LZMA-alone encoder, then reshaped to Unity's raw layout by removing its eight-byte unpacked-size field. The size-prefixed fixture uses the same encoder but replaces its unknown-size sentinel with the known decoded size. These exercise both decoder and bundle integrations but are not independent codec evidence.

The Zstandard frame fixtures are constructed from the RFC frame and raw/RLE block fields, independently of the decoder implementation. They cover raw and RLE blocks, concatenated frames, skippable frames, malformed input, output bounds, and window bounds. GZip fixtures manually construct stored-DEFLATE members and their CRC trailers. The Brotli output fixture comes from the pinned decoder crate's upstream `test_10x_10y` test; it is not independent codec evidence. No Brotli wrapper with Unity's metadata marker, Huffman/FSE entropy-compressed Zstandard block from an independent encoder, Unity-produced LZMA sample, real Unity bundle, or AssetRipper differential has been run yet. See [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878.html), the [ruzstd API](https://docs.rs/ruzstd/0.9.0/ruzstd/decoding/struct.StreamingDecoder.html), [Brotli decoder API](https://docs.rs/brotli-decompressor/5.0.3/brotli_decompressor/), and [flate2 backend documentation](https://docs.rs/flate2/1.1.10/flate2/).
