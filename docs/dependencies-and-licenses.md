# Rust dependencies and license record

## Direct runtime dependencies

`lzma-rs` provides raw LZMA decoding. Unity's five-byte properties header and expected output length are parsed by `src/compression/lzma.rs`; the Rust wrapper bounds output allocation and decoder history memory. Only its `raw_decoder` feature is enabled.

`ruzstd` provides Zstandard frame decoding in pure Rust. It is built with `std` and `hash` for streaming I/O and frame checksum validation. Its decoder window is bounded by `UnityFsLimits::max_zstd_window_bytes`. The implementation uses Zstandard only for UnityFS storage blocks whose compression type is unknown to the source enum and whose data has the Zstandard frame signature. LZHAM type 4 remains explicitly unsupported, and metadata compression does not use this fallback.

| Crate | Locked version | Use | Declared license | Rust version metadata |
|---|---:|---|---|---|
| [lzma-rs](https://github.com/gendx/lzma-rs) | 0.3.0 | Pure Rust raw LZMA decoder | MIT | Not declared in the published crate manifest |
| [ruzstd](https://github.com/KillingSpark/zstd-rs) | 0.9.0 | Pure Rust Zstandard frame decoder | MIT | 1.87 |
| [byteorder](https://github.com/BurntSushi/byteorder) | 1.5.0 | Little-endian parsing used by `lzma-rs` | Unlicense OR MIT | 1.60 |
| [crc](https://github.com/mrhooray/crc-rs) | 3.4.0 | CRC support compiled as an `lzma-rs` dependency | MIT OR Apache-2.0 | 1.83 |
| [crc-catalog](https://github.com/akhilles/crc-catalog) | 2.5.0 | CRC algorithm definitions used by `crc` | MIT OR Apache-2.0 | Not declared in the published crate manifest |
| [twox-hash](https://github.com/shepmaster/twox-hash) | 2.1.4 | XXH64 checksums used by `ruzstd` | MIT | 1.81 |

`Cargo.lock` records the exact selected dependency graph. The package minimum is Rust 1.87 to satisfy `ruzstd` 0.9.0. Rust 1.87 is not installed in the current environment, so only Rust 1.98.1 has been used to build and test this repository. The selected crates are pure Rust and do not require a native C codec toolchain.

The AssetRipper port is GPL-3.0-only. Before distributing binaries, package the applicable third-party license texts and notices alongside the product; this record is not a substitute for those distribution artifacts.

## Verification scope

The LZMA fixture is encoded with `lzma-rs`'s LZMA-alone encoder, then reshaped to Unity's raw layout by removing its eight-byte unpacked-size field. It exercises raw decoder and bundle integration but is not independent codec evidence.

The Zstandard frame fixtures are constructed from the RFC frame and raw/RLE block fields, independently of the decoder implementation. They cover raw and RLE blocks, concatenated frames, skippable frames, malformed input, output bounds, and window bounds. No Huffman/FSE entropy-compressed Zstandard block from an independent encoder, Unity-produced LZMA sample, real Unity bundle, or AssetRipper differential has been run yet. See [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878.html), the [ruzstd API](https://docs.rs/ruzstd/0.9.0/ruzstd/decoding/struct.StreamingDecoder.html), and its [upstream repository](https://github.com/KillingSpark/zstd-rs).
