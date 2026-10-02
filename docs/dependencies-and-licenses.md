# Rust dependencies and license record

## Direct runtime dependency

`lzma-rs` is used only for raw LZMA decoding. Unity's five-byte properties header and expected output length are parsed by `src/compression/lzma.rs`; the Rust wrapper bounds output allocation and decoder history memory. `Cargo.toml` disables default features and enables only `raw_decoder`.

| Crate | Locked version | Use | Declared license | Rust version metadata |
|---|---:|---|---|---|
| [lzma-rs](https://github.com/gendx/lzma-rs) | 0.3.0 | Pure Rust LZMA raw decoder | MIT | Not declared in the published crate manifest |
| [byteorder](https://github.com/BurntSushi/byteorder) | 1.5.0 | Little-endian parsing used by `lzma-rs` | Unlicense OR MIT | 1.60 |
| [crc](https://github.com/mrhooray/crc-rs) | 3.3.0 | CRC support compiled as an `lzma-rs` dependency | MIT OR Apache-2.0 | 1.65 |
| [crc-catalog](https://github.com/akhilles/crc-catalog) | 2.5.0 | CRC algorithm definitions used by `crc` | MIT OR Apache-2.0 | Not declared in the published crate manifest |

The dependency graph and exact selected versions are locked in `Cargo.lock`. `crc` is held at 3.3.0 because 3.4.0 declares Rust 1.83, above this package's declared Rust 1.74 minimum. The Rust 1.74 toolchain is not installed in the current environment, so compatibility with that minimum remains unverified. Current builds use Rust 1.98.1.

The AssetRipper port is GPL-3.0-only. Before distributing binaries, package the applicable third-party license texts and notices alongside the product; this record is not a substitute for those distribution artifacts.

## Verification scope

The current LZMA fixture is encoded with `lzma-rs`'s LZMA-alone encoder, then transformed to Unity's raw layout by removing its eight-byte unpacked-size field. It exercises the raw decoder API and Unity block integration but is not independent codec conformance evidence. No external Unity-produced LZMA fixture or AssetRipper differential has been run yet.
