pub(crate) mod crn_consts;
pub(crate) mod crn_decomp;
pub(crate) mod crn_static_huffman_data_model;
pub(crate) mod crn_symbol_codec;
pub(crate) mod crn_unpacker;
pub(crate) mod crn_utils;
use super::crnlib::{CrnFormat, CrnTextureInfo};
use crate::bcn;
use core::cmp::max;
extern crate alloc;

struct CrunchDecodeHandler {
    format: CrnFormat,
    dxt_data: alloc::vec::Vec<u8>,
    face_data_size: usize,
}

fn crunch_unpack_level(
    data: &[u8],
    data_size: u32,
    level_index: u32,
    expected_faces: usize,
) -> Result<CrunchDecodeHandler, &'static str> {
    let mut tex_info: CrnTextureInfo = CrnTextureInfo::default();
    if !tex_info.crnd_get_texture_info(data, data_size) {
        return Err("Invalid crunch texture encoding.");
    }
    if expected_faces == 0 || usize::try_from(tex_info.faces).ok() != Some(expected_faces) {
        return Err("Crunch face count does not match the requested layer count.");
    }
    let mut p_context: crn_unpacker::CrnUnpacker<'_> =
        crn_decomp::crnd_unpack_begin(data, data_size)?;
    let width = max(1, tex_info.width >> level_index);
    let height = max(1, tex_info.height >> level_index);
    let blocks_x: u32 = max(1, (width + 3) >> 2);
    let blocks_y: u32 = max(1, (height + 3) >> 2);
    let row_pitch: u32 = blocks_x * crn_decomp::crnd_get_bytes_per_dxt_block(&mut tex_info.format)?;
    let total_face_size: u32 = row_pitch * blocks_y;
    let total_size = total_face_size
        .checked_mul(tex_info.faces)
        .ok_or("Crunch face data size overflow.")?;
    match p_context.crnd_unpack_level(total_size, row_pitch, level_index) {
        Ok(res) => Ok(CrunchDecodeHandler {
            format: tex_info.format,
            dxt_data: res,
            face_data_size: usize::try_from(total_face_size)
                .map_err(|_| "Crunch face data size is not addressable.")?,
        }),
        Err(err) => Err(err),
    }
}

pub fn decode_crunch(
    data: &[u8],
    width: usize,
    height: usize,
    image: &mut [u32],
) -> Result<(), &'static str> {
    decode_crunch_faces(data, width, height, 1, image)
}

/// Decodes every CRN face into adjacent `width * height` pixel layers.
pub fn decode_crunch_faces(
    data: &[u8],
    width: usize,
    height: usize,
    faces: usize,
    image: &mut [u32],
) -> Result<(), &'static str> {
    if width == 0 || height == 0 || faces == 0 {
        return Err("Crunch texture dimensions and face count must be non-zero.");
    }
    let pixels_per_face = width
        .checked_mul(height)
        .ok_or("Crunch pixel count overflow.")?;
    let required_pixels = pixels_per_face
        .checked_mul(faces)
        .ok_or("Crunch face pixel count overflow.")?;
    if image.len() < required_pixels {
        return Err("Image buffer is too small for all Crunch faces.");
    }
    let data_size = u32::try_from(data.len()).map_err(|_| "Crunch input is too large.")?;
    let handler = crunch_unpack_level(data, data_size, 0, faces)?;
    decode_faces(
        &handler,
        width,
        height,
        pixels_per_face,
        &mut image[..required_pixels],
    )
}

fn decode_faces(
    handler: &CrunchDecodeHandler,
    width: usize,
    height: usize,
    pixels_per_face: usize,
    image: &mut [u32],
) -> Result<(), &'static str> {
    let face_count = image.len() / pixels_per_face;
    let required_data = handler
        .face_data_size
        .checked_mul(face_count)
        .ok_or("Crunch face data size overflow.")?;
    if handler.dxt_data.len() < required_data {
        return Err("Crunch decoder returned incomplete face data.");
    }
    let decode = match handler.format {
        CrnFormat::Dxt1 => {
            bcn::decode_bc1 as fn(&[u8], usize, usize, &mut [u32]) -> Result<(), &'static str>
        }
        CrnFormat::CCrnfmtDxt5
        | CrnFormat::Dxt5CcxY
        | CrnFormat::Dxt5XGbr
        | CrnFormat::Dxt5Agbr
        | CrnFormat::Dxt5XGxR => bcn::decode_bc3,
        CrnFormat::Dxt5a => bcn::decode_bc4,
        CrnFormat::DxnXy | CrnFormat::DxnYx => bcn::decode_bc5,
        _ => return Err("Invalid crunch format."),
    };
    for (face, pixels) in image.chunks_exact_mut(pixels_per_face).enumerate() {
        let start = face
            .checked_mul(handler.face_data_size)
            .ok_or("Crunch face offset overflow.")?;
        let end = start
            .checked_add(handler.face_data_size)
            .ok_or("Crunch face offset overflow.")?;
        decode(&handler.dxt_data[start..end], width, height, pixels)?;
    }
    Ok(())
}
