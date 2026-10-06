//! `PackedBitVector_Quaternionf` packing and unpacking ported from `PackedQuatVectorExtensions`.
//!
//! Compressed meshes/animations store rotations with the smallest-component trick: a 3-bit
//! `flags` value names the largest-magnitude component and its sign, and the other three
//! components are quantized to 9 or 10 bits. The flag index is `0 = X, 1 = Y, 2 = Z, 3 = W`
//! (`QuaternionExtensions.GetAt`/`SetAt`/`FlipSignAt`), and the reconstructed component is
//! `sqrt(1 - sum of the other squares)` with the sign restored from bit 2.
//!
//! One source quirk is preserved: `Pack` scales with
//! `BitConverter.Int32BitsToSingle((1 << bitSize) - 1)` — the integer mask reinterpreted as
//! float bits, not converted numerically — so packed data does not round-trip in general. The
//! isolated probe in `Temp/opencode/quatprobe` confirmed this (for example an all-zero
//! quaternion packs to the same bytes as the identity quaternion).
//!
//! Reads and writes are bounded by [`PackedBitVectorLimits`].

use std::fmt::{self, Display, Formatter};

use super::packed_bit_vector::PackedBitVectorLimits;

/// Errors returned by quaternion-vector packing and unpacking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackedQuatVectorError {
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

impl Display for PackedQuatVectorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ItemLimitExceeded { requested, limit } => {
                write!(
                    formatter,
                    "quaternion vector has {requested} items, over the limit of {limit}"
                )
            }
            Self::ByteLimitExceeded { requested, limit } => write!(
                formatter,
                "quaternion vector needs {requested} bytes, over the limit of {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(formatter, "could not allocate {requested} for {resource}"),
            Self::TruncatedData { byte_index, length } => write!(
                formatter,
                "quaternion vector data ended at byte {byte_index} of {length}"
            ),
            Self::ItemCountOverflow { requested } => {
                write!(
                    formatter,
                    "quaternion vector item count {requested} overflows uint"
                )
            }
        }
    }
}

impl std::error::Error for PackedQuatVectorError {}

/// A rotation with `System.Numerics.Quaternion` component order (`X, Y, Z, W`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quaternion {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Z`.
    pub z: f32,
    /// `W`.
    pub w: f32,
}

impl Quaternion {
    /// Build from components in `X, Y, Z, W` order.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// `QuaternionExtensions.GetAt`: `0 = X, 1 = Y, 2 = Z, 3 = W`.
    fn component(&self, index: u32) -> f32 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => self.w,
        }
    }

    /// `QuaternionExtensions.SetAt`.
    fn set_component(&mut self, index: u32, value: f32) {
        match index {
            0 => self.x = value,
            1 => self.y = value,
            2 => self.z = value,
            _ => self.w = value,
        }
    }

    /// `QuaternionExtensions.FlipSignAt`.
    fn flip_sign(&mut self, index: u32) {
        self.set_component(index, -self.component(index));
    }

    /// Components in `X, Y, Z, W` order.
    pub const fn components(&self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

/// The `PackedBitVector_Quaternionf` serialized shape.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackedBitVectorQuaternion {
    /// `NumItems`: the number of packed rotations.
    pub num_items: u32,
    /// `Data`: the packed bits.
    pub data: Vec<u8>,
}

impl PackedBitVectorQuaternion {
    /// `Pack(ReadOnlySpan<Quaternion>)`.
    pub fn pack(
        &mut self,
        input: &[Quaternion],
        limits: PackedBitVectorLimits,
    ) -> Result<(), PackedQuatVectorError> {
        if input.len() > limits.max_items {
            return Err(PackedQuatVectorError::ItemLimitExceeded {
                requested: input.len(),
                limit: limits.max_items,
            });
        }
        if input.len() > u32::MAX as usize {
            return Err(PackedQuatVectorError::ItemCountOverflow {
                requested: input.len(),
            });
        }
        let byte_count =
            input
                .len()
                .checked_mul(4)
                .ok_or(PackedQuatVectorError::ByteLimitExceeded {
                    requested: usize::MAX,
                    limit: limits.max_bytes,
                })?;
        if byte_count > limits.max_bytes {
            return Err(PackedQuatVectorError::ByteLimitExceeded {
                requested: byte_count,
                limit: limits.max_bytes,
            });
        }

        self.num_items = input.len() as u32;
        self.data.clear();
        self.data.try_reserve_exact(byte_count).map_err(|_| {
            PackedQuatVectorError::AllocationFailed {
                resource: "packed quaternion vector data",
                requested: byte_count,
            }
        })?;
        self.data.resize(byte_count, 0);

        let mut bit_index = 0usize;
        let mut byte_index = 0usize;
        for quaternion in input {
            let mut flags: u8 = if quaternion.x < 0.0 { 4 } else { 0 };
            let mut max = quaternion.x.abs();
            if quaternion.y.abs() > max {
                max = quaternion.y.abs();
                flags = 1;
                if quaternion.y < 0.0 {
                    flags |= 4;
                }
            }
            if quaternion.z.abs() > max {
                max = quaternion.z.abs();
                flags = 2;
                if quaternion.z < 0.0 {
                    flags |= 4;
                }
            }
            if quaternion.w.abs() > max {
                flags = 3;
                if quaternion.w < 0.0 {
                    flags |= 4;
                }
            }

            let mut bit_offset = 0u32;
            while bit_offset < 3 {
                self.data[byte_index] |= (flags >> bit_offset) << bit_index;
                let num = min_u32(3 - bit_offset, 8 - bit_index as u32);
                bit_index += num as usize;
                bit_offset += num;
                if bit_index == 8 {
                    byte_index += 1;
                    bit_index = 0;
                }
            }

            for j in 0..4u32 {
                if u32::from(flags & 3) != j {
                    let bit_size = if (u32::from(flags & 3) + 1) % 4 == j {
                        9u32
                    } else {
                        10u32
                    };
                    let mut scaled = (f64::from(quaternion.component(j)) + 1.0) * 0.5;
                    scaled = scaled.clamp(0.0, 1.0);
                    // The source reinterprets the integer mask as float bits.
                    let f = f32::from_bits(((1i32 << bit_size) - 1) as u32);
                    let x = ((scaled * f64::from(f)) as f32).to_bits();

                    let mut bit_offset = 0u32;
                    while bit_offset < bit_size {
                        self.data[byte_index] |= ((x >> bit_offset) << bit_index) as u8;
                        let read = min_u32(bit_size - bit_offset, 8 - bit_index as u32);
                        bit_index += read as usize;
                        bit_offset += read;
                        if bit_index == 8 {
                            byte_index += 1;
                            bit_index = 0;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// `Unpack()`.
    pub fn unpack(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<Quaternion>, PackedQuatVectorError> {
        let count = self.num_items as usize;
        if count > limits.max_items {
            return Err(PackedQuatVectorError::ItemLimitExceeded {
                requested: count,
                limit: limits.max_items,
            });
        }

        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(count)
            .map_err(|_| PackedQuatVectorError::AllocationFailed {
                resource: "unpacked quaternion vector",
                requested: count,
            })?;

        let mut bit_index = 0usize;
        let mut byte_index = 0usize;
        for _ in 0..count {
            let mut flags = 0u32;
            let mut bit_offset = 0u32;
            while bit_offset < 3 {
                let byte = self.byte_at(byte_index)?;
                flags |= (u32::from(byte) >> bit_index) << bit_offset;
                let read = min_u32(3 - bit_offset, 8 - bit_index as u32);
                bit_index += read as usize;
                bit_offset += read;
                if bit_index == 8 {
                    byte_index += 1;
                    bit_index = 0;
                }
            }
            flags &= 7;

            let mut sum = 0f64;
            let mut quaternion = Quaternion::default();
            for j in 0..4u32 {
                if (flags & 3) != j {
                    let bit_size = if ((flags & 3) + 1) % 4 == j {
                        9u32
                    } else {
                        10u32
                    };

                    let mut value = 0u32;
                    let mut bit_offset = 0u32;
                    while bit_offset < bit_size {
                        let byte = self.byte_at(byte_index)?;
                        value |= (u32::from(byte) >> bit_index) << bit_offset;
                        let num = min_u32(bit_size - bit_offset, 8 - bit_index as u32);
                        bit_index += num as usize;
                        bit_offset += num;
                        if bit_index == 8 {
                            byte_index += 1;
                            bit_index = 0;
                        }
                    }
                    value &= (1u32 << bit_size) - 1;

                    // The final value's range is [-1.0 : 1.0].
                    let half_max_value = 0.5 * f64::from((1u32 << bit_size) - 1);
                    let component = f64::from(value) / half_max_value - 1.0;
                    quaternion.set_component(j, component as f32);
                    sum += component * component;
                }
            }

            let last_component = flags & 3;
            quaternion.set_component(last_component, (1.0 - sum).sqrt() as f32);
            if flags & 4 != 0 {
                quaternion.flip_sign(last_component);
            }
            buffer.push(quaternion);
        }
        Ok(buffer)
    }

    fn byte_at(&self, byte_index: usize) -> Result<u8, PackedQuatVectorError> {
        self.data
            .get(byte_index)
            .copied()
            .ok_or(PackedQuatVectorError::TruncatedData {
                byte_index,
                length: self.data.len(),
            })
    }
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
    use super::{PackedBitVectorQuaternion, Quaternion};
    use crate::resources::packed_bit_vector::PackedBitVectorLimits;

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

    /// Byte-for-byte differential against the reference `PackedQuatVectorExtensions`.
    /// Expected values come from the isolated probe `Temp/opencode/quatprobe`.
    #[test]
    #[allow(clippy::type_complexity)]
    fn quaternion_vectors_match_the_reference_runtime() {
        let cases: &[(u32, &str, &[u32], &[u32])] = &[
            (
                1,
                "03082080",
                &[0, 0, 0, 1065353216],
                &[989872160, 981475336, 981475336, 1065353168],
            ),
            (
                1,
                "00082080",
                &[1065353216, 0, 0, 0],
                &[1065353168, 989872160, 981475336, 981475336],
            ),
            (
                1,
                "01102080",
                &[0, 1065353216, 0, 0],
                &[981475336, 1065353168, 989872160, 981475336],
            ),
            (
                1,
                "02104080",
                &[0, 0, 1065353216, 0],
                &[981475336, 981475336, 1065353168, 989872160],
            ),
            (
                1,
                "fc0bd0bf",
                &[3204448256, 1056964608, 3204448256, 1056964608],
                &[3204480984, 1056931776, 3204431856, 1056948208],
            ),
            (
                1,
                "cb6866a6",
                &[1036831949, 1045220557, 1050253722, 1063675494],
                &[1036805683, 1045246797, 1050257002, 1064132957],
            ),
            (
                1,
                "00082080",
                &[0, 0, 0, 0],
                &[1065353168, 989872160, 981475336, 981475336],
            ),
            (
                1,
                "37979959",
                &[3184315597, 3192704205, 3197737370, 3211159142],
                &[3184289331, 3192730445, 3197740650, 3211616605],
            ),
        ];
        for (num_items, data, input, expected) in cases {
            let values: Vec<Quaternion> = input
                .chunks_exact(4)
                .map(|chunk| {
                    Quaternion::new(
                        f32::from_bits(chunk[0]),
                        f32::from_bits(chunk[1]),
                        f32::from_bits(chunk[2]),
                        f32::from_bits(chunk[3]),
                    )
                })
                .collect();
            let mut vector = PackedBitVectorQuaternion::default();
            vector.pack(&values, limits()).unwrap();
            assert_eq!(vector.num_items, *num_items);
            assert_eq!(hex(&vector.data), *data);
            let unpacked = vector.unpack(limits()).unwrap();
            let actual: Vec<u32> = unpacked
                .iter()
                .flat_map(|quaternion| quaternion.components().map(f32::to_bits))
                .collect();
            assert_eq!(&actual, expected, "case {data}");
        }
    }

    #[test]
    fn every_component_can_be_the_largest() {
        for index in 0..4u32 {
            let mut values = [0f32; 4];
            values[index as usize] = 1.0;
            let quaternion = Quaternion::new(values[0], values[1], values[2], values[3]);
            let mut vector = PackedBitVectorQuaternion::default();
            vector.pack(&[quaternion], limits()).unwrap();
            let unpacked = vector.unpack(limits()).unwrap();
            let largest = unpacked[0].components()[index as usize];
            assert!(largest > 0.9, "index {index} largest was {largest}");
        }
    }

    #[test]
    fn pack_and_unpack_edges_report_instead_of_panicking() {
        use super::PackedQuatVectorError;
        // Empty vectors round-trip with no bytes.
        let mut vector = PackedBitVectorQuaternion::default();
        vector.pack(&[], limits()).unwrap();
        assert_eq!(vector.num_items, 0);
        assert!(vector.data.is_empty());
        assert!(vector.unpack(limits()).unwrap().is_empty());
        // Item and byte limits gate both directions.
        let tight = PackedBitVectorLimits {
            max_items: 0,
            max_bytes: 0,
        };
        let one = [Quaternion::new(0.0, 0.0, 0.0, 1.0)];
        assert_eq!(
            PackedBitVectorQuaternion::default().pack(&one, tight),
            Err(PackedQuatVectorError::ItemLimitExceeded {
                requested: 1,
                limit: 0,
            })
        );
        let no_bytes = PackedBitVectorLimits {
            max_items: 1,
            max_bytes: 0,
        };
        assert_eq!(
            PackedBitVectorQuaternion::default().pack(&one, no_bytes),
            Err(PackedQuatVectorError::ByteLimitExceeded {
                requested: 4,
                limit: 0,
            })
        );
        let over = PackedBitVectorQuaternion {
            num_items: 1,
            data: Vec::new(),
        };
        assert_eq!(
            over.unpack(tight),
            Err(PackedQuatVectorError::ItemLimitExceeded {
                requested: 1,
                limit: 0,
            })
        );
        // Short buffers fail at the first missing byte.
        let empty = PackedBitVectorQuaternion {
            num_items: 1,
            data: Vec::new(),
        };
        assert_eq!(
            empty.unpack(limits()),
            Err(PackedQuatVectorError::TruncatedData {
                byte_index: 0,
                length: 0,
            })
        );
        let short = PackedBitVectorQuaternion {
            num_items: 1,
            data: vec![0x03],
        };
        assert_eq!(
            short.unpack(limits()),
            Err(PackedQuatVectorError::TruncatedData {
                byte_index: 1,
                length: 1,
            })
        );
        // Out-of-range component indices address W, like the source fallthrough.
        let probe = Quaternion::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(probe.component(7), 4.0);
        let mut flipped = probe;
        flipped.flip_sign(9);
        assert_eq!(flipped.components(), [1.0, 2.0, 3.0, -4.0]);
        let mut set = probe;
        set.set_component(4, 5.0);
        assert_eq!(set.components(), [1.0, 2.0, 3.0, 5.0]);
        // `ItemCountOverflow` (a longer-than-`u32` slice) and `AllocationFailed`
        // cannot be triggered deterministically and stay untested.
    }
}
