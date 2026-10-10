//! Bounded image decoding for browser pixel consumers. These functions never
//! fetches resources; the caller must establish origin cleanliness first.
use std::io::Cursor;

#[path = "svg_pixels.rs"]
mod svg;

const MAX_PIXELS: u64 = 16_777_216;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if svg::is_candidate(bytes) {
        svg::dimensions(bytes)
    } else {
        raster_image_dimensions(bytes)
    }
}

/// Decode an origin-clean image without fetching anything from its markup.
/// SVG uses the same embedded fonts as paint and returns straight-alpha RGBA.
pub fn decode_image_rgba(bytes: &[u8], destination: &mut [u8]) -> bool {
    if svg::is_candidate(bytes) {
        svg::decode(bytes, destination)
    } else {
        decode_raster_rgba(bytes, destination)
    }
}

pub fn raster_image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let dimensions = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    bounded_length(dimensions.0, dimensions.1)?;
    Some(dimensions)
}

fn bounded_length(width: u32, height: u32) -> Option<usize> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || width > 32767 || height > 32767 || pixels > MAX_PIXELS {
        return None;
    }
    usize::try_from(pixels * 4).ok()
}

/// Returns straight-alpha RGBA8 at the image's encoded dimensions. The exact
/// destination size is checked before decoding, including animated images
/// (whose first frame is used). Color-profile conversion is not claimed here.
pub fn decode_raster_rgba(bytes: &[u8], destination: &mut [u8]) -> bool {
    let Some((width, height)) = raster_image_dimensions(bytes) else {
        return false;
    };
    if bounded_length(width, height) != Some(destination.len()) {
        return false;
    }
    let Ok(mut reader) = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return false;
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32767);
    limits.max_image_height = Some(32767);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let Ok(image) = reader.decode() else {
        return false;
    };
    if image.width() != width || image.height() != height {
        return false;
    }
    destination.copy_from_slice(image.to_rgba8().as_raw());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let image =
            image::RgbaImage::from_raw(2, 1, vec![255, 12, 34, 0, 17, 33, 65, 128]).unwrap();
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }
    #[test]
    fn straight_alpha_pixels_preserve_transparent_rgb() {
        let bytes = png();
        assert_eq!(raster_image_dimensions(&bytes), Some((2, 1)));
        let mut pixels = [0; 8];
        assert!(decode_raster_rgba(&bytes, &mut pixels));
        assert_eq!(pixels, [255, 12, 34, 0, 17, 33, 65, 128]);
    }
    #[test]
    fn image_dispatch_preserves_raster_pixels_and_renders_svg() {
        let bytes = png();
        assert_eq!(image_dimensions(&bytes), Some((2, 1)));
        let mut pixels = [0; 8];
        assert!(decode_image_rgba(&bytes, &mut pixels));
        assert_eq!(pixels, [255, 12, 34, 0, 17, 33, 65, 128]);
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='2' height='1'><rect width='2' height='1' fill='lime'/></svg>";
        assert_eq!(image_dimensions(svg), Some((2, 1)));
        assert!(decode_image_rgba(svg, &mut pixels));
        assert_eq!(pixels, [0, 255, 0, 255, 0, 255, 0, 255]);
    }
    #[test]
    fn image_dispatch_rejects_invalid_svg_and_raster_without_writing() {
        for bytes in [b"bad raster".as_slice(), b"<!-- invalid SVG", b"\x1f\x8b"] {
            let mut pixels = [19; 4];
            assert!(image_dimensions(bytes).is_none());
            assert!(!decode_image_rgba(bytes, &mut pixels));
            assert_eq!(pixels, [19; 4]);
        }
    }
    #[test]
    fn malformed_or_mismatched_images_do_not_change_the_destination() {
        for bytes in [vec![], b"not an image".to_vec(), png()[..20].to_vec()] {
            let mut pixels = [99; 8];
            assert!(!decode_raster_rgba(&bytes, &mut pixels));
            assert_eq!(pixels, [99; 8]);
        }
        for length in [0, 4, 9, 16] {
            let mut pixels = vec![77; length];
            assert!(!decode_raster_rgba(&png(), &mut pixels));
            assert!(pixels.iter().all(|v| *v == 77));
        }
    }
    #[test]
    fn dimensions_are_bounded_before_allocating_decoded_pixels() {
        assert_eq!(bounded_length(4096, 4096), Some(67_108_864));
        for (w, h) in [
            (0, 1),
            (1, 0),
            (32768, 1),
            (1, 32768),
            (4097, 4096),
            (u32::MAX, u32::MAX),
        ] {
            assert_eq!(bounded_length(w, h), None);
        }
    }
}
