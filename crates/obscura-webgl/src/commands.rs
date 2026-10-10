//! Scalar WebGL commands. No pointer-valued driver entry point belongs here.
//! Browser-only enum rules are checked before ANGLE validates GLES state.
use crate::api::CanvasContext;
use glow::HasContext;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    ActiveTexture {
        texture: u32,
    },
    BlendColor {
        red: f32,
        green: f32,
        blue: f32,
        alpha: f32,
    },
    BlendEquation {
        mode: u32,
    },
    BlendEquationSeparate {
        rgb: u32,
        alpha: u32,
    },
    BlendFunc {
        src: u32,
        dst: u32,
    },
    BlendFuncSeparate {
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
    },
    Clear {
        mask: u32,
    },
    ClearColor {
        red: f32,
        green: f32,
        blue: f32,
        alpha: f32,
    },
    ClearDepth {
        depth: f32,
    },
    ClearStencil {
        stencil: i32,
    },
    ColorMask {
        red: bool,
        green: bool,
        blue: bool,
        alpha: bool,
    },
    CullFace {
        mode: u32,
    },
    DepthFunc {
        func: u32,
    },
    DepthMask {
        flag: bool,
    },
    DepthRange {
        near: f32,
        far: f32,
    },
    Disable {
        cap: u32,
    },
    Enable {
        cap: u32,
    },
    Finish {},
    Flush {},
    FrontFace {
        mode: u32,
    },
    Hint {
        target: u32,
        mode: u32,
    },
    LineWidth {
        width: f32,
    },
    PolygonOffset {
        factor: f32,
        units: f32,
    },
    SampleCoverage {
        value: f32,
        invert: bool,
    },
    Scissor {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    StencilFunc {
        func: u32,
        reference: i32,
        mask: u32,
    },
    StencilFuncSeparate {
        face: u32,
        func: u32,
        reference: i32,
        mask: u32,
    },
    StencilMask {
        mask: u32,
    },
    StencilMaskSeparate {
        face: u32,
        mask: u32,
    },
    StencilOp {
        fail: u32,
        zfail: u32,
        zpass: u32,
    },
    StencilOpSeparate {
        face: u32,
        fail: u32,
        zfail: u32,
        zpass: u32,
    },
    Viewport {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    DrawArrays {
        mode: u32,
        first: i32,
        count: i32,
    },
    DrawElements {
        mode: u32,
        count: i32,
        element_type: u32,
        offset: i32,
    },
    DisableVertexAttribArray {
        index: u32,
    },
    EnableVertexAttribArray {
        index: u32,
    },
    VertexAttrib1f {
        index: u32,
        x: f32,
    },
    VertexAttrib2f {
        index: u32,
        x: f32,
        y: f32,
    },
    VertexAttrib3f {
        index: u32,
        x: f32,
        y: f32,
        z: f32,
    },
    VertexAttrib4f {
        index: u32,
        x: f32,
        y: f32,
        z: f32,
        w: f32,
    },
    VertexAttribPointer {
        index: u32,
        size: i32,
        data_type: u32,
        normalized: bool,
        stride: i32,
        offset: i32,
    },
    GenerateMipmap {
        target: u32,
    },
    TexParameterf {
        target: u32,
        name: u32,
        value: f32,
    },
    TexParameteri {
        target: u32,
        name: u32,
        value: i32,
    },
    RenderbufferStorage {
        target: u32,
        format: u32,
        width: i32,
        height: i32,
    },
    CopyTexImage2D {
        target: u32,
        level: i32,
        format: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        border: i32,
    },
    CopyTexSubImage2D {
        target: u32,
        level: i32,
        xoffset: i32,
        yoffset: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    DrawArraysInstanced {
        mode: u32,
        first: i32,
        count: i32,
        instances: i32,
    },
    DrawElementsInstanced {
        mode: u32,
        count: i32,
        element_type: u32,
        offset: i32,
        instances: i32,
    },
    VertexAttribDivisor {
        index: u32,
        divisor: u32,
    },
    VertexAttribIPointer {
        index: u32,
        size: i32,
        data_type: u32,
        stride: i32,
        offset: i32,
    },
    VertexAttribI4i {
        index: u32,
        x: i32,
        y: i32,
        z: i32,
        w: i32,
    },
    VertexAttribI4ui {
        index: u32,
        x: u32,
        y: u32,
        z: u32,
        w: u32,
    },
    ReadBuffer {
        source: u32,
    },
    RenderbufferStorageMultisample {
        target: u32,
        samples: i32,
        format: u32,
        width: i32,
        height: i32,
    },
    TexStorage2D {
        target: u32,
        levels: i32,
        format: u32,
        width: i32,
        height: i32,
    },
    TexStorage3D {
        target: u32,
        levels: i32,
        format: u32,
        width: i32,
        height: i32,
        depth: i32,
    },
    CopyTexSubImage3D {
        target: u32,
        level: i32,
        xoffset: i32,
        yoffset: i32,
        zoffset: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    BlitFramebuffer {
        sx0: i32,
        sy0: i32,
        sx1: i32,
        sy1: i32,
        dx0: i32,
        dy0: i32,
        dx1: i32,
        dy1: i32,
        mask: u32,
        filter: u32,
    },
    BeginTransformFeedback {
        mode: u32,
    },
    EndTransformFeedback {},
    PauseTransformFeedback {},
    ResumeTransformFeedback {},
    EndQuery {
        target: u32,
    },
}
impl Command {
    pub fn minimum_version(&self) -> u8 {
        match self {
            Self::DrawArraysInstanced { .. } => 2,
            Self::DrawElementsInstanced { .. } => 2,
            Self::VertexAttribDivisor { .. } => 2,
            Self::VertexAttribIPointer { .. } => 2,
            Self::VertexAttribI4i { .. } => 2,
            Self::VertexAttribI4ui { .. } => 2,
            Self::ReadBuffer { .. } => 2,
            Self::RenderbufferStorageMultisample { .. } => 2,
            Self::TexStorage2D { .. } => 2,
            Self::TexStorage3D { .. } => 2,
            Self::CopyTexSubImage3D { .. } => 2,
            Self::BlitFramebuffer { .. } => 2,
            Self::BeginTransformFeedback { .. } => 2,
            Self::EndTransformFeedback { .. } => 2,
            Self::PauseTransformFeedback { .. } => 2,
            Self::ResumeTransformFeedback { .. } => 2,
            Self::EndQuery { .. } => 2,
            _ => 1,
        }
    }
    fn writes_pixels(&self) -> bool {
        matches!(
            self,
            Self::Clear { .. }
                | Self::DrawArrays { .. }
                | Self::DrawElements { .. }
                | Self::DrawArraysInstanced { .. }
                | Self::DrawElementsInstanced { .. }
                | Self::BlitFramebuffer { .. }
        )
    }
}
impl CanvasContext {
    pub fn command(&mut self, command: Command) {
        if !self.activate() {
            return;
        }
        let extension_command = matches!(
            command,
            Command::DrawArraysInstanced { .. }
                | Command::DrawElementsInstanced { .. }
                | Command::VertexAttribDivisor { .. }
        ) && self.extensions.contains("ANGLE_instanced_arrays");
        if command.minimum_version() > self.version && !extension_command {
            self.error(glow::INVALID_OPERATION);
            return;
        }
        if let Err(error) = validate(&command, self.version) {
            self.error(error);
            return;
        }
        if matches!(command,Command::CopyTexImage2D {..}|Command::CopyTexSubImage2D {..}|Command::CopyTexSubImage3D {..}) {
            self.retain_driver_errors();
            if self.is_lost() { return; }
            if let Err(error)=self.copy_from_read_buffer(&command) { self.graphics_error(error); }
            return;
        }
        let ending_query = match &command {
            Command::EndQuery { target } => Some(*target),
            _ => None,
        };
        let checkpoint = ending_query.and_then(|_| self.begin_reference_change());
        if self.is_lost() {
            return;
        }
        let writes = command.writes_pixels();
        let attribute_type = match &command {
            Command::VertexAttrib1f { index, .. }
            | Command::VertexAttrib2f { index, .. }
            | Command::VertexAttrib3f { index, .. }
            | Command::VertexAttrib4f { index, .. } => Some((*index, 'f')),
            Command::VertexAttribI4i { index, .. } => Some((*index, 'i')),
            Command::VertexAttribI4ui { index, .. } => Some((*index, 'u')),
            _ => None,
        };
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            if let Some((index, kind)) = attribute_type {
                if index >= gl.get_parameter_i32(glow::MAX_VERTEX_ATTRIBS).max(0) as u32 {
                    self.error(glow::INVALID_VALUE);
                    return;
                }
                self.attribute_types.insert(index, kind);
            }
            match command {
                Command::ActiveTexture { texture } => gl.active_texture(texture),
                Command::BlendColor {
                    red,
                    green,
                    blue,
                    alpha,
                } => gl.blend_color(red, green, blue, alpha),
                Command::BlendEquation { mode } => gl.blend_equation(mode),
                Command::BlendEquationSeparate { rgb, alpha } => {
                    gl.blend_equation_separate(rgb, alpha)
                }
                Command::BlendFunc { src, dst } => gl.blend_func(src, dst),
                Command::BlendFuncSeparate {
                    src_rgb,
                    dst_rgb,
                    src_alpha,
                    dst_alpha,
                } => gl.blend_func_separate(src_rgb, dst_rgb, src_alpha, dst_alpha),
                Command::Clear { mask } => gl.clear(mask),
                Command::ClearColor {
                    red,
                    green,
                    blue,
                    alpha,
                } => gl.clear_color(red, green, blue, alpha),
                Command::ClearDepth { depth } => gl.clear_depth_f32(depth),
                Command::ClearStencil { stencil } => gl.clear_stencil(stencil),
                Command::ColorMask {
                    red,
                    green,
                    blue,
                    alpha,
                } => gl.color_mask(red, green, blue, alpha),
                Command::CullFace { mode } => gl.cull_face(mode),
                Command::DepthFunc { func } => gl.depth_func(func),
                Command::DepthMask { flag } => gl.depth_mask(flag),
                Command::DepthRange { near, far } => gl.depth_range_f32(near, far),
                Command::Disable { cap } => gl.disable(cap),
                Command::Enable { cap } => gl.enable(cap),
                Command::Finish {} => gl.finish(),
                Command::Flush {} => gl.flush(),
                Command::FrontFace { mode } => gl.front_face(mode),
                Command::Hint { target, mode } => gl.hint(target, mode),
                Command::LineWidth { width } => gl.line_width(width),
                Command::PolygonOffset { factor, units } => gl.polygon_offset(factor, units),
                Command::SampleCoverage { value, invert } => gl.sample_coverage(value, invert),
                Command::Scissor {
                    x,
                    y,
                    width,
                    height,
                } => gl.scissor(x, y, width, height),
                Command::StencilFunc {
                    func,
                    reference,
                    mask,
                } => gl.stencil_func(func, reference, mask),
                Command::StencilFuncSeparate {
                    face,
                    func,
                    reference,
                    mask,
                } => gl.stencil_func_separate(face, func, reference, mask),
                Command::StencilMask { mask } => gl.stencil_mask(mask),
                Command::StencilMaskSeparate { face, mask } => gl.stencil_mask_separate(face, mask),
                Command::StencilOp { fail, zfail, zpass } => gl.stencil_op(fail, zfail, zpass),
                Command::StencilOpSeparate {
                    face,
                    fail,
                    zfail,
                    zpass,
                } => gl.stencil_op_separate(face, fail, zfail, zpass),
                Command::Viewport {
                    x,
                    y,
                    width,
                    height,
                } => gl.viewport(x, y, width, height),
                Command::DrawArrays { mode, first, count } => gl.draw_arrays(mode, first, count),
                Command::DrawElements {
                    mode,
                    count,
                    element_type,
                    offset,
                } => gl.draw_elements(mode, count, element_type, offset),
                Command::DisableVertexAttribArray { index } => {
                    gl.disable_vertex_attrib_array(index)
                }
                Command::EnableVertexAttribArray { index } => gl.enable_vertex_attrib_array(index),
                Command::VertexAttrib1f { index, x } => gl.vertex_attrib_1_f32(index, x),
                Command::VertexAttrib2f { index, x, y } => gl.vertex_attrib_2_f32(index, x, y),
                Command::VertexAttrib3f { index, x, y, z } => {
                    gl.vertex_attrib_3_f32(index, x, y, z)
                }
                Command::VertexAttrib4f { index, x, y, z, w } => {
                    gl.vertex_attrib_4_f32(index, x, y, z, w)
                }
                Command::VertexAttribPointer {
                    index,
                    size,
                    data_type,
                    normalized,
                    stride,
                    offset,
                } => {
                    gl.vertex_attrib_pointer_f32(index, size, data_type, normalized, stride, offset)
                }
                Command::GenerateMipmap { target } => gl.generate_mipmap(target),
                Command::TexParameterf {
                    target,
                    name,
                    value,
                } => gl.tex_parameter_f32(target, name, value),
                Command::TexParameteri {
                    target,
                    name,
                    value,
                } => gl.tex_parameter_i32(target, name, value),
                Command::RenderbufferStorage {
                    target,
                    format,
                    width,
                    height,
                } => gl.renderbuffer_storage(target, format, width, height),
                Command::CopyTexImage2D {
                    target,
                    level,
                    format,
                    x,
                    y,
                    width,
                    height,
                    border,
                } => gl.copy_tex_image_2d(target, level, format, x, y, width, height, border),
                Command::CopyTexSubImage2D {
                    target,
                    level,
                    xoffset,
                    yoffset,
                    x,
                    y,
                    width,
                    height,
                } => gl.copy_tex_sub_image_2d(target, level, xoffset, yoffset, x, y, width, height),
                Command::DrawArraysInstanced {
                    mode,
                    first,
                    count,
                    instances,
                } => {
                    if self.version == 1 {
                        // The ES3 core entry point rejects ES2 even after the
                        // WebGL1 ANGLE extension is enabled.
                        let draw = self.driver.as_ref().unwrap().entry::<
                            unsafe extern "system" fn(u32, i32, i32, i32),
                        >(c"glDrawArraysInstancedANGLE");
                        match draw {
                            Ok(draw) => draw(mode, first, count, instances),
                            Err(error) => { self.error(error); return; }
                        }
                    } else {
                        gl.draw_arrays_instanced(mode, first, count, instances);
                    }
                },
                Command::DrawElementsInstanced {
                    mode,
                    count,
                    element_type,
                    offset,
                    instances,
                } => {
                    if self.version == 1 {
                        let draw = self.driver.as_ref().unwrap().entry::<
                            unsafe extern "system" fn(u32, i32, u32, *const std::ffi::c_void, i32),
                        >(c"glDrawElementsInstancedANGLE");
                        match draw {
                            Ok(draw) => draw(mode, count, element_type, offset as usize as *const _, instances),
                            Err(error) => { self.error(error); return; }
                        }
                    } else {
                        gl.draw_elements_instanced(mode, count, element_type, offset, instances);
                    }
                },
                Command::VertexAttribDivisor { index, divisor } => {
                    if self.version == 1 {
                        let set = self.driver.as_ref().unwrap().entry::<
                            unsafe extern "system" fn(u32, u32),
                        >(c"glVertexAttribDivisorANGLE");
                        match set {
                            Ok(set) => set(index, divisor),
                            Err(error) => { self.error(error); return; }
                        }
                    } else {
                        gl.vertex_attrib_divisor(index, divisor);
                    }
                }
                Command::VertexAttribIPointer {
                    index,
                    size,
                    data_type,
                    stride,
                    offset,
                } => gl.vertex_attrib_pointer_i32(index, size, data_type, stride, offset),
                Command::VertexAttribI4i { index, x, y, z, w } => {
                    gl.vertex_attrib_4_i32(index, x, y, z, w)
                }
                Command::VertexAttribI4ui { index, x, y, z, w } => {
                    gl.vertex_attrib_4_u32(index, x, y, z, w)
                }
                Command::ReadBuffer { source } => {
                    if source!=glow::BACK && source!=glow::NONE && !(glow::COLOR_ATTACHMENT0..glow::COLOR_ATTACHMENT0+16).contains(&source) {
                        self.error(glow::INVALID_ENUM);return;
                    }
                    if self.default_bound(glow::READ_FRAMEBUFFER) {
                        match crate::framebuffer::default_buffer_token(source,self.drawing_storage.is_some()) {
                            Ok(token)=>{ gl.read_buffer(token);self.default_read_buffer=source; }
                            Err(error)=>self.error(error),
                        }
                    } else { gl.read_buffer(source); }
                },
                Command::RenderbufferStorageMultisample {
                    target,
                    samples,
                    format,
                    width,
                    height,
                } => gl.renderbuffer_storage_multisample(target, samples, format, width, height),
                Command::TexStorage2D {
                    target,
                    levels,
                    format,
                    width,
                    height,
                } => gl.tex_storage_2d(target, levels, format, width, height),
                Command::TexStorage3D {
                    target,
                    levels,
                    format,
                    width,
                    height,
                    depth,
                } => gl.tex_storage_3d(target, levels, format, width, height, depth),
                Command::CopyTexSubImage3D {
                    target,
                    level,
                    xoffset,
                    yoffset,
                    zoffset,
                    x,
                    y,
                    width,
                    height,
                } => gl.copy_tex_sub_image_3d(
                    target, level, xoffset, yoffset, zoffset, x, y, width, height,
                ),
                Command::BlitFramebuffer {
                    sx0,
                    sy0,
                    sx1,
                    sy1,
                    dx0,
                    dy0,
                    dx1,
                    dy1,
                    mask,
                    filter,
                } => gl.blit_framebuffer(sx0, sy0, sx1, sy1, dx0, dy0, dx1, dy1, mask, filter),
                Command::BeginTransformFeedback { mode } => gl.begin_transform_feedback(mode),
                Command::EndTransformFeedback {} => gl.end_transform_feedback(),
                Command::PauseTransformFeedback {} => gl.pause_transform_feedback(),
                Command::ResumeTransformFeedback {} => gl.resume_transform_feedback(),
                Command::EndQuery { target } => gl.end_query(target),
            }
            // Offscreen FBO work does not invalidate the canvas snapshot.
            if writes
                && self.default_bound(glow::DRAW_FRAMEBUFFER)
            {
                self.dirty = true;
            }
        }
        if let Some(target) = ending_query {
            if self.finish_reference_change(checkpoint) {
                self.active_queries.remove(&target);
            }
        }
    }
}

fn validate(command: &Command, version: u8) -> Result<(), u32> {
    use Command::*;
    match *command {
        Enable { cap } | Disable { cap } if !valid_capability(cap, version) => {
            Err(glow::INVALID_ENUM)
        }
        DrawElements { offset, .. } | DrawElementsInstanced { offset, .. } if offset < 0 => {
            Err(glow::INVALID_VALUE)
        }
        VertexAttribPointer { stride, offset, .. }
        | VertexAttribIPointer { stride, offset, .. }
            if !(0..=255).contains(&stride) || offset < 0 =>
        {
            Err(glow::INVALID_VALUE)
        }
        DepthRange { near, far } if near > far => Err(glow::INVALID_OPERATION),
        // GLES allows these independently, but WebGL requires the same face
        // state at draw time. ANGLE's WebGL-compatible validation enforces it.
        _ => Ok(()),
    }
}

pub(crate) fn valid_capability(cap: u32, version: u8) -> bool {
    matches!(
        cap,
        glow::BLEND
            | glow::CULL_FACE
            | glow::DEPTH_TEST
            | glow::DITHER
            | glow::POLYGON_OFFSET_FILL
            | glow::SAMPLE_ALPHA_TO_COVERAGE
            | glow::SAMPLE_COVERAGE
            | glow::SCISSOR_TEST
            | glow::STENCIL_TEST
    ) || version == 2 && cap == glow::RASTERIZER_DISCARD
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_gl_capabilities_cannot_be_enabled_from_a_web_context() {
        assert!(!valid_capability(0x0B50, 2)); // LIGHTING
        assert!(!valid_capability(glow::RASTERIZER_DISCARD, 1));
        assert!(valid_capability(glow::RASTERIZER_DISCARD, 2));
        assert!(valid_capability(glow::DITHER, 1));
    }
    #[test]
    fn pointers_are_offsets_and_respect_webgl_stride_limits() {
        for (stride, offset, valid) in [
            (255, 0, true),
            (256, 0, false),
            (-1, 0, false),
            (0, -1, false),
        ] {
            let command = Command::VertexAttribPointer {
                index: 0,
                size: 4,
                data_type: glow::FLOAT,
                normalized: false,
                stride,
                offset,
            };
            assert_eq!(validate(&command, 1).is_ok(), valid);
        }
        assert_eq!(
            validate(
                &Command::DrawElements {
                    mode: glow::TRIANGLES,
                    count: 3,
                    element_type: glow::UNSIGNED_SHORT,
                    offset: -1
                },
                1
            ),
            Err(glow::INVALID_VALUE)
        );
    }
    #[test]
    fn webgl_two_commands_require_a_webgl_two_context() {
        assert_eq!(Command::EndTransformFeedback {}.minimum_version(), 2);
        assert_eq!(Command::Flush {}.minimum_version(), 1);
        assert!(
            serde_json::from_str::<Command>(r#"{"method":"readBuffer","args":{"source":1029}}"#)
                .is_ok()
        );
        assert!(serde_json::from_str::<Command>(r#"{"method":"mapBuffer","args":{}}"#).is_err());
        assert!(
            serde_json::from_str::<Command>(r#"{"method":"clear","args":{"mask":-1}}"#).is_err()
        );
    }
    #[test]
    fn reversed_depth_range_is_invalid_but_equal_endpoints_are_valid() {
        assert_eq!(
            validate(
                &Command::DepthRange {
                    near: 1.0,
                    far: 0.0
                },
                1
            ),
            Err(glow::INVALID_OPERATION)
        );
        assert!(
            validate(
                &Command::DepthRange {
                    near: 0.5,
                    far: 0.5
                },
                1
            )
            .is_ok()
        );
    }
}
