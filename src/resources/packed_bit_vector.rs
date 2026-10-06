//! `PackedBitVector_Int32` packing and unpacking ported from `PackedIntVectorExtensions`.
//!
//! This is the first resource-decoding primitive: Unity stores packed integer, float, and
//! quaternion vectors as a bit-packed `Data` buffer with an element count and a per-element bit
//! size, and mesh/animation decompression reads them back. The pack and unpack loops, the
//! `ceil(log2(max + 1))` bit-size rule, and the `0xFFFFFFFF` special case come straight from the
//! frozen snapshot.
//!
//! One source quirk is preserved deliberately: the final mask is computed as
//! `(1 << BitSize) - 1`, and C# masks shift counts to five bits, so `BitSize == 32` produces
//! `(1 << 0) - 1 == 0`. Every value therefore unpacks to zero whenever `BitSize` is 32. The
//! isolated reference-runtime probe in `Temp/opencode/packedprobe` confirmed this
//! (`unpackUInts`/`unpackInts`/`unpackUShorts` all return zeros for the 32-bit cases); the Rust
//! code reproduces it with `wrapping_shl` instead of "fixing" it.
//!
//! Allocation is bounded by [`PackedBitVectorLimits`]; malformed inputs yield contextual errors
//! rather than panics or silent truncation.

use std::fmt::{self, Display, Formatter};

/// Bounds for packing and unpacking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackedBitVectorLimits {
    /// Maximum element count for a single vector.
    pub max_items: usize,
    /// Maximum packed byte length for a single vector.
    pub max_bytes: usize,
}

impl Default for PackedBitVectorLimits {
    fn default() -> Self {
        Self {
            max_items: 1 << 28,
            max_bytes: 1 << 30,
        }
    }
}

/// Errors returned by packing and unpacking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackedBitVectorError {
    /// The element count exceeds [`PackedBitVectorLimits::max_items`].
    ItemLimitExceeded { requested: usize, limit: usize },
    /// The packed byte length exceeds [`PackedBitVectorLimits::max_bytes`].
    ByteLimitExceeded { requested: usize, limit: usize },
    /// A bounded allocation could not be reserved.
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    /// The `Data` buffer ended before the requested element count was decoded.
    TruncatedData { byte_index: usize, length: usize },
    /// The element count cannot be represented in the source's `uint` field.
    ItemCountOverflow { requested: usize },
}

impl Display for PackedBitVectorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ItemLimitExceeded { requested, limit } => {
                write!(
                    formatter,
                    "packed vector has {requested} items, over the limit of {limit}"
                )
            }
            Self::ByteLimitExceeded { requested, limit } => write!(
                formatter,
                "packed vector needs {requested} bytes, over the limit of {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(formatter, "could not allocate {requested} for {resource}"),
            Self::TruncatedData { byte_index, length } => write!(
                formatter,
                "packed vector data ended at byte {byte_index} of {length}"
            ),
            Self::ItemCountOverflow { requested } => {
                write!(
                    formatter,
                    "packed vector item count {requested} overflows uint"
                )
            }
        }
    }
}

impl std::error::Error for PackedBitVectorError {}

/// The `PackedBitVector_Int32` serialized shape.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackedBitVector {
    /// `NumItems`: the number of packed elements.
    pub num_items: u32,
    /// `BitSize`: the per-element bit width.
    pub bit_size: u8,
    /// `Data`: the packed bits.
    pub data: Vec<u8>,
}

impl PackedBitVector {
    /// `IsSet`: whether any element is present.
    pub const fn is_set(&self) -> bool {
        self.num_items > 0
    }

    /// `CopyValuesFrom`: copy count, data, and bit size.
    pub fn copy_values_from(&mut self, source: &Self) {
        self.num_items = source.num_items;
        self.data = source.data.clone();
        self.bit_size = source.bit_size;
    }

    /// `PackInts`: pack the bit patterns of signed integers.
    pub fn pack_ints(
        &mut self,
        data: &[i32],
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedBitVectorError> {
        let values: Vec<u32> = data.iter().map(|value| *value as u32).collect();
        self.pack_uints(&values, limits)
    }

    /// `PackUInts`: pack unsigned integers.
    pub fn pack_uints(
        &mut self,
        data: &[u32],
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedBitVectorError> {
        self.pack_values(data, limits)
    }

    /// `PackUShorts`: pack unsigned 16-bit integers.
    pub fn pack_ushorts(
        &mut self,
        data: &[u16],
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedBitVectorError> {
        let values: Vec<u32> = data.iter().map(|value| u32::from(*value)).collect();
        self.pack_values(&values, limits)
    }

    fn pack_values(
        &mut self,
        data: &[u32],
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedBitVectorError> {
        if data.len() > limits.max_items {
            return Err(PackedBitVectorError::ItemLimitExceeded {
                requested: data.len(),
                limit: limits.max_items,
            });
        }
        if data.len() > u32::MAX as usize {
            return Err(PackedBitVectorError::ItemCountOverflow {
                requested: data.len(),
            });
        }

        let max_data_value = data.iter().copied().max().unwrap_or(0);
        let bit_size = if max_data_value == u32::MAX {
            32
        } else {
            get_bit_count(max_data_value.wrapping_add(1))
        };
        let bit_count = data.len().checked_mul(bit_size as usize).ok_or(
            PackedBitVectorError::ByteLimitExceeded {
                requested: usize::MAX,
                limit: limits.max_bytes,
            },
        )?;
        let byte_count =
            bit_count
                .checked_add(7)
                .ok_or(PackedBitVectorError::ByteLimitExceeded {
                    requested: usize::MAX,
                    limit: limits.max_bytes,
                })?
                / 8;
        if byte_count > limits.max_bytes {
            return Err(PackedBitVectorError::ByteLimitExceeded {
                requested: byte_count,
                limit: limits.max_bytes,
            });
        }

        self.num_items = data.len() as u32;
        self.bit_size = bit_size;
        self.data.clear();
        self.data.try_reserve_exact(byte_count).map_err(|_| {
            PackedBitVectorError::AllocationFailed {
                resource: "packed bit vector data",
                requested: byte_count,
            }
        })?;
        self.data.resize(byte_count, 0);

        let mut bit_index = 0usize;
        let mut byte_index = 0usize;
        for &value in data {
            let mut bit_offset = 0u32;
            while bit_offset < u32::from(bit_size) {
                self.data[byte_index] |= ((value >> bit_offset) << bit_index) as u8;
                let read = min_u32(u32::from(bit_size) - bit_offset, 8 - bit_index as u32);
                bit_index += read as usize;
                bit_offset += read;
                if bit_index == 8 {
                    byte_index += 1;
                    bit_index = 0;
                }
            }
        }
        Ok(())
    }

    /// `UnpackInts`: unpack the elements as signed integers.
    pub fn unpack_ints(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<i32>, PackedBitVectorError> {
        let values = self.unpack_raw(limits)?;
        Ok(values.into_iter().map(|value| value as i32).collect())
    }

    /// `UnpackUInts`: unpack the elements as unsigned integers.
    pub fn unpack_uints(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<u32>, PackedBitVectorError> {
        self.unpack_raw(limits)
    }

    /// `UnpackUShorts`: unpack the elements as unsigned 16-bit integers.
    pub fn unpack_ushorts(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<u16>, PackedBitVectorError> {
        let values = self.unpack_raw(limits)?;
        Ok(values.into_iter().map(|value| value as u16).collect())
    }

    fn unpack_raw(&self, limits: PackedBitVectorLimits) -> Result<Vec<u32>, PackedBitVectorError> {
        let count = self.num_items as usize;
        if count > limits.max_items {
            return Err(PackedBitVectorError::ItemLimitExceeded {
                requested: count,
                limit: limits.max_items,
            });
        }
        // C# masks the shift count to five bits, so `1 << 32` is `1` and the mask becomes zero.
        let mask = 1u32.wrapping_shl(u32::from(self.bit_size)).wrapping_sub(1);

        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(count)
            .map_err(|_| PackedBitVectorError::AllocationFailed {
                resource: "unpacked bit vector",
                requested: count,
            })?;

        let mut bit_index = 0usize;
        let mut byte_index = 0usize;
        for _ in 0..count {
            let mut bit_offset = 0u32;
            let mut value = 0u32;
            while bit_offset < u32::from(self.bit_size) {
                let byte =
                    *self
                        .data
                        .get(byte_index)
                        .ok_or(PackedBitVectorError::TruncatedData {
                            byte_index,
                            length: self.data.len(),
                        })?;
                value |= (u32::from(byte) >> bit_index) << bit_offset;
                let read = min_u32(u32::from(self.bit_size) - bit_offset, 8 - bit_index as u32);
                bit_index += read as usize;
                bit_offset += read;
                if bit_index == 8 {
                    byte_index += 1;
                    bit_index = 0;
                }
            }
            buffer.push(value & mask);
        }
        Ok(buffer)
    }
}

fn min_u32(left: u32, right: u32) -> u32 {
    if left < right {
        left
    } else {
        right
    }
}

/// `GetBitCount`: the number of bits needed to represent `value`, `ceil(log2(value))`.
///
/// The source uses `Math.Log2`; the differential test pins the floating-point boundaries.
pub fn get_bit_count(value: u32) -> u8 {
    (value as f64).log2().ceil() as u8
}

#[cfg(test)]
mod tests {
    use super::{get_bit_count, PackedBitVector, PackedBitVectorLimits};

    fn limits() -> PackedBitVectorLimits {
        PackedBitVectorLimits::default()
    }

    fn hex(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len() * 2);
        for byte in data {
            out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap());
            out.push(char::from_digit(u32::from(byte & 0x0F), 16).unwrap());
        }
        out
    }

    /// Byte-for-byte differential against the reference `PackedIntVectorExtensions`.
    /// Expected values come from the isolated probe `Temp/opencode/packedprobe`
    /// (`packed_int_probe.json`), which packed through the real `PackedBitVector_Int32`.
    #[test]
    #[allow(clippy::type_complexity)] // Differential tables carry input plus three expected views.
    fn packed_vectors_match_the_reference_runtime() {
        let uint_cases: &[(&[u32], u8, &str, &[u32], &[i32], &[u16])] = &[
            (
                &[0, 1, 2, 3],
                2,
                "e4",
                &[0, 1, 2, 3],
                &[0, 1, 2, 3],
                &[0, 1, 2, 3],
            ),
            (
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                4,
                "10325476980a",
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            ),
            (
                &[4294967295, 0, 1, 2147483648],
                32,
                "ffffffff000000000100000000000080",
                &[0, 0, 0, 0],
                &[0, 0, 0, 0],
                &[0, 0, 0, 0],
            ),
            (&[0], 0, "", &[0], &[0], &[0]),
            (&[0, 0, 0], 0, "", &[0, 0, 0], &[0, 0, 0], &[0, 0, 0]),
            (
                &[65535, 65536, 7],
                17,
                "ffff00001e0000",
                &[65535, 65536, 7],
                &[65535, 65536, 7],
                &[65535, 0, 7],
            ),
            (
                &[31, 32, 33, 63, 64, 100],
                7,
                "1f50e8072403",
                &[31, 32, 33, 63, 64, 100],
                &[31, 32, 33, 63, 64, 100],
                &[31, 32, 33, 63, 64, 100],
            ),
        ];
        for (input, bit_size, data, uints, ints, ushorts) in uint_cases {
            let mut vector = PackedBitVector::default();
            vector.pack_uints(input, limits()).unwrap();
            assert_eq!(vector.num_items, input.len() as u32);
            assert_eq!(vector.bit_size, *bit_size);
            assert_eq!(hex(&vector.data), *data);
            assert_eq!(vector.unpack_uints(limits()).unwrap(), *uints);
            assert_eq!(vector.unpack_ints(limits()).unwrap(), *ints);
            assert_eq!(vector.unpack_ushorts(limits()).unwrap(), *ushorts);
        }

        let int_cases: &[(&[i32], u8, &str, &[u32], &[i32], &[u16])] = &[
            (
                &[0, 1, -1, 2],
                32,
                "0000000001000000ffffffff02000000",
                &[0, 0, 0, 0],
                &[0, 0, 0, 0],
                &[0, 0, 0, 0],
            ),
            (
                &[-1, -2, -3],
                32,
                "fffffffffefffffffdffffff",
                &[0, 0, 0],
                &[0, 0, 0],
                &[0, 0, 0],
            ),
            (
                &[7, 8, 9, 10, 11, 12, 13],
                4,
                "87a9cb0d",
                &[7, 8, 9, 10, 11, 12, 13],
                &[7, 8, 9, 10, 11, 12, 13],
                &[7, 8, 9, 10, 11, 12, 13],
            ),
        ];
        for (input, bit_size, data, uints, ints, ushorts) in int_cases {
            let mut vector = PackedBitVector::default();
            vector.pack_ints(input, limits()).unwrap();
            assert_eq!(vector.num_items, input.len() as u32);
            assert_eq!(vector.bit_size, *bit_size);
            assert_eq!(hex(&vector.data), *data);
            assert_eq!(vector.unpack_uints(limits()).unwrap(), *uints);
            assert_eq!(vector.unpack_ints(limits()).unwrap(), *ints);
            assert_eq!(vector.unpack_ushorts(limits()).unwrap(), *ushorts);
        }

        let ushort_cases: &[(&[u16], u8, &str, &[u32], &[i32], &[u16])] = &[
            (
                &[0, 1, 2, 3],
                2,
                "e4",
                &[0, 1, 2, 3],
                &[0, 1, 2, 3],
                &[0, 1, 2, 3],
            ),
            (
                &[65535, 0, 32768, 1],
                16,
                "ffff000000800100",
                &[65535, 0, 32768, 1],
                &[65535, 0, 32768, 1],
                &[65535, 0, 32768, 1],
            ),
            (
                &[100, 200, 300, 400, 500],
                9,
                "6490b1844c1f",
                &[100, 200, 300, 400, 500],
                &[100, 200, 300, 400, 500],
                &[100, 200, 300, 400, 500],
            ),
        ];
        for (input, bit_size, data, uints, ints, ushorts) in ushort_cases {
            let mut vector = PackedBitVector::default();
            vector.pack_ushorts(input, limits()).unwrap();
            assert_eq!(vector.num_items, input.len() as u32);
            assert_eq!(vector.bit_size, *bit_size);
            assert_eq!(hex(&vector.data), *data);
            assert_eq!(vector.unpack_uints(limits()).unwrap(), *uints);
            assert_eq!(vector.unpack_ints(limits()).unwrap(), *ints);
            assert_eq!(vector.unpack_ushorts(limits()).unwrap(), *ushorts);
        }
    }

    /// `ceil(log2(max + 1))` boundaries captured from the reference runtime.
    #[test]
    fn bit_sizes_match_the_reference_runtime() {
        let cases: &[(u32, u8)] = &[
            (1, 1),
            (2, 2),
            (3, 2),
            (4, 3),
            (5, 3),
            (7, 3),
            (8, 4),
            (9, 4),
            (15, 4),
            (16, 5),
            (17, 5),
            (31, 5),
            (32, 6),
            (33, 6),
            (63, 6),
            (64, 7),
            (65, 7),
            (127, 7),
            (128, 8),
            (129, 8),
            (255, 8),
            (256, 9),
            (257, 9),
            (511, 9),
            (512, 10),
            (513, 10),
            (1023, 10),
            (1024, 11),
            (1025, 11),
            (2047, 11),
            (2048, 12),
            (2049, 12),
            (4095, 12),
            (4096, 13),
            (4097, 13),
            (8191, 13),
            (8192, 14),
            (8193, 14),
            (16383, 14),
            (16384, 15),
            (16385, 15),
            (32767, 15),
            (32768, 16),
            (32769, 16),
            (65535, 16),
            (65536, 17),
            (65537, 17),
            (131071, 17),
            (131072, 18),
            (131073, 18),
            (262143, 18),
            (262144, 19),
            (262145, 19),
            (524287, 19),
            (524288, 20),
            (524289, 20),
            (1048575, 20),
            (1048576, 21),
            (1048577, 21),
            (2097151, 21),
            (2097152, 22),
            (2097153, 22),
            (4194303, 22),
            (4194304, 23),
            (4194305, 23),
            (8388607, 23),
            (8388608, 24),
            (8388609, 24),
            (16777215, 24),
            (16777216, 25),
            (16777217, 25),
            (33554431, 25),
            (33554432, 26),
            (33554433, 26),
            (67108863, 26),
            (67108864, 27),
            (67108865, 27),
            (134217727, 27),
            (134217728, 28),
            (134217729, 28),
            (268435455, 28),
            (268435456, 29),
            (268435457, 29),
            (536870911, 29),
            (536870912, 30),
            (536870913, 30),
            (1073741823, 30),
            (1073741824, 31),
            (1073741825, 31),
            (2147483647, 31),
            (2147483648, 32),
            (2147483649, 32),
            (4294967293, 32),
            (4294967294, 32),
            (4294967295, 32),
            (123456789, 27),
            (987654321, 30),
            (424242424, 29),
        ];
        for (value, expected) in cases {
            let mut vector = PackedBitVector::default();
            vector.pack_uints(&[*value], limits()).unwrap();
            assert_eq!(vector.bit_size, *expected, "value {value}");
        }
        assert_eq!(get_bit_count(1), 0);
        assert_eq!(get_bit_count(2), 1);
    }

    /// Mirrors the baseline `IntVectorTests` guarantees: full-width values round-trip
    /// exactly, and the signed unpack stays consistent with the unsigned unpack.
    /// (Negative inputs hit the 32-bit zero quirk pinned in the differential above,
    /// so only non-negative integers round-trip.)
    #[test]
    fn round_trips_match_the_baseline_test_guarantees() {
        // Every bit width 1..=31 plus arbitrary values, all below `u32::MAX`.
        let mut uints: Vec<u32> = (1..=31).map(|k| (1u32 << k) - 1).collect();
        uints.extend([0, 123_456_789, 987_654_321, 2_147_483_647]);
        let mut packed = PackedBitVector::default();
        packed.pack_uints(&uints, limits()).unwrap();
        assert_eq!(packed.unpack_uints(limits()).unwrap(), uints);
        // The old signed unpack agrees once reinterpreted as unsigned.
        let ints = packed.unpack_ints(limits()).unwrap();
        let back: Vec<u32> = ints.into_iter().map(|value| value as u32).collect();
        assert_eq!(back, uints);

        let ushorts: Vec<u16> = vec![0, 1, 255, 256, 1000, 32767, 32768, 65534, 65535];
        let mut packed = PackedBitVector::default();
        packed.pack_ushorts(&ushorts, limits()).unwrap();
        assert_eq!(packed.unpack_ushorts(limits()).unwrap(), ushorts);

        let ints: Vec<i32> = vec![0, 1, 127, 32767, 65535, 123_456_789, i32::MAX];
        let mut packed = PackedBitVector::default();
        packed.pack_ints(&ints, limits()).unwrap();
        assert_eq!(packed.unpack_ints(limits()).unwrap(), ints);
    }
}
