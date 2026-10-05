//! `ByteArrayExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/ByteArrayExtensions.cs` `SwapBytes`, which
//! byte-swaps a buffer in 2- or 4-byte groups (reading big-endian and writing little-endian).
//!
//! Differential fixture: `byte_array_fixture.txt` (probe `Temp/opencode/miscprobe`).

/// A failure while swapping bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteArrayError {
    /// The reference only supports 2- and 4-byte groups.
    UnsupportedSize {
        /// The requested group size.
        size: usize,
    },
    /// The buffer length is not a multiple of the group size; the reference's reader overruns and
    /// throws.
    NotMultipleOfSize {
        /// The requested group size.
        size: usize,
        /// The buffer length.
        length: usize,
    },
}

impl core::fmt::Display for ByteArrayError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedSize { size } => {
                write!(formatter, "byte swap size {size} is not supported")
            }
            Self::NotMultipleOfSize { size, length } => write!(
                formatter,
                "byte swap length {length} is not a multiple of {size}"
            ),
        }
    }
}

impl std::error::Error for ByteArrayError {}

/// `ByteArrayExtensions.SwapBytes`.
pub fn swap_bytes(input: &[u8], size: usize) -> Result<Vec<u8>, ByteArrayError> {
    if size != 2 && size != 4 {
        return Err(ByteArrayError::UnsupportedSize { size });
    }
    if !input.len().is_multiple_of(size) {
        return Err(ByteArrayError::NotMultipleOfSize {
            size,
            length: input.len(),
        });
    }
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks_exact(size) {
        output.extend(chunk.iter().rev());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `ByteArrayExtensions` (probe
    /// `Temp/opencode/miscprobe`).
    #[test]
    fn swap_bytes_matches_the_reference_runtime() {
        let fixture = include_str!("byte_array_fixture.txt");
        let input: Vec<u8> = (1..=8).collect();
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "SWAP", "{line}");
            let size: usize = parts[1].parse().unwrap();
            let result = swap_bytes(&input, size);
            if parts[3] == "1" {
                assert_eq!(
                    result,
                    Err(ByteArrayError::UnsupportedSize { size }),
                    "{line}"
                );
            } else {
                let expected: String = result
                    .unwrap()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                assert_eq!(expected, parts[2], "{line}");
            }
            rows += 1;
        }
        assert_eq!(rows, 3);
        assert_eq!(
            swap_bytes(&[1, 2, 3], 2),
            Err(ByteArrayError::NotMultipleOfSize { size: 2, length: 3 })
        );
    }
}
