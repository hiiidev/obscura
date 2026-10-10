//! Per-canvas browser state around a real ANGLE context. Only this layer should
//! be used by JavaScript bindings; EGL and GL names never enter the page heap.
use crate::{
    egl::{self, SurfaceOptions},
    objects::{Kind, Object, Objects},
    selection::{self, Attempt, Backend, Mode, Platform},
};
use glow::HasContext;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerPreference {
    #[default]
    Default,
    LowPower,
    HighPerformance,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Attributes {
    pub alpha: bool,
    pub depth: bool,
    pub stencil: bool,
    pub antialias: bool,
    pub premultiplied_alpha: bool,
    pub preserve_drawing_buffer: bool,
    pub fail_if_major_performance_caveat: bool,
    pub power_preference: PowerPreference,
    pub desynchronized: bool,
}
impl Default for Attributes {
    fn default() -> Self {
        Self {
            alpha: true,
            depth: true,
            stencil: false,
            antialias: true,
            premultiplied_alpha: true,
            preserve_drawing_buffer: false,
            fail_if_major_performance_caveat: false,
            power_preference: PowerPreference::Default,
            desynchronized: false,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Diagnostics {
    pub backend: Backend,
    pub renderer: String,
    pub vendor: String,
    pub gl_version: String,
    pub attempts: Vec<Attempt>,
}
#[derive(Clone, Debug, Serialize)]
pub struct CreationFailure {
    pub reason: String,
    pub attempts: Vec<Attempt>,
}

pub struct CanvasContext {
    pub(crate) driver: Option<egl::Context>,
    pub version: u8,
    pub width: u32,
    pub height: u32,
    pub(crate) canvas_size: (u32, u32),
    pub(crate) drawing_storage: Option<crate::drawing_buffer::Backing>,
    pub(crate) default_read_buffer: u32,
    pub(crate) default_draw_buffer: u32,
    pub drawing_color_space: crate::color::ColorSpace,
    pub unpack_color_space: crate::color::ColorSpace,
    pub attributes: Attributes,
    // Restoration retries the creation request, not the degraded attributes
    // returned by the previous backend (e.g. its missing MSAA/depth buffer).
    requested_attributes: Attributes,
    pub diagnostics: Diagnostics,
    pub objects: Objects,
    pub current_program: u32,
    pub(crate) active_queries: HashMap<u32, u32>,
    pub(crate) shader_sources: HashMap<u32, String>,
    pub(crate) shader_types: HashMap<u32, u32>,
    pub(crate) attribute_types: HashMap<u32, char>,
    pub(crate) unpack: crate::transfers::UnpackState,
    pub(crate) extensions: std::collections::HashSet<String>,
    errors: VecDeque<u32>,
    error_generation: u64,
    lost_error_pending: bool,
    pub dirty: bool,
}
impl CanvasContext {
    pub fn create(
        version: u8,
        width: u32,
        height: u32,
        attributes: Attributes,
        mode: Mode,
    ) -> Result<Self, CreationFailure> {
        // Selection includes drawing-buffer initialization. A backend that
        // fails before publication must not return an already-lost context or
        // prevent auto mode from trying the next backend.
        let (mut context, _, attempts) = selection::select(
            Platform::current(),
            mode,
            attributes.fail_if_major_performance_caveat,
            |backend| Self::initialize(backend, version, width, height, attributes.clone()),
        )
        .map_err(|attempts| CreationFailure {
            reason: if attempts.is_empty() {
                "WebGL backend is unsupported on this platform".into()
            } else {
                "No requested graphics backend created a usable WebGL context".into()
            },
            attempts,
        })?;
        context.diagnostics.attempts = attempts;
        Ok(context)
    }
    fn initialize(
        backend: Backend,
        version: u8,
        width: u32,
        height: u32,
        mut attributes: Attributes,
    ) -> Result<Self, String> {
        let requested_attributes = attributes.clone();
        let options = SurfaceOptions {
            alpha: attributes.alpha,
            depth: attributes.depth,
            stencil: attributes.stencil,
            antialias: attributes.antialias,
        };
        let driver = egl::Context::create(backend, version, width, height, options)?;
        let diagnostics = unsafe {
            Diagnostics {
                backend,
                renderer: driver.gl.get_parameter_string(glow::RENDERER),
                vendor: driver.gl.get_parameter_string(glow::VENDOR),
                gl_version: driver.gl.get_parameter_string(glow::VERSION),
                attempts: Vec::new(),
            }
        };
        attributes.antialias = driver.samples > 0;
        attributes.depth = driver.depth_bits > 0;
        attributes.stencil = driver.stencil_bits > 0;
        // Pbuffer rendering is synchronized. Do not promise a compositor mode
        // or a GPU power preference that this implementation cannot select.
        attributes.desynchronized = false;
        attributes.power_preference = PowerPreference::Default;
        let mut context = Self {
            driver: Some(driver),
            version,
            width,
            height,
            canvas_size: (width, height),
            drawing_storage: None,
            default_read_buffer: glow::BACK,
            default_draw_buffer: glow::BACK,
            drawing_color_space: crate::color::ColorSpace::Srgb,
            unpack_color_space: crate::color::ColorSpace::Srgb,
            attributes,
            requested_attributes,
            diagnostics,
            objects: Objects::default(),
            current_program: 0,
            active_queries: HashMap::new(),
            error_generation: 0,
            shader_sources: HashMap::new(),
            shader_types: HashMap::new(),
            attribute_types: HashMap::new(),
            unpack: crate::transfers::UnpackState::default(),
            extensions: std::collections::HashSet::new(),
            errors: VecDeque::new(),
            lost_error_pending: false,
            dirty: true,
        };
        context.initialize_drawing_buffer();
        if context.is_lost() {
            return Err("Initial WebGL drawing-buffer activation failed".into());
        }
        if context.retain_driver_errors() {
            return Err("Initial WebGL drawing-buffer setup failed".into());
        }
        Ok(context)
    }
    pub fn is_lost(&self) -> bool {
        self.driver.is_none()
    }
    pub fn lose(&mut self) {
        if self.driver.take().is_some() {
            // EGL context destruction already releases these private names.
            self.drawing_storage = None;
            self.objects = Objects::default();
            self.current_program = 0;
            self.active_queries.clear();
            self.shader_sources.clear();
            self.shader_types.clear();
            self.errors.clear();
            self.lost_error_pending = true;
            self.dirty = true;
        }
    }
    /// The browser binding calls this only after a cancelable context-lost
    /// event was prevented. All old object wrappers are invalidated by that
    /// binding's context generation before a restored event is dispatched.
    pub fn restore(&mut self, mode: Mode) -> Result<bool, CreationFailure> {
        if !self.is_lost() {
            return Ok(false);
        }
        let mut replacement = Self::create(
            self.version,
            self.canvas_size.0,
            self.canvas_size.1,
            self.requested_attributes.clone(),
            mode,
        )?;
        // Color interpretation is an attribute of the context object, not an
        // ANGLE pixel-store setting. It can also be changed while context-lost.
        replacement.drawing_color_space = self.drawing_color_space;
        replacement.unpack_color_space = self.unpack_color_space;
        *self = replacement;
        Ok(true)
    }
    pub fn set_drawing_color_space(&mut self, color_space: crate::color::ColorSpace) {
        if self.drawing_color_space==color_space { return; }
        self.drawing_color_space=color_space;
        if !self.activate() { return; }
        self.retain_driver_errors();
        if self.is_lost() { return; }
        let (width,height)=(self.width,self.height);
        if self.drawing_storage.is_some() {
            if !self.drawing_buffer_storage(self.drawing_buffer_format(),width.max(1),height.max(1)) { self.lose(); }
            self.width=width;self.height=height;
        } else {
            if self.driver.as_mut().unwrap().resize(width,height).is_err() { self.lose();return; }
            self.initialize_drawing_buffer();self.dirty=true;
        }
    }
    pub fn error(&mut self, error: u32) {
        if !self.is_lost() && error != glow::NO_ERROR {
            self.error_generation = self.error_generation.wrapping_add(1);
            if !self.errors.contains(&error) {
                self.errors.push_back(error);
            }
        }
    }
    /// Reference changes require an acceptance receipt so the wrapper's strong
    /// bindings change only when ANGLE accepted the corresponding operation.
    /// Previously pending errors remain available to the page and cannot make a
    /// later valid operation appear rejected. Draw/upload hot paths do not use
    /// these checks.
    pub fn begin_reference_change(&mut self) -> Option<u64> {
        if !self.activate() {
            return None;
        }
        self.retain_driver_errors();
        (!self.is_lost()).then_some(self.error_generation)
    }
    pub fn finish_reference_change(&mut self, generation: Option<u64>) -> bool {
        let Some(generation) = generation else {
            return false;
        };
        self.retain_driver_errors();
        !self.is_lost() && generation == self.error_generation
    }
    /// Every GL entry point first makes this canvas current. A device failure
    /// drops its resources and becomes context loss, never another canvas's GL.
    pub(crate) fn activate(&mut self) -> bool {
        let Some(driver) = self.driver.as_ref() else {
            return false;
        };
        if driver.make_current().is_err() {
            self.lose();
            false
        } else {
            true
        }
    }
    pub fn get_error(&mut self) -> u32 {
        if self.lost_error_pending {
            self.lost_error_pending = false;
            return 0x9242; // CONTEXT_LOST_WEBGL
        }
        if self.is_lost() {
            return glow::NO_ERROR;
        }
        if let Some(error) = self.errors.pop_front() {
            return error;
        }
        if !self.activate() {
            return self.get_error();
        }
        let error = unsafe { self.driver.as_ref().unwrap().gl.get_error() };
        if error == glow::CONTEXT_LOST {
            self.lose();
            return self.get_error();
        }
        error
    }
    /// Preserve page-visible errors around an internal compositor operation.
    /// A bounded drain cannot spin forever on a failing driver.
    pub(crate) fn retain_driver_errors(&mut self) -> bool {
        let mut found = false;
        for _ in 0..16 {
            let Some(driver) = self.driver.as_ref() else {
                return true;
            };
            let error = unsafe { driver.gl.get_error() };
            if error == glow::NO_ERROR {
                return found;
            }
            found = true;
            if error == glow::CONTEXT_LOST {
                self.lose();
                return true;
            }
            self.error(error);
        }
        found
    }
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        self.canvas_size = (width, height);
        if let Some(storage) = self.drawing_storage.as_ref() {
            let format = storage.spec.format;
            // Canvas dimensions of zero have a one-pixel native backing. The
            // bitmap remains empty for serialization of a zero-sized canvas.
            let resized = self.drawing_buffer_storage(format, width.max(1), height.max(1));
            if resized { self.width = width; self.height = height; }
            else { self.lose(); }
            return resized;
        }
        self.width = width;
        self.height = height;
        let Some(driver) = self.driver.as_mut() else {
            return false;
        };
        if driver.resize(width, height).is_err() {
            self.lose();
            return false;
        }
        self.dirty = true;
        self.initialize_drawing_buffer();
        true
    }
    pub fn create_object(&mut self, kind: Kind, shader_type: u32) -> Option<u32> {
        if !self.activate() {
            return None;
        }
        if self.version == 1
            && !(kind == Kind::VertexArray && self.extensions.contains("OES_vertex_array_object"))
            && matches!(
                kind,
                Kind::VertexArray
                    | Kind::Query
                    | Kind::Sampler
                    | Kind::TransformFeedback
                    | Kind::Sync
            )
        {
            self.error(glow::INVALID_OPERATION);
            return None;
        }
        if kind == Kind::Shader
            && ![glow::VERTEX_SHADER, glow::FRAGMENT_SHADER].contains(&shader_type)
        {
            self.error(glow::INVALID_ENUM);
            return None;
        }
        let driver = self.driver.as_ref().unwrap();
        let created = unsafe {
            let gl = &driver.gl;
            match kind {
                Kind::Buffer => gl.create_buffer().map(Object::Buffer),
                Kind::Texture => gl.create_texture().map(Object::Texture),
                Kind::Shader => gl.create_shader(shader_type).map(Object::Shader),
                Kind::Program => gl.create_program().map(Object::Program),
                Kind::Framebuffer => gl.create_framebuffer().map(Object::Framebuffer),
                Kind::Renderbuffer => gl.create_renderbuffer().map(Object::Renderbuffer),
                Kind::VertexArray => gl.create_vertex_array().map(Object::VertexArray),
                Kind::Query => gl.create_query().map(Object::Query),
                Kind::Sampler => gl.create_sampler().map(Object::Sampler),
                Kind::TransformFeedback => gl
                    .create_transform_feedback()
                    .map(Object::TransformFeedback),
                // fenceSync validates condition/flags in its separate API method.
                Kind::Sync => {
                    self.error(glow::INVALID_OPERATION);
                    return None;
                }
            }
        };
        let object = match created {
            Ok(object) => object,
            Err(_) => {
                self.error(glow::OUT_OF_MEMORY);
                return None;
            }
        };
        match self.objects.insert(object) {
            Ok(id) => {
                if kind == Kind::Shader {
                    self.shader_types.insert(id, shader_type);
                }
                Some(id)
            }
            Err(error) => {
                unsafe {
                    object.delete(&self.driver.as_ref().unwrap().gl);
                }
                self.error(error);
                None
            }
        }
    }
    pub fn delete_object(&mut self, id: u32, kind: Kind) {
        let checkpoint = self.begin_reference_change();
        if checkpoint.is_none() {
            return;
        }
        match self.objects.for_deletion(id, kind) {
            Ok(Some(object)) => {
                // WebGL, unlike GLES, ends an active query on explicit deletion.
                if kind == Kind::Query {
                    let target = self
                        .active_queries
                        .iter()
                        .find_map(|(&target, &query)| (query == id).then_some(target));
                    if let Some(target) = target {
                        unsafe {
                            self.driver.as_ref().unwrap().gl.end_query(target);
                        }
                        if !self.finish_reference_change(checkpoint) {
                            return;
                        }
                        self.active_queries.remove(&target);
                    }
                }
                unsafe {
                    object.delete(&self.driver.as_ref().unwrap().gl);
                }
                if self.finish_reference_change(checkpoint) {
                    if kind == Kind::Framebuffer { self.rebind_default_after_deletion(); }
                    let _ = self.objects.mark_deleted(id, kind);
                }
            }
            Ok(None) => {}
            Err(error) => self.error(error),
        }
    }
    pub fn collect_object(&mut self, id: u32) {
        if !self.activate() {
            return;
        }
        if let Some(object) = self.objects.forget(id) {
            unsafe {
                object.delete(&self.driver.as_ref().unwrap().gl);
            }
            if matches!(object, Object::Framebuffer(_)) { self.rebind_default_after_deletion(); }
        }
        self.shader_sources.remove(&id);
        self.shader_types.remove(&id);
    }
    pub fn shader_source(&mut self, id: u32, source: &str) {
        if !self.activate() {
            return;
        }
        match self.objects.get(id, Kind::Shader) {
            Ok(Object::Shader(shader)) => unsafe {
                self.driver
                    .as_ref()
                    .unwrap()
                    .gl
                    .shader_source(shader, source);
                self.shader_sources.insert(id, source.to_owned());
            },
            Err(error) => self.error(error),
            _ => unreachable!(),
        }
    }
    pub fn compile_shader(&mut self, id: u32) {
        if !self.activate() {
            return;
        }
        match self.objects.get(id, Kind::Shader) {
            Ok(Object::Shader(shader)) => unsafe {
                self.driver.as_ref().unwrap().gl.compile_shader(shader);
            },
            Err(error) => self.error(error),
            _ => unreachable!(),
        }
    }
    pub fn attach_shader(&mut self, program: u32, shader: u32, detach: bool) {
        if !self.activate() {
            return;
        }
        let objects = self.objects.get(program, Kind::Program).and_then(|p| {
            self.objects
                .get_for_query(shader, Kind::Shader)
                .map(|s| (p, s))
        });
        match objects {
            Ok((Object::Program(p), Object::Shader(s))) => unsafe {
                let gl = &self.driver.as_ref().unwrap().gl;
                if detach {
                    gl.detach_shader(p, s);
                } else {
                    gl.attach_shader(p, s);
                }
            },
            Err(error) => self.error(error),
            _ => unreachable!(),
        }
    }
    pub fn link_program(&mut self, id: u32) {
        if !self.activate() {
            return;
        }
        let object = self
            .objects
            .get(id, Kind::Program)
            .and_then(|p| self.objects.relink(id).map(|_| p));
        match object {
            Ok(Object::Program(p)) => unsafe {
                self.driver.as_ref().unwrap().gl.link_program(p);
            },
            Err(error) => self.error(error),
            _ => unreachable!(),
        }
    }
    pub fn use_program(&mut self, id: u32) {
        let checkpoint = self.begin_reference_change();
        if checkpoint.is_none() {
            return;
        }
        let program = if id == 0 {
            None
        } else {
            match self.objects.get(id, Kind::Program) {
                Ok(Object::Program(p)) => Some(p),
                Err(error) => {
                    self.error(error);
                    return;
                }
                _ => unreachable!(),
            }
        };
        let gl = &self.driver.as_ref().unwrap().gl;
        unsafe {
            // An unlinked program must not replace the recorded current one.
            if program.is_some_and(|p| !gl.get_program_link_status(p)) {
                self.error(glow::INVALID_OPERATION);
                return;
            }
            gl.use_program(program);
        }
        if self.finish_reference_change(checkpoint) {
            self.current_program = id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omitted_context_attributes_keep_webgl_defaults() {
        let attrs: Attributes = serde_json::from_str("{}").unwrap();
        assert!(attrs.alpha && attrs.depth && attrs.antialias && attrs.premultiplied_alpha);
        assert!(
            !attrs.stencil
                && !attrs.preserve_drawing_buffer
                && !attrs.fail_if_major_performance_caveat
        );
        assert!(serde_json::from_str::<Attributes>(r#"{"powerPreference":"unknown"}"#).is_err());
    }
    #[test]
    fn explicit_context_attributes_do_not_override_unrelated_defaults() {
        let attrs:Attributes=serde_json::from_str(r#"{"alpha":false,"stencil":true,"preserveDrawingBuffer":true,"powerPreference":"low-power"}"#).unwrap();
        assert!(!attrs.alpha && attrs.stencil && attrs.preserve_drawing_buffer && attrs.depth);
        assert!(matches!(attrs.power_preference, PowerPreference::LowPower));
    }
}
