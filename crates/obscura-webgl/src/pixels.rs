//! Checked sizes and conversion at the GPU/browser drawing-buffer boundary.
//! GL returns bottom-up RGBA; screenshots and canvas serialization use top-down
//! straight-alpha bytes. No conversion touches the WebGL readPixels API.
pub const MAX_TRANSFER_BYTES: usize = 256 * 1024 * 1024;

pub fn checked_bytes(width: u32, height: u32, bytes_per_pixel: usize) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(bytes_per_pixel))
        .filter(|&n| n <= MAX_TRANSFER_BYTES)
        .ok_or_else(|| "graphics transfer exceeds the allocation limit".into())
}

/// GLES pack/unpack row stride, including all rows except trailing row padding.
/// Required for typed-array bounds checks before passing any pointer to ANGLE.
pub fn transfer_layout(
    width: u32,
    height: u32,
    depth: u32,
    bytes_per_pixel: usize,
    alignment: u32,
    row_length: u32,
    image_height: u32,
    skip_pixels: u32,
    skip_rows: u32,
    skip_images: u32,
) -> Result<(usize, usize), String> {
    if ![1, 2, 4, 8].contains(&alignment) {
        return Err("invalid pixel alignment".into());
    }
    if width == 0 || height == 0 || depth == 0 {
        return Ok((0, 0));
    }
    let row_length = if row_length == 0 { width } else { row_length };
    let image_height = if image_height == 0 {
        height
    } else {
        image_height
    };
    if u64::from(skip_pixels) + u64::from(width) > u64::from(row_length) || image_height < height {
        return Err("pixel storage dimensions are smaller than the image".into());
    }
    let raw = checked_bytes(row_length, 1, bytes_per_pixel)?;
    let align = alignment as usize;
    let stride = raw
        .checked_add(align - 1)
        .map(|n| n / align * align)
        .ok_or("pixel stride overflow")?;
    let image = stride
        .checked_mul(image_height as usize)
        .ok_or("pixel image stride overflow")?;
    let start = (skip_images as usize)
        .checked_mul(image)
        .and_then(|n| {
            (skip_rows as usize)
                .checked_mul(stride)
                .and_then(|r| n.checked_add(r))
        })
        .and_then(|n| {
            (skip_pixels as usize)
                .checked_mul(bytes_per_pixel)
                .and_then(|p| n.checked_add(p))
        })
        .ok_or("pixel offset overflow")?;
    let end = (depth as usize - 1)
        .checked_mul(image)
        .and_then(|n| {
            (height as usize - 1)
                .checked_mul(stride)
                .and_then(|r| n.checked_add(r))
        })
        .and_then(|n| {
            (width as usize)
                .checked_mul(bytes_per_pixel)
                .and_then(|r| n.checked_add(r))
        })
        .and_then(|n| n.checked_add(start))
        .filter(|&n| n <= MAX_TRANSFER_BYTES)
        .ok_or("pixel transfer overflow or allocation limit")?;
    Ok((start, end))
}

pub fn browser_rgba(
    bytes: &mut [u8],
    width: u32,
    height: u32,
    alpha: bool,
    premultiplied: bool,
) -> Result<(), String> {
    if bytes.len() != checked_bytes(width, height, 4)? {
        return Err("drawing buffer byte length mismatch".into());
    }
    if bytes.is_empty() {
        return Ok(());
    }
    let row = width as usize * 4;
    for y in 0..height as usize / 2 {
        let opposite = height as usize - 1 - y;
        let (before, after) = bytes.split_at_mut(opposite * row);
        before[y * row..(y + 1) * row].swap_with_slice(&mut after[..row]);
    }
    for pixel in bytes.chunks_exact_mut(4) {
        if !alpha {
            pixel[3] = 255;
        } else if premultiplied {
            let a = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = if a == 0 {
                    0
                } else {
                    ((u32::from(*channel) * 255 + a / 2) / a).min(255) as u8
                };
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_bounds_reject_overflow_without_allocating() {
        assert_eq!(checked_bytes(2, 3, 4).unwrap(), 24);
        assert_eq!(checked_bytes(0, 3, 4).unwrap(), 0);
        assert!(checked_bytes(u32::MAX, u32::MAX, 4).is_err());
        assert!(checked_bytes(1, 1, usize::MAX).is_err());
    }
    #[test]
    fn pack_alignment_does_not_require_padding_after_the_last_row() {
        assert_eq!(
            transfer_layout(1, 2, 1, 3, 4, 0, 0, 0, 0, 0).unwrap(),
            (0, 7)
        );
        assert_eq!(
            transfer_layout(1, 2, 1, 3, 1, 0, 0, 0, 0, 0).unwrap(),
            (0, 6)
        );
        assert_eq!(
            transfer_layout(1, 1, 1, 4, 4, 2, 2, 1, 1, 1).unwrap(),
            (28, 32)
        );
    }
    #[test]
    fn pixel_store_validation_covers_empty_too_small_and_overflow() {
        assert_eq!(
            transfer_layout(0, 1, 1, 4, 4, 0, 0, 0, 0, 0).unwrap(),
            (0, 0)
        );
        assert!(transfer_layout(1, 1, 1, 4, 3, 0, 0, 0, 0, 0).is_err());
        assert!(transfer_layout(2, 1, 1, 4, 4, 1, 0, 0, 0, 0).is_err());
        assert!(transfer_layout(1, 1, 1, 4, 4, 1, 0, 1, 0, 0).is_err());
        assert!(transfer_layout(1, 1, 1, 4, 4, u32::MAX, 0, u32::MAX, 0, 0).is_err());
        assert!(transfer_layout(1, 2, 1, 4, 4, 0, 1, 0, 0, 0).is_err());
        assert!(transfer_layout(
            1,
            1,
            u32::MAX,
            4,
            8,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX
        )
        .is_err());
    }
    #[test]
    fn browser_output_is_top_down_with_real_alpha() {
        let mut pixels = [0, 64, 0, 128, 255, 0, 0, 255];
        browser_rgba(&mut pixels, 1, 2, true, true).unwrap();
        assert_eq!(pixels, [255, 0, 0, 255, 0, 128, 0, 128]);
        let mut transparent = [255, 255, 255, 0];
        browser_rgba(&mut transparent, 1, 1, true, true).unwrap();
        assert_eq!(transparent, [0, 0, 0, 0]);
        let mut opaque = [1, 2, 3, 0];
        browser_rgba(&mut opaque, 1, 1, false, true).unwrap();
        assert_eq!(opaque, [1, 2, 3, 255]);
        let mut straight = [1, 2, 3, 128];
        browser_rgba(&mut straight, 1, 1, true, false).unwrap();
        assert_eq!(straight, [1, 2, 3, 128]);
    }
    #[test]
    fn conversion_handles_odd_rows_and_rejects_wrong_lengths() {
        let mut rows = [1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255];
        browser_rgba(&mut rows, 1, 3, true, false).unwrap();
        assert_eq!(rows[0], 3);
        assert_eq!(rows[4], 2);
        assert_eq!(rows[8], 1);
        assert!(browser_rgba(&mut [0; 3], 1, 1, true, false).is_err());
        assert!(browser_rgba(&mut [], 0, 0, true, false).is_ok());
        assert!(browser_rgba(&mut [], 0, u32::MAX, true, false).is_ok());
    }
}
