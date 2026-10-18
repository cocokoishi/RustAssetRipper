//! Unity script FileIDs and GUIDs replicated from the frozen baseline's `ScriptHashing`.
//!
//! The baseline computes a script FileID as the little-endian `i32` of an MD4 digest over the
//! bytes `['s', 0, 0, 0] + namespace_utf8 + name_utf8`. The MD4 implementation below follows
//! RFC 1320 directly; the byte layout and the expected outputs come from the baseline's
//! `ScriptHashing.CalculateScriptFileID` overloads and its `ScriptHashingTests` vectors.
//!
//! The GUID companions (`CalculateScriptGuid`, `CalculateAssemblyGuid`) hash through
//! `UnityGuid.Md5Hash`, whose byte layout lives in the external `AssetRipper.Primitives`
//! package rather than the frozen snapshot. The layout below follows that package's
//! `UnityGuid.cs` (`Md5Hash` plus `ConvertSystemOrUnityBytes`) and is pinned row-for-row by
//! the committed `script_guid_fixture.txt` runtime differential (shipped
//! `AssetRipper.Export.UnityProjects.dll` plus `AssetRipper.Primitives.dll`, SDK 10.0.401):
//! MD5 over the concatenated UTF-8 bytes, then a field-aware byte flip (the first four bytes
//! reversed, each of the next two pairs swapped) plus a per-byte nibble swap, read back as
//! little-endian words. `ToString` renders each word least-significant nibble first, which is
//! the same convention the existing `format_unity_guid` port asserts.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Bounds applied to the unbounded `stackalloc` in the baseline overloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptHashLimits {
    /// Maximum combined namespace and name bytes hashed for one FileID.
    pub max_combined_bytes: usize,
    /// Maximum concatenated assembly, namespace, and class bytes hashed for one script GUID.
    pub max_guid_combined_bytes: usize,
}

impl Default for ScriptHashLimits {
    fn default() -> Self {
        Self {
            max_combined_bytes: 16 * 1024 * 1024,
            max_guid_combined_bytes: 16 * 1024 * 1024,
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

/// `AssetRipper.Primitives.UnityGuid`: four words identifying an asset in Unity project output.
///
/// Words are stored in Unity layout (the converted buffer read little-endian); `to_byte_array`
/// round-trips them the way the baseline `Write` does.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnityGuid {
    /// First word (`Data0`).
    pub data0: u32,
    /// Second word (`Data1`).
    pub data1: u32,
    /// Third word (`Data2`).
    pub data2: u32,
    /// Fourth word (`Data3`).
    pub data3: u32,
}

impl UnityGuid {
    /// The all-zero GUID (`UnityGuid.Zero`).
    pub const ZERO: Self = Self {
        data0: 0,
        data1: 0,
        data2: 0,
        data3: 0,
    };

    /// `UnityGuid.MissingReference` (`0x0000000DEADBEEF15DEADF00D0000000`), used for fail,
    /// skip, and missing-reference pointers.
    pub const MISSING_REFERENCE: Self = Self {
        data0: 0xD000_0000,
        data1: 0x1FEE_BDAE,
        data2: 0x00FD_AED5,
        data3: 0x0000_000D,
    };

    /// Assemble a GUID from its four words.
    pub const fn new(data0: u32, data1: u32, data2: u32, data3: u32) -> Self {
        Self {
            data0,
            data1,
            data2,
            data3,
        }
    }

    /// `UnityGuid.IsZero`.
    pub const fn is_zero(self) -> bool {
        self.data0 == 0 && self.data1 == 0 && self.data2 == 0 && self.data3 == 0
    }

    /// Words as a tuple, matching the fixture column order.
    pub const fn words(self) -> (u32, u32, u32, u32) {
        (self.data0, self.data1, self.data2, self.data3)
    }

    /// The 16 Unity-layout bytes (`Write`: each word little-endian in word order).
    pub fn to_byte_array(self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&self.data0.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.data1.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.data2.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.data3.to_le_bytes());
        bytes
    }

    /// `UnityGuid.ToString`: the canonical 32-character lowercase Unity GUID string. Each
    /// word's nibbles render least-significant first, the same convention as
    /// `format_unity_guid` over [`UnityGuid::to_byte_array`].
    pub fn to_unity_string(self) -> String {
        let mut value = String::with_capacity(32);
        for word in [self.data0, self.data1, self.data2, self.data3] {
            for shift in 0..8 {
                let digit = ((word >> (shift * 4)) & 0x0f) as usize;
                value.push(b"0123456789abcdef"[digit] as char);
            }
        }
        value
    }
}

impl Display for UnityGuid {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_unity_string())
    }
}

/// `UnityGuid.ConvertSystemOrUnityBytes`: converts MD5 digest bytes to Unity GUID bytes (and
/// back). The first four bytes are reversed, each of the next two byte pairs is swapped, the
/// trailing eight bytes are untouched, and then every byte has its nibbles swapped.
fn convert_system_or_unity_bytes(buffer: &mut [u8; 16]) {
    buffer[0..4].reverse();
    buffer[4..6].reverse();
    buffer[6..8].reverse();
    for byte in buffer.iter_mut() {
        *byte = byte.rotate_left(4);
    }
}

/// MD5 message digest as specified by RFC 1321 (constants and shifts from the reference
/// implementation; test vectors from section A.5).
pub(crate) fn md5(message: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, //
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, //
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, //
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    #[rustfmt::skip]
    const TABLE: [u32; 64] = [
        0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee,
        0xf57c_0faf, 0x4787_c62a, 0xa830_4613, 0xfd46_9501,
        0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be,
        0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821,
        0xf61e_2562, 0xc040_b340, 0x265e_5a51, 0xe9b6_c7aa,
        0xd62f_105d, 0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8,
        0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed,
        0xa9e3_e905, 0xfcef_a3f8, 0x676f_02d9, 0x8d2a_4c8a,
        0xfffa_3942, 0x8771_f681, 0x6d9d_6122, 0xfde5_380c,
        0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70,
        0x289b_7ec6, 0xeaa1_27fa, 0xd4ef_3085, 0x0488_1d05,
        0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665,
        0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039,
        0x655b_59c3, 0x8f0c_cc92, 0xffef_f47d, 0x8584_5dd1,
        0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1,
        0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
    ];

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

        for i in 0..64 {
            let (function, index) = match i {
                0..=15 => (f(b, c, d), i),
                16..=31 => (md5_g(b, c, d), (5 * i + 1) % 16),
                32..=47 => (h(b, c, d), (3 * i + 5) % 16),
                _ => (md5_i(b, c, d), (7 * i) % 16),
            };
            let sum = a
                .wrapping_add(function)
                .wrapping_add(TABLE[i])
                .wrapping_add(x[index]);
            let rotated = sum.rotate_left(SHIFTS[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(rotated);
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
fn md5_g(x: u32, y: u32, z: u32) -> u32 {
    (x & z) | (y & !z)
}

#[inline]
fn md5_i(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

/// `UnityGuid.Md5Hash(input)`: MD5 the input bytes, convert to Unity layout, and read the
/// little-endian words.
pub fn unity_guid_md5_hash(input: &[u8]) -> UnityGuid {
    let mut buffer = md5(input);
    convert_system_or_unity_bytes(&mut buffer);
    UnityGuid::new(
        u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]),
        u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
        u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
        u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]),
    )
}

/// Converts Unity-layout GUID bytes to the `System.Guid.ToByteArray` layout used in PE MVIDs.
/// `UnityGuid`'s explicit `Guid` conversion reuses the same byte swap and nibble swap as
/// `ConvertSystemOrUnityBytes`.
pub fn unity_guid_to_system_bytes(guid: UnityGuid) -> [u8; 16] {
    let mut bytes = guid.to_byte_array();
    convert_system_or_unity_bytes(&mut bytes);
    bytes
}

/// `UnityGuid.Md5Hash(assemblyName, namespace, className)`: MD5 the concatenated UTF-8 bytes.
/// The baseline concatenates caller-supplied spans; lengths are bounded here instead of the
/// unbounded `stackalloc`/GC allocation.
pub fn unity_guid_md5_hash_three(
    assembly_name: &[u8],
    namespace: &[u8],
    class_name: &[u8],
    limits: ScriptHashLimits,
) -> Result<UnityGuid, ScriptHashError> {
    let combined = assembly_name
        .len()
        .checked_add(namespace.len())
        .and_then(|length| length.checked_add(class_name.len()))
        .ok_or(ScriptHashError::InputTooLong {
            byte_count: usize::MAX,
            limit: limits.max_guid_combined_bytes,
        })?;
    if combined > limits.max_guid_combined_bytes {
        return Err(ScriptHashError::InputTooLong {
            byte_count: combined,
            limit: limits.max_guid_combined_bytes,
        });
    }
    let mut input = Vec::new();
    input
        .try_reserve(combined)
        .map_err(|_| ScriptHashError::AllocationFailed {
            resource: "script GUID input",
        })?;
    input.extend_from_slice(assembly_name);
    input.extend_from_slice(namespace);
    input.extend_from_slice(class_name);
    Ok(unity_guid_md5_hash(&input))
}

/// `ScriptHashing.CalculateScriptGuid(assemblyName, namespace, className)` over pre-encoded
/// UTF-8 bytes.
pub fn calculate_script_guid(
    assembly_name: &[u8],
    namespace: &[u8],
    class_name: &[u8],
    limits: ScriptHashLimits,
) -> Result<UnityGuid, ScriptHashError> {
    unity_guid_md5_hash_three(assembly_name, namespace, class_name, limits)
}

/// `ScriptHashing.CalculateScriptGuid(assemblyName, namespace, className)` over `str` values.
///
/// UTF-8 encoding makes this byte-identical to the span overload by construction. The
/// `IMonoScript` overload needs live generated script objects and stays outside.
pub fn calculate_script_guid_str(
    assembly_name: &str,
    namespace: &str,
    class_name: &str,
    limits: ScriptHashLimits,
) -> Result<UnityGuid, ScriptHashError> {
    calculate_script_guid(
        assembly_name.as_bytes(),
        namespace.as_bytes(),
        class_name.as_bytes(),
        limits,
    )
}

/// `SpecialFileNames.AssemblyExtension` (`".dll"`).
pub const ASSEMBLY_EXTENSION: &str = ".dll";

/// `SpecialFileNames.RemoveAssemblyFileExtension`: strip one ordinal `".dll"` suffix.
/// The comparison is case-sensitive, so `".DLL"` is preserved.
pub fn remove_assembly_file_extension(assembly: &str) -> &str {
    assembly
        .strip_suffix(ASSEMBLY_EXTENSION)
        .unwrap_or(assembly)
}

/// `SpecialFileNames.FixAssemblyName`: prefix legacy assembly identifiers with `"Assembly - "`
/// and strip one `".dll"` suffix. The identifier check runs on the raw name first, so
/// `"CSharp.dll"` becomes `"CSharp"` without the prefix.
pub fn fix_assembly_name(assembly: &str) -> String {
    let prefixed = if is_legacy_assembly_identifier(assembly) {
        format!("Assembly - {assembly}")
    } else {
        assembly.to_owned()
    };
    remove_assembly_file_extension(&prefixed).to_owned()
}

/// The legacy UnityScript/Boo identifier set behind `FixAssemblyName`.
fn is_legacy_assembly_identifier(assembly: &str) -> bool {
    matches!(
        assembly,
        "Boo"
            | "Boo - first pass"
            | "CSharp"
            | "CSharp - first pass"
            | "UnityScript"
            | "UnityScript - first pass"
    )
}

/// `ScriptHashing.CalculateAssemblyGuid(assemblyName)`: strip one `".dll"` suffix, then MD5
/// the remaining UTF-8 bytes into a GUID. The `IMonoScript` overload (which fixes the stored
/// assembly name first) needs live generated script objects and stays outside.
pub fn calculate_assembly_guid(assembly_name: &str) -> UnityGuid {
    unity_guid_md5_hash(remove_assembly_file_extension(assembly_name).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_assembly_guid, calculate_script_file_id, calculate_script_file_id_str,
        calculate_script_guid, calculate_script_guid_str, fix_assembly_name, md4, md5,
        remove_assembly_file_extension, unity_guid_md5_hash, ScriptHashError, ScriptHashLimits,
        UnityGuid,
    };

    fn hex(digest: [u8; 16]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn md5_matches_the_rfc1321_test_suite() {
        // Section A.5 of RFC 1321; the implementation follows the reference transform
        // (rounds, shifts, and sine table) rather than any memorized digest.
        for (input, expected) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(hex(md5(input.as_bytes())), expected, "input {input:?}");
        }
    }

    #[test]
    fn unity_guid_byte_conversion_matches_the_package_rule() {
        // The empty-input GUID is also the first fixture row; its words and string are
        // asserted here directly and again row-for-row in the fixture test below.
        let guid = unity_guid_md5_hash(&[]);
        assert_eq!(
            guid.words(),
            (0x4DD1_C89D, 0x2B40_F800, 0x8990_089E, 0xE724_8FCE)
        );
        assert_eq!(guid.to_unity_string(), "d98c1dd4008f04b2e9800998ecf8427e");
        // The rendered string matches the pre-existing `format_unity_guid` convention over
        // the Unity-layout bytes.
        assert_eq!(
            crate::export::unity_yaml::format_unity_guid_words(guid.to_byte_array()),
            guid.to_unity_string()
        );
    }

    #[test]
    fn missing_reference_and_zero_format_as_probed() {
        assert!(UnityGuid::ZERO.is_zero());
        assert!(!UnityGuid::MISSING_REFERENCE.is_zero());
        assert_eq!(
            UnityGuid::ZERO.to_unity_string(),
            "00000000000000000000000000000000"
        );
        assert_eq!(
            UnityGuid::MISSING_REFERENCE.to_unity_string(),
            "0000000deadbeef15deadf00d0000000"
        );
        assert_eq!(
            UnityGuid::MISSING_REFERENCE.to_string(),
            UnityGuid::MISSING_REFERENCE.to_unity_string()
        );
    }

    fn script_guid_fixture_lines() -> Vec<String> {
        include_str!("script_guid_fixture.txt")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn parse_words(columns: &[&str]) -> (u32, u32, u32, u32) {
        let word = |column: &str| {
            u32::from_str_radix(column, 16).unwrap_or_else(|_| panic!("invalid word {column:?}"))
        };
        (
            word(columns[0]),
            word(columns[1]),
            word(columns[2]),
            word(columns[3]),
        )
    }

    #[test]
    fn script_and_assembly_guids_match_the_runtime_fixture() {
        // Every row comes from the shipped `AssetRipper.Export.UnityProjects.dll`
        // (`ScriptHashing.CalculateScriptGuid/CalculateAssemblyGuid`) plus
        // `AssetRipper.Primitives.dll` (`UnityGuid.Md5Hash`), SDK 10.0.401.
        let mut script_cases = 0;
        let mut assembly_cases = 0;
        let mut word_cases = 0;
        for line in script_guid_fixture_lines() {
            let columns: Vec<&str> = line.split('|').collect();
            match columns.as_slice() {
                ["GUID3", assembly, namespace, class, expected] => {
                    let guid = calculate_script_guid(
                        assembly.as_bytes(),
                        namespace.as_bytes(),
                        class.as_bytes(),
                        ScriptHashLimits::default(),
                    )
                    .unwrap();
                    assert_eq!(guid.to_unity_string(), *expected, "{line}");
                    assert_eq!(
                        calculate_script_guid_str(
                            assembly,
                            namespace,
                            class,
                            ScriptHashLimits::default()
                        )
                        .unwrap(),
                        guid,
                        "str overload for {line}"
                    );
                    script_cases += 1;
                }
                ["GUID1", assembly, expected] => {
                    let guid = calculate_assembly_guid(assembly);
                    assert_eq!(guid.to_unity_string(), *expected, "{line}");
                    assembly_cases += 1;
                }
                ["WORDS", tag, rest @ ..] => {
                    let expected = parse_words(rest);
                    let guid = match *tag {
                        "script-lit" => calculate_script_guid_str(
                            "Assembly-CSharp",
                            "",
                            "Player",
                            ScriptHashLimits::default(),
                        )
                        .unwrap(),
                        "script-flags" => calculate_script_guid_str(
                            "Assembly-CSharp",
                            "Game",
                            "Player",
                            ScriptHashLimits::default(),
                        )
                        .unwrap(),
                        "script-mylib" => calculate_script_guid_str(
                            "MyLib",
                            "A",
                            "B",
                            ScriptHashLimits::default(),
                        )
                        .unwrap(),
                        "script-empty" | "asm-empty" => {
                            calculate_script_guid_str("", "", "", ScriptHashLimits::default())
                                .unwrap()
                        }
                        "asm-mylib" | "asm-dll-suffix" => calculate_assembly_guid("MyLib.dll"),
                        "asm-csharp" => calculate_assembly_guid("Assembly-CSharp"),
                        _ => panic!("unknown fixture tag {tag:?}"),
                    };
                    assert_eq!(guid.words(), expected, "{line}");
                    word_cases += 1;
                }
                _ => panic!("malformed fixture line {line:?}"),
            }
        }
        assert_eq!(script_cases, 4, "all GUID3 rows must run");
        assert_eq!(assembly_cases, 4, "all GUID1 rows must run");
        assert_eq!(word_cases, 8, "all WORDS rows must run");
    }

    #[test]
    fn assembly_name_helpers_follow_special_file_names() {
        // `RemoveAssemblyFileExtension` strips one ordinal suffix only.
        assert_eq!(remove_assembly_file_extension("MyLib.dll"), "MyLib");
        assert_eq!(remove_assembly_file_extension("MyLib"), "MyLib");
        assert_eq!(remove_assembly_file_extension("MyLib.DLL"), "MyLib.DLL");
        assert_eq!(remove_assembly_file_extension(""), "");
        // `FixAssemblyName` prefixes legacy identifiers before stripping.
        assert_eq!(fix_assembly_name("CSharp"), "Assembly - CSharp");
        assert_eq!(
            fix_assembly_name("CSharp - first pass"),
            "Assembly - CSharp - first pass"
        );
        assert_eq!(fix_assembly_name("Assembly-CSharp"), "Assembly-CSharp");
        assert_eq!(fix_assembly_name("MyLib.dll"), "MyLib");
        assert_eq!(fix_assembly_name("CSharp.dll"), "CSharp");
        // The `.dll` probe row proves the strip at the GUID layer.
        assert_eq!(
            calculate_assembly_guid("MyLib.dll"),
            calculate_assembly_guid("MyLib")
        );
    }

    #[test]
    fn script_guid_inputs_respect_bounds() {
        assert_eq!(
            calculate_script_guid(
                b"assembly",
                b"namespace",
                b"class",
                ScriptHashLimits {
                    max_guid_combined_bytes: 3,
                    ..Default::default()
                }
            ),
            Err(ScriptHashError::InputTooLong {
                byte_count: 22,
                limit: 3,
            })
        );
        assert_eq!(
            calculate_script_guid(
                b"a",
                b"b",
                b"c",
                ScriptHashLimits {
                    max_guid_combined_bytes: 3,
                    ..Default::default()
                }
            ),
            calculate_script_guid(b"a", b"b", b"c", ScriptHashLimits::default())
        );
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
    fn md4_matches_reference_implementation_vectors() {
        // Digests produced by the frozen baseline's own dependency
        // (`AssetRipper.HashAlgorithms` 1.0.0, `MD4.HashData`) executed with .NET SDK 10.0.401
        // from an isolated probe at `%LOCALAPPDATA%\Temp\opencode\md4diff`. Inputs cover the
        // padding boundary (55 bytes fit one block, 56 need two), a full block, a multi-block
        // patterned input, and a real script FileID preimage.
        for (input, expected) in [
            (vec![b'x'; 55], "92f32bb82c95ad10e8f87ae58ab06807"),
            (vec![b'x'; 56], "374d5f08103b7092c83b4626ebceffab"),
            (vec![b'y'; 64], "7b31cd9bcbd9a24b501d527e91129d61"),
            (
                (0..1000)
                    .map(|i| ((i * 31 + 7) & 0xFF) as u8)
                    .collect::<Vec<u8>>(),
                "a1449ce09255ece450ace1589258503a",
            ),
        ] {
            assert_eq!(hex(md4(&input)), expected, "length {}", input.len());
        }
        let preimage = b"s\0\0\0UnityEngineComponent".to_vec();
        assert_eq!(preimage.len(), 24);
        assert_eq!(hex(md4(&preimage)), "9917ea1b5bd0dcfcaf5d6c0d67808695");
        // The preimage digest's little-endian head is the NUnit FileID for
        // ("UnityEngine", "Component") asserted in the next test.
        assert_eq!(
            calculate_script_file_id(b"UnityEngine", b"Component", ScriptHashLimits::default()),
            Ok(468_326_297)
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
                    max_combined_bytes: 3,
                    ..Default::default()
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
                    max_combined_bytes: 3,
                    ..Default::default()
                }
            ),
            calculate_script_file_id(b"ns", b"n", ScriptHashLimits::default())
        );
    }
}
