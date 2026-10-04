//! Unity script FileIDs replicated from the frozen baseline's `ScriptHashing`.
//!
//! The baseline computes a script FileID as the little-endian `i32` of an MD4 digest over the
//! bytes `['s', 0, 0, 0] + namespace_utf8 + name_utf8`. The MD4 implementation below follows
//! RFC 1320 directly; the byte layout and the expected outputs come from the baseline's
//! `ScriptHashing.CalculateScriptFileID` overloads and its `ScriptHashingTests` vectors.
//!
//! The GUID companions (`CalculateScriptGuid`, `CalculateAssemblyGuid`) hash through
//! `UnityGuid.Md5Hash`, which lives in the external `AssetRipper.Primitives` package rather than
//! the frozen snapshot, and the snapshot has no test vectors for them. They are deliberately not
//! ported here instead of guessing the package's byte layout.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Bounds applied to the unbounded `stackalloc` in the baseline overloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptHashLimits {
    /// Maximum combined namespace and name bytes hashed for one FileID.
    pub max_combined_bytes: usize,
}

impl Default for ScriptHashLimits {
    fn default() -> Self {
        Self {
            max_combined_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Error returned when a script FileID cannot be computed within the configured bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptHashError {
    /// The combined namespace and name bytes exceed the configured bound.
    InputTooLong { byte_count: usize, limit: usize },
    /// A bounded allocation for the hash input could not be reserved.
    AllocationFailed { resource: &'static str },
}

impl Display for ScriptHashError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { byte_count, limit } => write!(
                formatter,
                "script hash input uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { resource } => {
                write!(formatter, "could not allocate {resource}")
            }
        }
    }
}

impl Error for ScriptHashError {}

/// `ScriptHashing.CalculateScriptFileID(namespace, name)` over pre-encoded UTF-8 bytes.
pub fn calculate_script_file_id(
    namespace: &[u8],
    name: &[u8],
    limits: ScriptHashLimits,
) -> Result<i32, ScriptHashError> {
    let combined =
        namespace
            .len()
            .checked_add(name.len())
            .ok_or(ScriptHashError::InputTooLong {
                byte_count: usize::MAX,
                limit: limits.max_combined_bytes,
            })?;
    if combined > limits.max_combined_bytes {
        return Err(ScriptHashError::InputTooLong {
            byte_count: combined,
            limit: limits.max_combined_bytes,
        });
    }
    let mut source = Vec::new();
    source
        .try_reserve(4 + combined)
        .map_err(|_| ScriptHashError::AllocationFailed {
            resource: "script hash input",
        })?;
    source.extend_from_slice(b"s\0\0\0");
    source.extend_from_slice(namespace);
    source.extend_from_slice(name);
    let digest = md4(&source);
    Ok(i32::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3],
    ]))
}

/// `ScriptHashing.CalculateScriptFileID(namespace, name)` over `str` values.
///
/// UTF-8 encoding makes this byte-identical to the span overload by construction.
pub fn calculate_script_file_id_str(
    namespace: &str,
    name: &str,
    limits: ScriptHashLimits,
) -> Result<i32, ScriptHashError> {
    calculate_script_file_id(namespace.as_bytes(), name.as_bytes(), limits)
}

/// MD4 message digest as specified by RFC 1320.
fn md4(message: &[u8]) -> [u8; 16] {
    let mut padded = message.to_vec();
    let bit_length = (message.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_le_bytes());

    let mut a: u32 = 0x6745_2301;
    let mut b: u32 = 0xefcd_ab89;
    let mut c: u32 = 0x98ba_dcfe;
    let mut d: u32 = 0x1032_5476;

    for block in padded.chunks_exact(64) {
        let mut x = [0u32; 16];
        for (word, bytes) in x.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        let (aa, bb, cc, dd) = (a, b, c, d);

        // Round 1.
        for i in (0..16).step_by(4) {
            a = a.wrapping_add(f(b, c, d)).wrapping_add(x[i]).rotate_left(3);
            d = d
                .wrapping_add(f(a, b, c))
                .wrapping_add(x[i + 1])
                .rotate_left(7);
            c = c
                .wrapping_add(f(d, a, b))
                .wrapping_add(x[i + 2])
                .rotate_left(11);
            b = b
                .wrapping_add(f(c, d, a))
                .wrapping_add(x[i + 3])
                .rotate_left(19);
        }
        // Round 2 with constant 0x5a827999.
        for i in [0, 1, 2, 3] {
            a = a
                .wrapping_add(g(b, c, d))
                .wrapping_add(x[i])
                .wrapping_add(0x5a82_7999)
                .rotate_left(3);
            d = d
                .wrapping_add(g(a, b, c))
                .wrapping_add(x[i + 4])
                .wrapping_add(0x5a82_7999)
                .rotate_left(5);
            c = c
                .wrapping_add(g(d, a, b))
                .wrapping_add(x[i + 8])
                .wrapping_add(0x5a82_7999)
                .rotate_left(9);
            b = b
                .wrapping_add(g(c, d, a))
                .wrapping_add(x[i + 12])
                .wrapping_add(0x5a82_7999)
                .rotate_left(13);
        }
        // Round 3 with constant 0x6ed9eba1.
        for i in [0, 2, 1, 3] {
            a = a
                .wrapping_add(h(b, c, d))
                .wrapping_add(x[i])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(3);
            d = d
                .wrapping_add(h(a, b, c))
                .wrapping_add(x[i + 8])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(9);
            c = c
                .wrapping_add(h(d, a, b))
                .wrapping_add(x[i + 4])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(11);
            b = b
                .wrapping_add(h(c, d, a))
                .wrapping_add(x[i + 12])
                .wrapping_add(0x6ed9_eba1)
                .rotate_left(15);
        }

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut digest = [0u8; 16];
    for (word, slot) in [a, b, c, d].iter().zip(digest.chunks_exact_mut(4)) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    digest
}

#[inline]
fn f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}

#[inline]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (x & z) | (y & z)
}

#[inline]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_script_file_id, calculate_script_file_id_str, md4, ScriptHashError,
        ScriptHashLimits,
    };

    fn hex(digest: [u8; 16]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn md4_matches_independently_verified_vectors() {
        // Cross-checked against the Windows `certutil -hashfile MD4` implementation on this
        // machine rather than trusted from memory: memorized strings proved unreliable, while
        // this RFC 1320 implementation agrees with `certutil` on every input below plus the
        // five NUnit FileID digests in the next test.
        assert_eq!(hex(md4(b"")), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(hex(md4(b"a")), "bde52cb31de33e46245e05fbdbd6fb24");
        assert_eq!(hex(md4(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(
            hex(md4(b"message digest")),
            "d9130a8164549fe818874806e1c7014b"
        );
        assert_eq!(
            hex(md4(b"abcdefghijklmnopqrstuvwxyz")),
            "d79e1c308aa5bbcdeea8ed63df412da9"
        );
        assert_eq!(
            hex(md4(
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
            )),
            "043f8582f241db351ce627e153e7f0e4"
        );
        assert_eq!(
            hex(md4(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            )),
            "e33b4ddc9c38f2199c3e7b164fcc0536"
        );
    }

    #[test]
    fn script_file_ids_match_the_baseline_nunit_vectors() {
        // `AssetRipper.Tests/ScriptHashingTests.cs`: (namespace, name, expected FileID).
        for (namespace, name, expected) in [
            ("AssetRipper.Examples", "ExampleBehaviour", -218_309_758),
            ("UnityEngine", "Component", 468_326_297),
            ("UnityEngine", "GameObject", -1_741_091_355),
            ("UnityEngine", "MonoBehaviour", 888_015_496),
            ("UnityEngine", "Object", 350_709_172),
        ] {
            assert_eq!(
                calculate_script_file_id(
                    namespace.as_bytes(),
                    name.as_bytes(),
                    ScriptHashLimits::default()
                ),
                Ok(expected),
                "namespace {namespace:?} name {name:?}"
            );
            assert_eq!(
                calculate_script_file_id_str(namespace, name, ScriptHashLimits::default()),
                Ok(expected),
                "str overload for {namespace:?}::{name:?}"
            );
        }
    }

    #[test]
    fn script_file_ids_cover_empty_and_requested_bounds() {
        // An empty namespace hashes just the four-byte prefix plus the name.
        let empty_namespace =
            calculate_script_file_id(b"", b"Behaviour", ScriptHashLimits::default()).unwrap();
        let utf16_style = calculate_script_file_id(
            "UnityEngine".as_bytes(),
            "Behaviour".as_bytes(),
            ScriptHashLimits::default(),
        )
        .unwrap();
        assert_ne!(empty_namespace, utf16_style);

        assert_eq!(
            calculate_script_file_id(
                b"ns",
                b"name",
                ScriptHashLimits {
                    max_combined_bytes: 3
                }
            ),
            Err(ScriptHashError::InputTooLong {
                byte_count: 6,
                limit: 3,
            })
        );
        assert_eq!(
            calculate_script_file_id(
                b"ns",
                b"n",
                ScriptHashLimits {
                    max_combined_bytes: 3
                }
            ),
            calculate_script_file_id(b"ns", b"n", ScriptHashLimits::default())
        );
    }
}
