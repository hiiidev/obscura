//! Indexed state, attachment identity and WebGL 2 synchronization. Driver
//! outputs are sized from bounded native counts, never from page pointers.
use crate::{
    api::CanvasContext,
    objects::{Kind, Object},
    queries::{Value, native_name, valid_shader_name},
    resources::valid_framebuffer_target,
};
use glow::HasContext;
use serde::Deserialize;
use std::num::NonZeroU32;

#[derive(Debug, Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Advanced {
    GetAttachedShaders {
        program: u32,
    },
    GetVertexAttrib {
        index: u32,
        name: u32,
    },
    GetVertexAttribOffset {
        index: u32,
        name: u32,
    },
    GetFramebufferAttachmentParameter {
        target: u32,
        attachment: u32,
        name: u32,
    },
    GetQuery {
        target: u32,
        name: u32,
    },
    GetQueryParameter {
        id: u32,
        name: u32,
    },
    GetSamplerParameter {
        id: u32,
        name: u32,
    },
    FenceSync {
        condition: u32,
        flags: u32,
    },
    ClientWaitSync {
        id: u32,
        flags: u32,
        timeout: u64,
    },
    WaitSync {
        id: u32,
        flags: u32,
        timeout: i64,
    },
    GetSyncParameter {
        id: u32,
        name: u32,
    },
    GetTransformFeedbackVarying {
        program: u32,
        index: u32,
    },
    GetIndexedParameter {
        target: u32,
        index: u32,
    },
    GetUniformIndices {
        program: u32,
        names: Vec<String>,
    },
    GetActiveUniforms {
        program: u32,
        indices: Vec<u32>,
        name: u32,
    },
    GetUniformBlockIndex {
        program: u32,
        name: String,
    },
    GetFragDataLocation {
        program: u32,
        name: String,
    },
    GetActiveUniformBlockName {
        program: u32,
        index: u32,
    },
    GetActiveUniformBlockParameter {
        program: u32,
        index: u32,
        name: u32,
    },
    GetInternalformatParameter {
        target: u32,
        format: u32,
        name: u32,
    },
    DrawRangeElements {
        mode: u32,
        start: u32,
        end: u32,
        count: i32,
        data_type: u32,
        offset: i32,
    },
    ClearBufferFloat {
        buffer: u32,
        index: i32,
        values: Vec<f32>,
    },
    ClearBufferInt {
        buffer: u32,
        index: i32,
        values: Vec<i32>,
    },
    ClearBufferUint {
        buffer: u32,
        index: i32,
        values: Vec<u32>,
    },
    ClearBufferDepthStencil {
        buffer: u32,
        index: i32,
        depth: f32,
        stencil: i32,
    },
}
impl Advanced {
    fn failure_value(&self) -> Value {
        match self {
            Self::GetVertexAttribOffset { .. } => Value::Number(0.0),
            Self::GetFragDataLocation { .. } => Value::Int(-1),
            Self::GetUniformBlockIndex { .. } => Value::UInt(0),
            Self::ClientWaitSync { .. } => Value::UInt(glow::WAIT_FAILED),
            _ => Value::Null,
        }
    }
    pub fn minimum_version(&self) -> u8 {
        match self {
            Self::GetAttachedShaders { .. }
            | Self::GetVertexAttrib { .. }
            | Self::GetVertexAttribOffset { .. }
            | Self::GetFramebufferAttachmentParameter { .. } => 1,
            _ => 2,
        }
    }
}
fn bounded_count(value: i32) -> Result<usize, u32> {
    usize::try_from(value)
        .ok()
        .filter(|&n| n <= 65536)
        .ok_or(glow::INVALID_OPERATION)
}
fn native_output_name(bytes: &[u8], written: i32) -> Result<String, u32> {
    let length = bounded_count(written)?;
    if length >= bytes.len() || bytes[length] != 0 || bytes[..length].contains(&0) {
        return Err(glow::INVALID_OPERATION);
    }
    String::from_utf8(bytes[..length].to_vec()).map_err(|_| glow::INVALID_OPERATION)
}
fn query_target(target: u32) -> bool {
    matches!(
        target,
        glow::ANY_SAMPLES_PASSED
            | glow::ANY_SAMPLES_PASSED_CONSERVATIVE
            | glow::TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN
    )
}
impl CanvasContext {
    pub fn advanced(&mut self, request: Advanced) -> Value {
        let failure = request.failure_value();
        if !self.activate() {
            return failure;
        }
        if request.minimum_version() > self.version {
            self.error(glow::INVALID_OPERATION);
            return failure;
        }
        match self.advanced_current(request) {
            Ok(value) => value,
            Err(error) => {
                self.error(error);
                failure
            }
        }
    }
    fn advanced_current(&mut self, request: Advanced) -> Result<Value, u32> {
        use Advanced::*;
        let driver = self.driver.as_ref().unwrap();
        let gl = &driver.gl;
        macro_rules! object {
            ($id:expr,$kind:ident) => {{
                let Object::$kind(value) = self.objects.get_for_query($id, Kind::$kind)? else {
                    unreachable!()
                };
                value
            }};
        }
        unsafe {
            Ok(match request {
                GetAttachedShaders { program } => {
                    let program = object!(program, Program);
                    let count = bounded_count(
                        gl.get_program_parameter_i32(program, glow::ATTACHED_SHADERS),
                    )?;
                    let mut names = vec![0; count];
                    let mut written = 0;
                    driver.entry::<unsafe extern "system" fn(u32, i32, *mut i32, *mut u32)>(
                        c"glGetAttachedShaders",
                    )?(
                        program.0.get(),
                        count as i32,
                        &mut written,
                        names.as_mut_ptr(),
                    );
                    names.truncate(bounded_count(written)?.min(count));
                    Value::Values(
                        names
                            .into_iter()
                            .map(|n| {
                                self.object_value(
                                    NonZeroU32::new(n)
                                        .map(|n| Object::Shader(glow::NativeShader(n))),
                                )
                            })
                            .collect(),
                    )
                }
                GetVertexAttrib { index, name } => {
                    if index >= gl.get_parameter_i32(glow::MAX_VERTEX_ATTRIBS).max(0) as u32 {
                        return Err(glow::INVALID_VALUE);
                    }
                    let get = driver.entry::<unsafe extern "system" fn(u32, u32, *mut i32)>(
                        c"glGetVertexAttribiv",
                    )?;
                    match name {
                        glow::CURRENT_VERTEX_ATTRIB => {
                            match self.attribute_types.get(&index).copied().unwrap_or('f') {
                                'i' => {
                                    let mut values = vec![0; 4];
                                    driver.entry::<unsafe extern "system" fn(u32, u32, *mut i32)>(
                                        c"glGetVertexAttribIiv",
                                    )?(
                                        index, name, values.as_mut_ptr()
                                    );
                                    Value::Int32(values)
                                }
                                'u' => {
                                    let mut values = vec![0; 4];
                                    driver.entry::<unsafe extern "system" fn(u32, u32, *mut u32)>(
                                        c"glGetVertexAttribIuiv",
                                    )?(
                                        index, name, values.as_mut_ptr()
                                    );
                                    Value::Uint32(values)
                                }
                                _ => {
                                    let mut values = vec![0.0; 4];
                                    gl.get_vertex_attrib_parameter_f32_slice(
                                        index,
                                        name,
                                        &mut values,
                                    );
                                    Value::Float32(values)
                                }
                            }
                        }
                        glow::VERTEX_ATTRIB_ARRAY_BUFFER_BINDING => {
                            let mut value = 0;
                            get(index, name, &mut value);
                            self.object_value(
                                NonZeroU32::new(value as u32)
                                    .map(|n| Object::Buffer(glow::NativeBuffer(n))),
                            )
                        }
                        glow::VERTEX_ATTRIB_ARRAY_ENABLED
                        | glow::VERTEX_ATTRIB_ARRAY_NORMALIZED => {
                            let mut v = 0;
                            get(index, name, &mut v);
                            Value::Bool(v != 0)
                        }
                        glow::VERTEX_ATTRIB_ARRAY_INTEGER if self.version == 2 => {
                            let mut v = 0;
                            get(index, name, &mut v);
                            Value::Bool(v != 0)
                        }
                        glow::VERTEX_ATTRIB_ARRAY_SIZE
                        | glow::VERTEX_ATTRIB_ARRAY_STRIDE
                        | glow::VERTEX_ATTRIB_ARRAY_TYPE => {
                            let mut v = 0;
                            get(index, name, &mut v);
                            Value::Int(v)
                        }
                        glow::VERTEX_ATTRIB_ARRAY_DIVISOR
                            if self.version == 2
                                || self.extensions.contains("ANGLE_instanced_arrays") =>
                        {
                            let mut v = 0;
                            get(index, name, &mut v);
                            Value::Int(v)
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    }
                }
                GetVertexAttribOffset { index, name } => {
                    if name != glow::VERTEX_ATTRIB_ARRAY_POINTER {
                        return Err(glow::INVALID_ENUM);
                    }
                    if index >= gl.get_parameter_i32(glow::MAX_VERTEX_ATTRIBS).max(0) as u32 {
                        return Err(glow::INVALID_VALUE);
                    }
                    let mut pointer = std::ptr::null_mut();
                    driver
                        .entry::<unsafe extern "system" fn(u32, u32, *mut *mut std::ffi::c_void)>(
                            c"glGetVertexAttribPointerv",
                        )?(index, name, &mut pointer);
                    Value::Number(pointer as usize as f64)
                }
                GetFramebufferAttachmentParameter {
                    target,
                    attachment,
                    name,
                } => {
                    if !valid_framebuffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    if self.default_bound(target) { return self.default_attachment_parameter(attachment,name); }
                    framebuffer_parameter(name, attachment, self.version,
                        self.extensions.contains("EXT_color_buffer_half_float"))?;
                    if name == glow::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME {
                        let kind = gl.get_framebuffer_attachment_parameter_i32(
                            target,
                            attachment,
                            glow::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE,
                        ) as u32;
                        let native = gl
                            .get_framebuffer_attachment_parameter_i32(target, attachment, name)
                            as u32;
                        let object = NonZeroU32::new(native).and_then(|n| match kind {
                            glow::TEXTURE => Some(Object::Texture(glow::NativeTexture(n))),
                            glow::RENDERBUFFER => {
                                Some(Object::Renderbuffer(glow::NativeRenderbuffer(n)))
                            }
                            _ => None,
                        });
                        self.object_value(object)
                    } else {
                        Value::Int(
                            gl.get_framebuffer_attachment_parameter_i32(target, attachment, name),
                        )
                    }
                }
                GetQuery { target, name } => {
                    if !query_target(target) || name != glow::CURRENT_QUERY {
                        return Err(glow::INVALID_ENUM);
                    }
                    let mut value = 0;
                    driver
                        .entry::<unsafe extern "system" fn(u32, u32, *mut i32)>(c"glGetQueryiv")?(
                        target, name, &mut value,
                    );
                    self.object_value(
                        NonZeroU32::new(value as u32).map(|n| Object::Query(glow::NativeQuery(n))),
                    )
                }
                GetQueryParameter { id, name } => {
                    if !matches!(name, glow::QUERY_RESULT | glow::QUERY_RESULT_AVAILABLE) {
                        return Err(glow::INVALID_ENUM);
                    }
                    let value = gl.get_query_parameter_u32(object!(id, Query), name);
                    if name == glow::QUERY_RESULT_AVAILABLE {
                        Value::Bool(value != 0)
                    } else {
                        Value::UInt(value)
                    }
                }
                GetSamplerParameter { id, name } => {
                    let sampler = object!(id, Sampler);
                    match name {
                        glow::TEXTURE_MIN_LOD | glow::TEXTURE_MAX_LOD => {
                            Value::Float(gl.get_sampler_parameter_f32(sampler, name))
                        }
                        glow::TEXTURE_COMPARE_FUNC
                        | glow::TEXTURE_COMPARE_MODE
                        | glow::TEXTURE_MAG_FILTER
                        | glow::TEXTURE_MIN_FILTER
                        | glow::TEXTURE_WRAP_R
                        | glow::TEXTURE_WRAP_S
                        | glow::TEXTURE_WRAP_T => {
                            Value::Int(gl.get_sampler_parameter_i32(sampler, name))
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    }
                }
                FenceSync { condition, flags } => {
                    if condition != glow::SYNC_GPU_COMMANDS_COMPLETE {
                        return Err(glow::INVALID_ENUM);
                    }
                    if flags != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    let sync = gl
                        .fence_sync(condition, flags)
                        .map_err(|_| glow::OUT_OF_MEMORY)?;
                    match self.objects.insert(Object::Sync(sync)) {
                        Ok(id) => Value::UInt(id),
                        Err(error) => {
                            gl.delete_sync(sync);
                            return Err(error);
                        }
                    }
                }
                ClientWaitSync { id, flags, timeout } => {
                    if flags & !glow::SYNC_FLUSH_COMMANDS_BIT != 0 || timeout != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    Value::UInt(gl.client_wait_sync(object!(id, Sync), flags, 0))
                }
                WaitSync { id, flags, timeout } => {
                    if flags != 0 || timeout != -1 {
                        return Err(glow::INVALID_VALUE);
                    }
                    gl.wait_sync(object!(id, Sync), 0, glow::TIMEOUT_IGNORED);
                    Value::Null
                }
                GetSyncParameter { id, name } => {
                    if !matches!(
                        name,
                        glow::OBJECT_TYPE
                            | glow::SYNC_STATUS
                            | glow::SYNC_CONDITION
                            | glow::SYNC_FLAGS
                    ) {
                        return Err(glow::INVALID_ENUM);
                    }
                    Value::Int(gl.get_sync_parameter_i32(object!(id, Sync), name))
                }
                GetTransformFeedbackVarying { program, index } => {
                    let program = self.program(program)?;
                    // WebGL rejects these queries before index validation when
                    // the program is unlinked, including a failed relink.
                    if !gl.get_program_link_status(program) {
                        return Err(glow::INVALID_OPERATION);
                    }
                    let count = bounded_count(gl.get_program_parameter_i32(program, glow::TRANSFORM_FEEDBACK_VARYINGS))?;
                    if index as usize >= count { return Err(glow::INVALID_VALUE); }
                    // glow 0.17 uses a fixed 256-byte buffer here. GLES exposes
                    // the actual maximum including NUL; valid longer names must
                    // retain every byte rather than silently truncating.
                    let capacity = bounded_count(gl.get_program_parameter_i32(program, glow::TRANSFORM_FEEDBACK_VARYING_MAX_LENGTH))?;
                    if capacity == 0 { return Err(glow::INVALID_OPERATION); }
                    let mut bytes = vec![0_u8; capacity];
                    let (mut written, mut size, mut data_type) = (0, 0, 0);
                    driver.entry::<unsafe extern "system" fn(u32, u32, i32, *mut i32, *mut i32, *mut u32, *mut std::ffi::c_char)>(
                        c"glGetTransformFeedbackVarying",
                    )?(program.0.get(), index, capacity as i32, &mut written, &mut size, &mut data_type, bytes.as_mut_ptr().cast());
                    let name = native_output_name(&bytes, written)?;
                    Value::Active { size, data_type, name }
                },
                GetIndexedParameter { target, index } => {
                    let (limit, object) = match target {
                        glow::UNIFORM_BUFFER_BINDING => (glow::MAX_UNIFORM_BUFFER_BINDINGS, true),
                        glow::UNIFORM_BUFFER_START | glow::UNIFORM_BUFFER_SIZE => {
                            (glow::MAX_UNIFORM_BUFFER_BINDINGS, false)
                        }
                        glow::TRANSFORM_FEEDBACK_BUFFER_BINDING => {
                            (glow::MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS, true)
                        }
                        glow::TRANSFORM_FEEDBACK_BUFFER_START
                        | glow::TRANSFORM_FEEDBACK_BUFFER_SIZE => {
                            (glow::MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS, false)
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    };
                    if index >= gl.get_parameter_i32(limit).max(0) as u32 {
                        return Err(glow::INVALID_VALUE);
                    }
                    if object {
                        self.object_value(
                            NonZeroU32::new(gl.get_parameter_indexed_i32(target, index) as u32)
                                .map(|n| Object::Buffer(glow::NativeBuffer(n))),
                        )
                    } else {
                        Value::Number(gl.get_parameter_indexed_i64(target, index) as f64)
                    }
                }
                GetUniformIndices { program, names } => {
                    let names: Vec<_> = names.iter().map(|name| native_name(name)).collect();
                    Value::UInts(
                        gl.get_uniform_indices(object!(program, Program), &names)
                            .into_iter()
                            .map(|n| n.unwrap_or(glow::INVALID_INDEX))
                            .collect(),
                    )
                }
                GetActiveUniforms {
                    program,
                    indices,
                    name,
                } => {
                    if !matches!(
                        name,
                        glow::UNIFORM_TYPE
                            | glow::UNIFORM_SIZE
                            | glow::UNIFORM_BLOCK_INDEX
                            | glow::UNIFORM_OFFSET
                            | glow::UNIFORM_ARRAY_STRIDE
                            | glow::UNIFORM_MATRIX_STRIDE
                            | glow::UNIFORM_IS_ROW_MAJOR
                    ) {
                        return Err(glow::INVALID_ENUM);
                    }
                    let values =
                        gl.get_active_uniforms_parameter(object!(program, Program), &indices, name);
                    if name == glow::UNIFORM_IS_ROW_MAJOR {
                        Value::Bools(values.into_iter().map(|v| v != 0).collect())
                    } else {
                        Value::Ints(values)
                    }
                }
                GetFragDataLocation { program, name } => {
                    let program = self.program(program)?;
                    if !gl.get_program_link_status(program) {
                        return Err(glow::INVALID_OPERATION);
                    }
                    Value::Int(gl.get_frag_data_location(program, native_name(&name)))
                }
                GetUniformBlockIndex { program, name } => {
                    if !valid_shader_name(&name) {
                        return Err(glow::INVALID_VALUE);
                    }
                    Value::UInt(
                        gl.get_uniform_block_index(object!(program, Program), &name)
                            .unwrap_or(glow::INVALID_INDEX),
                    )
                }
                GetActiveUniformBlockName { program, index } => {
                    let program = self.program(program)?;
                    if !gl.get_program_link_status(program) {
                        return Err(glow::INVALID_OPERATION);
                    }
                    Value::String(gl.get_active_uniform_block_name(program, index))
                }
                GetActiveUniformBlockParameter {
                    program,
                    index,
                    name,
                } => {
                    let program = self.program(program)?;
                    if !gl.get_program_link_status(program) {
                        return Err(glow::INVALID_OPERATION);
                    }
                    match name {
                        glow::UNIFORM_BLOCK_BINDING
                        | glow::UNIFORM_BLOCK_DATA_SIZE
                        | glow::UNIFORM_BLOCK_ACTIVE_UNIFORMS => Value::Int(
                            gl.get_active_uniform_block_parameter_i32(program, index, name),
                        ),
                        glow::UNIFORM_BLOCK_REFERENCED_BY_VERTEX_SHADER
                        | glow::UNIFORM_BLOCK_REFERENCED_BY_FRAGMENT_SHADER => Value::Bool(
                            gl.get_active_uniform_block_parameter_i32(program, index, name) != 0,
                        ),
                        glow::UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES => {
                            let count = bounded_count(gl.get_active_uniform_block_parameter_i32(
                                program,
                                index,
                                glow::UNIFORM_BLOCK_ACTIVE_UNIFORMS,
                            ))?;
                            let mut values = vec![0; count];
                            gl.get_active_uniform_block_parameter_i32_slice(
                                program,
                                index,
                                name,
                                &mut values,
                            );
                            Value::Uint32(values.into_iter().map(|v| v as u32).collect())
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    }
                }
                GetInternalformatParameter {
                    target,
                    format,
                    name,
                } => {
                    if target != glow::RENDERBUFFER || name != glow::SAMPLES {
                        return Err(glow::INVALID_ENUM);
                    }
                    let mut count = [0];
                    gl.get_internal_format_i32_slice(
                        target,
                        format,
                        glow::NUM_SAMPLE_COUNTS,
                        &mut count,
                    );
                    let mut values = vec![0; bounded_count(count[0])?];
                    gl.get_internal_format_i32_slice(target, format, name, &mut values);
                    Value::Int32(values)
                }
                DrawRangeElements {
                    mode,
                    start,
                    end,
                    count,
                    data_type,
                    offset,
                } => {
                    if start > end || offset < 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    driver.entry::<unsafe extern "system" fn(
                        u32,
                        u32,
                        u32,
                        i32,
                        u32,
                        *const std::ffi::c_void,
                    )>(c"glDrawRangeElements")?(
                        mode,
                        start,
                        end,
                        count,
                        data_type,
                        offset as usize as *const _,
                    );
                    if self.default_bound(glow::DRAW_FRAMEBUFFER)
                    {
                        self.dirty = true;
                    }
                    Value::Null
                }
                ClearBufferFloat {
                    buffer,
                    index,
                    values,
                } => {
                    clear_values(buffer, index, values.len(), 'f')?;
                    gl.clear_buffer_f32_slice(buffer, index as u32, &values);
                    if self.default_bound(glow::DRAW_FRAMEBUFFER)
                    {
                        self.dirty = true;
                    }
                    Value::Null
                }
                ClearBufferInt {
                    buffer,
                    index,
                    values,
                } => {
                    clear_values(buffer, index, values.len(), 'i')?;
                    gl.clear_buffer_i32_slice(buffer, index as u32, &values);
                    if self.default_bound(glow::DRAW_FRAMEBUFFER)
                    {
                        self.dirty = true;
                    }
                    Value::Null
                }
                ClearBufferUint {
                    buffer,
                    index,
                    values,
                } => {
                    clear_values(buffer, index, values.len(), 'u')?;
                    gl.clear_buffer_u32_slice(buffer, index as u32, &values);
                    if self.default_bound(glow::DRAW_FRAMEBUFFER)
                    {
                        self.dirty = true;
                    }
                    Value::Null
                }
                ClearBufferDepthStencil {
                    buffer,
                    index,
                    depth,
                    stencil,
                } => {
                    if buffer != glow::DEPTH_STENCIL {
                        return Err(glow::INVALID_ENUM);
                    }
                    if index != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    gl.clear_buffer_depth_stencil(buffer, index as u32, depth, stencil);
                    if self.default_bound(glow::DRAW_FRAMEBUFFER)
                    {
                        self.dirty = true;
                    }
                    Value::Null
                }
            })
        }
    }
}
fn framebuffer_parameter(name: u32, attachment: u32, version: u8, half_float: bool) -> Result<(), u32> {
    if name == glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE
        && (version == 2 || half_float)
        && attachment == glow::DEPTH_STENCIL_ATTACHMENT
    {
        return Err(glow::INVALID_OPERATION);
    }
    let valid = matches!(name,
        glow::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME | glow::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE
        | glow::FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL | glow::FRAMEBUFFER_ATTACHMENT_TEXTURE_CUBE_MAP_FACE)
        || name == glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE && half_float
        || version == 2 && matches!(name,
            glow::FRAMEBUFFER_ATTACHMENT_RED_SIZE | glow::FRAMEBUFFER_ATTACHMENT_GREEN_SIZE
            | glow::FRAMEBUFFER_ATTACHMENT_BLUE_SIZE | glow::FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE
            | glow::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE | glow::FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE
            | glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE | glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING
            | glow::FRAMEBUFFER_ATTACHMENT_TEXTURE_LAYER);
    if valid { Ok(()) } else { Err(glow::INVALID_ENUM) }
}
fn clear_values(buffer: u32, index: i32, length: usize, kind: char) -> Result<(), u32> {
    let count = match (buffer, kind) {
        (glow::COLOR, _) => 4,
        (glow::DEPTH, 'f') | (glow::STENCIL, 'i') => 1,
        _ => return Err(glow::INVALID_ENUM),
    };
    if index < 0 || buffer != glow::COLOR && index != 0 || length < count {
        Err(glow::INVALID_VALUE)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advanced_failures_preserve_offsets_indices_and_wait_status_types() {
        assert_eq!(Advanced::GetVertexAttribOffset { index: 0, name: 0 }.failure_value(), Value::Number(0.0));
        assert_eq!(Advanced::GetFragDataLocation { program: 0, name: String::new() }.failure_value(), Value::Int(-1));
        assert_eq!(Advanced::GetUniformBlockIndex { program: 0, name: String::new() }.failure_value(), Value::UInt(0));
        assert_eq!(Advanced::ClientWaitSync { id: 0, flags: 0, timeout: 0 }.failure_value(), Value::UInt(glow::WAIT_FAILED));
        assert_eq!(Advanced::GetAttachedShaders { program: 0 }.failure_value(), Value::Null);
        assert_eq!(Advanced::GetFragDataLocation { program: 0, name: String::new() }.minimum_version(), 2);
    }
    #[test]
    fn framebuffer_component_type_requires_half_float_or_webgl_two() {
        for version in [1, 2] {
            for enabled in [false, true] {
                let allowed = version == 2 || enabled;
                assert_eq!(framebuffer_parameter(glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE,
                    glow::COLOR_ATTACHMENT0, version, enabled),
                    if allowed { Ok(()) } else { Err(glow::INVALID_ENUM) });
                assert_eq!(framebuffer_parameter(glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE,
                    glow::DEPTH_STENCIL_ATTACHMENT, version, enabled),
                    Err(if allowed { glow::INVALID_OPERATION } else { glow::INVALID_ENUM }));
                assert_eq!(framebuffer_parameter(glow::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME,
                    glow::COLOR_ATTACHMENT0, version, enabled), Ok(()));
                assert_eq!(framebuffer_parameter(glow::FRAMEBUFFER_ATTACHMENT_TEXTURE_LAYER,
                    glow::COLOR_ATTACHMENT0, version, enabled),
                    if version == 2 { Ok(()) } else { Err(glow::INVALID_ENUM) });
                assert_eq!(framebuffer_parameter(0, glow::COLOR_ATTACHMENT0, version, enabled), Err(glow::INVALID_ENUM));
            }
        }
    }
    #[test]
    fn native_output_counts_are_bounded_and_never_cast_negative_to_large_allocations() {
        assert!(bounded_count(-1).is_err());
        assert!(bounded_count(65537).is_err());
        assert_eq!(bounded_count(0), Ok(0));
        assert_eq!(bounded_count(4), Ok(4));
    }
    #[test]
    fn native_output_names_preserve_long_names_and_reject_invalid_lengths() {
        let name = format!("v{}", "x".repeat(256));
        let bytes = [name.as_bytes(), &[0]].concat();
        assert_eq!(native_output_name(&bytes, 257), Ok(name));
        assert_eq!(native_output_name(b"\0", 0), Ok(String::new()));
        for (bytes, length) in [(&b"x\0"[..], -1), (&b"x\0"[..], 2), (&b"xx"[..], 1), (&b"x\0x\0"[..], 3), (&b"\xff\0"[..], 1)] {
            assert_eq!(native_output_name(bytes, length), Err(glow::INVALID_OPERATION));
        }
    }
    #[test]
    fn clear_buffer_requires_all_components_before_passing_a_pointer() {
        assert!(clear_values(glow::COLOR, 0, 3, 'f').is_err());
        assert!(clear_values(glow::COLOR, 0, 4, 'u').is_ok());
        assert!(clear_values(glow::DEPTH, 0, 1, 'f').is_ok());
        assert!(clear_values(glow::STENCIL, 0, 1, 'i').is_ok());
        assert!(clear_values(glow::DEPTH, 0, 1, 'u').is_err());
        assert!(clear_values(glow::DEPTH, 1, 4, 'f').is_err());
        assert!(clear_values(glow::COLOR, -1, 4, 'f').is_err());
    }
}
