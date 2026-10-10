//! Typed, bounded browser queries. A page cannot select a multi-value GL query
//! for a one-element native destination or receive a native driver object name.
use crate::{
    api::CanvasContext,
    commands::valid_capability,
    objects::{Kind, Object},
};
use glow::HasContext;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i32),
    UInt(u32),
    Float(f32),
    Number(f64),
    String(String),
    Ints(Vec<i32>),
    UInts(Vec<u32>),
    Values(Vec<Value>),
    Float32(Vec<f32>),
    Int32(Vec<i32>),
    Uint32(Vec<u32>),
    Bools(Vec<bool>),
    Object {
        kind: Kind,
        id: u32,
    },
    Active {
        size: i32,
        data_type: u32,
        name: String,
    },
    Precision {
        range_min: i32,
        range_max: i32,
        precision: i32,
    },
}
#[derive(Debug, Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum Query {
    GetParameter {
        name: u32,
    },
    IsEnabled {
        cap: u32,
    },
    IsObject {
        kind: Kind,
        id: u32,
    },
    GetShaderParameter {
        shader: u32,
        name: u32,
    },
    GetShaderInfoLog {
        shader: u32,
    },
    GetShaderSource {
        shader: u32,
    },
    GetShaderPrecisionFormat {
        shader_type: u32,
        precision_type: u32,
    },
    GetProgramParameter {
        program: u32,
        name: u32,
    },
    GetProgramInfoLog {
        program: u32,
    },
    GetActiveAttrib {
        program: u32,
        index: u32,
    },
    GetActiveUniform {
        program: u32,
        index: u32,
    },
    GetAttribLocation {
        program: u32,
        name: String,
    },
    GetUniformLocation {
        program: u32,
        name: String,
    },
    GetBufferParameter {
        target: u32,
        name: u32,
    },
    GetRenderbufferParameter {
        target: u32,
        name: u32,
    },
    GetTexParameter {
        target: u32,
        name: u32,
    },
    CheckFramebufferStatus {
        target: u32,
    },
}
impl Query {
    fn failure_value(&self, lost: bool) -> Value {
        match self {
            Self::IsEnabled { .. } | Self::IsObject { .. } => Value::Bool(false),
            Self::GetAttribLocation { .. } => Value::Int(-1),
            Self::CheckFramebufferStatus { .. } =>
                Value::UInt(if lost { glow::FRAMEBUFFER_UNSUPPORTED } else { 0 }),
            _ => Value::Null,
        }
    }
}
impl CanvasContext {
    pub fn query(&mut self, query: Query) -> Value {
        if !self.activate() {
            return query.failure_value(true);
        }
        let failure = query.failure_value(false);
        match self.query_current(query) {
            Ok(value) => value,
            Err(error) => {
                self.error(error);
                failure
            }
        }
    }
    pub(crate) fn object_value(&self, object: Option<Object>) -> Value {
        // GL may reuse a deleted page object's integer for private storage.
        // Tombstones must never turn the logical default FBO into that wrapper.
        if let (Some(Object::Framebuffer(name)),Some(storage))=(object,self.drawing_storage.as_ref()) {
            if name==storage.draw || name==storage.read { return Value::Null; }
        }
        object
            .and_then(|object| {
                self.objects.id_for(object).map(|id| Value::Object {
                    kind: object.kind(),
                    id,
                })
            })
            .unwrap_or(Value::Null)
    }
    fn shader(&self, id: u32) -> Result<glow::NativeShader, u32> {
        let Object::Shader(shader) = self.objects.get_for_query(id, Kind::Shader)? else {
            unreachable!()
        };
        if unsafe { self.driver.as_ref().unwrap().gl.is_shader(shader) } {
            Ok(shader)
        } else {
            Err(glow::INVALID_VALUE)
        }
    }
    pub(crate) fn program(&self, id: u32) -> Result<glow::NativeProgram, u32> {
        let Object::Program(program) = self.objects.get_for_query(id, Kind::Program)? else {
            unreachable!()
        };
        if unsafe { self.driver.as_ref().unwrap().gl.is_program(program) } {
            Ok(program)
        } else {
            Err(glow::INVALID_VALUE)
        }
    }
    fn query_current(&mut self, query: Query) -> Result<Value, u32> {
        use Query::*;
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            Ok(match query {
                GetParameter { name } => self.parameter(name)?,
                IsEnabled { cap } => {
                    if !valid_capability(cap, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    Value::Bool(gl.is_enabled(cap))
                }
                IsObject { kind, id } => {
                    if id == 0 {
                        return Ok(Value::Bool(false));
                    }
                    let Ok(object) = self.objects.get_for_query(id, kind) else {
                        return Ok(Value::Bool(false));
                    };
                    Value::Bool(match object {
                        Object::Buffer(v) => gl.is_buffer(v),
                        Object::Texture(v) => gl.is_texture(v),
                        Object::Shader(v) => gl.is_shader(v),
                        Object::Program(v) => gl.is_program(v),
                        Object::Framebuffer(v) => gl.is_framebuffer(v),
                        Object::Renderbuffer(v) => gl.is_renderbuffer(v),
                        Object::VertexArray(v) => {
                            self.driver
                                .as_ref()
                                .unwrap()
                                .entry::<unsafe extern "system" fn(u32) -> u8>(c"glIsVertexArray")?(
                                v.0.get(),
                            ) != 0
                        }
                        Object::Query(v) => {
                            self.driver
                                .as_ref()
                                .unwrap()
                                .entry::<unsafe extern "system" fn(u32) -> u8>(c"glIsQuery")?(
                                v.0.get(),
                            ) != 0
                        }
                        Object::Sampler(v) => {
                            self.driver
                                .as_ref()
                                .unwrap()
                                .entry::<unsafe extern "system" fn(u32) -> u8>(c"glIsSampler")?(
                                v.0.get(),
                            ) != 0
                        }
                        Object::TransformFeedback(v) => gl.is_transform_feedback(v),
                        Object::Sync(v) => gl.is_sync(v),
                    })
                }
                GetShaderParameter { shader, name } => {
                    let native = self.shader(shader)?;
                    match name {
                        glow::COMPILE_STATUS => Value::Bool(gl.get_shader_compile_status(native)),
                        glow::DELETE_STATUS => Value::Bool(self.objects.deleted(shader)),
                        glow::SHADER_TYPE => {
                            Value::UInt(*self.shader_types.get(&shader).ok_or(glow::INVALID_VALUE)?)
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    }
                }
                GetShaderInfoLog { shader } => {
                    Value::String(gl.get_shader_info_log(self.shader(shader)?))
                }
                GetShaderSource { shader } => {
                    self.shader(shader)?;
                    Value::String(
                        self.shader_sources
                            .get(&shader)
                            .cloned()
                            .unwrap_or_default(),
                    )
                }
                GetShaderPrecisionFormat {
                    shader_type,
                    precision_type,
                } => {
                    if ![glow::VERTEX_SHADER, glow::FRAGMENT_SHADER].contains(&shader_type)
                        || ![
                            glow::LOW_FLOAT,
                            glow::MEDIUM_FLOAT,
                            glow::HIGH_FLOAT,
                            glow::LOW_INT,
                            glow::MEDIUM_INT,
                            glow::HIGH_INT,
                        ]
                        .contains(&precision_type)
                    {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.get_shader_precision_format(shader_type, precision_type)
                        .map(|p| Value::Precision {
                            range_min: p.range_min,
                            range_max: p.range_max,
                            precision: p.precision,
                        })
                        .unwrap_or(Value::Null)
                }
                GetProgramParameter { program, name } => {
                    let native = self.program(program)?;
                    match name {
                        glow::LINK_STATUS => Value::Bool(gl.get_program_link_status(native)),
                        glow::VALIDATE_STATUS => {
                            Value::Bool(gl.get_program_validate_status(native))
                        }
                        glow::DELETE_STATUS => Value::Bool(self.objects.deleted(program)),
                        glow::ATTACHED_SHADERS
                        | glow::ACTIVE_ATTRIBUTES
                        | glow::ACTIVE_UNIFORMS => {
                            Value::Int(gl.get_program_parameter_i32(native, name))
                        }
                        glow::ACTIVE_UNIFORM_BLOCKS
                        | glow::TRANSFORM_FEEDBACK_VARYINGS
                        | glow::TRANSFORM_FEEDBACK_BUFFER_MODE
                            if self.version == 2 =>
                        {
                            Value::Int(gl.get_program_parameter_i32(native, name))
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    }
                }
                GetProgramInfoLog { program } => {
                    Value::String(gl.get_program_info_log(self.program(program)?))
                }
                GetActiveAttrib { program, index } => gl
                    .get_active_attribute(self.program(program)?, index)
                    .map(|v| Value::Active {
                        size: v.size,
                        data_type: v.atype,
                        name: v.name,
                    })
                    .unwrap_or(Value::Null),
                GetActiveUniform { program, index } => gl
                    .get_active_uniform(self.program(program)?, index)
                    .map(|v| Value::Active {
                        size: v.size,
                        data_type: v.utype,
                        name: v.name,
                    })
                    .unwrap_or(Value::Null),
                GetAttribLocation { program, name } => {
                    if !valid_location_name(&name, self.version) {
                        return Err(glow::INVALID_VALUE);
                    }
                    Value::Int(
                        gl.get_attrib_location(self.program(program)?, &name)
                            .map(|v| v as i32)
                            .unwrap_or(-1),
                    )
                }
                GetUniformLocation { program, name } => {
                    if !valid_location_name(&name, self.version) {
                        return Err(glow::INVALID_VALUE);
                    }
                    match gl.get_uniform_location(self.program(program)?, &name) {
                        Some(location) => {
                            Value::UInt(self.objects.insert_location(program, location)?)
                        }
                        None => Value::Null,
                    }
                }
                GetBufferParameter { target, name } => {
                    if ![glow::BUFFER_SIZE, glow::BUFFER_USAGE].contains(&name) {
                        return Err(glow::INVALID_ENUM);
                    }
                    if !crate::resources::valid_buffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    Value::Int(gl.get_buffer_parameter_i32(target, name))
                }
                GetRenderbufferParameter { target, name } => {
                    if target != glow::RENDERBUFFER
                        || !matches!(
                            name,
                            glow::RENDERBUFFER_WIDTH
                                | glow::RENDERBUFFER_HEIGHT
                                | glow::RENDERBUFFER_INTERNAL_FORMAT
                                | glow::RENDERBUFFER_RED_SIZE
                                | glow::RENDERBUFFER_GREEN_SIZE
                                | glow::RENDERBUFFER_BLUE_SIZE
                                | glow::RENDERBUFFER_ALPHA_SIZE
                                | glow::RENDERBUFFER_DEPTH_SIZE
                                | glow::RENDERBUFFER_STENCIL_SIZE
                        ) && !(self.version == 2 && name == glow::RENDERBUFFER_SAMPLES)
                    {
                        return Err(glow::INVALID_ENUM);
                    }
                    Value::Int(gl.get_renderbuffer_parameter_i32(target, name))
                }
                GetTexParameter { target, name } => {
                    if !crate::resources::valid_texture_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    match name {
                        0x84FE if self.extensions.contains("EXT_texture_filter_anisotropic") => {
                            Value::Float(gl.get_tex_parameter_f32(target, name))
                        }
                        glow::TEXTURE_MAG_FILTER
                        | glow::TEXTURE_MIN_FILTER
                        | glow::TEXTURE_WRAP_S
                        | glow::TEXTURE_WRAP_T => {
                            Value::Int(gl.get_tex_parameter_i32(target, name))
                        }
                        glow::TEXTURE_BASE_LEVEL
                        | glow::TEXTURE_MAX_LEVEL
                        | glow::TEXTURE_COMPARE_FUNC
                        | glow::TEXTURE_COMPARE_MODE
                        | glow::TEXTURE_WRAP_R
                        | glow::TEXTURE_IMMUTABLE_LEVELS
                            if self.version == 2 =>
                        {
                            Value::Int(gl.get_tex_parameter_i32(target, name))
                        }
                        glow::TEXTURE_IMMUTABLE_FORMAT if self.version == 2 => {
                            Value::Bool(gl.get_tex_parameter_i32(target, name) != 0)
                        }
                        glow::TEXTURE_MIN_LOD | glow::TEXTURE_MAX_LOD if self.version == 2 => {
                            Value::Float(gl.get_tex_parameter_f32(target, name))
                        }
                        _ => return Err(glow::INVALID_ENUM),
                    }
                }
                CheckFramebufferStatus { target } => {
                    if !crate::resources::valid_framebuffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    Value::UInt(gl.check_framebuffer_status(target))
                }
            })
        }
    }
    fn parameter(&self, name: u32) -> Result<Value, u32> {
        if let Some(value) = self.extension_parameter(name) {
            return Ok(value);
        }
        let gl = &self.driver.as_ref().unwrap().gl;
        if self.version == 2 {
            if name == glow::READ_BUFFER && self.default_bound(glow::READ_FRAMEBUFFER) {
                return Ok(Value::Int(self.default_read_buffer as i32));
            }
            if (glow::DRAW_BUFFER0..=glow::DRAW_BUFFER15).contains(&name) && self.default_bound(glow::DRAW_FRAMEBUFFER) {
                return Ok(Value::Int(if name==glow::DRAW_BUFFER0 {self.default_draw_buffer} else {glow::NONE} as i32));
            }
            if name == 0x9247 {
                return Ok(Value::Number(0.0));
            } // MAX_CLIENT_WAIT_TIMEOUT_WEBGL
            if matches!(
                name,
                glow::MAX_ELEMENT_INDEX
                    | glow::MAX_SERVER_WAIT_TIMEOUT
                    | glow::MAX_COMBINED_FRAGMENT_UNIFORM_COMPONENTS
                    | glow::MAX_COMBINED_VERTEX_UNIFORM_COMPONENTS
                    | glow::MAX_UNIFORM_BLOCK_SIZE
            ) {
                return Ok(Value::Number(unsafe { gl.get_parameter_i64(name) } as f64));
            }
        }
        unsafe {
            Ok(match parameter_kind(name, self.version)? {
                ParameterKind::Bool => Value::Bool(gl.get_parameter_bool(name)),
                ParameterKind::Int => Value::Int(gl.get_parameter_i32(name)),
                ParameterKind::Float => Value::Float(gl.get_parameter_f32(name)),
                ParameterKind::Ints(n) => {
                    let mut values = vec![0; n];
                    gl.get_parameter_i32_slice(name, &mut values);
                    Value::Int32(values)
                }
                ParameterKind::Floats(n) => {
                    let mut values = vec![0.0; n];
                    gl.get_parameter_f32_slice(name, &mut values);
                    Value::Float32(values)
                }
                ParameterKind::ColorMask => {
                    Value::Bools(gl.get_parameter_bool_array::<4>(name).to_vec())
                }
                ParameterKind::Buffer => {
                    self.object_value(gl.get_parameter_buffer(name).map(Object::Buffer))
                }
                ParameterKind::Texture => {
                    self.object_value(gl.get_parameter_texture(name).map(Object::Texture))
                }
                ParameterKind::Framebuffer => {
                    self.object_value(gl.get_parameter_framebuffer(name).map(Object::Framebuffer))
                }
                ParameterKind::Renderbuffer => self.object_value(
                    gl.get_parameter_renderbuffer(name)
                        .map(Object::Renderbuffer),
                ),
                ParameterKind::Program => {
                    self.object_value(gl.get_parameter_program(name).map(Object::Program))
                }
                ParameterKind::VertexArray => {
                    self.object_value(gl.get_parameter_vertex_array(name).map(Object::VertexArray))
                }
                ParameterKind::Sampler => {
                    self.object_value(gl.get_parameter_sampler(name).map(Object::Sampler))
                }
                ParameterKind::TransformFeedback => self.object_value(
                    gl.get_parameter_transform_feedback(name)
                        .map(Object::TransformFeedback),
                ),
                ParameterKind::Version => Value::String(format!(
                    "WebGL {}.0 (OpenGL ES {})",
                    self.version,
                    if self.version == 1 { "2.0" } else { "3.0" }
                )),
                ParameterKind::ShaderVersion => Value::String(format!(
                    "WebGL GLSL ES {}.00",
                    if self.version == 1 { 1 } else { 3 }
                )),
                // The unmasked driver strings remain available to host diagnostics.
                // Do not invent a hardware model or overwrite stealth identity.
                ParameterKind::Vendor => Value::String("WebKit".into()),
                ParameterKind::Renderer => Value::String("WebKit WebGL".into()),
                ParameterKind::CompressedFormats => Value::Uint32(Vec::new()),
            })
        }
    }
}

// Location limits apply to bindAttribLocation and the two location queries,
// not every GLES entry point taking a string. Accepted characters are ASCII,
// so byte lengths and DOMString character lengths are identical here.
pub(crate) fn valid_location_name(name: &str, version: u8) -> bool {
    let limit = if version == 2 { 1024 } else { 256 };
    name.len() <= limit && valid_shader_name(name)
}
pub(crate) fn valid_shader_name(name: &str) -> bool {
    name.bytes().all(|c| matches!(c, 9..=13)
        || ((32..=126).contains(&c) && !b"\"$`@\\'".contains(&c)))
}
// GLES string entry points without WebGL's source-character validation use
// null-terminated UTF-8. Strip the suffix before glow constructs a CString;
// passing an interior NUL to glow would panic across the native boundary.
pub(crate) fn native_name(name: &str) -> &str {
    name.split('\0').next().unwrap_or("")
}
#[derive(Debug, PartialEq)]
enum ParameterKind {
    Bool,
    Int,
    Float,
    Ints(usize),
    Floats(usize),
    ColorMask,
    Buffer,
    Texture,
    Framebuffer,
    Renderbuffer,
    Program,
    VertexArray,
    Sampler,
    TransformFeedback,
    Version,
    ShaderVersion,
    Vendor,
    Renderer,
    CompressedFormats,
}
fn parameter_kind(name: u32, version: u8) -> Result<ParameterKind, u32> {
    use ParameterKind::*;
    Ok(match name {
        glow::VERSION => Version,
        glow::SHADING_LANGUAGE_VERSION => ShaderVersion,
        glow::VENDOR => Vendor,
        glow::RENDERER => Renderer,
        glow::COMPRESSED_TEXTURE_FORMATS => CompressedFormats,
        glow::COLOR_WRITEMASK => ColorMask,
        glow::BLEND_COLOR | glow::COLOR_CLEAR_VALUE => Floats(4),
        glow::DEPTH_RANGE | glow::ALIASED_LINE_WIDTH_RANGE | glow::ALIASED_POINT_SIZE_RANGE => {
            Floats(2)
        }
        glow::MAX_VIEWPORT_DIMS => Ints(2),
        glow::SCISSOR_BOX | glow::VIEWPORT => Ints(4),
        glow::LINE_WIDTH
        | glow::DEPTH_CLEAR_VALUE
        | glow::POLYGON_OFFSET_FACTOR
        | glow::POLYGON_OFFSET_UNITS
        | glow::SAMPLE_COVERAGE_VALUE => Float,
        glow::BLEND
        | glow::CULL_FACE
        | glow::DEPTH_TEST
        | glow::DEPTH_WRITEMASK
        | glow::DITHER
        | glow::POLYGON_OFFSET_FILL
        | glow::SAMPLE_ALPHA_TO_COVERAGE
        | glow::SAMPLE_COVERAGE
        | glow::SAMPLE_COVERAGE_INVERT
        | glow::SCISSOR_TEST
        | glow::STENCIL_TEST => Bool,
        glow::ARRAY_BUFFER_BINDING | glow::ELEMENT_ARRAY_BUFFER_BINDING => Buffer,
        glow::TEXTURE_BINDING_2D | glow::TEXTURE_BINDING_CUBE_MAP => Texture,
        glow::FRAMEBUFFER_BINDING => Framebuffer,
        glow::RENDERBUFFER_BINDING => Renderbuffer,
        glow::CURRENT_PROGRAM => Program,
        glow::ACTIVE_TEXTURE
        | glow::ALPHA_BITS
        | glow::BLUE_BITS
        | glow::GREEN_BITS
        | glow::RED_BITS
        | glow::DEPTH_BITS
        | glow::STENCIL_BITS
        | glow::BLEND_DST_ALPHA
        | glow::BLEND_DST_RGB
        | glow::BLEND_SRC_ALPHA
        | glow::BLEND_SRC_RGB
        | glow::BLEND_EQUATION_ALPHA
        | glow::BLEND_EQUATION_RGB
        | glow::CULL_FACE_MODE
        | glow::FRONT_FACE
        | glow::DEPTH_FUNC
        | glow::GENERATE_MIPMAP_HINT
        | glow::MAX_COMBINED_TEXTURE_IMAGE_UNITS
        | glow::MAX_CUBE_MAP_TEXTURE_SIZE
        | glow::MAX_FRAGMENT_UNIFORM_VECTORS
        | glow::MAX_RENDERBUFFER_SIZE
        | glow::MAX_TEXTURE_IMAGE_UNITS
        | glow::MAX_TEXTURE_SIZE
        | glow::MAX_VARYING_VECTORS
        | glow::MAX_VERTEX_ATTRIBS
        | glow::MAX_VERTEX_TEXTURE_IMAGE_UNITS
        | glow::MAX_VERTEX_UNIFORM_VECTORS
        | glow::PACK_ALIGNMENT
        | glow::UNPACK_ALIGNMENT
        | glow::SAMPLE_BUFFERS
        | glow::SAMPLES
        | glow::STENCIL_BACK_FAIL
        | glow::STENCIL_BACK_FUNC
        | glow::STENCIL_BACK_PASS_DEPTH_FAIL
        | glow::STENCIL_BACK_PASS_DEPTH_PASS
        | glow::STENCIL_BACK_REF
        | glow::STENCIL_BACK_VALUE_MASK
        | glow::STENCIL_BACK_WRITEMASK
        | glow::STENCIL_CLEAR_VALUE
        | glow::STENCIL_FAIL
        | glow::STENCIL_FUNC
        | glow::STENCIL_PASS_DEPTH_FAIL
        | glow::STENCIL_PASS_DEPTH_PASS
        | glow::STENCIL_REF
        | glow::STENCIL_VALUE_MASK
        | glow::STENCIL_WRITEMASK => Int,
        glow::READ_FRAMEBUFFER_BINDING if version == 2 => Framebuffer,
        glow::COPY_READ_BUFFER_BINDING
        | glow::COPY_WRITE_BUFFER_BINDING
        | glow::PIXEL_PACK_BUFFER_BINDING
        | glow::PIXEL_UNPACK_BUFFER_BINDING
        | glow::UNIFORM_BUFFER_BINDING
        | glow::TRANSFORM_FEEDBACK_BUFFER_BINDING
            if version == 2 =>
        {
            Buffer
        }
        glow::TEXTURE_BINDING_3D | glow::TEXTURE_BINDING_2D_ARRAY if version == 2 => Texture,
        glow::VERTEX_ARRAY_BINDING if version == 2 => VertexArray,
        glow::SAMPLER_BINDING if version == 2 => Sampler,
        glow::TRANSFORM_FEEDBACK_BINDING if version == 2 => TransformFeedback,
        glow::RASTERIZER_DISCARD
        | glow::TRANSFORM_FEEDBACK_ACTIVE
        | glow::TRANSFORM_FEEDBACK_PAUSED
            if version == 2 =>
        {
            Bool
        }
        glow::MAX_TEXTURE_LOD_BIAS if version == 2 => Float,
        glow::MAX_3D_TEXTURE_SIZE
        | glow::MAX_ARRAY_TEXTURE_LAYERS
        | glow::MAX_COLOR_ATTACHMENTS
        | glow::MAX_COMBINED_FRAGMENT_UNIFORM_COMPONENTS
        | glow::MAX_COMBINED_UNIFORM_BLOCKS
        | glow::MAX_COMBINED_VERTEX_UNIFORM_COMPONENTS
        | glow::MAX_DRAW_BUFFERS
        | glow::MAX_ELEMENT_INDEX
        | glow::MAX_ELEMENTS_INDICES
        | glow::MAX_ELEMENTS_VERTICES
        | glow::MAX_FRAGMENT_INPUT_COMPONENTS
        | glow::MAX_FRAGMENT_UNIFORM_BLOCKS
        | glow::MAX_FRAGMENT_UNIFORM_COMPONENTS
        | glow::MAX_SAMPLES
        | glow::MAX_SERVER_WAIT_TIMEOUT
        | glow::MAX_TRANSFORM_FEEDBACK_INTERLEAVED_COMPONENTS
        | glow::MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS
        | glow::MAX_TRANSFORM_FEEDBACK_SEPARATE_COMPONENTS
        | glow::MAX_UNIFORM_BLOCK_SIZE
        | glow::MAX_UNIFORM_BUFFER_BINDINGS
        | glow::MAX_VARYING_COMPONENTS
        | glow::MAX_VERTEX_OUTPUT_COMPONENTS
        | glow::MAX_VERTEX_UNIFORM_BLOCKS
        | glow::MAX_VERTEX_UNIFORM_COMPONENTS
        | glow::MIN_PROGRAM_TEXEL_OFFSET
        | glow::MAX_PROGRAM_TEXEL_OFFSET
        | glow::PACK_ROW_LENGTH
        | glow::PACK_SKIP_PIXELS
        | glow::PACK_SKIP_ROWS
        | glow::READ_BUFFER
        | glow::UNIFORM_BUFFER_OFFSET_ALIGNMENT
        | glow::UNPACK_IMAGE_HEIGHT
        | glow::UNPACK_ROW_LENGTH
        | glow::UNPACK_SKIP_IMAGES
        | glow::UNPACK_SKIP_PIXELS
        | glow::UNPACK_SKIP_ROWS
            if version == 2 =>
        {
            Int
        }
        v if version == 2 && (glow::DRAW_BUFFER0..=glow::DRAW_BUFFER15).contains(&v) => Int,
        _ => return Err(glow::INVALID_ENUM),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_failures_preserve_scalar_types_and_loss_specific_framebuffer_status() {
        for lost in [false, true] {
            assert_eq!(Query::IsEnabled { cap: 0 }.failure_value(lost), Value::Bool(false));
            assert_eq!(Query::IsObject { kind: Kind::Buffer, id: 0 }.failure_value(lost), Value::Bool(false));
            assert_eq!(Query::GetAttribLocation { program: 0, name: String::new() }.failure_value(lost), Value::Int(-1));
            assert_eq!(Query::GetParameter { name: 0 }.failure_value(lost), Value::Null);
            assert_eq!(Query::GetUniformLocation { program: 0, name: String::new() }.failure_value(lost), Value::Null);
            assert_eq!(Query::CheckFramebufferStatus { target: 0 }.failure_value(lost),
                Value::UInt(if lost { glow::FRAMEBUFFER_UNSUPPORTED } else { 0 }));
        }
    }
    #[test]
    fn vector_queries_have_bounded_native_destinations() {
        assert_eq!(
            parameter_kind(glow::VIEWPORT, 1),
            Ok(ParameterKind::Ints(4))
        );
        assert_eq!(
            parameter_kind(glow::COLOR_CLEAR_VALUE, 2),
            Ok(ParameterKind::Floats(4))
        );
        assert_eq!(
            parameter_kind(glow::MAX_VIEWPORT_DIMS, 1),
            Ok(ParameterKind::Ints(2))
        );
        assert_eq!(
            parameter_kind(glow::COLOR_WRITEMASK, 2),
            Ok(ParameterKind::ColorMask)
        );
        assert_eq!(
            parameter_kind(glow::COMPRESSED_TEXTURE_FORMATS, 1),
            Ok(ParameterKind::CompressedFormats)
        );
        assert_eq!(parameter_kind(glow::EXTENSIONS, 2), Err(glow::INVALID_ENUM));
    }
    #[test]
    fn version_specific_queries_and_names_are_checked_before_native_calls() {
        assert!(parameter_kind(glow::MAX_3D_TEXTURE_SIZE, 1).is_err());
        assert!(parameter_kind(glow::MAX_3D_TEXTURE_SIZE, 2).is_ok());
        assert!(valid_location_name("someArray[0].field", 1));
    }
    #[test]
    fn location_name_limits_follow_the_context_version() {
        for (version, limit) in [(1, 256), (2, 1024)] {
            assert!(valid_location_name("", version));
            assert!(valid_location_name(&"a".repeat(limit), version));
            assert!(!valid_location_name(&"a".repeat(limit + 1), version));
            assert!(!valid_location_name("x\0y", version));
            assert!(!valid_location_name("aλ", version));
        }
    }
    #[test]
    fn shader_names_accept_only_the_glsl_source_character_set() {
        for byte in 0..=127u8 {
            let expected = matches!(byte, 9..=13 | 32..=33 | 35 | 37..=38
                | 40..=63 | 65..=91 | 93..=95 | 97..=126);
            assert_eq!(valid_shader_name(&(byte as char).to_string()), expected, "byte {byte}");
        }
        assert!(!valid_shader_name("λ"));
        // Block-name validation has no location-length cap.
        assert!(valid_shader_name(&"a".repeat(1025)));
    }
    #[test]
    fn native_names_preserve_utf8_and_use_c_string_termination() {
        for (input, expected) in [("", ""), ("aλ", "aλ"), ("x\0y", "x"), ("\0x", "")] {
            assert_eq!(native_name(input), expected);
        }
        let long = "a".repeat(1025);
        assert_eq!(native_name(&long), long);
    }
}
