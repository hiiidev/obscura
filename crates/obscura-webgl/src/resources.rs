//! Resource commands resolve per-context objects and validate browser buffer
//! bounds before entering ANGLE. Client-memory vertex arrays are never enabled.
use crate::{
    api::CanvasContext,
    objects::{Kind, Object},
    pixels::MAX_TRANSFER_BYTES,
    queries::{native_name, valid_location_name},
};
use glow::HasContext;
use serde::Deserialize;

pub(crate) fn valid_buffer_target(target: u32, version: u8) -> bool {
    matches!(target, glow::ARRAY_BUFFER | glow::ELEMENT_ARRAY_BUFFER)
        || version == 2
            && matches!(
                target,
                glow::COPY_READ_BUFFER
                    | glow::COPY_WRITE_BUFFER
                    | glow::PIXEL_PACK_BUFFER
                    | glow::PIXEL_UNPACK_BUFFER
                    | glow::TRANSFORM_FEEDBACK_BUFFER
                    | glow::UNIFORM_BUFFER
            )
}
pub(crate) fn valid_texture_target(target: u32, version: u8) -> bool {
    matches!(target, glow::TEXTURE_2D | glow::TEXTURE_CUBE_MAP)
        || version == 2 && matches!(target, glow::TEXTURE_3D | glow::TEXTURE_2D_ARRAY)
}
pub(crate) fn valid_framebuffer_target(target: u32, version: u8) -> bool {
    target == glow::FRAMEBUFFER
        || version == 2 && matches!(target, glow::READ_FRAMEBUFFER | glow::DRAW_FRAMEBUFFER)
}
fn valid_usage(usage: u32, version: u8) -> bool {
    matches!(
        usage,
        glow::STATIC_DRAW | glow::STREAM_DRAW | glow::DYNAMIC_DRAW
    ) || version == 2
        && matches!(
            usage,
            glow::STATIC_READ
                | glow::STATIC_COPY
                | glow::STREAM_READ
                | glow::STREAM_COPY
                | glow::DYNAMIC_READ
                | glow::DYNAMIC_COPY
        )
}
pub(crate) fn checked_range(offset: i64, length: usize, size: i64) -> Result<(), u32> {
    if offset < 0
        || size < 0
        || offset
            .checked_add(i64::try_from(length).map_err(|_| glow::INVALID_VALUE)?)
            .is_none_or(|end| end > size)
    {
        Err(glow::INVALID_VALUE)
    } else {
        Ok(())
    }
}

// A failed mapping must not dereference a null pointer, unmap somebody else's
// mapping, or modify the caller's destination. Publish bytes only after the
// driver's successful unmap confirms that the mapped contents remained valid.
unsafe fn copy_buffer_mapping(
    destination: &mut [u8],
    map: impl FnOnce() -> *const u8,
    unmap: impl FnOnce() -> bool,
) -> Result<(), u32> {
    copy_buffer_mapping_with_allocator(destination, map, unmap, |length| {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| glow::OUT_OF_MEMORY)?;
        bytes.resize(length, 0);
        Ok(bytes)
    })
}
unsafe fn copy_buffer_mapping_with_allocator(
    destination: &mut [u8],
    map: impl FnOnce() -> *const u8,
    unmap: impl FnOnce() -> bool,
    allocate: impl FnOnce(usize) -> Result<Vec<u8>, u32>,
) -> Result<(), u32> {
    if destination.is_empty() { return Ok(()); }
    if destination.len() > MAX_TRANSFER_BYTES { return Err(glow::OUT_OF_MEMORY); }
    // Reserve the destination-sized transactional copy before acquiring native
    // mapping ownership. Allocation failure cannot strand a mapped buffer.
    let mut bytes = allocate(destination.len())?;
    if bytes.len() != destination.len() { return Err(glow::OUT_OF_MEMORY); }
    let pointer = map();
    if pointer.is_null() { return Err(glow::INVALID_OPERATION); }
    bytes.copy_from_slice(std::slice::from_raw_parts(pointer,destination.len()));
    if !unmap() { return Err(glow::INVALID_OPERATION); }
    destination.copy_from_slice(&bytes);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ResourceCommand {
    BindBuffer {
        target: u32,
        id: u32,
    },
    BindTexture {
        target: u32,
        id: u32,
    },
    BindFramebuffer {
        target: u32,
        id: u32,
    },
    BindRenderbuffer {
        target: u32,
        id: u32,
    },
    BindVertexArray {
        id: u32,
    },
    BindSampler {
        unit: u32,
        id: u32,
    },
    BindTransformFeedback {
        target: u32,
        id: u32,
    },
    BindBufferBase {
        target: u32,
        index: u32,
        id: u32,
    },
    BindBufferRange {
        target: u32,
        index: u32,
        id: u32,
        offset: i32,
        size: i32,
    },
    BufferData {
        target: u32,
        size: i64,
        usage: u32,
    },
    BufferSubData {
        target: u32,
        offset: i64,
    },
    CopyBufferSubData {
        read_target: u32,
        write_target: u32,
        read_offset: i32,
        write_offset: i32,
        size: i32,
    },
    FramebufferTexture2D {
        target: u32,
        attachment: u32,
        texture_target: u32,
        id: u32,
        level: i32,
    },
    FramebufferTextureLayer {
        target: u32,
        attachment: u32,
        id: u32,
        level: i32,
        layer: i32,
    },
    FramebufferRenderbuffer {
        target: u32,
        attachment: u32,
        renderbuffer_target: u32,
        id: u32,
    },
    BindAttribLocation {
        program: u32,
        index: u32,
        name: String,
    },
    ValidateProgram {
        program: u32,
    },
    DrawBuffers {
        buffers: Vec<u32>,
    },
    InvalidateFramebuffer {
        target: u32,
        attachments: Vec<u32>,
    },
    InvalidateSubFramebuffer {
        target: u32,
        attachments: Vec<u32>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    TransformFeedbackVaryings {
        program: u32,
        varyings: Vec<String>,
        mode: u32,
    },
    BeginQuery {
        target: u32,
        id: u32,
    },
    SamplerParameteri {
        id: u32,
        name: u32,
        value: i32,
    },
    SamplerParameterf {
        id: u32,
        name: u32,
        value: f32,
    },
    UniformBlockBinding {
        program: u32,
        index: u32,
        binding: u32,
    },
}
impl ResourceCommand {
    pub fn minimum_version(&self) -> u8 {
        match self {
            Self::BindBuffer { .. }
            | Self::BindTexture { .. }
            | Self::BindFramebuffer { .. }
            | Self::BindRenderbuffer { .. }
            | Self::BufferData { .. }
            | Self::BufferSubData { .. }
            | Self::FramebufferTexture2D { .. }
            | Self::FramebufferRenderbuffer { .. }
            | Self::BindAttribLocation { .. }
            | Self::ValidateProgram { .. } => 1,
            _ => 2,
        }
    }
}
impl CanvasContext {
    pub fn resource(&mut self, command: ResourceCommand, data: Option<&[u8]>) {
        if !self.activate() {
            return;
        }
        let extension_command = matches!(command, ResourceCommand::BindVertexArray { .. })
            && self.extensions.contains("OES_vertex_array_object");
        if command.minimum_version() > self.version && !extension_command {
            self.error(glow::INVALID_OPERATION);
            return;
        }
        let query = match &command {
            ResourceCommand::BeginQuery { target, id } => Some((*target, *id)),
            _ => None,
        };
        let checkpoint = query.and_then(|_| self.begin_reference_change());
        if self.is_lost() {
            return;
        }
        if let Err(error) = self.resource_current(command, data) {
            self.error(error);
        }
        if let Some((target, id)) = query {
            if self.finish_reference_change(checkpoint) {
                self.active_queries.insert(target, id);
            }
        }
    }
    fn resource_current(
        &mut self,
        command: ResourceCommand,
        data: Option<&[u8]>,
    ) -> Result<(), u32> {
        use ResourceCommand::*;
        let gl = &self.driver.as_ref().unwrap().gl;
        // These macros only accept IDs that the browser wrapper has checked for
        // realm, context generation, and object interface. Rust checks again.
        macro_rules! object {
            ($id:expr,$kind:ident) => {{
                let Object::$kind(value) = self.objects.get($id, Kind::$kind)? else {
                    unreachable!()
                };
                value
            }};
        }
        macro_rules! optional {
            ($id:expr,$kind:ident) => {
                if $id == 0 {
                    None
                } else {
                    Some(object!($id, $kind))
                }
            };
        }
        unsafe {
            match command {
                BindBuffer { target, id } => {
                    if !valid_buffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.bind_buffer(target, optional!(id, Buffer));
                }
                BindTexture { target, id } => {
                    if !valid_texture_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.bind_texture(target, optional!(id, Texture));
                }
                BindFramebuffer { target, id } => {
                    if !valid_framebuffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.bind_framebuffer(target, if id == 0 { self.default_framebuffer() } else { optional!(id, Framebuffer) });
                }
                BindRenderbuffer { target, id } => {
                    if target != glow::RENDERBUFFER {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.bind_renderbuffer(target, optional!(id, Renderbuffer));
                }
                BindVertexArray { id } => gl.bind_vertex_array(optional!(id, VertexArray)),
                BindSampler { unit, id } => gl.bind_sampler(unit, optional!(id, Sampler)),
                BindTransformFeedback { target, id } => {
                    if target != glow::TRANSFORM_FEEDBACK {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.bind_transform_feedback(target, optional!(id, TransformFeedback));
                }
                BindBufferBase { target, index, id } => {
                    if !matches!(
                        target,
                        glow::TRANSFORM_FEEDBACK_BUFFER | glow::UNIFORM_BUFFER
                    ) {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.bind_buffer_base(target, index, optional!(id, Buffer));
                }
                BindBufferRange {
                    target,
                    index,
                    id,
                    offset,
                    size,
                } => {
                    if !matches!(
                        target,
                        glow::TRANSFORM_FEEDBACK_BUFFER | glow::UNIFORM_BUFFER
                    ) {
                        return Err(glow::INVALID_ENUM);
                    }
                    if offset < 0 || size <= 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    gl.bind_buffer_range(target, index, optional!(id, Buffer), offset, size);
                }
                BufferData {
                    target,
                    size,
                    usage,
                } => {
                    if !valid_buffer_target(target, self.version)
                        || !valid_usage(usage, self.version)
                    {
                        return Err(glow::INVALID_ENUM);
                    }
                    if size < 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    if size > MAX_TRANSFER_BYTES as i64 {
                        return Err(glow::OUT_OF_MEMORY);
                    }
                    match data {
                        Some(data) => {
                            if data.len() != size as usize {
                                return Err(glow::INVALID_VALUE);
                            }
                            gl.buffer_data_u8_slice(target, data, usage);
                        }
                        None => gl.buffer_data_size(target, size as i32, usage),
                    }
                }
                BufferSubData { target, offset } => {
                    if !valid_buffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    let data = data.ok_or(glow::INVALID_VALUE)?;
                    let size = gl.get_buffer_parameter_i32(target, glow::BUFFER_SIZE);
                    checked_range(offset, data.len(), i64::from(size))?;
                    if offset > i32::MAX as i64 {
                        return Err(glow::INVALID_VALUE);
                    }
                    gl.buffer_sub_data_u8_slice(target, offset as i32, data);
                }
                CopyBufferSubData {
                    read_target,
                    write_target,
                    read_offset,
                    write_offset,
                    size,
                } => {
                    if !valid_buffer_target(read_target, 2) || !valid_buffer_target(write_target, 2)
                    {
                        return Err(glow::INVALID_ENUM);
                    }
                    if size < 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    checked_range(
                        i64::from(read_offset),
                        size as usize,
                        i64::from(gl.get_buffer_parameter_i32(read_target, glow::BUFFER_SIZE)),
                    )?;
                    checked_range(
                        i64::from(write_offset),
                        size as usize,
                        i64::from(gl.get_buffer_parameter_i32(write_target, glow::BUFFER_SIZE)),
                    )?;
                    gl.copy_buffer_sub_data(
                        read_target,
                        write_target,
                        read_offset,
                        write_offset,
                        size,
                    );
                }
                FramebufferTexture2D {
                    target,
                    attachment,
                    texture_target,
                    id,
                    level,
                } => {
                    if !valid_framebuffer_target(target, self.version) {
                        return Err(glow::INVALID_ENUM);
                    }
                    if self.version == 1 && level != 0 {
                        return Err(glow::INVALID_VALUE);
                    }
                    if self.default_bound(target) { return Err(glow::INVALID_OPERATION); }
                    gl.framebuffer_texture_2d(
                        target,
                        attachment,
                        texture_target,
                        optional!(id, Texture),
                        level,
                    );
                }
                FramebufferTextureLayer {
                    target,
                    attachment,
                    id,
                    level,
                    layer,
                } => {
                    if !valid_framebuffer_target(target, 2) {
                        return Err(glow::INVALID_ENUM);
                    }
                    if self.default_bound(target) { return Err(glow::INVALID_OPERATION); }
                    gl.framebuffer_texture_layer(
                        target,
                        attachment,
                        optional!(id, Texture),
                        level,
                        layer,
                    );
                }
                FramebufferRenderbuffer {
                    target,
                    attachment,
                    renderbuffer_target,
                    id,
                } => {
                    if !valid_framebuffer_target(target, self.version)
                        || renderbuffer_target != glow::RENDERBUFFER
                    {
                        return Err(glow::INVALID_ENUM);
                    }
                    if self.default_bound(target) { return Err(glow::INVALID_OPERATION); }
                    gl.framebuffer_renderbuffer(
                        target,
                        attachment,
                        renderbuffer_target,
                        optional!(id, Renderbuffer),
                    );
                }
                BindAttribLocation {
                    program,
                    index,
                    name,
                } => {
                    if !valid_location_name(&name, self.version) {
                        return Err(glow::INVALID_VALUE);
                    }
                    if name.starts_with("gl_") || name.starts_with("webgl_") || name.starts_with("_webgl_") {
                        return Err(glow::INVALID_OPERATION);
                    }
                    gl.bind_attrib_location(object!(program, Program), index, &name);
                }
                ValidateProgram { program } => gl.validate_program(object!(program, Program)),
                DrawBuffers { buffers } => {
                    let max = gl.get_parameter_i32(glow::MAX_DRAW_BUFFERS).max(0) as usize;
                    if buffers.len() > max {
                        return Err(glow::INVALID_VALUE);
                    }
                    if self.default_bound(glow::DRAW_FRAMEBUFFER) {
                        if buffers.len()!=1 { return Err(glow::INVALID_OPERATION); }
                        let token=crate::framebuffer::default_buffer_token(buffers[0],self.drawing_storage.is_some())?;
                        gl.draw_buffers(&[token]);self.default_draw_buffer=buffers[0];
                    } else { gl.draw_buffers(&buffers); }
                }
                InvalidateFramebuffer {
                    target,
                    attachments,
                } => {
                    if !valid_framebuffer_target(target, 2) {
                        return Err(glow::INVALID_ENUM);
                    }
                    let attachments=if self.default_bound(target) {
                        crate::framebuffer::default_invalidation(&attachments,self.drawing_storage.is_some())?
                    } else { attachments };
                    gl.invalidate_framebuffer(target, &attachments);
                }
                InvalidateSubFramebuffer {
                    target,
                    attachments,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if !valid_framebuffer_target(target, 2) {
                        return Err(glow::INVALID_ENUM);
                    }
                    let attachments=if self.default_bound(target) {
                        crate::framebuffer::default_invalidation(&attachments,self.drawing_storage.is_some())?
                    } else { attachments };
                    gl.invalidate_sub_framebuffer(target, &attachments, x, y, width, height);
                }
                TransformFeedbackVaryings {
                    program,
                    varyings,
                    mode,
                } => {
                    let names: Vec<_> = varyings.iter().map(|name| native_name(name)).collect();
                    gl.transform_feedback_varyings(object!(program, Program), &names, mode);
                }
                BeginQuery { target, id } => {
                    if !matches!(
                        target,
                        glow::ANY_SAMPLES_PASSED
                            | glow::ANY_SAMPLES_PASSED_CONSERVATIVE
                            | glow::TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN
                    ) {
                        return Err(glow::INVALID_ENUM);
                    }
                    gl.begin_query(target, object!(id, Query));
                }
                SamplerParameteri { id, name, value } => {
                    gl.sampler_parameter_i32(object!(id, Sampler), name, value)
                }
                SamplerParameterf { id, name, value } => {
                    gl.sampler_parameter_f32(object!(id, Sampler), name, value)
                }
                UniformBlockBinding {
                    program,
                    index,
                    binding,
                } => gl.uniform_block_binding(object!(program, Program), index, binding),
            }
        }
        Ok(())
    }
    pub fn get_buffer_sub_data(&mut self, target: u32, offset: i64, destination: &mut [u8]) {
        let _ = self.get_buffer_sub_data_with_status(target, offset, destination);
    }
    /// Internal bridge completion receipt; public readback keeps its void API.
    #[doc(hidden)]
    pub fn get_buffer_sub_data_with_status(&mut self, target: u32, offset: i64, destination: &mut [u8]) -> bool {
        if !self.activate() {
            return false;
        }
        // Preserve older page errors before checking internal mapping errors.
        self.retain_driver_errors();
        if self.is_lost() { return false; }
        let result = (|| {
            if self.version != 2 {
                return Err(glow::INVALID_OPERATION);
            }
            if !valid_buffer_target(target, 2) {
                return Err(glow::INVALID_ENUM);
            }
            let driver = self.driver.as_ref().unwrap();
            let gl = &driver.gl;
            unsafe {
                let binding = match target {
                    glow::ARRAY_BUFFER => glow::ARRAY_BUFFER_BINDING,
                    glow::ELEMENT_ARRAY_BUFFER => glow::ELEMENT_ARRAY_BUFFER_BINDING,
                    glow::COPY_READ_BUFFER => glow::COPY_READ_BUFFER_BINDING,
                    glow::COPY_WRITE_BUFFER => glow::COPY_WRITE_BUFFER_BINDING,
                    glow::PIXEL_PACK_BUFFER => glow::PIXEL_PACK_BUFFER_BINDING,
                    glow::PIXEL_UNPACK_BUFFER => glow::PIXEL_UNPACK_BUFFER_BINDING,
                    glow::TRANSFORM_FEEDBACK_BUFFER => glow::TRANSFORM_FEEDBACK_BUFFER_BINDING,
                    glow::UNIFORM_BUFFER => glow::UNIFORM_BUFFER_BINDING,
                    _ => unreachable!("validated buffer target"),
                };
                if gl.get_parameter_buffer(binding).is_none() {
                    return Err(glow::INVALID_OPERATION);
                }
                checked_range(offset, destination.len(),
                    i64::from(gl.get_buffer_parameter_i32(target, glow::BUFFER_SIZE)))?;
                if offset > i32::MAX as i64 || destination.len() > i32::MAX as usize {
                    return Err(glow::INVALID_VALUE);
                }
                // glGetBufferSubData is desktop GL, not GLES. WebGL2 reads use
                // GLES3 mapping while leaving the page's target binding intact.
                type Map = unsafe extern "C" fn(u32,isize,isize,u32) -> *const u8;
                type Unmap = unsafe extern "C" fn(u32) -> u8;
                let map: Map = driver.entry(c"glMapBufferRange")?;
                let unmap: Unmap = driver.entry(c"glUnmapBuffer")?;
                let length = destination.len();
                if let Err(fallback) = copy_buffer_mapping(destination,
                    || map(target,offset as isize,length as isize,glow::MAP_READ_BIT),
                    || unmap(target) != 0) {
                    let error = gl.get_error();
                    return Err(if error == glow::NO_ERROR {fallback} else {error});
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => true,
            Err(error) => { self.error(error); false }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buffer_mapping_failures_preserve_destination_and_only_unmap_acquired_memory() {
        use std::cell::Cell;
        let source=[1,2,3,4];let mut destination=[9;4];let unmapped=Cell::new(0);
        unsafe {
            assert_eq!(copy_buffer_mapping(&mut destination, std::ptr::null,
                || {unmapped.set(unmapped.get()+1);true}),Err(glow::INVALID_OPERATION));
            assert_eq!(destination,[9;4]);assert_eq!(unmapped.get(),0);
            assert_eq!(copy_buffer_mapping(&mut destination, || source.as_ptr(),
                || {unmapped.set(unmapped.get()+1);false}),Err(glow::INVALID_OPERATION));
            assert_eq!(destination,[9;4]);assert_eq!(unmapped.get(),1);
            assert_eq!(copy_buffer_mapping(&mut destination, || source.as_ptr(),
                || {unmapped.set(unmapped.get()+1);true}),Ok(()));
            assert_eq!(destination,source);assert_eq!(unmapped.get(),2);
            assert_eq!(copy_buffer_mapping(&mut [], || panic!("empty read mapped"),
                || panic!("empty read unmapped")),Ok(()));
        }
    }
    #[test]
    fn buffer_readback_allocation_failure_never_maps_or_changes_destination() {
        let mut destination = [19; 4];
        unsafe {
            assert_eq!(copy_buffer_mapping_with_allocator(&mut destination,
                || panic!("allocation failure mapped native memory"),
                || panic!("allocation failure unmapped unacquired memory"),
                |length| { assert_eq!(length, 4); Err(glow::OUT_OF_MEMORY) }), Err(glow::OUT_OF_MEMORY));
            assert_eq!(destination, [19; 4]);
            assert_eq!(copy_buffer_mapping_with_allocator(&mut [],
                || panic!("empty read mapped"), || panic!("empty read unmapped"),
                |_| panic!("empty read allocated")), Ok(()));
        }
    }
    #[test]
    fn buffer_ranges_reject_overflow_and_allow_empty_end_slice() {
        assert_eq!(checked_range(4, 0, 4), Ok(()));
        assert_eq!(checked_range(3, 1, 4), Ok(()));
        for (offset, length, size) in [
            (-1, 1, 4),
            (4, 1, 4),
            (0, 0, -1),
            (i64::MAX, 1, i64::MAX),
            (1, usize::MAX, 4),
        ] {
            assert_eq!(
                checked_range(offset, length, size),
                Err(glow::INVALID_VALUE)
            );
        }
    }
    #[test]
    fn webgl_one_cannot_bind_pixel_or_indexed_buffers() {
        for target in [
            glow::PIXEL_PACK_BUFFER,
            glow::PIXEL_UNPACK_BUFFER,
            glow::UNIFORM_BUFFER,
        ] {
            assert!(!valid_buffer_target(target, 1));
            assert!(valid_buffer_target(target, 2));
        }
        assert!(!valid_buffer_target(glow::DRAW_INDIRECT_BUFFER, 2));
        assert!(!valid_usage(glow::STATIC_READ, 1));
        assert!(valid_usage(glow::STATIC_READ, 2));
    }
    #[test]
    fn texture_and_framebuffer_targets_match_the_context_version() {
        assert!(valid_texture_target(glow::TEXTURE_CUBE_MAP, 1));
        assert!(!valid_texture_target(glow::TEXTURE_3D, 1));
        assert!(valid_texture_target(glow::TEXTURE_3D, 2));
        assert!(!valid_texture_target(glow::TEXTURE_1D, 2));
        assert!(valid_framebuffer_target(glow::FRAMEBUFFER, 1));
        assert!(!valid_framebuffer_target(glow::READ_FRAMEBUFFER, 1));
        assert!(valid_framebuffer_target(glow::READ_FRAMEBUFFER, 2));
    }
}
