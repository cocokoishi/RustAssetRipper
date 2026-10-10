use super::*;
pub(super) fn unique_temp_file_path(extension: &str) -> PathBuf {
    crate::test_support::unique_test_path("mount", extension)
}

pub(super) fn source_multifile_path_fields(prefix: &str) -> Vec<&'static str> {
    include_str!("../../io/multifile_path_fixture.txt")
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing source MultiFile fixture row {prefix:?}"))
        .split('|')
        .collect()
}

pub(super) fn decode_hex_fixture(value: &str) -> Vec<u8> {
    assert!(value.len().is_multiple_of(2));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap();
            let low = (pair[1] as char).to_digit(16).unwrap();
            u8::try_from((high << 4) | low).unwrap()
        })
        .collect()
}

pub(super) fn webdata_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
    assert!(entries.iter().all(|(name, _)| name.len() < 1024 * 1024));
    let header_size = 20
        + entries
            .iter()
            .map(|(name, _)| 12 + name.len())
            .sum::<usize>();
    let mut input = b"UnityWebData1.0\0".to_vec();
    input.extend_from_slice(&(header_size as i32).to_le_bytes());

    let mut data_offset = header_size;
    for (name, payload) in entries {
        input.extend_from_slice(&(data_offset as i32).to_le_bytes());
        input.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        input.extend_from_slice(&(name.len() as u32).to_le_bytes());
        input.extend_from_slice(name.as_bytes());
        data_offset += payload.len();
    }
    for (_, payload) in entries {
        input.extend_from_slice(payload);
    }
    input
}

pub(super) fn serialized_file_header_fixture() -> Vec<u8> {
    // Scheme detection only needs a valid legacy header envelope; this deliberately minimal
    // payload is not passed to the SerializedFile parser.
    let mut bytes = vec![0; 29];
    bytes[0..4].copy_from_slice(&13_i32.to_be_bytes());
    bytes[4..8].copy_from_slice(&29_u32.to_be_bytes());
    bytes[8..12].copy_from_slice(&6_i32.to_be_bytes());
    bytes
}

pub(super) fn write_bits(bytes: &mut Vec<u8>, bit_position: &mut usize, value: u32, count: usize) {
    for bit in 0..count {
        let byte_index = *bit_position / 8;
        if bytes.len() <= byte_index {
            bytes.resize(byte_index + 1, 0);
        }
        if value & (1 << bit) != 0 {
            bytes[byte_index] |= 1 << (*bit_position % 8);
        }
        *bit_position += 1;
    }
}

pub(super) fn unity_marked_brotli_fixture(output: &[u8], window_bits: u8) -> Vec<u8> {
    let marker = b"UnityWeb Compressed Content (brotli)";
    let mut bytes = Vec::new();
    let mut bit_position = 0;

    // RFC 7932 stream header, followed by an empty metadata meta-block.
    let (window_code, window_code_bits) = match window_bits {
        10 => (33, 7),
        11 => (49, 7),
        12 => (65, 7),
        13 => (81, 7),
        14 => (97, 7),
        15 => (113, 7),
        16 => (0, 1),
        17 => (1, 7),
        18 => (3, 4),
        19 => (5, 4),
        20 => (7, 4),
        21 => (9, 4),
        22 => (11, 4),
        23 => (13, 4),
        24 => (15, 4),
        _ => panic!("invalid fixture WBITS"),
    };
    write_bits(&mut bytes, &mut bit_position, window_code, window_code_bits);
    write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST
    write_bits(&mut bytes, &mut bit_position, 3, 2); // MNIBBLES=0
    write_bits(&mut bytes, &mut bit_position, 0, 1); // reserved
    write_bits(&mut bytes, &mut bit_position, 1, 2); // MSKIPBYTES=1
    write_bits(&mut bytes, &mut bit_position, marker.len() as u32 - 1, 8);
    while bit_position & 7 != 0 {
        write_bits(&mut bytes, &mut bit_position, 0, 1);
    }
    bytes.extend_from_slice(marker);
    bit_position += marker.len() * 8;

    // Non-final uncompressed meta-block containing the requested payload.
    let length_minus_one = output.len().checked_sub(1).unwrap() as u32;
    write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST=0
    write_bits(&mut bytes, &mut bit_position, 0, 2); // MNIBBLES=4
    write_bits(&mut bytes, &mut bit_position, length_minus_one, 16);
    write_bits(&mut bytes, &mut bit_position, 1, 1); // ISUNCOMPRESSED
    while bit_position & 7 != 0 {
        write_bits(&mut bytes, &mut bit_position, 0, 1);
    }
    bytes.extend_from_slice(output);
    bit_position += output.len() * 8;

    // Final empty meta-block.
    write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLAST
    write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLASTEMPTY
    while bit_position & 7 != 0 {
        write_bits(&mut bytes, &mut bit_position, 0, 1);
    }
    bytes
}
