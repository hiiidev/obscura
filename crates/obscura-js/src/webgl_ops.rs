//! Feature-gated bridge to ANGLE. Contexts belong to a document, use never-reused
//! handles, and drop with navigation or frame teardown. No driver call unwinds
//! across a V8 callback. Native handles never appear on page-owned properties.
use crate::ops::{frame_state, ObscuraState};
use deno_core::{op2, v8, OpDecl, OpState};
use obscura_dom::NodeId;
use obscura_webgl::{
    api::{Attributes, CanvasContext, CreationFailure, Diagnostics},
    commands::Command,
    objects::Kind,
    queries::{Query, Value},
    resources::ResourceCommand,
    selection::Mode,
    transfers::{ReadPixels, TextureImage},
    uniforms::Uniform,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::{Rc, Weak},
    sync::atomic::{AtomicU32, Ordering},
};

#[path = "canvas_placeholder.rs"]
pub(crate) mod canvas_placeholder;

static NEXT_CONTEXT: AtomicU32 = AtomicU32::new(1);
pub(crate) struct Entry {
    // Standalone OffscreenCanvas contexts have no DOM node or compositor
    // surface. Keeping them in the document registry still retires them on
    // navigation, frame teardown or collection of their weak JS callback.
    pub node: Option<NodeId>,
    pub context: CanvasContext,
    pending_presentation: bool,
    loss_delivery: Option<LossDelivery>,
    loss_notified: bool,
}

/// A native callback must not keep its JS context, canvas or frame alive. JS
/// owns the callback for as long as the context is reachable. Its weak V8
/// handle also provides a deferred cleanup path for a collected canvas.
struct LossDelivery {
    owner: Weak<RefCell<ObscuraState>>,
    document_generation: u64,
    context_id: u32,
    callback: v8::Weak<v8::Function>,
    spawner: deno_core::V8TaskSpawner,
}
impl Entry {
    fn observe_loss(&mut self) {
        if !self.context.is_lost() {
            self.loss_notified = false;
            return;
        }
        if self.loss_notified {
            return;
        }
        let Some(delivery) = &self.loss_delivery else {
            return;
        };
        self.loss_notified = true;
        let owner = delivery.owner.clone();
        let generation = delivery.document_generation;
        let id = delivery.context_id;
        let callback = delivery.callback.clone();
        delivery.spawner.spawn(move |scope| {
            let Some(owner) = owner.upgrade() else { return };
            let current = owner.try_borrow().ok().is_some_and(|state| {
                state.document_generation == generation
                    && state
                        .webgl
                        .entries
                        .get(&id)
                        .is_some_and(|entry| entry.context.is_lost())
            });
            if !current {
                return;
            }
            v8::tc_scope!(let scope, scope);
            let Some(callback) = callback.to_local(scope) else {
                return;
            };
            let receiver = v8::undefined(scope).into();
            if callback.call(scope, receiver, &[]).is_none() {
                // Contain callback failures within this task's TryCatch. Never
                // leave a pending exception on deno_core's event-loop scope.
                tracing::warn!("WebGL context-loss notification failed");
            }
        });
    }
}
#[derive(Default)]
pub(crate) struct Contexts {
    pub entries: HashMap<u32, Entry>,
    pub placeholders: canvas_placeholder::Placeholders,
}

struct PendingCleanup {
    owner: Weak<RefCell<ObscuraState>>,
    generation: u64,
    id: u32,
}

/// Only contended finalizers enter this queue. Runtime boundaries retry each
/// request once, without a timer, self-wake or strong document reference.
#[derive(Clone, Default)]
pub(crate) struct DeferredCleanup(Rc<RefCell<Vec<PendingCleanup>>>);

impl DeferredCleanup {
    pub(crate) fn request(&self, owner: Weak<RefCell<ObscuraState>>, generation: u64, id: u32) {
        if retire_collected_context(&owner, generation, id) { return; }
        let mut pending = self.0.borrow_mut();
        if !pending.iter().any(|p| p.generation == generation && p.id == id && p.owner.ptr_eq(&owner)) {
            pending.push(PendingCleanup { owner, generation, id });
        }
    }

    pub(crate) fn drain(&self) {
        let Ok(mut pending) = self.0.try_borrow_mut() else { return };
        pending.retain(|p| !retire_collected_context(&p.owner, p.generation, p.id));
    }

    #[cfg(test)]
    pub(crate) fn pending_count(&self) -> usize { self.0.borrow().len() }
}

pub(crate) fn deferred_cleanup(runtime: &deno_core::JsRuntime) -> Option<DeferredCleanup> {
    runtime.op_state().try_borrow().ok()?.try_borrow::<DeferredCleanup>().cloned()
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum Creation {
    Ready {
        id: u32,
        width: u32,
        height: u32,
        attributes: Attributes,
        diagnostics: Diagnostics,
    },
    Failed {
        reason: String,
    },
}
impl Creation {
    fn failure(reason: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum Operation {
    Advanced(obscura_webgl::advanced::Advanced),
    Command(Command),
    Resource {
        command: ResourceCommand,
        has_data: bool,
    },
    Query(Query),
    Uniform(Uniform),
    Create {
        kind: Kind,
        shader_type: u32,
    },
    Delete {
        kind: Kind,
        id: u32,
    },
    Collect {
        id: u32,
    },
    CollectLocation {
        id: u32,
    },
    ShaderSource {
        id: u32,
        source: String,
    },
    CompileShader {
        id: u32,
    },
    AttachShader {
        program: u32,
        shader: u32,
        detach: bool,
    },
    LinkProgram {
        id: u32,
    },
    UseProgram {
        id: u32,
    },
    GetUniform {
        program: u32,
        location: u32,
    },
    PixelStore {
        name: u32,
        value: i32,
    },
    TextureImage {
        image: TextureImage,
        has_data: bool,
        #[serde(default)]
        buffer_offset: Option<u32>,
    },
    TextureSource {
        image: TextureImage,
        width: u32,
        height: u32,
        #[serde(default)]
        bitmap: bool,
        #[serde(default)]
        source_space: obscura_webgl::color::ColorSpace,
        #[serde(default)]
        source_premultiplied: bool,
    },
    ReadPixels(ReadPixels),
    ReadPixelsBuffer {
        request: ReadPixels,
        offset: u32,
    },
    BufferRead {
        target: u32,
        offset: i64,
    },
    Resize {
        width: u32,
        height: u32,
    },
    DrawingBufferStorage { format: u32, width: u32, height: u32 },
    DrawingBufferInfo,
    ColorSpace { drawing: bool, color_space: obscura_webgl::color::ColorSpace },
    Lose,
    Restore,
    GetError,
    IsLost,
    Attributes,
    SupportedExtensions,
    Extension {
        name: String,
    },
    Readback,
    SourceReadback,
    TransferBitmap,
    Error {
        error: u32,
    },
}
#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
enum ResultValue {
    PixelRead(Option<obscura_webgl::transfers::PixelReadLayout>),
    DrawingBuffer { width: u32, height: u32, format: u32 },
    Query(Value),
    Attributes(Option<Attributes>),
    Extensions(Option<Vec<String>>),
    Text(Option<String>),
    Number(u32),
    Boolean(bool),
    None,
}
#[derive(Serialize)]
struct Reply {
    lost: bool,
    dirty: bool,
    // Present only for reference-changing operations. A failed native command
    // must not release a wrapper that is still bound or attached.
    #[serde(skip_serializing_if = "Option::is_none")]
    accepted: Option<bool>,
    value: ResultValue,
}

impl Operation {
    fn changes_references(&self) -> bool {
        match self {
            Self::Delete { .. } | Self::AttachShader { .. } | Self::UseProgram { .. } => true,
            Self::Command(command) => matches!(
                command,
                Command::ActiveTexture { .. }
                    | Command::VertexAttribPointer { .. }
                    | Command::VertexAttribIPointer { .. }
                    | Command::EndQuery { .. }
                    | Command::BeginTransformFeedback { .. }
                    | Command::EndTransformFeedback { .. }
            ),
            Self::Resource { command, .. } => matches!(
                command,
                ResourceCommand::BindBuffer { .. }
                    | ResourceCommand::BindTexture { .. }
                    | ResourceCommand::BindFramebuffer { .. }
                    | ResourceCommand::BindRenderbuffer { .. }
                    | ResourceCommand::BindVertexArray { .. }
                    | ResourceCommand::BindSampler { .. }
                    | ResourceCommand::BindTransformFeedback { .. }
                    | ResourceCommand::BindBufferBase { .. }
                    | ResourceCommand::BindBufferRange { .. }
                    | ResourceCommand::FramebufferTexture2D { .. }
                    | ResourceCommand::FramebufferTextureLayer { .. }
                    | ResourceCommand::FramebufferRenderbuffer { .. }
                    | ResourceCommand::BeginQuery { .. }
            ),
            _ => false,
        }
    }
}

fn mode() -> Result<Mode, String> {
    Mode::parse(&std::env::var("OBSCURA_WEBGL_BACKEND").unwrap_or_else(|_| "auto".into()))
}

#[op2]
#[serde]
fn op_webgl_create(
    state: &OpState,
    frame: u32,
    epoch: u32,
    node: Option<u32>,
    version: u32,
    width: u32,
    height: u32,
    #[serde] attributes: Attributes,
) -> Creation {
    let shared = frame_state(state, frame);
    let Ok(mut state) = shared.try_borrow_mut() else {
        return Creation::failure("WebGL context owner is busy");
    };
    let node = node.map(NodeId::new);
    if !matches!(version, 1 | 2) || !crate::ops::canvas_owner_matches(&state, frame, epoch) {
        return Creation::failure("invalid WebGL context owner or version");
    }
    if let Some(node) = node {
        let is_canvas = state
            .dom
            .as_ref()
            .and_then(|dom| dom.get_node(node))
            .is_some_and(|n| n.as_element().is_some_and(|n| n.local.as_ref() == "canvas"));
        if !is_canvas || state.canvas_surfaces.contains_key(&node)
            || state.webgl.placeholders.owns_node(node) {
            return Creation::failure("canvas is unavailable or has another context type");
        }
        if let Some((&id, entry)) = state
            .webgl
            .entries
            .iter()
            .find(|(_, e)| e.node == Some(node))
        {
            if entry.context.version != version as u8 {
                return Creation::failure("canvas has another context type");
            }
            return Creation::Ready {
                id,
                width: entry.context.width.max(1),
                height: entry.context.height.max(1),
                attributes: entry.context.attributes.clone(),
                diagnostics: entry.context.diagnostics.clone(),
            };
        }
    }
    if state.webgl.entries.len() >= 16 {
        return Creation::failure("document WebGL context limit reached");
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mode = mode().map_err(|reason| CreationFailure {
            reason,
            attempts: Vec::new(),
        })?;
        CanvasContext::create(version as u8, width, height, attributes, mode)
    }));
    match result {
        Ok(Ok(context)) => {
            let Ok(id) = NEXT_CONTEXT
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            else {
                return Creation::failure("WebGL context handle space exhausted");
            };
            if state.webgl.entries.try_reserve(1).is_err() {
                return Creation::failure("WebGL context registry allocation failed");
            }
            tracing::debug!(context=id,backend=?context.diagnostics.backend,renderer=%context.diagnostics.renderer,"WebGL context initialized");
            let reply = Creation::Ready {
                id,
                width: width.max(1),
                height: height.max(1),
                attributes: context.attributes.clone(),
                diagnostics: context.diagnostics.clone(),
            };
            state.webgl.entries.insert(
                id,
                Entry {
                    node,
                    context,
                    pending_presentation: false,
                    loss_delivery: None,
                    loss_notified: false,
                },
            );
            state.activity_generation = state.activity_generation.wrapping_add(1);
            reply
        }
        Ok(Err(error)) => {
            tracing::warn!(reason=%error.reason,attempts=?error.attempts,"WebGL context creation failed");
            Creation::failure(error.reason)
        }
        Err(_) => Creation::failure("WebGL backend initialization panicked"),
    }
}

#[op2]
fn op_webgl_watch_loss(
    scope: &mut v8::PinScope,
    op_state: &OpState,
    frame: u32,
    id: u32,
    #[scoped] callback: v8::Global<v8::Function>,
) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        let shared = frame_state(op_state, frame);
        let Ok(mut state) = shared.try_borrow_mut() else {
            return false;
        };
        if state.frame_id != frame {
            return false;
        }
        let generation = state.document_generation;
        let Some(entry) = state.webgl.entries.get_mut(&id) else {
            return false;
        };
        if entry.loss_delivery.is_some() {
            return false;
        }
        let owner = Rc::downgrade(&shared);
        let spawner = op_state.borrow::<deno_core::V8TaskSpawner>().clone();
        let cleanup_owner = owner.clone();
        let cleanup_spawner = spawner.clone();
        let cleanup = op_state.borrow::<DeferredCleanup>().clone();
        let callback = v8::Weak::with_finalizer(
            scope,
            callback,
            Box::new(move |_| {
                // V8 can collect during another native op. Do not borrow document
                // state or destroy EGL resources inside that GC callback.
                cleanup_spawner.spawn(move |_| {
                    cleanup.request(cleanup_owner, generation, id);
                });
            }),
        );
        entry.loss_delivery = Some(LossDelivery {
            owner,
            document_generation: generation,
            context_id: id,
            callback,
            spawner,
        });
        entry.observe_loss();
        true
    }))
    .unwrap_or(false)
}

fn retire_collected_context(owner: &Weak<RefCell<ObscuraState>>, generation: u64, id: u32) -> bool {
    let Some(owner) = owner.upgrade() else { return true };
    let Ok(mut state) = owner.try_borrow_mut() else {
        return false;
    };
    if state.document_generation != generation {
        return true;
    }
    // Removal precedes destruction. A backend destructor panic must not unwind
    // through V8's task callback or an isolate-entry Drop implementation.
    if catch_unwind(AssertUnwindSafe(|| release_context(&mut state, id))).is_err() {
        tracing::warn!("WebGL collected-context destruction failed");
    }
    true
}

fn release_context(state: &mut ObscuraState, id: u32) -> bool {
    if let Some(entry) = state.webgl.entries.remove(&id) {
        state.webgl.placeholders.retire_context(id);
        if let Some(node) = entry.node {
            state.webgl_surfaces.remove(&node);
        }
        state.activity_generation = state.activity_generation.wrapping_add(1);
        // Entry drops here on the isolate's owning thread, outside GC.
        true
    } else {
        false
    }
}

#[op2(fast)]
fn op_webgl_release(op_state: &OpState, frame: u32, id: u32) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        let shared = frame_state(op_state, frame);
        let Ok(mut state) = shared.try_borrow_mut() else {
            return false;
        };
        state.frame_id == frame && release_context(&mut state, id)
    }))
    .unwrap_or(false)
}

#[op2]
#[serde]
fn op_webgl_call(
    state: &OpState,
    frame: u32,
    id: u32,
    #[serde] operation: Operation,
    #[buffer] data: &mut [u8],
) -> Reply {
    let shared = frame_state(state, frame);
    let Ok(mut state) = shared.try_borrow_mut() else {
        return Reply { lost: true, dirty: false, accepted: None, value: ResultValue::None };
    };
    if state.frame_id != frame {
        return Reply {
            lost: true,
            dirty: false,
            accepted: None,
            value: ResultValue::None,
        };
    }
    let Some(entry) = state.webgl.entries.get_mut(&id) else {
        return Reply {
            lost: true,
            dirty: false,
            accepted: None,
            value: ResultValue::None,
        };
    };
    let context = &mut entry.context;
    let was_dirty = context.dirty;
    let result = catch_unwind(AssertUnwindSafe(|| {
        let tracks_references = operation.changes_references();
        let checkpoint = if tracks_references {
            context.begin_reference_change()
        } else {
            None
        };
        let value = call(context, operation, data);
        let accepted = tracks_references.then(|| context.finish_reference_change(checkpoint));
        (value, accepted)
    }));
    let (value, accepted) = match result {
        Ok(result) => result,
        Err(_) => {
            context.lose();
            (ResultValue::None, Some(false))
        }
    };
    let lost = context.is_lost();
    let dirty = context.dirty;
    entry.observe_loss();
    if dirty && !was_dirty {
        state.activity_generation = state.activity_generation.wrapping_add(1);
    }
    Reply {
        lost,
        dirty,
        accepted,
        value,
    }
}
fn call(context: &mut CanvasContext, operation: Operation, data: &mut [u8]) -> ResultValue {
    use Operation::*;
    match operation {
        Advanced(request) => return ResultValue::Query(context.advanced(request)),
        Command(command) => context.command(command),
        Resource { command, has_data } => context.resource(command, has_data.then_some(&*data)),
        Query(query) => return ResultValue::Query(context.query(query)),
        Uniform(uniform) => context.uniform(uniform),
        Create { kind, shader_type } => {
            return ResultValue::Number(context.create_object(kind, shader_type).unwrap_or(0));
        }
        Delete { kind, id } => context.delete_object(id, kind),
        Collect { id } => context.collect_object(id),
        CollectLocation { id } => context.objects.forget_location(id),
        ShaderSource { id, source } => context.shader_source(id, &source),
        CompileShader { id } => context.compile_shader(id),
        AttachShader {
            program,
            shader,
            detach,
        } => context.attach_shader(program, shader, detach),
        LinkProgram { id } => context.link_program(id),
        UseProgram { id } => context.use_program(id),
        GetUniform { program, location } => {
            return ResultValue::Query(context.get_uniform(program, location));
        }
        PixelStore { name, value } => context.pixel_store(name, value),
        TextureImage {
            image,
            has_data,
            buffer_offset,
        } => {
            if let Some(offset) = buffer_offset {
                context.texture_image_from_buffer(image, offset);
            } else {
                context.texture_image(image, has_data.then_some(&*data));
            }
        }
        ReadPixels(request) => return ResultValue::PixelRead(context.read_pixels_with_layout(request, data)),
        TextureSource {
            image,
            width,
            height,
            bitmap,
            source_space,
            source_premultiplied,
        } => context.texture_source_color(image, width, height, data, bitmap,source_space,source_premultiplied),
        ReadPixelsBuffer { request, offset } => context.read_pixels_to_buffer(request, offset),
        BufferRead { target, offset } => return ResultValue::Boolean(context.get_buffer_sub_data_with_status(target, offset, data)),
        Resize { width, height } => {
            if context.resize(width, height) { return buffer_info(context); }
        }
        DrawingBufferStorage { format, width, height } => {
            if context.drawing_buffer_storage(format,width,height) { return buffer_info(context); }
        }
        DrawingBufferInfo => { if !context.is_lost() { return buffer_info(context); } }
        ColorSpace {drawing,color_space} => {
            if drawing {context.set_drawing_color_space(color_space);} else {context.unpack_color_space=color_space;}
        }
        Lose => context.lose(),
        Restore => {
            return ResultValue::Boolean(
                mode()
                    .ok()
                    .and_then(|mode| context.restore(mode).ok())
                    .unwrap_or(false),
            );
        }
        GetError => return ResultValue::Number(context.get_error()),
        IsLost => return ResultValue::Boolean(context.is_lost()),
        Attributes => {
            return ResultValue::Attributes(
                (!context.is_lost()).then(|| context.attributes.clone()),
            );
        }
        SupportedExtensions => return ResultValue::Extensions(context.supported_extensions()),
        Extension { name } => return ResultValue::Text(context.enable_extension(&name)),
        Readback | SourceReadback => {
            let pixels=if matches!(operation,SourceReadback) {context.drawing_buffer_source_rgba()} else {context.drawing_buffer_rgba()};
            return ResultValue::Boolean(match pixels {
                Ok(bytes) if bytes.len() == data.len() => {
                    data.copy_from_slice(&bytes);
                    true
                }
                _ => false,
            });
        }
        TransferBitmap => return ResultValue::Boolean(context.transfer_source_bitmap(data)),
        Error { error } => context.error(error),
    }
    ResultValue::None
}

fn buffer_info(context: &CanvasContext) -> ResultValue {
    ResultValue::DrawingBuffer { width:context.width,height:context.height,format:context.drawing_buffer_format() }
}

pub(crate) fn declarations() -> Vec<OpDecl> {
    vec![
        canvas_placeholder::declaration(),
        op_webgl_create(),
        op_webgl_watch_loss(),
        op_webgl_release(),
        op_webgl_call(),
        op_webgl_image_info(),
        op_webgl_image_pixels(),
        op_webgl_decode_info(),
        op_webgl_decode_pixels(),
        op_webgl_convert_color(),
        op_webgl_image_data(),
    ]
}

// The private slot belongs to the object, so cross-realm consumers can verify
// its brand without trusting page properties or retaining a global registry.
#[op2]
fn op_webgl_image_data<'s>(
    scope: &mut v8::PinScope<'s, '_>, object: v8::Local<'s, v8::Object>, data: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
    let Some(name)=v8::String::new(scope,"obscura.graphics.image_data") else {return v8::null(scope).into();};
    let key=v8::Private::for_api(scope,Some(name));
    if data.is_object() && object.set_private(scope,key,data)!=Some(true) {return v8::null(scope).into();}
    object.get_private(scope,key).unwrap_or_else(||v8::null(scope).into())
}

#[op2(fast)]
fn op_webgl_convert_color(
    #[string] source: String, #[string] target: String, premultiplied: bool, #[buffer] pixels: &mut [u8],
) -> bool {
    use obscura_webgl::color::{ColorSpace,convert_rgba8};
    let (Some(source),Some(target))=(ColorSpace::parse(&source),ColorSpace::parse(&target)) else {return false;};
    convert_rgba8(pixels,source,target,premultiplied)
}

fn image_source(state: &OpState, frame: u32, node: u32) -> Option<(std::sync::Arc<[u8]>, bool)> {
    let shared = frame_state(state, frame);
    let mut state = shared.borrow_mut();
    if state.frame_id != frame {
        return None;
    }
    let base = crate::ops::document_base_url(&state);
    let viewport = state.viewport;
    let ObscuraState {
        dom,
        render_resources,
        ..
    } = &mut *state;
    render_resources.cached_image_element_source(
        dom.as_ref()?,
        NodeId::new(node),
        viewport,
        base.as_deref(),
    )
}

// Both calls recheck ownership and origin. A page can mutate the image between
// calls; it must not substitute new bytes using a previous size/permission.
#[op2]
#[serde]
fn op_webgl_image_info(state: &OpState, frame: u32, node: u32, allow_tainted: bool) -> serde_json::Value {
    catch_unwind(AssertUnwindSafe(|| {
        let Some((bytes, clean)) = image_source(state, frame, node) else {
            return serde_json::json!({"status":"unavailable"});
        };
        if !clean && !allow_tainted {
            return serde_json::json!({"status":"security"});
        }
        match obscura_render::image_pixels::image_dimensions(&bytes) {
            Some((width, height)) => {
                serde_json::json!({"status":"ready","width":width,"height":height,"originClean":clean})
            }
            None => serde_json::json!({"status":"invalid"}),
        }
    }))
    .unwrap_or_else(|_| serde_json::json!({"status":"invalid"}))
}

#[op2(fast)]
fn op_webgl_image_pixels(
    state: &OpState,
    frame: u32,
    node: u32,
    #[buffer] pixels: &mut [u8],
    allow_tainted: bool,
) -> u32 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some((bytes, clean)) = image_source(state, frame, node) else {
            return 1;
        };
        if !clean && !allow_tainted {
            return 2;
        }
        if obscura_render::image_pixels::decode_image_rgba(&bytes, pixels) {
            if clean { 0 } else { 3 }
        } else {
            1
        }
    }))
    .unwrap_or(1)
}

// Blob decoding has no URL or ambient origin authority. SVG decoding uses
// the renderer's restricted image resolver, never local files or networking.
const MAX_ENCODED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
#[op2]
#[serde]
fn op_webgl_decode_info(#[buffer] bytes: &[u8]) -> serde_json::Value {
    catch_unwind(AssertUnwindSafe(|| {
        if bytes.len() <= MAX_ENCODED_IMAGE_BYTES {
            if let Some((width, height)) = obscura_render::image_pixels::image_dimensions(bytes) {
                return serde_json::json!({"status":"ready","width":width,"height":height});
            }
        }
        serde_json::json!({"status":"invalid"})
    })).unwrap_or_else(|_| serde_json::json!({"status":"invalid"}))
}
#[op2(fast)]
fn op_webgl_decode_pixels(#[buffer] bytes: &[u8], #[buffer] pixels: &mut [u8]) -> bool {
    catch_unwind(AssertUnwindSafe(|| bytes.len() <= MAX_ENCODED_IMAGE_BYTES
        && obscura_render::image_pixels::decode_image_rgba(bytes, pixels))).unwrap_or(false)
}

/// Capture only dirty canvases at a paint boundary. Document teardown drops
/// this map and its ANGLE contexts; there is no background graphics polling.
pub(crate) fn prepare_surfaces(state: &mut ObscuraState) {
    for entry in state.webgl.entries.values_mut() {
        // A standalone bitmap is not presented by a document paint. Avoid
        // readbacks, allocations and preserveDrawingBuffer clearing for it.
        let Some(node) = entry.node else { continue };
        if !entry.context.dirty {
            continue;
        }
        let result = catch_unwind(AssertUnwindSafe(|| entry.context.drawing_buffer_rgba()));
        let pixels = match result {
            Ok(Ok(pixels)) => pixels,
            _ => {
                entry.context.lose();
                Vec::new()
            }
        };
        let width = entry.context.width;
        let height = entry.context.height;
        state.webgl_surfaces.insert(node, (width, height, pixels));
        entry.context.dirty = false;
        entry.pending_presentation = true;
        entry.observe_loss();
    }
}

/// A failed layout/capture never consumes the page's drawing buffer.
pub(crate) fn did_present(state: &mut ObscuraState) {
    for entry in state.webgl.entries.values_mut() {
        if !entry.pending_presentation {
            continue;
        }
        entry.pending_presentation = false;
        if catch_unwind(AssertUnwindSafe(|| entry.context.did_present())).is_err() {
            entry.context.lose();
        }
        entry.observe_loss();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collected_cleanup_distinguishes_busy_owners_from_completed_requests() {
        let state = Rc::new(RefCell::new(ObscuraState::new()));
        state.borrow_mut().document_generation = 7;
        let owner = Rc::downgrade(&state);
        assert!(retire_collected_context(&owner, 6, 1));
        assert_eq!(state.borrow().activity_generation, 0);
        let guard = state.borrow_mut();
        assert!(!retire_collected_context(&owner, 7, 1));
        drop(guard);
        assert!(retire_collected_context(&owner, 7, 1));
        assert_eq!(state.borrow().activity_generation, 0);
        assert!(state.borrow().webgl.entries.is_empty());
        drop(state);
        assert!(retire_collected_context(&owner, 7, 1));
    }
    #[test]
    fn deferred_cleanup_retains_busy_requests_once_and_never_owns_the_document() {
        let state = Rc::new(RefCell::new(ObscuraState::new()));
        let owner = Rc::downgrade(&state);
        let cleanup = DeferredCleanup::default();
        let guard = state.borrow_mut();
        cleanup.request(owner.clone(), 0, 1);
        cleanup.request(owner.clone(), 0, 1);
        cleanup.request(owner.clone(), 0, 2);
        assert_eq!(cleanup.pending_count(), 2);
        assert_eq!(Rc::strong_count(&state), 1);
        for _ in 0..4 { cleanup.drain(); }
        assert_eq!(cleanup.pending_count(), 2);
        drop(guard);
        cleanup.drain();
        assert_eq!(cleanup.pending_count(), 0);
        assert_eq!(state.borrow().activity_generation, 0);
        cleanup.request(owner, 0, 3);
        assert_eq!(cleanup.pending_count(), 0);
    }
    #[test]
    fn deferred_cleanup_discards_replaced_or_dropped_owners_and_tolerates_reentry() {
        let state = Rc::new(RefCell::new(ObscuraState::new()));
        let cleanup = DeferredCleanup::default();
        let mut guard = state.borrow_mut();
        cleanup.request(Rc::downgrade(&state), 0, 1);
        guard.document_generation = 1;
        drop(guard);
        let queue_guard = cleanup.0.borrow_mut();
        cleanup.drain();
        assert_eq!(queue_guard.len(), 1);
        drop(queue_guard);
        cleanup.drain();
        assert_eq!(cleanup.pending_count(), 0);
        let guard = state.borrow_mut();
        cleanup.request(Rc::downgrade(&state), 1, 2);
        drop(guard);
        drop(state);
        cleanup.drain();
        assert_eq!(cleanup.pending_count(), 0);
    }
    #[test]
    fn only_reference_changes_request_acceptance_receipts() {
        let resources = [
            serde_json::json!({"method":"bindBuffer","args":{"target":0,"id":0}}),
            serde_json::json!({"method":"bindTexture","args":{"target":0,"id":0}}),
            serde_json::json!({"method":"bindFramebuffer","args":{"target":0,"id":0}}),
            serde_json::json!({"method":"bindRenderbuffer","args":{"target":0,"id":0}}),
            serde_json::json!({"method":"bindVertexArray","args":{"id":0}}),
            serde_json::json!({"method":"bindSampler","args":{"unit":0,"id":0}}),
            serde_json::json!({"method":"bindTransformFeedback","args":{"target":0,"id":0}}),
            serde_json::json!({"method":"bindBufferBase","args":{"target":0,"index":0,"id":0}}),
            serde_json::json!({"method":"bindBufferRange","args":{"target":0,"index":0,"id":0,"offset":0,"size":0}}),
            serde_json::json!({"method":"framebufferTexture2D","args":{"target":0,"attachment":0,"texture_target":0,"id":0,"level":0}}),
            serde_json::json!({"method":"framebufferTextureLayer","args":{"target":0,"attachment":0,"id":0,"level":0,"layer":0}}),
            serde_json::json!({"method":"framebufferRenderbuffer","args":{"target":0,"attachment":0,"renderbuffer_target":0,"id":0}}),
            serde_json::json!({"method":"beginQuery","args":{"target":0,"id":0}}),
        ];
        for command in resources {
            let operation: Operation = serde_json::from_value(
                serde_json::json!({"kind":"resource","value":{"command":command,"has_data":false}}),
            )
            .unwrap();
            assert!(operation.changes_references());
        }
        let commands = [
            Command::ActiveTexture { texture: 0 },
            Command::VertexAttribPointer {
                index: 0,
                size: 2,
                data_type: 0,
                normalized: false,
                stride: 0,
                offset: 0,
            },
            Command::VertexAttribIPointer {
                index: 0,
                size: 2,
                data_type: 0,
                stride: 0,
                offset: 0,
            },
            Command::EndQuery { target: 0 },
            Command::BeginTransformFeedback { mode: 0 },
            Command::EndTransformFeedback {},
        ];
        for command in commands {
            assert!(Operation::Command(command).changes_references());
        }
        assert!(Operation::Delete {
            kind: Kind::Buffer,
            id: 1
        }
        .changes_references());
        assert!(Operation::AttachShader {
            program: 1,
            shader: 2,
            detach: false
        }
        .changes_references());
        assert!(Operation::UseProgram { id: 1 }.changes_references());
        for operation in [
            Operation::Command(Command::Clear { mask: 0 }),
            Operation::Resource {
                command: ResourceCommand::BufferData {
                    target: 0,
                    size: 0,
                    usage: 0,
                },
                has_data: false,
            },
            Operation::Collect { id: 1 },
            Operation::CollectLocation { id: 1 },
            Operation::GetError,
            Operation::IsLost,
            Operation::Readback,
        ] {
            assert!(!operation.changes_references());
        }
    }
}
