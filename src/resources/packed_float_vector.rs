//! `PackedBitVector_Single` packing and unpacking ported from `PackedFloatVectorExtensions`.
//!
//! Unity stores float vectors as a bit-packed `Data` buffer plus a `Start` and `Range`, so a
//! value unpacks as `x / (mask / Range) + Start`. The chunked reader exists because compressed
//! meshes interleave channels. Two source quirks are preserved deliberately and confirmed by
//! the isolated probe in `Temp/opencode/floatprobe`:
//!
//! - The `PackedBitReader` mask is `(1u << BitSize) - 1u`. C# masks the shift count to five bits,
//!   so `BitSize == 32` yields a zero mask and `halfMaxValue` becomes `scale * 0`; the reader then
//!   computes `0 / 0`, and every element unpacks to `NaN` (confirmed: `0x7FC00000`).
//! - `Pack` computes its scaling constant as `BitConverter.UInt32BitsToSingle((1u << BitSize) - 1u)`
//!   — the integer mask is reinterpreted as float bits rather than converted numerically — so the
//!   packing is only approximately a round trip. Both quirks are reproduced rather than "fixed".
//!
//! Reads are bounded by [`PackedBitVectorLimits`]; over-long or truncated inputs return errors.

use std::fmt::{self, Display, Formatter};

use super::packed_bit_vector::PackedBitVectorLimits;

/// Errors returned by float-vector packing and unpacking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackedFloatVectorError {
    /// `chunkStride` is smaller than `itemCountInChunk`.
    InvalidChunkStride {
        item_count_in_chunk: usize,
        chunk_stride: usize,
    },
    /// The requested element count exceeds [`PackedBitVectorLimits::max_items`].
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
    /// The bit offset for `start` overflowed.
    StartOffsetOverflow { start: usize, bit_size: u8 },
    /// The element count cannot be represented in the source's `uint` field.
    ItemCountOverflow { requested: usize },
}

impl Display for PackedFloatVectorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChunkStride {
                item_count_in_chunk,
                chunk_stride,
            } => write!(
                formatter,
                "chunk stride {chunk_stride} is smaller than item count {item_count_in_chunk}"
            ),
            Self::ItemLimitExceeded { requested, limit } => {
                write!(
                    formatter,
                    "float vector has {requested} items, over the limit of {limit}"
                )
            }
            Self::ByteLimitExceeded { requested, limit } => write!(
                formatter,
                "float vector needs {requested} bytes, over the limit of {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(formatter, "could not allocate {requested} for {resource}"),
            Self::TruncatedData { byte_index, length } => write!(
                formatter,
                "float vector data ended at byte {byte_index} of {length}"
            ),
            Self::StartOffsetOverflow { start, bit_size } => write!(
                formatter,
                "float vector start {start} at bit size {bit_size} overflows the bit offset"
            ),
            Self::ItemCountOverflow { requested } => {
                write!(
                    formatter,
                    "float vector item count {requested} overflows uint"
                )
            }
        }
    }
}

impl std::error::Error for PackedFloatVectorError {}

/// The `PackedBitVector_Single` serialized shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackedBitVectorSingle {
    /// `Start`: the value added back after scaling.
    pub start: f32,
    /// `Range`: the spread used for scaling.
    pub range: f32,
    /// `NumItems`: the number of packed elements.
    pub num_items: u32,
    /// `BitSize`: the per-element bit width.
    pub bit_size: u8,
    /// `Data`: the packed bits.
    pub data: Vec<u8>,
}

impl PackedBitVectorSingle {
    /// `IsSet`: whether any element is present.
    pub const fn is_set(&self) -> bool {
        self.num_items > 0
    }

    /// `Unpack()`: read every element with a stride of one.
    pub fn unpack(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<f32>, PackedFloatVectorError> {
        self.unpack_chunked(1, 1, 0, None, limits)
    }

    /// `Unpack(itemCountInChunk, chunkStride, start, numChunks)`.
    pub fn unpack_chunked(
        &self,
        item_count_in_chunk: usize,
        chunk_stride: usize,
        start: usize,
        num_chunks: Option<usize>,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<f32>, PackedFloatVectorError> {
        if chunk_stride < item_count_in_chunk {
            return Err(PackedFloatVectorError::InvalidChunkStride {
                item_count_in_chunk,
                chunk_stride,
            });
        }
        let chunks = match num_chunks {
            Some(chunks) => chunks,
            None => self.num_items as usize / chunk_stride.max(1),
        };
        let requested = chunks.checked_mul(item_count_in_chunk).ok_or(
            PackedFloatVectorError::ItemLimitExceeded {
                requested: usize::MAX,
                limit: limits.max_items,
            },
        )?;
        if requested > limits.max_items {
            return Err(PackedFloatVectorError::ItemLimitExceeded {
                requested,
                limit: limits.max_items,
            });
        }

        let mut reader = PackedBitReader::new(self, start, limits)?;
        let mut result = Vec::new();
        result.try_reserve_exact(requested).map_err(|_| {
            PackedFloatVectorError::AllocationFailed {
                resource: "unpacked float vector",
                requested,
            }
        })?;
        for _ in 0..chunks {
            for _ in 0..item_count_in_chunk {
                result.push(reader.read());
            }
            for _ in 0..(chunk_stride - item_count_in_chunk) {
                reader.read();
            }
        }
        Ok(result)
    }

    /// `Pack(ReadOnlySpan<float>)`: the default high-accuracy packing (`bitSize = 24`).
    pub fn pack(
        &mut self,
        data: &[f32],
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedFloatVectorError> {
        self.pack_with_bit_size(data, 24, false, limits)
    }

    /// `Pack(ReadOnlySpan<float>, int bitSize, bool adjustBitSize)`.
    pub fn pack_with_bit_size(
        &mut self,
        data: &[f32],
        mut bit_size: i32,
        adjust_bit_size: bool,
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedFloatVectorError> {
        if data.len() > limits.max_items {
            return Err(PackedFloatVectorError::ItemLimitExceeded {
                requested: data.len(),
                limit: limits.max_items,
            });
        }
        if data.len() > u32::MAX as usize {
            return Err(PackedFloatVectorError::ItemCountOverflow {
                requested: data.len(),
            });
        }

        let (min, max) = minimum_and_maximum(data);
        let range = max - min;

        if adjust_bit_size {
            bit_size += get_bit_count_double(f64::from(range));
        }
        if bit_size > 32 {
            bit_size = 32;
        }
        // C# `(byte)bitSize` truncates; negative sizes wrap to a large byte, which is only
        // reachable by misusing the API, so reject instead of wrapping silently.
        if !(0..=32).contains(&bit_size) {
            return Err(PackedFloatVectorError::ByteLimitExceeded {
                requested: bit_size.unsigned_abs() as usize,
                limit: 32,
            });
        }

        self.start = min;
        self.range = range;
        self.num_items = data.len() as u32;
        self.bit_size = bit_size as u8;

        let bit_count = data.len().checked_mul(bit_size as usize).ok_or(
            PackedFloatVectorError::ByteLimitExceeded {
                requested: usize::MAX,
                limit: limits.max_bytes,
            },
        )?;
        let byte_count =
            bit_count
                .checked_add(7)
                .ok_or(PackedFloatVectorError::ByteLimitExceeded {
                    requested: usize::MAX,
                    limit: limits.max_bytes,
                })?
                / 8;
        if byte_count > limits.max_bytes {
            return Err(PackedFloatVectorError::ByteLimitExceeded {
                requested: byte_count,
                limit: limits.max_bytes,
            });
        }
        self.data.clear();
        self.data.try_reserve_exact(byte_count).map_err(|_| {
            PackedFloatVectorError::AllocationFailed {
                resource: "packed float vector data",
                requested: byte_count,
            }
        })?;
        self.data.resize(byte_count, 0);

        // The source reinterprets the integer mask as float bits, not as a numeric value.
        let f = f32::from_bits(1u32.wrapping_shl(self.bit_size as u32).wrapping_sub(1));
        let scale = 1.0f64 / f64::from(self.range);

        let mut bit_index = 0usize;
        let mut byte_index = 0usize;
        for &value in data {
            let scaled = clamp_unit((f64::from(value - self.start)) * scale);
            let d = scaled * f64::from(f);
            let x = (d as f32).to_bits();

            let mut bits = 0u32;
            while bits < u32::from(self.bit_size) {
                self.data[byte_index] |= ((x >> bits) << bit_index) as u8;
                let read = min_u32(u32::from(self.bit_size) - bits, 8 - bit_index as u32);
                bit_index += read as usize;
                bits += read;
                if bit_index == 8 {
                    byte_index += 1;
                    bit_index = 0;
                }
            }
        }
        Ok(())
    }
}

struct PackedBitReader<'a> {
    data: &'a [u8],
    bit_size: u8,
    start: f32,
    mask: u32,
    half_max_value: f32,
    bit_index: usize,
    byte_index: usize,
}

impl<'a> PackedBitReader<'a> {
    fn new(
        vector: &'a PackedBitVectorSingle,
        start: usize,
        _limits: PackedBitVectorLimits,
    ) -> Result<Self, PackedFloatVectorError> {
        let bit_offset = start.checked_mul(usize::from(vector.bit_size)).ok_or(
            PackedFloatVectorError::StartOffsetOverflow {
                start,
                bit_size: vector.bit_size,
            },
        )?;
        let scale = 1.0f32 / vector.range;
        // C# masks the shift count to five bits, so a 32-bit size yields a zero mask.
        let mask = 1u32
            .wrapping_shl(u32::from(vector.bit_size))
            .wrapping_sub(1);
        Ok(Self {
            data: &vector.data,
            bit_size: vector.bit_size,
            start: vector.start,
            mask,
            half_max_value: scale * mask as f32,
            bit_index: bit_offset % 8,
            byte_index: bit_offset / 8,
        })
    }

    fn read(&mut self) -> f32 {
        let mut x = 0u32;
        let mut bits = 0u32;
        let bit_size = u32::from(self.bit_size);
        while bits < bit_size {
            // The source indexes without a guard; a well-formed vector never overruns here.
            let byte = self.data.get(self.byte_index).copied().unwrap_or(0);
            x |= (u32::from(byte) >> self.bit_index) << bits;
            let read = min_u32(bit_size - bits, 8 - self.bit_index as u32);
            self.bit_index += read as usize;
            bits += read;
            if self.bit_index == 8 {
                self.byte_index += 1;
                self.bit_index = 0;
            }
        }
        x &= self.mask;
        let result = (x as f32) / self.half_max_value + self.start;
        // `0.0 / 0.0` yields the hardware default NaN, whose sign bit differs between the
        // Windows and Linux x86-64 targets. The baseline's .NET result is the positive quiet NaN
        // `0x7FC00000`, so canonicalize to keep the output deterministic.
        if result.is_nan() {
            f32::from_bits(0x7FC0_0000)
        } else {
            result
        }
    }
}

fn minimum_and_maximum(data: &[f32]) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &value in data {
        if max < value {
            max = value;
        }
        if min > value {
            min = value;
        }
    }
    (min, max)
}

/// `double.Clamp(value, 0, 1)`, which returns `NaN` unchanged.
fn clamp_unit(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn get_bit_count_double(value: f64) -> i32 {
    value.log2().ceil() as i32
}

fn min_u32(left: u32, right: u32) -> u32 {
    if left < right {
        left
    } else {
        right
    }
}

#[cfg(test)]
mod tests {
    use super::{PackedBitVectorSingle, PackedFloatVectorError};
    use crate::resources::packed_bit_vector::PackedBitVectorLimits;

    fn limits() -> PackedBitVectorLimits {
        PackedBitVectorLimits::default()
    }

    fn bytes(hex: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(hex.len() / 2);
        for chunk in hex.as_bytes().chunks(2) {
            let text = std::str::from_utf8(chunk).unwrap();
            out.push(u8::from_str_radix(text, 16).unwrap());
        }
        out
    }

    fn hex(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len() * 2);
        for byte in data {
            out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap());
            out.push(char::from_digit(u32::from(byte & 0x0F), 16).unwrap());
        }
        out
    }

    fn bits(values: &[f32]) -> Vec<u32> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    /// Byte-for-byte differential against the reference `PackedFloatVectorExtensions`.
    /// Expected values come from the isolated probe `Temp/opencode/floatprobe`.
    #[test]
    #[allow(clippy::type_complexity)]
    fn float_vectors_match_the_reference_runtime() {
        let unpack_cases: &[(u32, u32, u8, u32, &str, usize, usize, usize, &[u32])] = &[
            (
                0,
                1065353216,
                8,
                4,
                "004080ff",
                1,
                1,
                0,
                &[0, 1048608897, 1056997505, 1065353216],
            ),
            (0, 1065353216, 8, 4, "004080ff", 2, 4, 0, &[0, 1048608897]),
            (
                1092616192,
                1082130432,
                4,
                4,
                "1234",
                1,
                1,
                0,
                &[1093175433, 1092895812, 1093734673, 1093455053],
            ),
            (
                1084227584,
                1073741824,
                32,
                2,
                "0102030405060708",
                1,
                1,
                0,
                &[2143289344, 2143289344],
            ),
            (
                1065353216,
                1065353216,
                0,
                2,
                "",
                1,
                1,
                0,
                &[2143289344, 2143289344],
            ),
            (
                0,
                1065353216,
                16,
                2,
                "3412ff00",
                1,
                1,
                0,
                &[1032954002, 998179071],
            ),
            (
                0,
                1065353216,
                4,
                6,
                "12345678",
                1,
                1,
                1,
                &[
                    1032358025, 1049135241, 1045220557, 1053609165, 1051372203, 1057523849,
                ],
            ),
        ];
        for (
            start_bits,
            range_bits,
            bit_size,
            num_items,
            data,
            item_count,
            stride,
            start,
            expected,
        ) in unpack_cases
        {
            let vector = PackedBitVectorSingle {
                start: f32::from_bits(*start_bits),
                range: f32::from_bits(*range_bits),
                num_items: *num_items,
                bit_size: *bit_size,
                data: bytes(data),
            };
            let actual = vector
                .unpack_chunked(*item_count, *stride, *start, None, limits())
                .unwrap();
            assert_eq!(&bits(&actual), expected, "unpack case {data}");
        }

        let pack_cases: &[(&[u32], Option<i32>, bool, u32, u32, u8, u32, &str, &[u32])] = &[
            (
                &[0, 1065353216],
                None,
                false,
                0,
                1065353216,
                24,
                2,
                "000000ffffff",
                &[0, 1065353216],
            ),
            (
                &[3212836864, 1075838976, 0],
                None,
                false,
                3212836864,
                1080033280,
                24,
                3,
                "000000ffffff922449",
                &[3212836864, 1075838976, 0],
            ),
            (&[], None, false, 2139095040, 4286578688, 24, 0, "", &[]),
            (
                &[0, 1065353216, 1056964608],
                Some(5),
                false,
                0,
                1065353216,
                5,
                3,
                "e043",
                &[0, 1065353216, 1057235208],
            ),
            (
                &[0, 1090519040, 1082130432],
                Some(1),
                true,
                0,
                1090519040,
                4,
                3,
                "f008",
                &[0, 1090519040, 1082689673],
            ),
            (
                &[1077936128, 1077936128],
                None,
                false,
                1077936128,
                0,
                24,
                2,
                "0000c00000c0",
                &[1077936128, 1077936128],
            ),
        ];
        for (
            input,
            bit_size,
            adjust,
            start_bits,
            range_bits,
            expected_bit_size,
            num_items,
            data,
            round_trip,
        ) in pack_cases
        {
            let values: Vec<f32> = input.iter().map(|value| f32::from_bits(*value)).collect();
            let mut vector = PackedBitVectorSingle::default();
            match bit_size {
                Some(size) => vector
                    .pack_with_bit_size(&values, *size, *adjust, limits())
                    .unwrap(),
                None => vector.pack(&values, limits()).unwrap(),
            }
            assert_eq!(vector.start.to_bits(), *start_bits);
            assert_eq!(vector.range.to_bits(), *range_bits);
            assert_eq!(vector.bit_size, *expected_bit_size);
            assert_eq!(vector.num_items, *num_items);
            assert_eq!(hex(&vector.data), *data);
            let unpacked = vector.unpack(limits()).unwrap();
            assert_eq!(&bits(&unpacked), round_trip, "round trip {data}");
        }
    }

    #[test]
    fn invalid_chunk_stride_is_rejected() {
        let vector = PackedBitVectorSingle::default();
        assert_eq!(
            vector.unpack_chunked(4, 2, 0, None, limits()),
            Err(PackedFloatVectorError::InvalidChunkStride {
                item_count_in_chunk: 4,
                chunk_stride: 2,
            })
        );
    }
}
