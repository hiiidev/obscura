//! Browser image conversion, before ANGLE sees tightly packed client bytes.
//! Source pixels are straight-alpha, top-down RGBA8. No network or GL calls.
use crate::pixels::{checked_bytes, MAX_TRANSFER_BYTES};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Selection {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub skip_pixels: u32,
    pub skip_rows: u32,
    pub skip_images: u32,
    pub image_height: u32,
    pub flip_y: bool,
    pub premultiply: bool,
    pub source_premultiplied: bool,
    pub source_space: crate::color::ColorSpace,
    pub target_space: crate::color::ColorSpace,
}

pub(crate) fn convert_rgba(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    selection: Selection,
    format: u32,
    data_type: u32,
) -> Result<Vec<u8>, u32> {
    let expected =
        checked_bytes(source_width, source_height, 4).map_err(|_| glow::INVALID_VALUE)?;
    if expected != source.len() || source_width == 0 || source_height == 0 {
        return Err(glow::INVALID_VALUE);
    }
    let channels: &[usize] = match format {
        glow::ALPHA => &[3],
        glow::RED | glow::LUMINANCE => &[0],
        glow::RG => &[0, 1],
        glow::LUMINANCE_ALPHA => &[0, 3],
        glow::RGB => &[0, 1, 2],
        glow::RGBA => &[0, 1, 2, 3],
        // Integer and depth uploads from DOM sources are not permitted.
        _ => return Err(glow::INVALID_ENUM),
    };
    let bpp = match data_type {
        glow::UNSIGNED_BYTE => channels.len(),
        glow::FLOAT => channels.len() * 4,
        glow::HALF_FLOAT | 0x8D61 => channels.len() * 2,
        glow::UNSIGNED_SHORT_5_6_5 if format == glow::RGB => 2,
        glow::UNSIGNED_SHORT_4_4_4_4 | glow::UNSIGNED_SHORT_5_5_5_1 if format == glow::RGBA => 2,
        glow::UNSIGNED_INT_2_10_10_10_REV if format == glow::RGBA => 4,
        _ => return Err(glow::INVALID_OPERATION),
    };
    let s = selection;
    if u64::from(s.skip_pixels) + u64::from(s.width) > u64::from(source_width) {
        return Err(glow::INVALID_OPERATION);
    }
    let image_height = if s.image_height == 0 {
        s.height
    } else {
        s.image_height
    };
    if s.depth > 1 && s.height > image_height {
        return Err(glow::INVALID_OPERATION);
    }
    if s.width == 0 || s.height == 0 || s.depth == 0 {
        return Ok(Vec::new());
    }
    let start_row = u64::from(s.skip_images) * u64::from(image_height) + u64::from(s.skip_rows);
    let end_row = start_row
        .checked_add(u64::from(s.depth - 1) * u64::from(image_height))
        .and_then(|v| v.checked_add(u64::from(s.height)))
        .ok_or(glow::INVALID_OPERATION)?;
    if end_row > u64::from(source_height) {
        return Err(glow::INVALID_OPERATION);
    }
    let size = checked_bytes(s.width, s.height, bpp)
        .map_err(|_| glow::OUT_OF_MEMORY)?
        .checked_mul(s.depth as usize)
        .filter(|v| *v <= MAX_TRANSFER_BYTES)
        .ok_or(glow::OUT_OF_MEMORY)?;
    let mut out = Vec::new();
    out.try_reserve_exact(size)
        .map_err(|_| glow::OUT_OF_MEMORY)?;
    for image in 0..s.depth {
        for row in 0..s.height {
            let selected_row = start_row as u32 + image * image_height + row;
            let y = if s.flip_y {
                source_height - 1 - selected_row
            } else {
                selected_row
            };
            let start = ((y as usize * source_width as usize) + s.skip_pixels as usize) * 4;
            for pixel in source[start..start + s.width as usize * 4].chunks_exact(4) {
                let mut color = [
                    pixel[0] as f32 / 255.0,
                    pixel[1] as f32 / 255.0,
                    pixel[2] as f32 / 255.0,
                    pixel[3] as f32 / 255.0,
                ];
                if s.source_space!=s.target_space || s.premultiply!=s.source_premultiplied {
                    if s.source_premultiplied {
                        for c in 0..3 { color[c]=if color[3]>0.0 {color[c]/color[3]} else {0.0}; }
                    }
                    color=crate::color::convert(color,s.source_space,s.target_space);
                    if !matches!(data_type,glow::FLOAT|glow::HALF_FLOAT|0x8D61) {
                        for component in &mut color[..3] { *component=component.clamp(0.0,1.0); }
                    }
                    if s.premultiply { for c in 0..3 { color[c]*=color[3]; } }
                }
                let quantize = |c: usize, max: u32| (color[c].clamp(0.0,1.0) * max as f32).round() as u32;
                match data_type {
                    glow::UNSIGNED_BYTE => {
                        for &c in channels {
                            out.push(quantize(c, 255) as u8);
                        }
                    }
                    glow::FLOAT => {
                        for &c in channels {
                            out.extend_from_slice(&color[c].to_ne_bytes());
                        }
                    }
                    glow::HALF_FLOAT | 0x8D61 => {
                        for &c in channels {
                            out.extend_from_slice(&float_to_half(color[c]).to_ne_bytes());
                        }
                    }
                    glow::UNSIGNED_SHORT_5_6_5 => {
                        let p = (quantize(0, 31) << 11) | (quantize(1, 63) << 5) | quantize(2, 31);
                        out.extend_from_slice(&(p as u16).to_ne_bytes());
                    }
                    glow::UNSIGNED_SHORT_4_4_4_4 => {
                        let p = (quantize(0, 15) << 12)
                            | (quantize(1, 15) << 8)
                            | (quantize(2, 15) << 4)
                            | quantize(3, 15);
                        out.extend_from_slice(&(p as u16).to_ne_bytes());
                    }
                    glow::UNSIGNED_SHORT_5_5_5_1 => {
                        let p = (quantize(0, 31) << 11)
                            | (quantize(1, 31) << 6)
                            | (quantize(2, 31) << 1)
                            | quantize(3, 1);
                        out.extend_from_slice(&(p as u16).to_ne_bytes());
                    }
                    glow::UNSIGNED_INT_2_10_10_10_REV => {
                        let p = quantize(0, 1023)
                            | (quantize(1, 1023) << 10)
                            | (quantize(2, 1023) << 20)
                            | (quantize(3, 3) << 30);
                        out.extend_from_slice(&p.to_ne_bytes());
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    Ok(out)
}

/// IEEE-754 binary16, round to nearest, ties to even. Used for the actual
/// uploaded value; native byte order matches GLES client memory conventions.
pub(crate) fn float_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let fraction = bits & 0x7FFFFF;
    if exponent == 255 {
        return sign | if fraction == 0 { 0x7C00 } else { 0x7E00 };
    }
    let e = exponent - 127 + 15;
    if e >= 31 {
        return sign | 0x7C00;
    }
    if e < -10 {
        return sign;
    }
    let (mantissa, shift) = if e <= 0 {
        (fraction | 0x800000, (14 - e) as u32)
    } else {
        (fraction, 13)
    };
    let truncated = mantissa >> shift;
    let remainder = mantissa & ((1 << shift) - 1);
    let halfway = 1 << (shift - 1);
    let rounded =
        truncated + u32::from(remainder > halfway || remainder == halfway && truncated & 1 != 0);
    sign | ((if e > 0 { (e as u32) << 10 } else { 0 }) + rounded) as u16
}

pub(crate) fn half_to_float(value: u16) -> f32 {
    let sign = (u32::from(value) & 0x8000) << 16;
    let exponent = (value >> 10) & 31;
    let fraction = u32::from(value & 1023);
    match exponent {
        0 => {
            let f = fraction as f32 * 2.0_f32.powi(-24);
            if sign == 0 {
                f
            } else {
                -f
            }
        }
        31 => f32::from_bits(sign | 0x7F800000 | fraction << 13),
        _ => f32::from_bits(sign | (u32::from(exponent) + 112) << 23 | fraction << 13),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn one() -> Selection {
        Selection {
            width: 1,
            height: 1,
            depth: 1,
            ..Default::default()
        }
    }
    #[test]
    fn image_color_conversion_precedes_format_and_alpha_packing() {
        use crate::color::ColorSpace::{Srgb,DisplayP3};
        let selection=Selection {source_space:Srgb,target_space:DisplayP3,..one()};
        assert_eq!(convert_rgba(&[255,0,0,255],1,1,selection,glow::RGBA,glow::UNSIGNED_BYTE).unwrap(),[234,51,35,255]);
        assert_eq!(convert_rgba(&[255,0,0,255],1,1,selection,glow::LUMINANCE_ALPHA,glow::UNSIGNED_BYTE).unwrap(),[234,255]);
        let selection=Selection {source_space:DisplayP3,target_space:Srgb,premultiply:true,source_premultiplied:true,..one()};
        assert_eq!(convert_rgba(&[128,0,0,128],1,1,selection,glow::RGBA,glow::UNSIGNED_BYTE).unwrap(),[128,0,0,128]);
        let raw=[7,19,231,0];let selection=Selection {target_space:DisplayP3,source_space:DisplayP3,premultiply:true,source_premultiplied:true,..one()};
        assert_eq!(convert_rgba(&raw,1,1,selection,glow::RGBA,glow::UNSIGNED_BYTE).unwrap(),raw);
    }
    #[test]
    fn float_image_color_conversion_keeps_out_of_gamut_values() {
        use crate::color::ColorSpace::{Srgb,DisplayP3};
        let selection=Selection {source_space:DisplayP3,target_space:Srgb,..one()};
        let bytes=convert_rgba(&[255,0,0,255],1,1,selection,glow::RGBA,glow::FLOAT).unwrap();
        let values:Vec<_>=bytes.chunks_exact(4).map(|p|f32::from_ne_bytes(p.try_into().unwrap())).collect();
        assert!(values[0]>1.0&&values[1]<0.0&&values[2]<0.0);assert_eq!(values[3],1.0);
        let bytes=convert_rgba(&[255,0,0,255],1,1,selection,glow::RGBA,glow::HALF_FLOAT).unwrap();
        let values:Vec<_>=bytes.chunks_exact(2).map(|p|half_to_float(u16::from_ne_bytes(p.try_into().unwrap()))).collect();
        assert!(values[0]>1.0&&values[1]<0.0&&values[2]<0.0);assert_eq!(values[3],1.0);
    }
    #[test]
    fn formats_select_actual_source_channels() {
        for (format, bytes) in [
            (glow::ALPHA, vec![128]),
            (glow::LUMINANCE, vec![17]),
            (glow::LUMINANCE_ALPHA, vec![17, 128]),
            (glow::RED, vec![17]),
            (glow::RG, vec![17, 33]),
            (glow::RGB, vec![17, 33, 65]),
            (glow::RGBA, vec![17, 33, 65, 128]),
        ] {
            assert_eq!(
                convert_rgba(&[17, 33, 65, 128], 1, 1, one(), format, glow::UNSIGNED_BYTE),
                Ok(bytes)
            );
        }
        let premultiply = Selection {
            premultiply: true,
            ..one()
        };
        assert_eq!(
            convert_rgba(
                &[128, 64, 32, 128],
                1,
                1,
                premultiply,
                glow::RGBA,
                glow::UNSIGNED_BYTE
            ),
            Ok(vec![64, 32, 16, 128])
        );
    }
    #[test]
    fn packed_formats_and_float_outputs_have_the_requested_representation() {
        for (ty, bits) in [
            (glow::UNSIGNED_SHORT_5_6_5, 0xF800u16),
            (glow::UNSIGNED_SHORT_4_4_4_4, 0xF00F),
            (glow::UNSIGNED_SHORT_5_5_5_1, 0xF801),
        ] {
            let format = if ty == glow::UNSIGNED_SHORT_5_6_5 {
                glow::RGB
            } else {
                glow::RGBA
            };
            assert_eq!(
                convert_rgba(&[255, 0, 0, 255], 1, 1, one(), format, ty),
                Ok(bits.to_ne_bytes().to_vec())
            );
        }
        assert_eq!(
            convert_rgba(
                &[255, 0, 0, 255],
                1,
                1,
                one(),
                glow::RGBA,
                glow::UNSIGNED_INT_2_10_10_10_REV
            ),
            Ok(0xC00003FFu32.to_ne_bytes().to_vec())
        );
        let bytes = convert_rgba(&[255, 0, 0, 255], 1, 1, one(), glow::RGBA, glow::FLOAT).unwrap();
        assert_eq!(
            bytes
                .chunks_exact(4)
                .map(|p| f32::from_ne_bytes(p.try_into().unwrap()))
                .collect::<Vec<_>>(),
            [1.0, 0.0, 0.0, 1.0]
        );
        for ty in [glow::HALF_FLOAT, 0x8D61] {
            let bytes = convert_rgba(&[255, 0, 0, 255], 1, 1, one(), glow::RGBA, ty).unwrap();
            assert_eq!(
                bytes
                    .chunks_exact(2)
                    .map(|p| u16::from_ne_bytes(p.try_into().unwrap()))
                    .collect::<Vec<_>>(),
                [0x3C00, 0, 0, 0x3C00]
            );
        }
    }
    #[test]
    fn slicing_flipping_and_3d_image_stride_do_not_overlap() {
        let source = (0..24u8).flat_map(|i| [i, 0, 0, 255]).collect::<Vec<_>>();
        let selection = Selection {
            width: 1,
            height: 1,
            depth: 2,
            skip_pixels: 1,
            skip_rows: 1,
            image_height: 3,
            ..Default::default()
        };
        assert_eq!(
            convert_rgba(&source, 4, 6, selection, glow::RED, glow::UNSIGNED_BYTE),
            Ok(vec![5, 17])
        );
        assert_eq!(
            convert_rgba(
                &source,
                4,
                6,
                Selection {
                    flip_y: true,
                    ..selection
                },
                glow::RED,
                glow::UNSIGNED_BYTE
            ),
            Ok(vec![17, 5])
        );
        assert_eq!(
            convert_rgba(
                &source,
                4,
                6,
                Selection {
                    skip_images: 1,
                    depth: 1,
                    ..selection
                },
                glow::RED,
                glow::UNSIGNED_BYTE
            ),
            Ok(vec![17])
        );
    }
    #[test]
    fn invalid_sources_regions_and_formats_are_rejected_before_slicing() {
        let source = [1, 2, 3, 4];
        assert_eq!(
            convert_rgba(&source, 2, 1, one(), glow::RGBA, glow::UNSIGNED_BYTE),
            Err(glow::INVALID_VALUE)
        );
        for s in [
            Selection {
                skip_pixels: 1,
                ..one()
            },
            Selection {
                skip_rows: 1,
                ..one()
            },
            Selection {
                skip_images: u32::MAX,
                image_height: u32::MAX,
                ..one()
            },
            Selection { depth: 2, ..one() },
        ] {
            assert_eq!(
                convert_rgba(&source, 1, 1, s, glow::RGBA, glow::UNSIGNED_BYTE),
                Err(glow::INVALID_OPERATION)
            );
        }
        assert_eq!(
            convert_rgba(
                &source,
                1,
                1,
                one(),
                glow::RGBA_INTEGER,
                glow::UNSIGNED_BYTE
            ),
            Err(glow::INVALID_ENUM)
        );
        assert_eq!(
            convert_rgba(&source, 1, 1, one(), glow::RGBA, glow::UNSIGNED_SHORT_5_6_5),
            Err(glow::INVALID_OPERATION)
        );
        assert_eq!(
            convert_rgba(
                &source,
                1,
                1,
                Selection { width: 0, ..one() },
                glow::RGBA,
                glow::UNSIGNED_BYTE
            ),
            Ok(vec![])
        );
    }
    #[test]
    fn binary16_roundtrip_covers_normals_subnormals_infinities_and_signed_zero() {
        for bits in 0..=u16::MAX {
            let value = half_to_float(bits);
            if !value.is_nan() {
                assert_eq!(float_to_half(value), bits);
            }
        }
        assert_eq!(float_to_half(1.0 + 2.0_f32.powi(-11)), 0x3C00);
        assert_eq!(float_to_half(1.0 + 3.0 * 2.0_f32.powi(-11)), 0x3C02);
        assert_eq!(float_to_half(f32::NAN) & 0x7FFF, 0x7E00);
    }
}
