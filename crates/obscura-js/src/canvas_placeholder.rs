//! Document-scoped placeholder ownership and last-presented snapshots. No JS
//! object or backing-store root is retained here. Standalone GL contexts keep
//! their separate lifetime; collecting one does not erase its last frame.
use super::*;
use crate::ops::invalidate_render_resource_geometry;
use obscura_webgl::color::{ColorSpace, convert_rgba8};

static NEXT_PLACEHOLDER: AtomicU32 = AtomicU32::new(1);
const MAX_PLACEHOLDERS: usize = 1024;
const MAX_PRESENTED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Unbound,
    Cpu,
    Gl(u32),
    Retired,
}
#[derive(Clone, Copy)]
struct Placeholder {
    node: NodeId,
    width: u32,
    height: u32,
    origin_clean: bool,
    blank: bool,
    revision: u32,
    bytes: usize,
    source: Source,
    color_space: ColorSpace,
}
#[derive(Default)]
pub(crate) struct Placeholders {
    entries: HashMap<u32, Placeholder>,
    retained_bytes: usize,
    // Only wide-gamut frames need a second representation. The compositor
    // consumes sRGB; image-source reads must retain the original gamut.
    source_pixels: HashMap<u32, Vec<u8>>,
}
impl Placeholders {
    pub(crate) fn owns_node(&self, node: NodeId) -> bool {
        self.entries.values().any(|p| p.node == node)
    }
    pub(crate) fn retire_context(&mut self, context: u32) {
        for p in self.entries.values_mut() {
            if p.source == Source::Gl(context) {
                p.source = Source::Retired;
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Request {
    Register {
        node: u32,
        width: u32,
        height: u32,
    },
    Bind {
        id: u32,
        context: u32,
    },
    PresentBlank {
        id: u32,
        width: u32,
        height: u32,
    },
    PresentCpu {
        id: u32,
        width: u32,
        height: u32,
        origin_clean: bool,
    },
    PresentGl {
        id: u32,
        context: u32,
    },
    Info {
        id: u32,
    },
    Pixels {
        id: u32,
        revision: u32,
    },
    SourcePixels {
        id: u32,
        revision: u32,
    },
    Retire {
        id: u32,
    },
}
fn failure(reason: &str) -> serde_json::Value {
    serde_json::json!({"status":"failed","reason":reason})
}
fn ready(p: Placeholder) -> serde_json::Value {
    serde_json::json!({"status":"ready","width":p.width,"height":p.height,
        "originClean":p.origin_clean,"revision":p.revision,"colorSpace":p.color_space})
}
fn pixel_bytes(width: u32, height: u32) -> Option<usize> {
    if width > 32767 || height > 32767 {
        return None;
    }
    (width as usize)
        .checked_mul(height as usize)
        .filter(|n| *n <= 16_777_216)?
        .checked_mul(4)
}
fn replacement_total(state: &ObscuraState, p: Placeholder, bytes: usize) -> Option<usize> {
    state
        .webgl
        .placeholders
        .retained_bytes
        .checked_sub(p.bytes)?
        .checked_add(bytes)
        .filter(|n| *n <= MAX_PRESENTED_BYTES)
}

#[op2]
#[serde]
fn op_canvas_placeholder(
    op_state: &OpState,
    frame: u32,
    epoch: u32,
    #[serde] request: Request,
    #[buffer] pixels: &mut [u8],
) -> serde_json::Value {
    catch_unwind(AssertUnwindSafe(|| {
        let shared = frame_state(op_state, frame);
        if !shared.try_borrow().ok().is_some_and(|s| crate::ops::canvas_owner_matches(&s, frame, epoch)) {
            return failure("canvas owner is unavailable");
        }
        dispatch_owner(&shared, frame, request, pixels)
    }))
    .unwrap_or_else(|_| failure("canvas presentation failed"))
}
pub(super) fn declaration() -> OpDecl {
    op_canvas_placeholder()
}

fn dispatch_owner(
    shared: &Rc<RefCell<ObscuraState>>,
    frame: u32,
    request: Request,
    pixels: &mut [u8],
) -> serde_json::Value {
    let Ok(mut state) = shared.try_borrow_mut() else {
        return failure("canvas owner is busy");
    };
    if state.frame_id != frame {
        return failure("canvas owner is unavailable");
    }
    dispatch(&mut state, request, pixels)
}

fn dispatch(state: &mut ObscuraState, request: Request, pixels: &mut [u8]) -> serde_json::Value {
    let source_pixels = matches!(request, Request::SourcePixels { .. });
    match request {
        Request::Register {
            node,
            width,
            height,
        } => {
            let node = NodeId::new(node);
            let canvas = state
                .dom
                .as_ref()
                .and_then(|dom| dom.get_node(node))
                .is_some_and(|n| n.as_element().is_some_and(|n| n.local.as_ref() == "canvas"));
            if !canvas
                || state.canvas_surfaces.contains_key(&node)
                || state.webgl.entries.values().any(|e| e.node == Some(node))
                || state.webgl.placeholders.owns_node(node)
            {
                return failure("canvas already has a context or is unavailable");
            }
            if state.webgl.placeholders.entries.len() >= MAX_PLACEHOLDERS
                || state.webgl.placeholders.entries.try_reserve(1).is_err()
            {
                return failure("placeholder registry allocation unavailable");
            }
            let Ok(id) = NEXT_PLACEHOLDER
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            else {
                return failure("placeholder handle space exhausted");
            };
            // Initial dimensions require no pixel allocation or backend. A
            // later bounded read/presentation can reject an enormous bitmap.
            let changed = match state
                .render_resources
                .set_canvas_bitmap_size(node, width, height)
            {
                Ok(changed) => changed,
                Err(reason) => return failure(reason),
            };
            state.webgl.placeholders.entries.insert(
                id,
                Placeholder {
                    node,
                    width,
                    height,
                    origin_clean: true,
                    blank: true,
                    revision: 0,
                    bytes: 0,
                    source: Source::Unbound,
                    color_space: ColorSpace::Srgb,
                },
            );
            if changed {
                invalidate_render_resource_geometry(state);
            }
            serde_json::json!({"status":"ready","id":id})
        }
        Request::Bind { id, context } => {
            let Some(p) = state.webgl.placeholders.entries.get(&id) else {
                return failure("placeholder is unavailable");
            };
            if p.source != Source::Unbound
                || state
                    .webgl
                    .placeholders
                    .entries
                    .values()
                    .any(|p| p.source == Source::Gl(context))
                || !state
                    .webgl
                    .entries
                    .get(&context)
                    .is_some_and(|entry| entry.node.is_none())
            {
                return failure("context belongs to another canvas");
            }
            state
                .webgl
                .placeholders
                .entries
                .get_mut(&id)
                .unwrap()
                .source = Source::Gl(context);
            serde_json::json!({"status":"ready"})
        }
        Request::PresentBlank { id, width, height } => {
            let Some(previous) = state.webgl.placeholders.entries.get(&id).copied() else {
                return failure("placeholder is unavailable");
            };
            if previous.source != Source::Unbound {
                return failure("canvas has a rendering context");
            }
            let Some(revision) = previous.revision.checked_add(1) else {
                return failure("canvas revision space exhausted");
            };
            let changed =
                match state
                    .render_resources
                    .set_canvas_bitmap_size(previous.node, width, height)
                {
                    Ok(changed) => changed,
                    Err(reason) => return failure(reason),
                };
            let current = Placeholder {
                width,
                height,
                revision,
                ..previous
            };
            state.webgl.placeholders.entries.insert(id, current);
            if changed {
                invalidate_render_resource_geometry(state);
            }
            state.activity_generation = state.activity_generation.wrapping_add(1);
            ready(current)
        }
        Request::PresentCpu {
            id,
            width,
            height,
            origin_clean,
        } => {
            let Some(p) = state.webgl.placeholders.entries.get(&id).copied() else {
                return failure("placeholder is unavailable");
            };
            if !matches!(p.source, Source::Unbound | Source::Cpu)
                || pixel_bytes(width, height) != Some(pixels.len())
                || replacement_total(state, p, pixels.len()).is_none()
            {
                return failure("invalid CPU canvas presentation");
            }
            let mut copy = Vec::new();
            if copy.try_reserve_exact(pixels.len()).is_err() {
                return failure("canvas snapshot allocation failed");
            }
            copy.extend_from_slice(pixels);
            commit(state, id, p, width, height, origin_clean, Source::Cpu, ColorSpace::Srgb, copy)
        }
        Request::PresentGl { id, context } => {
            let Some(p) = state.webgl.placeholders.entries.get(&id).copied() else {
                return failure("placeholder is unavailable");
            };
            if p.source != Source::Gl(context) {
                return failure("context belongs to another canvas");
            }
            let Some(entry) = state.webgl.entries.get(&context) else {
                return failure("context is unavailable");
            };
            let (width, height) = (entry.context.width, entry.context.height);
            let color_space = entry.context.drawing_color_space;
            if !entry.context.dirty {
                return ready(p);
            }
            let Some(bytes) = pixel_bytes(width, height) else {
                return failure("canvas bitmap is too large");
            };
            if retained_size(bytes, color_space).and_then(|n| replacement_total(state, p, n)).is_none() {
                return failure("canvas snapshot budget exceeded");
            }
            let entry = state.webgl.entries.get_mut(&context).unwrap();
            let snapshot =
                match catch_unwind(AssertUnwindSafe(|| entry.context.drawing_buffer_source_rgba())) {
                    Ok(Ok(bytes)) => bytes,
                    _ => {
                        entry.context.lose();
                        entry.observe_loss();
                        return failure("drawing buffer readback failed");
                    }
                };
            let result = commit(
                state,
                id,
                p,
                width,
                height,
                true,
                Source::Gl(context),
                color_space,
                snapshot,
            );
            if result["status"] == "ready" {
                let entry = state.webgl.entries.get_mut(&context).unwrap();
                // Consume only a successfully published buffer. Preserve GL
                // state and keep the copied last frame after a later loss.
                if catch_unwind(AssertUnwindSafe(|| entry.context.did_present())).is_err() {
                    entry.context.lose();
                }
                entry.context.dirty = false;
                entry.observe_loss();
            }
            result
        }
        Request::Info { id } => state
            .webgl
            .placeholders
            .entries
            .get(&id)
            .copied()
            .map(ready)
            .unwrap_or_else(|| failure("placeholder is unavailable")),
        Request::Pixels { id, revision } | Request::SourcePixels { id, revision } => {
            let Some(p) = state.webgl.placeholders.entries.get(&id).copied() else {
                return failure("placeholder is unavailable");
            };
            if revision != p.revision || pixel_bytes(p.width, p.height) != Some(pixels.len()) {
                return failure("canvas snapshot changed or destination is invalid");
            }
            if source_pixels && p.color_space != ColorSpace::Srgb && !p.blank {
                let Some(snapshot) = state.webgl.placeholders.source_pixels.get(&id) else {
                    return failure("canvas source snapshot is unavailable");
                };
                if snapshot.len() != pixels.len() { return failure("canvas source snapshot is invalid"); }
                pixels.copy_from_slice(snapshot);
            } else if let Some((width, height, snapshot)) = state.webgl_surfaces.get(&p.node) {
                if (*width, *height) != (p.width, p.height) || snapshot.len() != pixels.len() {
                    return failure("canvas snapshot is unavailable");
                }
                pixels.copy_from_slice(snapshot);
            } else if p.blank {
                pixels.fill(0);
            } else {
                return failure("canvas snapshot is unavailable");
            }
            let mut reply = ready(p);
            if !source_pixels { reply["colorSpace"] = serde_json::json!(ColorSpace::Srgb); }
            reply
        }
        Request::Retire { id } => {
            if let Some(p) = state.webgl.placeholders.entries.remove(&id) {
                state.webgl.placeholders.retained_bytes -= p.bytes;
                state.webgl.placeholders.source_pixels.remove(&id);
                state.webgl_surfaces.remove(&p.node);
                if state.render_resources.remove_canvas_bitmap_size(p.node) {
                    invalidate_render_resource_geometry(state);
                }
            }
            serde_json::json!({"status":"ready"})
        }
    }
}

fn retained_size(bytes: usize, color_space: ColorSpace) -> Option<usize> {
    bytes.checked_mul(if color_space == ColorSpace::Srgb { 1 } else { 2 })
}

fn commit(
    state: &mut ObscuraState,
    id: u32,
    previous: Placeholder,
    width: u32,
    height: u32,
    origin_clean: bool,
    source: Source,
    color_space: ColorSpace,
    pixels: Vec<u8>,
) -> serde_json::Value {
    if width == 0 || height == 0 {
        return failure("canvas has no presentable pixels");
    }
    if pixel_bytes(width, height) != Some(pixels.len()) {
        return failure("canvas snapshot dimensions do not match pixels");
    }
    let Some(bytes) = retained_size(pixels.len(), color_space) else {
        return failure("canvas snapshot size overflow");
    };
    let Some(total) = replacement_total(state, previous, bytes) else {
        return failure("canvas snapshot budget exceeded");
    };
    let Some(revision) = previous.revision.checked_add(1) else {
        return failure("canvas revision space exhausted");
    };
    if !state.webgl_surfaces.contains_key(&previous.node)
        && state.webgl_surfaces.try_reserve(1).is_err()
    {
        return failure("canvas surface allocation failed");
    }
    let mut display = None;
    if color_space != ColorSpace::Srgb {
        if !state.webgl.placeholders.source_pixels.contains_key(&id)
            && state.webgl.placeholders.source_pixels.try_reserve(1).is_err() {
            return failure("canvas source registry allocation failed");
        }
        let mut converted = Vec::new();
        if converted.try_reserve_exact(pixels.len()).is_err() {
            return failure("canvas color conversion allocation failed");
        }
        converted.extend_from_slice(&pixels);
        if !convert_rgba8(&mut converted, color_space, ColorSpace::Srgb, false) {
            return failure("canvas color conversion failed");
        }
        display = Some(converted);
    }
    // All fallible pixel allocations precede publication. The metadata entry
    // already exists, so an existing-size update cannot allocate or evict it.
    let changed = match state
        .render_resources
        .set_canvas_bitmap_size(previous.node, width, height)
    {
        Ok(changed) => changed,
        Err(reason) => return failure(reason),
    };
    let current = Placeholder {
        width,
        height,
        origin_clean,
        blank: false,
        revision,
        source,
        color_space,
        bytes,
        ..previous
    };
    let display = if let Some(display) = display {
        state.webgl.placeholders.source_pixels.insert(id, pixels);
        display
    } else {
        state.webgl.placeholders.source_pixels.remove(&id);
        pixels
    };
    state
        .webgl_surfaces
        .insert(previous.node, (width, height, display));
    state.webgl.placeholders.entries.insert(id, current);
    state.webgl.placeholders.retained_bytes = total;
    if changed {
        invalidate_render_resource_geometry(state);
    }
    state.activity_generation = state.activity_generation.wrapping_add(1);
    ready(current)
}

pub(crate) fn clear(state: &mut ObscuraState) {
    let mut changed = false;
    for p in state.webgl.placeholders.entries.values() {
        state.webgl_surfaces.remove(&p.node);
        changed |= state.render_resources.remove_canvas_bitmap_size(p.node);
    }
    state.webgl.placeholders = Placeholders::default();
    if changed {
        invalidate_render_resource_geometry(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (ObscuraState, NodeId) {
        let mut state = ObscuraState::new();
        let dom =
            obscura_dom::parse_html("<canvas id=c width=2 height=1></canvas><div id=other></div>");
        let node = dom.get_element_by_id("c").unwrap();
        state.dom = Some(dom);
        (state, node)
    }
    fn register(state: &mut ObscuraState, node: NodeId, width: u32, height: u32) -> u32 {
        let result = dispatch(
            state,
            Request::Register {
                node: node.raw(),
                width,
                height,
            },
            &mut [],
        );
        assert_eq!(result["status"], "ready");
        result["id"].as_u64().unwrap() as u32
    }
    fn present(
        state: &mut ObscuraState,
        id: u32,
        clean: bool,
        data: &mut [u8],
    ) -> serde_json::Value {
        dispatch(
            state,
            Request::PresentCpu {
                id,
                width: 2,
                height: 1,
                origin_clean: clean,
            },
            data,
        )
    }
    #[test]
    fn placeholder_color_retains_raw_pixels_and_converts_only_display_storage() {
        let (mut state,node)=state();let id=register(&mut state,node,2,1);
        let p=state.webgl.placeholders.entries[&id];
        let input=vec![128,64,32,255,255,0,0,255];
        let reply=commit(&mut state,id,p,2,1,true,Source::Gl(42),ColorSpace::DisplayP3,input.clone());
        assert_eq!(reply["colorSpace"],"display-p3");assert_eq!(reply["revision"],1);
        assert_eq!(state.webgl.placeholders.retained_bytes,16);
        assert_eq!(state.webgl_surfaces[&node].2,[138,59,21,255,255,0,0,255]);
        let mut raw=[0;8];let info=dispatch(&mut state,Request::SourcePixels{id,revision:1},&mut raw);
        assert_eq!(info["colorSpace"],"display-p3");assert_eq!(raw.as_slice(),input);
        let mut displayed=[0;8];let info=dispatch(&mut state,Request::Pixels{id,revision:1},&mut displayed);
        assert_eq!(info["colorSpace"],"srgb");assert_eq!(displayed,state.webgl_surfaces[&node].2.as_slice());
        state.webgl.placeholders.retire_context(42);
        assert_eq!(dispatch(&mut state,Request::SourcePixels{id,revision:1},&mut raw)["status"],"ready");assert_eq!(raw.as_slice(),input);
        dispatch(&mut state,Request::Retire{id},&mut []);
        assert!(state.webgl.placeholders.source_pixels.is_empty());assert!(state.webgl_surfaces.is_empty());assert_eq!(state.webgl.placeholders.retained_bytes,0);
    }
    #[test]
    fn placeholder_color_replacement_charges_both_buffers_and_drops_obsolete_raw_storage() {
        let (mut state,node)=state();let id=register(&mut state,node,2,1);present(&mut state,id,true,&mut [7;8]);
        let p=state.webgl.placeholders.entries[&id];state.webgl.placeholders.retained_bytes=MAX_PRESENTED_BYTES;
        assert_eq!(commit(&mut state,id,p,2,1,false,Source::Cpu,ColorSpace::DisplayP3,vec![255;8])["status"],"failed");
        assert_eq!(state.webgl_surfaces[&node].2,[7;8]);assert!(state.webgl.placeholders.source_pixels.is_empty());
        assert_eq!(ready(state.webgl.placeholders.entries[&id])["colorSpace"],"srgb");
        state.webgl.placeholders.retained_bytes=8;
        assert_eq!(commit(&mut state,id,p,2,1,false,Source::Cpu,ColorSpace::DisplayP3,vec![255;8])["status"],"ready");assert_eq!(state.webgl.placeholders.retained_bytes,16);
        let previous=state.webgl.placeholders.entries[&id];
        assert_eq!(commit(&mut state,id,previous,2,1,true,Source::Cpu,ColorSpace::Srgb,vec![99;7])["status"],"failed");
        assert_eq!(state.webgl.placeholders.retained_bytes,16);assert_eq!(state.webgl.placeholders.source_pixels[&id],[255;8]);
        assert_eq!(present(&mut state,id,true,&mut [9;8])["status"],"ready");
        assert_eq!(state.webgl.placeholders.retained_bytes,8);assert!(state.webgl.placeholders.source_pixels.is_empty());assert_eq!(state.webgl_surfaces[&node].2,[9;8]);
        assert_eq!(retained_size(usize::MAX,ColorSpace::DisplayP3),None);assert_eq!(retained_size(usize::MAX,ColorSpace::Srgb),Some(usize::MAX));
        let p=state.webgl.placeholders.entries[&id];commit(&mut state,id,p,2,1,true,Source::Cpu,ColorSpace::DisplayP3,vec![19;8]);
        clear(&mut state);assert!(state.webgl.placeholders.source_pixels.is_empty());assert_eq!(state.webgl.placeholders.retained_bytes,0);
    }
    #[test]
    fn empty_frames_preserve_snapshot_revision_and_budget_until_resume_or_retirement() {
        for color in [ColorSpace::Srgb, ColorSpace::DisplayP3] {
            for retire in [false, true] {
                let (mut state, node) = state();
                let id = register(&mut state, node, 2, 1);
                let initial = state.webgl.placeholders.entries[&id];
                assert_eq!(commit(&mut state, id, initial, 2, 1, false, Source::Cpu, color, vec![9; 8])["status"], "ready");
                let previous = state.webgl.placeholders.entries[&id];
                let display = state.webgl_surfaces[&node].clone();
                let source = state.webgl.placeholders.source_pixels.get(&id).cloned();
                let charged = state.webgl.placeholders.retained_bytes;
                let generation = state.activity_generation;
                for (width, height) in [(0, 1), (2, 0), (0, 0)] {
                    assert_eq!(commit(&mut state, id, previous, width, height, true, Source::Cpu, ColorSpace::Srgb, vec![])["status"], "failed");
                    assert_eq!(state.webgl_surfaces[&node], display);
                    assert_eq!(state.webgl.placeholders.source_pixels.get(&id), source.as_ref());
                    assert_eq!(state.webgl.placeholders.retained_bytes, charged);
                    assert_eq!(state.webgl.placeholders.entries[&id].revision, previous.revision);
                    assert!(!state.webgl.placeholders.entries[&id].origin_clean);
                    assert_eq!(state.activity_generation, generation);
                }
                assert_eq!(commit(&mut state, id, previous, 3, 1, true, Source::Cpu, ColorSpace::Srgb, vec![7; 12])["status"], "ready");
                assert_eq!(state.webgl.placeholders.retained_bytes, 12);
                assert_eq!(state.webgl.placeholders.entries[&id].revision, previous.revision + 1);
                assert!(state.webgl.placeholders.source_pixels.is_empty());
                assert_eq!(state.webgl_surfaces[&node].2, [7; 12]);
                if retire { dispatch(&mut state, Request::Retire { id }, &mut []); }
                else { clear(&mut state); }
                assert!(state.webgl_surfaces.is_empty());
                assert!(state.webgl.placeholders.entries.is_empty());
                assert!(state.webgl.placeholders.source_pixels.is_empty());
                assert_eq!(state.webgl.placeholders.retained_bytes, 0);
            }
        }
    }
    #[test]
    fn placeholder_raw_read_failure_preserves_destination_and_display_snapshot() {
        let (mut state,node)=state();let id=register(&mut state,node,2,1);let p=state.webgl.placeholders.entries[&id];
        commit(&mut state,id,p,2,1,true,Source::Gl(42),ColorSpace::DisplayP3,vec![255;8]);
        let mut output=[37;8];
        assert_eq!(dispatch(&mut state,Request::SourcePixels{id,revision:0},&mut output)["status"],"failed");assert_eq!(output,[37;8]);
        state.webgl.placeholders.source_pixels.remove(&id);
        assert_eq!(dispatch(&mut state,Request::SourcePixels{id,revision:1},&mut output)["status"],"failed");assert_eq!(output,[37;8]);
        state.webgl.placeholders.source_pixels.insert(id,vec![255;7]);
        assert_eq!(dispatch(&mut state,Request::SourcePixels{id,revision:1},&mut output)["status"],"failed");assert_eq!(output,[37;8]);
        assert_eq!(dispatch(&mut state,Request::Pixels{id,revision:1},&mut output)["status"],"ready");assert_eq!(output,state.webgl_surfaces[&node].2.as_slice());
    }
    #[test]
    fn registration_is_lazy_exclusive_and_has_never_reused_document_handles() {
        let (mut state, node) = state();
        let id = register(&mut state, node, 2, 1);
        assert!(state.webgl.entries.is_empty());
        assert!(state.webgl_surfaces.is_empty());
        assert_eq!(state.webgl.placeholders.retained_bytes, 0);
        assert_eq!(
            dispatch(
                &mut state,
                Request::Register {
                    node: node.raw(),
                    width: 1,
                    height: 1
                },
                &mut []
            )["status"],
            "failed"
        );
        let other = state
            .dom
            .as_ref()
            .unwrap()
            .get_element_by_id("other")
            .unwrap();
        for node in [other.raw(), u32::MAX] {
            assert_eq!(
                dispatch(
                    &mut state,
                    Request::Register {
                        node,
                        width: 1,
                        height: 1
                    },
                    &mut []
                )["status"],
                "failed"
            );
        }
        let mut pixels = [91; 8];
        assert_eq!(
            dispatch(&mut state, Request::Pixels { id, revision: 0 }, &mut pixels)["status"],
            "ready"
        );
        assert_eq!(pixels, [0; 8]);
        dispatch(&mut state, Request::Retire { id }, &mut []);
        let replacement = register(&mut state, node, 2, 1);
        assert_ne!(replacement, id);
        assert_eq!(
            dispatch(&mut state, Request::Info { id }, &mut [])["status"],
            "failed"
        );
    }
    #[test]
    fn cpu_presentation_copies_pixels_and_commits_origin_size_and_revision_together() {
        let (mut state, node) = state();
        let id = register(&mut state, node, 2, 1);
        let mut input = [1, 2, 3, 255, 4, 5, 6, 128];
        let first = present(&mut state, id, false, &mut input);
        assert_eq!(first["revision"], 1);
        assert_eq!(first["originClean"], false);
        input.fill(90);
        assert_eq!(state.webgl_surfaces[&node].2, [1, 2, 3, 255, 4, 5, 6, 128]);
        assert_eq!(state.webgl.placeholders.retained_bytes, 8);
        let mut output = [77; 8];
        assert_eq!(
            dispatch(&mut state, Request::Pixels { id, revision: 0 }, &mut output)["status"],
            "failed"
        );
        assert_eq!(output, [77; 8]);
        assert_eq!(
            dispatch(&mut state, Request::Pixels { id, revision: 1 }, &mut output)["originClean"],
            false
        );
        assert_eq!(output, [1, 2, 3, 255, 4, 5, 6, 128]);
        assert_eq!(present(&mut state, id, true, &mut input)["revision"], 2);
        assert_eq!(
            state.webgl.placeholders.retained_bytes, 8,
            "replacement is not charged twice"
        );
        let zero = dispatch(
            &mut state,
            Request::PresentCpu {
                id,
                width: 0,
                height: 12,
                origin_clean: true,
            },
            &mut [],
        );
        assert_eq!(zero["status"], "failed");
        let retained = dispatch(&mut state, Request::Info { id }, &mut []);
        assert_eq!((retained["width"].as_u64(), retained["height"].as_u64()), (Some(2), Some(1)));
        assert_eq!(retained["revision"], 2);
        assert_eq!(state.webgl.placeholders.retained_bytes, 8);
        assert_eq!(state.webgl_surfaces[&node].2, [90; 8]);
        assert_eq!(dispatch(&mut state, Request::Pixels { id, revision: 2 }, &mut output)["status"], "ready");
        assert_eq!(output, [90; 8]);
    }
    #[test]
    fn invalid_destinations_sources_sizes_and_budgets_preserve_the_previous_frame() {
        let (mut state, node) = state();
        let id = register(&mut state, node, 2, 1);
        present(&mut state, id, true, &mut [7; 8]);
        for (width, height, len) in [(2, 1, 7), (32768, 0, 0), (5000, 5000, 0)] {
            let result = dispatch(
                &mut state,
                Request::PresentCpu {
                    id,
                    width,
                    height,
                    origin_clean: false,
                },
                &mut vec![9; len],
            );
            assert_eq!(result["status"], "failed");
            assert_eq!(state.webgl_surfaces[&node].2, [7; 8]);
        }
        state.webgl.placeholders.retained_bytes = MAX_PRESENTED_BYTES;
        assert_eq!(
            dispatch(
                &mut state,
                Request::PresentCpu {
                    id,
                    width: 3,
                    height: 1,
                    origin_clean: true
                },
                &mut [8; 12]
            )["status"],
            "failed"
        );
        state.webgl.placeholders.retained_bytes = 8;
        state
            .webgl
            .placeholders
            .entries
            .get_mut(&id)
            .unwrap()
            .revision = u32::MAX;
        assert_eq!(
            present(&mut state, id, false, &mut [9; 8])["status"],
            "failed"
        );
        assert_eq!(state.webgl_surfaces[&node].2, [7; 8]);
        state
            .webgl
            .placeholders
            .entries
            .get_mut(&id)
            .unwrap()
            .source = Source::Retired;
        assert_eq!(
            present(&mut state, id, false, &mut [9; 8])["status"],
            "failed"
        );
        assert_eq!(
            dispatch(&mut state, Request::Bind { id, context: 99 }, &mut [])["status"],
            "failed"
        );
        assert_eq!(
            dispatch(&mut state, Request::PresentGl { id, context: 99 }, &mut [])["status"],
            "failed"
        );
        let mut invalid = [42; 7];
        assert_eq!(
            dispatch(
                &mut state,
                Request::Pixels {
                    id,
                    revision: u32::MAX
                },
                &mut invalid
            )["status"],
            "failed"
        );
        assert_eq!(invalid, [42; 7]);
    }
    #[test]
    fn document_limit_and_retirement_release_native_snapshots_and_metadata() {
        let (mut state, _) = state();
        state.dom = Some(obscura_dom::parse_html(
            &"<canvas></canvas>".repeat(MAX_PLACEHOLDERS + 1),
        ));
        let nodes = state
            .dom
            .as_ref()
            .unwrap()
            .descendants(state.dom.as_ref().unwrap().document())
            .into_iter()
            .filter(|id| {
                state
                    .dom
                    .as_ref()
                    .unwrap()
                    .get_node(*id)
                    .unwrap()
                    .as_element()
                    .is_some_and(|n| n.local.as_ref() == "canvas")
            })
            .collect::<Vec<_>>();
        let mut ids = Vec::new();
        for node in &nodes[..MAX_PLACEHOLDERS] {
            ids.push(register(&mut state, *node, 2, 1));
        }
        let overflow = nodes[MAX_PLACEHOLDERS];
        assert_eq!(
            dispatch(
                &mut state,
                Request::Register {
                    node: overflow.raw(),
                    width: 2,
                    height: 1
                },
                &mut []
            )["status"],
            "failed"
        );
        present(&mut state, ids[0], true, &mut [3; 8]);
        for _ in 0..2 {
            dispatch(&mut state, Request::Retire { id: ids[0] }, &mut []);
        }
        assert_eq!(state.webgl.placeholders.retained_bytes, 0);
        assert!(!state.webgl_surfaces.contains_key(&nodes[0]));
        register(&mut state, overflow, 2, 1);
        present(&mut state, ids[1], true, &mut [4; 8]);
        clear(&mut state);
        assert!(state.webgl.placeholders.entries.is_empty());
        assert!(state.webgl_surfaces.is_empty());
        assert!(!state.render_resources.remove_canvas_bitmap_size(nodes[1]));
    }
    #[test]
    fn missing_snapshots_and_retired_gl_owners_do_not_disclose_or_erase_pixels() {
        let (mut state, node) = state();
        let id = register(&mut state, node, 2, 1);
        present(&mut state, id, false, &mut [19; 8]);
        state
            .webgl
            .placeholders
            .entries
            .get_mut(&id)
            .unwrap()
            .source = Source::Gl(42);
        state.webgl.placeholders.retire_context(42);
        let mut pixels = [0; 8];
        assert_eq!(
            dispatch(&mut state, Request::Pixels { id, revision: 1 }, &mut pixels)["status"],
            "ready"
        );
        assert_eq!(pixels, [19; 8]);
        state.webgl_surfaces.remove(&node);
        pixels.fill(9);
        assert_eq!(
            dispatch(&mut state, Request::Pixels { id, revision: 1 }, &mut pixels)["status"],
            "failed"
        );
        assert_eq!(pixels, [9; 8]);
        for request in [
            Request::Bind {
                id: u32::MAX,
                context: 0,
            },
            Request::PresentCpu {
                id: u32::MAX,
                width: 0,
                height: 0,
                origin_clean: true,
            },
            Request::PresentGl {
                id: u32::MAX,
                context: 0,
            },
            Request::Pixels {
                id: u32::MAX,
                revision: 0,
            },
        ] {
            assert_eq!(dispatch(&mut state, request, &mut [])["status"], "failed");
        }
    }
    #[test]
    fn blank_resize_is_lazy_and_preserves_unbound_context_selection() {
        let (mut state, node) = state();
        let id = register(&mut state, node, 2, 1);
        for (width, height) in [(3, 2), (0, 8), (u32::MAX, 0)] {
            let result = dispatch(
                &mut state,
                Request::PresentBlank { id, width, height },
                &mut [],
            );
            assert_eq!(result["status"], "ready");
            assert_eq!(result["width"], width);
            assert_eq!(result["height"], height);
            assert!(state.webgl_surfaces.is_empty());
            assert_eq!(state.webgl.placeholders.retained_bytes, 0);
            if width == 3 {
                let mut bytes = [99; 24];
                assert_eq!(
                    dispatch(&mut state, Request::Pixels { id, revision: 1 }, &mut bytes)["status"],
                    "ready"
                );
                assert_eq!(bytes, [0; 24]);
            }
        }
        present(&mut state, id, true, &mut [7; 8]);
        assert_eq!(
            dispatch(
                &mut state,
                Request::PresentBlank {
                    id,
                    width: 4,
                    height: 1
                },
                &mut []
            )["status"],
            "failed"
        );
        assert_eq!(
            dispatch(
                &mut state,
                Request::PresentBlank {
                    id: u32::MAX,
                    width: 0,
                    height: 0
                },
                &mut []
            )["status"],
            "failed"
        );
        assert_eq!(state.webgl_surfaces[&node].2, [7; 8]);
    }
    #[test]
    fn stale_or_busy_frame_owners_reject_requests_without_touching_destinations() {
        let (mut state, node) = state();
        state.frame_id = 7;
        let id = register(&mut state, node, 2, 1);
        let shared = Rc::new(RefCell::new(state));
        let mut bytes = [9; 8];
        assert_eq!(
            dispatch_owner(&shared, 8, Request::Pixels { id, revision: 0 }, &mut bytes)["status"],
            "failed"
        );
        let guard = shared.borrow_mut();
        assert_eq!(
            dispatch_owner(&shared, 7, Request::Pixels { id, revision: 0 }, &mut bytes)["status"],
            "failed"
        );
        drop(guard);
        assert_eq!(bytes, [9; 8]);
        assert_eq!(
            dispatch_owner(&shared, 7, Request::Pixels { id, revision: 0 }, &mut bytes)["status"],
            "ready"
        );
        assert_eq!(bytes, [0; 8]);
    }
}
