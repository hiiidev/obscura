//! Pixel transfers never pass a short slice, a forged client pointer, or a
//! slice interpreted as a PBO offset to the graphics driver.
use crate::{
    api::CanvasContext,
    pixels::{browser_rgba, checked_bytes, transfer_layout},
};
use glow::{HasContext, PixelPackData, PixelUnpackData};
use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub(crate) struct UnpackState {
    pub flip_y: bool,
    pub premultiply_alpha: bool,
    pub colorspace_none: bool,
}
#[derive(Debug, Deserialize)]
pub struct TextureImage {
    pub target: u32,
    pub level: i32,
    pub internal_format: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default = "one")]
    pub depth: i32,
    pub format: u32,
    pub data_type: u32,
    #[serde(default)]
    pub border: i32,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub z: i32,
    #[serde(default)]
    pub sub_image: bool,
    #[serde(default)]
    pub three_dimensional: bool,
}
fn one() -> i32 {
    1
}
#[derive(Debug, Deserialize)]
pub struct ReadPixels {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub format: u32,
    pub data_type: u32,
}

/// Bounded copyback receipt for shared destinations; excludes pack padding.
#[derive(Debug, serde::Serialize)]
pub struct PixelReadLayout {
    pub start: usize,
    pub stride: usize,
    pub rows: usize,
    pub row_bytes: usize,
}

pub(crate) fn pixel_size(format: u32, data_type: u32, version: u8) -> Result<usize, u32> {
    let channels = match format {
        glow::ALPHA | glow::LUMINANCE => 1,
        glow::LUMINANCE_ALPHA => 2,
        glow::RGB => 3,
        glow::RGBA => 4,
        glow::RED | glow::RED_INTEGER | glow::DEPTH_COMPONENT if version == 2 => 1,
        glow::RG | glow::RG_INTEGER if version == 2 => 2,
        glow::RGB_INTEGER if version == 2 => 3,
        glow::RGBA_INTEGER if version == 2 => 4,
        glow::DEPTH_STENCIL if version == 2 => 2,
        _ => return Err(glow::INVALID_ENUM),
    };
    match data_type {
        glow::UNSIGNED_BYTE => Ok(channels),
        glow::FLOAT if version == 2 => Ok(channels * 4),
        glow::BYTE if version == 2 => Ok(channels),
        glow::SHORT | glow::UNSIGNED_SHORT | glow::HALF_FLOAT if version == 2 => Ok(channels * 2),
        glow::INT | glow::UNSIGNED_INT if version == 2 => Ok(channels * 4),
        glow::UNSIGNED_SHORT_5_6_5 if format == glow::RGB => Ok(2),
        glow::UNSIGNED_SHORT_4_4_4_4 | glow::UNSIGNED_SHORT_5_5_5_1 if format == glow::RGBA => {
            Ok(2)
        }
        glow::UNSIGNED_INT_2_10_10_10_REV
            if version == 2 && matches!(format, glow::RGBA | glow::RGBA_INTEGER) =>
        {
            Ok(4)
        }
        glow::UNSIGNED_INT_10F_11F_11F_REV | glow::UNSIGNED_INT_5_9_9_9_REV
            if version == 2 && format == glow::RGB =>
        {
            Ok(4)
        }
        glow::UNSIGNED_INT_24_8 if version == 2 && format == glow::DEPTH_STENCIL => Ok(4),
        glow::FLOAT_32_UNSIGNED_INT_24_8_REV if version == 2 && format == glow::DEPTH_STENCIL => {
            Ok(8)
        }
        glow::UNSIGNED_SHORT_5_6_5
        | glow::UNSIGNED_SHORT_4_4_4_4
        | glow::UNSIGNED_SHORT_5_5_5_1 => Err(glow::INVALID_OPERATION),
        _ => Err(glow::INVALID_ENUM),
    }
}
impl CanvasContext {
    /// DOM source rows are tightly packed and selected using their actual
    /// dimensions. User UNPACK_ALIGNMENT/ROW_LENGTH do not apply to them.
    pub fn texture_source(
        &mut self,
        image: TextureImage,
        source_width: u32,
        source_height: u32,
        rgba: &[u8],
        bitmap: bool,
    ) {
        self.texture_source_color(image,source_width,source_height,rgba,bitmap,crate::color::ColorSpace::Srgb,false);
    }
    pub fn texture_source_color(
        &mut self, image: TextureImage, source_width: u32, source_height: u32,
        rgba: &[u8], bitmap: bool, source_space: crate::color::ColorSpace, source_premultiplied: bool,
    ) {
        if !self.activate() {
            return;
        }
        let result = (|| {
            self.pixel_size(image.format, image.data_type)?;
            if image.width < 0 || image.height < 0 || image.depth < 0 {
                return Err(glow::INVALID_VALUE);
            }
            let mut selection = crate::image_upload::Selection {
                width: image.width as u32,
                height: image.height as u32,
                depth: if image.three_dimensional {
                    image.depth as u32
                } else {
                    1
                },
                // ImageBitmap construction already applies these two options.
                // Color-space conversion remains a separate upload concern.
                flip_y: !bitmap && self.unpack.flip_y,
                premultiply: if bitmap {source_premultiplied} else {self.unpack.premultiply_alpha},
                source_premultiplied,
                source_space,
                target_space: if self.unpack.colorspace_none {source_space} else {self.unpack_color_space},
                ..Default::default()
            };
            let parameters: &[u32] = if self.version == 2 {
                &[
                    glow::UNPACK_ALIGNMENT,
                    glow::UNPACK_ROW_LENGTH,
                    glow::UNPACK_SKIP_PIXELS,
                    glow::UNPACK_SKIP_ROWS,
                    glow::UNPACK_IMAGE_HEIGHT,
                    glow::UNPACK_SKIP_IMAGES,
                ]
            } else {
                &[glow::UNPACK_ALIGNMENT]
            };
            let previous: Vec<i32>;
            unsafe {
                let gl = &self.driver.as_ref().unwrap().gl;
                if self.version == 2 {
                    if gl
                        .get_parameter_buffer(glow::PIXEL_UNPACK_BUFFER_BINDING)
                        .is_some()
                    {
                        return Err(glow::INVALID_OPERATION);
                    }
                    selection.skip_pixels = gl.get_parameter_i32(glow::UNPACK_SKIP_PIXELS) as u32;
                    selection.skip_rows = gl.get_parameter_i32(glow::UNPACK_SKIP_ROWS) as u32;
                    if image.three_dimensional {
                        selection.image_height =
                            gl.get_parameter_i32(glow::UNPACK_IMAGE_HEIGHT) as u32;
                        selection.skip_images =
                            gl.get_parameter_i32(glow::UNPACK_SKIP_IMAGES) as u32;
                    }
                }
                previous = parameters
                    .iter()
                    .map(|p| gl.get_parameter_i32(*p))
                    .collect();
            }
            let converted = crate::image_upload::convert_rgba(
                rgba,
                source_width,
                source_height,
                selection,
                image.format,
                image.data_type,
            )?;
            unsafe {
                let gl = &self.driver.as_ref().unwrap().gl;
                for &p in parameters {
                    gl.pixel_store_i32(p, if p == glow::UNPACK_ALIGNMENT { 1 } else { 0 });
                }
            }
            let unpack = std::mem::take(&mut self.unpack);
            let result = self.texture_image_current(image, Some(&converted), None);
            self.unpack = unpack;
            unsafe {
                let gl = &self.driver.as_ref().unwrap().gl;
                for (&p, &value) in parameters.iter().zip(&previous) {
                    gl.pixel_store_i32(p, value);
                }
            }
            result
        })();
        if let Err(error) = result {
            self.error(error);
        }
    }
    fn pixel_size(&self, format: u32, data_type: u32) -> Result<usize, u32> {
        if self.version == 1 {
            let depth = matches!(format, glow::DEPTH_COMPONENT | glow::DEPTH_STENCIL);
            if depth && !self.extensions.contains("WEBGL_depth_texture") {
                return Err(glow::INVALID_ENUM);
            }
            if !depth
                && !matches!(
                    format,
                    glow::ALPHA | glow::LUMINANCE | glow::LUMINANCE_ALPHA | glow::RGB | glow::RGBA
                )
            {
                return Err(glow::INVALID_ENUM);
            }
            if data_type == glow::FLOAT && self.extensions.contains("OES_texture_float") {
                return pixel_size(format, data_type, 2);
            }
            if data_type == 0x8D61 && self.extensions.contains("OES_texture_half_float") {
                return pixel_size(format, glow::HALF_FLOAT, 2);
            }
            if depth {
                return pixel_size(format, data_type, 2);
            }
        }
        pixel_size(format, data_type, self.version)
    }
    pub fn pixel_store(&mut self, name: u32, value: i32) {
        if !self.activate() {
            return;
        }
        match name {
            0x9240 => self.unpack.flip_y = value != 0,
            0x9241 => self.unpack.premultiply_alpha = value != 0,
            0x9243 => match value as u32 {
                0 => self.unpack.colorspace_none = true,
                0x9244 => self.unpack.colorspace_none = false,
                _ => self.error(glow::INVALID_VALUE),
            },
            glow::PACK_ALIGNMENT | glow::UNPACK_ALIGNMENT => {
                if ![1, 2, 4, 8].contains(&value) {
                    self.error(glow::INVALID_VALUE);
                    return;
                }
                unsafe {
                    self.driver
                        .as_ref()
                        .unwrap()
                        .gl
                        .pixel_store_i32(name, value);
                }
            }
            glow::PACK_ROW_LENGTH
            | glow::PACK_SKIP_PIXELS
            | glow::PACK_SKIP_ROWS
            | glow::UNPACK_ROW_LENGTH
            | glow::UNPACK_IMAGE_HEIGHT
            | glow::UNPACK_SKIP_PIXELS
            | glow::UNPACK_SKIP_ROWS
            | glow::UNPACK_SKIP_IMAGES
                if self.version == 2 =>
            {
                if value < 0 {
                    self.error(glow::INVALID_VALUE);
                    return;
                }
                unsafe {
                    self.driver
                        .as_ref()
                        .unwrap()
                        .gl
                        .pixel_store_i32(name, value);
                }
            }
            _ => self.error(glow::INVALID_ENUM),
        }
    }
    fn transfer_bounds(
        &self,
        width: i32,
        height: i32,
        depth: i32,
        bpp: usize,
        pack: bool,
        volume: bool,
    ) -> Result<(usize, usize, usize), u32> {
        if width < 0 || height < 0 || depth < 0 {
            return Err(glow::INVALID_VALUE);
        }
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            let alignment = gl.get_parameter_i32(if pack {
                glow::PACK_ALIGNMENT
            } else {
                glow::UNPACK_ALIGNMENT
            }) as u32;
            let (row, image, pixels, rows, images) = if self.version == 2 {
                (
                    gl.get_parameter_i32(if pack {
                        glow::PACK_ROW_LENGTH
                    } else {
                        glow::UNPACK_ROW_LENGTH
                    }) as u32,
                    if pack || !volume {
                        0
                    } else {
                        gl.get_parameter_i32(glow::UNPACK_IMAGE_HEIGHT) as u32
                    },
                    gl.get_parameter_i32(if pack {
                        glow::PACK_SKIP_PIXELS
                    } else {
                        glow::UNPACK_SKIP_PIXELS
                    }) as u32,
                    gl.get_parameter_i32(if pack {
                        glow::PACK_SKIP_ROWS
                    } else {
                        glow::UNPACK_SKIP_ROWS
                    }) as u32,
                    if pack || !volume {
                        0
                    } else {
                        gl.get_parameter_i32(glow::UNPACK_SKIP_IMAGES) as u32
                    },
                )
            } else {
                (0, 0, 0, 0, 0)
            };
            if volume
                && u64::from(rows) + height as u64
                    > u64::from(if image == 0 { height as u32 } else { image })
            {
                return Err(glow::INVALID_OPERATION);
            }
            let (start, end) = transfer_layout(
                width as u32,
                height as u32,
                depth as u32,
                bpp,
                alignment,
                row,
                image,
                pixels,
                rows,
                images,
            )
            .map_err(|_| glow::INVALID_OPERATION)?;
            let row = if row == 0 {
                width as usize
            } else {
                row as usize
            };
            let stride = (row * bpp).div_ceil(alignment as usize) * alignment as usize;
            Ok((start, end, stride))
        }
    }
    pub fn texture_image(&mut self, image: TextureImage, data: Option<&[u8]>) {
        if !self.activate() {
            return;
        }
        if let Err(error) = self.texture_image_current(image, data, None) {
            self.error(error);
        }
    }
    pub fn texture_image_from_buffer(&mut self, image: TextureImage, offset: u32) {
        if !self.activate() {
            return;
        }
        if let Err(error) = self.texture_image_current(image, None, Some(offset)) {
            self.error(error);
        }
    }
    fn texture_image_current(
        &mut self,
        image: TextureImage,
        data: Option<&[u8]>,
        buffer_offset: Option<u32>,
    ) -> Result<(), u32> {
        let TextureImage {
            target,
            level,
            internal_format,
            width,
            height,
            depth,
            format,
            data_type,
            border,
            x,
            y,
            z,
            sub_image,
            three_dimensional,
        } = image;
        if three_dimensional && self.version != 2 {
            return Err(glow::INVALID_OPERATION);
        }
        if border != 0 || level < 0 || width < 0 || height < 0 || depth < 0 {
            return Err(glow::INVALID_VALUE);
        }
        let target_valid = if three_dimensional {
            matches!(target, glow::TEXTURE_3D | glow::TEXTURE_2D_ARRAY)
        } else {
            target == glow::TEXTURE_2D
                || (glow::TEXTURE_CUBE_MAP_POSITIVE_X..=glow::TEXTURE_CUBE_MAP_NEGATIVE_Z)
                    .contains(&target)
        };
        if !target_valid {
            return Err(glow::INVALID_ENUM);
        }
        if self.version == 1 && !sub_image && internal_format as u32 != format {
            return Err(glow::INVALID_OPERATION);
        }
        let bpp = self.pixel_size(format, data_type)?;
        let (start, end, stride) = if data.is_none() && buffer_offset.is_none() {
            // A null upload allocates initialized storage, independent of
            // client-memory skips or row/image overlap constraints.
            let end = checked_bytes(width as u32, height as u32, bpp)
                .ok()
                .and_then(|n| n.checked_mul(if three_dimensional { depth as usize } else { 1 }))
                .filter(|&n| n <= crate::pixels::MAX_TRANSFER_BYTES)
                .ok_or(glow::OUT_OF_MEMORY)?;
            (0, end, width as usize * bpp)
        } else {
            self.transfer_bounds(
                width,
                height,
                if three_dimensional { depth } else { 1 },
                bpp,
                false,
                three_dimensional,
            )?
        };
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            let bound = self.version == 2
                && gl
                    .get_parameter_buffer(glow::PIXEL_UNPACK_BUFFER_BINDING)
                    .is_some();
            if bound != buffer_offset.is_some() {
                return Err(glow::INVALID_OPERATION);
            }
            if let Some(offset) = buffer_offset {
                if self.version != 2 || self.unpack.flip_y || self.unpack.premultiply_alpha {
                    return Err(glow::INVALID_OPERATION);
                }
                if offset as usize % type_alignment(data_type) != 0 {
                    return Err(glow::INVALID_OPERATION);
                }
                crate::resources::checked_range(
                    i64::from(offset),
                    end,
                    i64::from(
                        gl.get_buffer_parameter_i32(glow::PIXEL_UNPACK_BUFFER, glow::BUFFER_SIZE),
                    ),
                )
                .map_err(|_| glow::INVALID_OPERATION)?;
            }
            if let Some(data) = data {
                if data.len() < end {
                    return Err(glow::INVALID_OPERATION);
                }
            }
            if sub_image && data.is_none() && buffer_offset.is_none() {
                return Err(glow::INVALID_VALUE);
            }
            // Typed-array transformation must retain padding and skip offsets.
            // Three-dimensional typed uploads reject these WebGL-only flags.
            if three_dimensional
                && data.is_some()
                && (self.unpack.flip_y || self.unpack.premultiply_alpha)
            {
                return Err(glow::INVALID_OPERATION);
            }
            let transformed = match data {
                Some(bytes) if self.unpack.flip_y || self.unpack.premultiply_alpha => {
                    Some(transform_unpack(
                        bytes,
                        start,
                        end,
                        stride,
                        width as usize,
                        height as usize,
                        format,
                        data_type,
                        &self.unpack,
                    )?)
                }
                _ => None,
            };
            let pixels = match buffer_offset {
                Some(offset) => PixelUnpackData::BufferOffset(offset),
                None => PixelUnpackData::Slice(transformed.as_deref().or(data)),
            };
            match (three_dimensional, sub_image) {
                (false, false) => gl.tex_image_2d(
                    target,
                    level,
                    internal_format,
                    width,
                    height,
                    0,
                    format,
                    data_type,
                    pixels,
                ),
                (false, true) => gl.tex_sub_image_2d(
                    target, level, x, y, width, height, format, data_type, pixels,
                ),
                (true, false) => gl.tex_image_3d(
                    target,
                    level,
                    internal_format,
                    width,
                    height,
                    depth,
                    0,
                    format,
                    data_type,
                    pixels,
                ),
                (true, true) => gl.tex_sub_image_3d(
                    target, level, x, y, z, width, height, depth, format, data_type, pixels,
                ),
            }
        }
        Ok(())
    }
    pub fn read_pixels(&mut self, request: ReadPixels, destination: &mut [u8]) {
        let _ = self.read_pixels_with_layout(request, destination);
    }
    /// Internal bridge completion receipt; caller errors stay observable.
    #[doc(hidden)]
    pub fn read_pixels_with_layout(&mut self, request: ReadPixels, destination: &mut [u8]) -> Option<PixelReadLayout> {
        if !self.activate() {
            return None;
        }
        self.retain_driver_errors();
        if self.is_lost() { return None; }
        let result = (|| {
            let ReadPixels {
                x,
                y,
                width,
                height,
                format,
                data_type,
            } = request;
            // Renderbuffer readback does not require enabling float textures.
            let bpp = if self.version == 1 && format == glow::RGBA && data_type == glow::FLOAT
                && self.extensions.contains("EXT_color_buffer_half_float")
            {
                pixel_size(format, data_type, 2)?
            } else {
                self.pixel_size(format, data_type)?
            };
            let (start, end, stride) = self.transfer_bounds(width, height, 1, bpp, true, false)?;
            if destination.len() < end {
                return Err(glow::INVALID_OPERATION);
            }
            let gl = &self.driver.as_ref().unwrap().gl;
            unsafe {
                if self.version == 2
                    && gl
                        .get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING)
                        .is_some()
                {
                    return Err(glow::INVALID_OPERATION);
                }
                // ANGLE's robust WebGL context clips reads and zero-initializes
                // resources. Never return bytes from an uninitialized Rust Vec.
                let _read=self.resolved_read(false)?;
                gl.read_pixels(
                    x,
                    y,
                    width,
                    height,
                    format,
                    data_type,
                    PixelPackData::Slice(Some(destination)),
                );
                let error = gl.get_error();
                if error != glow::NO_ERROR { return Err(error); }
            }
            Ok(PixelReadLayout { start, stride, rows: if width == 0 { 0 } else { height as usize }, row_bytes: width as usize * bpp })
        })();
        match result {
            Ok(layout) => Some(layout),
            Err(error) => { self.graphics_error(error); None }
        }
    }
    pub fn read_pixels_to_buffer(&mut self, request: ReadPixels, offset: u32) {
        if !self.activate() {
            return;
        }
        if self.drawing_storage.is_some() { self.retain_driver_errors(); }
        if self.is_lost() { return; }
        let result = (|| {
            if self.version != 2 {
                return Err(glow::INVALID_OPERATION);
            }
            let ReadPixels {
                x,
                y,
                width,
                height,
                format,
                data_type,
            } = request;
            let bpp = self.pixel_size(format, data_type)?;
            let (_, end, _) = self.transfer_bounds(width, height, 1, bpp, true, false)?;
            let gl = &self.driver.as_ref().unwrap().gl;
            unsafe {
                if gl
                    .get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING)
                    .is_none()
                    || offset as usize % type_alignment(data_type) != 0
                {
                    return Err(glow::INVALID_OPERATION);
                }
                crate::resources::checked_range(
                    i64::from(offset),
                    end,
                    i64::from(
                        gl.get_buffer_parameter_i32(glow::PIXEL_PACK_BUFFER, glow::BUFFER_SIZE),
                    ),
                )
                .map_err(|_| glow::INVALID_OPERATION)?;
                let _read=self.resolved_read(false)?;
                gl.read_pixels(
                    x,
                    y,
                    width,
                    height,
                    format,
                    data_type,
                    PixelPackData::BufferOffset(offset),
                );
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.graphics_error(error);
        }
    }
    /// Serialize the current drawing buffer, independent of user FBO, pack
    /// alignment, skipped rows, and pixel-pack buffers. Every state is restored.
    pub fn drawing_buffer_rgba(&mut self) -> Result<Vec<u8>, String> {
        self.drawing_buffer_in(crate::color::ColorSpace::Srgb)
    }
    pub fn drawing_buffer_source_rgba(&mut self) -> Result<Vec<u8>, String> {
        self.drawing_buffer_in(self.drawing_color_space)
    }
    fn drawing_buffer_in(&mut self, target_space: crate::color::ColorSpace) -> Result<Vec<u8>, String> {
        if !self.activate() {
            return Err("WebGL context is lost".into());
        }
        self.retain_driver_errors();
        if self.is_lost() {
            return Err("WebGL context is lost".into());
        }
        let length = checked_bytes(self.width, self.height, 4)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(length)
            .map_err(|_| "drawing buffer readback allocation failed")?;
        pixels.resize(length, 0);
        if pixels.is_empty() {
            return Ok(pixels);
        }
        let floating=self.drawing_buffer_format()==glow::RGBA16F;
        let mut float_pixels=Vec::<f32>::new();
        if floating {
            if length > crate::pixels::MAX_TRANSFER_BYTES / 4 { return Err("float readback exceeds allocation limit".into()); }
            float_pixels.try_reserve_exact(length).map_err(|_| "float drawing buffer readback allocation failed")?;
            float_pixels.resize(length,0.0);
        }
        let driver = self.driver.as_ref().unwrap();
        let gl = &driver.gl;
        let resolved_readback=(|| -> Result<(),u32> {
        let _read=self.resolved_read(true)?;
        unsafe {
            let pack = if self.version == 2 {
                gl.get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING)
            } else {
                None
            };
            let parameters = if self.version == 2 {
                vec![
                    glow::PACK_ALIGNMENT,
                    glow::PACK_ROW_LENGTH,
                    glow::PACK_SKIP_PIXELS,
                    glow::PACK_SKIP_ROWS,
                ]
            } else {
                vec![glow::PACK_ALIGNMENT]
            };
            let previous: Vec<_> = parameters
                .iter()
                .map(|&p| gl.get_parameter_i32(p))
                .collect();
            if self.version == 2 {
                gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            }
            for &parameter in &parameters {
                gl.pixel_store_i32(
                    parameter,
                    if parameter == glow::PACK_ALIGNMENT {
                        1
                    } else {
                        0
                    },
                );
            }
            gl.read_pixels(
                0,
                0,
                self.width as i32,
                self.height as i32,
                glow::RGBA,
                if floating { glow::FLOAT } else { glow::UNSIGNED_BYTE },
                PixelPackData::Slice(Some(if floating {
                    // f32 storage supplies both required alignment and initialized bytes.
                    std::slice::from_raw_parts_mut(float_pixels.as_mut_ptr().cast::<u8>(),length*4)
                } else { &mut pixels })),
            );
            for (&parameter, &value) in parameters.iter().zip(&previous) {
                gl.pixel_store_i32(parameter, value);
            }
            if self.version == 2 {
                gl.bind_buffer(glow::PIXEL_PACK_BUFFER, pack);
            }
        }
        Ok(())
        })();
        if let Err(error)=resolved_readback {
            self.graphics_error(error);
            return Err(format!("drawing-buffer resolve failed: 0x{error:x}"));
        }
        if self.retain_driver_errors() {
            return Err("graphics driver rejected drawing-buffer readback".into());
        }
        if floating { float_browser_pixels(&float_pixels,&mut pixels,self.attributes.premultiplied_alpha,self.drawing_color_space,target_space); }
        browser_rgba(
            &mut pixels,
            self.width,
            self.height,
            self.attributes.alpha,
            self.attributes.premultiplied_alpha && !floating,
        )?;
        if !floating { crate::color::convert_rgba8(&mut pixels,self.drawing_color_space,target_space,false); }
        Ok(pixels)
    }
    /// The compositor retains its own snapshot. With preserveDrawingBuffer
    /// disabled, later page reads see the cleared drawing buffer. Internal
    /// clearing must not change the page's FBO, scissor, masks or clear values.
    pub fn did_present(&mut self) {
        if self.attributes.preserve_drawing_buffer || !self.activate() {
            return;
        }
        self.initialize_drawing_buffer();
    }
    /// Transfer the current bitmap and replace it with blank storage without
    /// resetting GL objects or drawing state. Unlike presentation, this always
    /// clears, including when preserveDrawingBuffer was requested. Validate the
    /// destination first so a rejected transfer cannot consume the bitmap.
    pub fn transfer_bitmap(&mut self, destination: &mut [u8]) -> bool {
        self.transfer_bitmap_in(destination,false)
    }
    pub fn transfer_source_bitmap(&mut self, destination: &mut [u8]) -> bool {
        self.transfer_bitmap_in(destination,true)
    }
    fn transfer_bitmap_in(&mut self, destination: &mut [u8], source: bool) -> bool {
        if checked_bytes(self.width, self.height, 4).ok() != Some(destination.len()) {
            return false;
        }
        let Ok(pixels) = (if source {self.drawing_buffer_source_rgba()} else {self.drawing_buffer_rgba()}) else {
            return false;
        };
        self.initialize_drawing_buffer();
        if self.is_lost() || self.retain_driver_errors() {
            self.lose();
            return false;
        }
        destination.copy_from_slice(&pixels);
        self.dirty = true;
        true
    }
    pub(crate) fn initialize_drawing_buffer(&mut self) {
        if !self.activate() {
            return;
        }
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            let target = if self.version == 2 {
                glow::DRAW_FRAMEBUFFER
            } else {
                glow::FRAMEBUFFER
            };
            let framebuffer = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
            // WebGL 2 drops Clear while RASTERIZER_DISCARD is enabled.
            let discard = self.version == 2 && gl.is_enabled(glow::RASTERIZER_DISCARD);
            let mask = gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK);
            let depth_mask = gl.get_parameter_bool(glow::DEPTH_WRITEMASK);
            let front = gl.get_parameter_i32(glow::STENCIL_WRITEMASK) as u32;
            let back = gl.get_parameter_i32(glow::STENCIL_BACK_WRITEMASK) as u32;
            let mut color = [0.0; 4];
            gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut color);
            let depth = gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE);
            let stencil = gl.get_parameter_i32(glow::STENCIL_CLEAR_VALUE);
            gl.bind_framebuffer(target, self.default_framebuffer());
            let draw_selection=if self.version==2 { let previous=gl.get_parameter_i32(glow::DRAW_BUFFER0);gl.draw_buffers(&[if self.drawing_storage.is_some() {glow::COLOR_ATTACHMENT0} else {glow::BACK}]);Some(previous) } else { None };
            gl.disable(glow::SCISSOR_TEST);
            if discard {
                gl.disable(glow::RASTERIZER_DISCARD);
            }
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.stencil_mask(u32::MAX);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear_depth_f32(1.0);
            gl.clear_stencil(0);
            let mut bits = glow::COLOR_BUFFER_BIT;
            if self.attributes.depth {
                bits |= glow::DEPTH_BUFFER_BIT;
            }
            if self.attributes.stencil {
                bits |= glow::STENCIL_BUFFER_BIT;
            }
            #[cfg(test)]
            if crate::creation_test_faults::CLEAR.with(|fault| fault.replace(false)) {
                bits = u32::MAX; // Native GL rejects this mask; no successful clear occurs.
            }
            gl.clear(bits);
            gl.clear_color(color[0], color[1], color[2], color[3]);
            gl.clear_depth_f32(depth);
            gl.clear_stencil(stencil);
            gl.color_mask(mask[0], mask[1], mask[2], mask[3]);
            gl.depth_mask(depth_mask);
            gl.stencil_mask_separate(glow::FRONT, front);
            gl.stencil_mask_separate(glow::BACK, back);
            if discard {
                gl.enable(glow::RASTERIZER_DISCARD);
            }
            if scissor {
                gl.enable(glow::SCISSOR_TEST);
            }
            if let Some(selection)=draw_selection { gl.draw_buffers(&[selection as u32]); }
            gl.bind_framebuffer(target, framebuffer);
        }
    }
}
fn type_alignment(data_type: u32) -> usize {
    match data_type {
        glow::BYTE | glow::UNSIGNED_BYTE => 1,
        glow::SHORT
        | glow::UNSIGNED_SHORT
        | glow::HALF_FLOAT
        | 0x8D61
        | glow::UNSIGNED_SHORT_5_6_5
        | glow::UNSIGNED_SHORT_4_4_4_4
        | glow::UNSIGNED_SHORT_5_5_5_1 => 2,
        _ => 4,
    }
}
fn transform_unpack(
    data: &[u8],
    start: usize,
    end: usize,
    stride: usize,
    width: usize,
    height: usize,
    format: u32,
    data_type: u32,
    state: &UnpackState,
) -> Result<Vec<u8>, u32> {
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }
    let bpp = pixel_size(
        format,
        if data_type == 0x8D61 {
            glow::HALF_FLOAT
        } else {
            data_type
        },
        2,
    )?;
    let mut out = Vec::new();
    out.try_reserve_exact(end)
        .map_err(|_| glow::OUT_OF_MEMORY)?;
    out.extend_from_slice(&data[..end]);
    for row in 0..height {
        let destination = start + row * stride;
        let source = start + if state.flip_y { height - 1 - row } else { row } * stride;
        let bytes = width * bpp;
        out[destination..destination + bytes].copy_from_slice(&data[source..source + bytes]);
        if state.premultiply_alpha && matches!(format, glow::RGBA | glow::LUMINANCE_ALPHA) {
            for pixel in out[destination..destination + bytes].chunks_exact_mut(bpp) {
                premultiply_pixel(pixel, data_type)?;
            }
        }
    }
    Ok(out)
}

fn premultiply_pixel(pixel: &mut [u8], data_type: u32) -> Result<(), u32> {
    match data_type {
        glow::UNSIGNED_BYTE => {
            let alpha = u16::from(*pixel.last().unwrap());
            let colors = pixel.len() - 1;
            for channel in &mut pixel[..colors] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        glow::FLOAT => {
            let end = pixel.len() - 4;
            let alpha = f32::from_ne_bytes(pixel[end..].try_into().unwrap());
            for channel in pixel[..end].chunks_exact_mut(4) {
                let value = f32::from_ne_bytes(channel[..].try_into().unwrap()) * alpha;
                channel.copy_from_slice(&value.to_ne_bytes());
            }
        }
        glow::HALF_FLOAT | 0x8D61 => {
            use crate::image_upload::{float_to_half, half_to_float};
            let end = pixel.len() - 2;
            let alpha = half_to_float(u16::from_ne_bytes(pixel[end..].try_into().unwrap()));
            for channel in pixel[..end].chunks_exact_mut(2) {
                let value =
                    half_to_float(u16::from_ne_bytes(channel[..].try_into().unwrap())) * alpha;
                channel.copy_from_slice(&float_to_half(value).to_ne_bytes());
            }
        }
        glow::UNSIGNED_SHORT_4_4_4_4 => {
            let original = u16::from_ne_bytes(pixel.try_into().unwrap());
            let alpha = original & 15;
            let mut value = alpha;
            for shift in [4, 8, 12] {
                value |= ((((original >> shift) & 15) * alpha + 7) / 15) << shift;
            }
            pixel.copy_from_slice(&value.to_ne_bytes());
        }
        glow::UNSIGNED_SHORT_5_5_5_1 => {
            let original = u16::from_ne_bytes(pixel.try_into().unwrap());
            if original & 1 == 0 {
                pixel.fill(0);
            }
        }
        glow::UNSIGNED_INT_2_10_10_10_REV => {
            let original = u32::from_ne_bytes(pixel.try_into().unwrap());
            let alpha = original >> 30;
            let mut value = alpha << 30;
            for shift in [0, 10, 20] {
                value |= ((((original >> shift) & 1023) * alpha + 1) / 3) << shift;
            }
            pixel.copy_from_slice(&value.to_ne_bytes());
        }
        _ => return Err(glow::INVALID_OPERATION),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_float_half_and_packed_premultiplication_preserve_alpha() {
        let mut floats = [1.0_f32, 0.5, 0.25, 0.5]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect::<Vec<_>>();
        premultiply_pixel(&mut floats, glow::FLOAT).unwrap();
        assert_eq!(
            floats
                .chunks_exact(4)
                .map(|p| f32::from_ne_bytes(p.try_into().unwrap()))
                .collect::<Vec<_>>(),
            [0.5, 0.25, 0.125, 0.5]
        );
        for ty in [glow::HALF_FLOAT, 0x8D61] {
            let mut half = [0x3C00u16, 0x3800, 0x3400, 0x3800]
                .into_iter()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<_>>();
            premultiply_pixel(&mut half, ty).unwrap();
            assert_eq!(
                half.chunks_exact(2)
                    .map(|p| u16::from_ne_bytes(p.try_into().unwrap()))
                    .collect::<Vec<_>>(),
                [0x3800, 0x3400, 0x3000, 0x3800]
            );
        }
        for (ty, original, expected) in [
            (glow::UNSIGNED_SHORT_4_4_4_4, 0xF848u16, 0x8428u16),
            (glow::UNSIGNED_SHORT_5_5_5_1, 0xFFFE, 0),
            (glow::UNSIGNED_SHORT_5_5_5_1, 0xFFFF, 0xFFFF),
        ] {
            let mut bytes = original.to_ne_bytes();
            premultiply_pixel(&mut bytes, ty).unwrap();
            assert_eq!(u16::from_ne_bytes(bytes), expected);
        }
        let mut packed = 0xFFFFFFFFu32.to_ne_bytes();
        premultiply_pixel(&mut packed, glow::UNSIGNED_INT_2_10_10_10_REV).unwrap();
        assert_eq!(u32::from_ne_bytes(packed), u32::MAX);
        assert_eq!(
            premultiply_pixel(&mut [0; 8], glow::UNSIGNED_SHORT),
            Err(glow::INVALID_OPERATION)
        );
    }
    #[test]
    fn packed_formats_have_actual_transfer_sizes() {
        assert_eq!(pixel_size(glow::RGBA, glow::UNSIGNED_BYTE, 1), Ok(4));
        assert_eq!(pixel_size(glow::RGB, glow::UNSIGNED_SHORT_5_6_5, 1), Ok(2));
        assert_eq!(
            pixel_size(glow::RGBA, glow::UNSIGNED_SHORT_5_6_5, 1),
            Err(glow::INVALID_OPERATION)
        );
        assert_eq!(
            pixel_size(glow::DEPTH_STENCIL, glow::FLOAT_32_UNSIGNED_INT_24_8_REV, 2),
            Ok(8)
        );
        assert_eq!(pixel_size(glow::RED_INTEGER, glow::UNSIGNED_INT, 2), Ok(4));
        assert!(pixel_size(glow::RED, glow::UNSIGNED_BYTE, 1).is_err());
        assert!(pixel_size(glow::RGBA, glow::DOUBLE, 2).is_err());
    }
    #[test]
    fn unpack_flip_and_premultiply_preserve_skips_padding_and_input() {
        let data = [
            77, 77, 77, 77, 128, 64, 32, 128, 99, 99, 99, 99, 4, 8, 12, 255,
        ];
        let state = UnpackState {
            flip_y: true,
            premultiply_alpha: true,
            colorspace_none: false,
        };
        let transformed = transform_unpack(
            &data,
            4,
            16,
            8,
            1,
            2,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            &state,
        )
        .unwrap();
        assert_eq!(
            transformed,
            [77, 77, 77, 77, 4, 8, 12, 255, 99, 99, 99, 99, 64, 32, 16, 128]
        );
        assert_eq!(data[4], 128);
    }
    #[test]
    fn empty_upload_does_not_slice_a_nonexistent_row() {
        let state = UnpackState {
            flip_y: true,
            ..Default::default()
        };
        assert!(
            transform_unpack(&[], 0, 0, 0, 0, 0, glow::RGBA, glow::UNSIGNED_BYTE, &state)
                .unwrap()
                .is_empty()
        );
        assert!(transform_unpack(
            &[],
            0,
            0,
            128,
            0,
            10,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            &state
        )
        .unwrap()
        .is_empty());
    }
}

// Convert HDR storage only at the browser image boundary. Page readPixels still
// receives the actual float values. Unpremultiply before clamping to avoid
// losing fractional color information when alpha is small.
fn float_browser_pixels(source: &[f32], destination: &mut [u8], premultiplied: bool, source_space: crate::color::ColorSpace, target_space: crate::color::ColorSpace) {
    for (input,output) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
        let alpha=if input[3].is_nan() {0.0} else {input[3].clamp(0.0,1.0)};
        let mut color=[input[0],input[1],input[2],alpha];
        for channel in 0..3 {
            color[channel]=if premultiplied {
                if alpha>0.0 {input[channel]/alpha} else {0.0}
            } else {input[channel]};
        }
        let color=crate::color::convert(color,source_space,target_space);
        for channel in 0..4 { output[channel]=(color[channel].clamp(0.0,1.0)*255.0).round() as u8; }
    }
}
#[cfg(test)]
mod float_readback_tests {
    #[test]
    fn float_serialization_clamps_hdr_and_unpremultiplies_before_quantization() {
        let source=[0.25,0.125,2.0,0.5, f32::NAN,f32::INFINITY,f32::NEG_INFINITY,1.0, 1.0,1.0,1.0,0.0];
        let mut output=[0;12];super::float_browser_pixels(&source,&mut output,true,crate::color::ColorSpace::Srgb,crate::color::ColorSpace::Srgb);
        assert_eq!(output,[128,64,255,128,0,255,0,255,0,0,0,0]);
        super::float_browser_pixels(&source,&mut output,false,crate::color::ColorSpace::Srgb,crate::color::ColorSpace::Srgb);
        assert_eq!(output,[64,32,255,128,0,255,0,255,255,255,255,0]);
    }
}
