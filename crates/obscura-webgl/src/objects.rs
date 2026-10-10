//! Native resource identity is scoped to a context and never reuses a page ID.
//! Web-facing wrappers keep these IDs in private storage, rather than exposing
//! mutable numeric names that could alias another context's GL resources.
use glow::HasContext;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Buffer,
    Texture,
    Shader,
    Program,
    Framebuffer,
    Renderbuffer,
    VertexArray,
    Query,
    Sampler,
    TransformFeedback,
    Sync,
}
#[derive(Clone, Copy, Debug)]
pub enum Object {
    Buffer(glow::NativeBuffer),
    Texture(glow::NativeTexture),
    Shader(glow::NativeShader),
    Program(glow::NativeProgram),
    Framebuffer(glow::NativeFramebuffer),
    Renderbuffer(glow::NativeRenderbuffer),
    VertexArray(glow::NativeVertexArray),
    Query(glow::NativeQuery),
    Sampler(glow::NativeSampler),
    TransformFeedback(glow::NativeTransformFeedback),
    Sync(glow::NativeFence),
}
impl Object {
    pub fn kind(self) -> Kind {
        match self {
            Self::Buffer(_) => Kind::Buffer,
            Self::Texture(_) => Kind::Texture,
            Self::Shader(_) => Kind::Shader,
            Self::Program(_) => Kind::Program,
            Self::Framebuffer(_) => Kind::Framebuffer,
            Self::Renderbuffer(_) => Kind::Renderbuffer,
            Self::VertexArray(_) => Kind::VertexArray,
            Self::Query(_) => Kind::Query,
            Self::Sampler(_) => Kind::Sampler,
            Self::TransformFeedback(_) => Kind::TransformFeedback,
            Self::Sync(_) => Kind::Sync,
        }
    }
    /// The caller has made the owning context current. No page-provided raw GL
    /// name reaches this dispatch; every object was returned by this context.
    pub unsafe fn delete(self, gl: &glow::Context) {
        match self {
            Self::Buffer(v) => gl.delete_buffer(v),
            Self::Texture(v) => gl.delete_texture(v),
            Self::Shader(v) => gl.delete_shader(v),
            Self::Program(v) => gl.delete_program(v),
            Self::Framebuffer(v) => gl.delete_framebuffer(v),
            Self::Renderbuffer(v) => gl.delete_renderbuffer(v),
            Self::VertexArray(v) => gl.delete_vertex_array(v),
            Self::Query(v) => gl.delete_query(v),
            Self::Sampler(v) => gl.delete_sampler(v),
            Self::TransformFeedback(v) => gl.delete_transform_feedback(v),
            Self::Sync(v) => gl.delete_sync(v),
        }
    }
}

struct Entry {
    object: Object,
    deleted: bool,
    superseded: bool,
    link_generation: u64,
}
#[derive(Clone, Copy)]
pub struct Location {
    pub native: glow::NativeUniformLocation,
    pub program: u32,
    pub generation: u64,
}

#[derive(Default)]
pub struct Objects {
    entries: HashMap<u32, Entry>,
    locations: HashMap<u32, Location>,
    program_shader_names: HashMap<(bool, u32), u32>,
    next: u32,
}
impl Objects {
    fn program_shader_name(object: Object) -> Option<(bool, u32)> {
        match object {
            Object::Program(name) => Some((true, name.0.get())),
            Object::Shader(name) => Some((false, name.0.get())),
            _ => None,
        }
    }
    pub fn deleted(&self, id: u32) -> bool {
        self.entries.get(&id).is_none_or(|entry| entry.deleted)
    }
    fn allocate_id(&mut self) -> Result<u32, u32> {
        let id = self.next.checked_add(1).ok_or(glow::OUT_OF_MEMORY)?;
        self.next = id;
        Ok(id)
    }
    pub fn insert(&mut self, object: Object) -> Result<u32, u32> {
        self.entries
            .try_reserve(1)
            .map_err(|_| glow::OUT_OF_MEMORY)?;
        let id = self.allocate_id()?;
        // Shader/program names cannot be reused until deferred deletion has
        // completed. Remember reuse even if the new object is later deleted
        // while current: glIsProgram/glIsShader alone cannot identify its old
        // wrapper once that numeric name belongs to a different object.
        if let Some(name) = Self::program_shader_name(object) {
            self.program_shader_names
                .try_reserve(1)
                .map_err(|_| glow::OUT_OF_MEMORY)?;
            if let Some(old) = self.program_shader_names.insert(name, id) {
                if let Some(entry) = self.entries.get_mut(&old) {
                    entry.superseded = true;
                }
            }
        }
        self.entries.insert(
            id,
            Entry {
                object,
                deleted: false,
                superseded: false,
                link_generation: 0,
            },
        );
        Ok(id)
    }
    pub fn get(&self, id: u32, kind: Kind) -> Result<Object, u32> {
        self.entries
            .get(&id)
            .filter(|e| !e.deleted && !e.superseded && e.object.kind() == kind)
            .map(|e| e.object)
            .ok_or(glow::INVALID_OPERATION)
    }
    pub fn contains(&self, id: u32, kind: Kind) -> bool {
        self.get(id, kind).is_ok()
    }
    /// Shader/program deletion can be deferred while attached or current. The
    /// caller additionally checks isShader/isProgram before querying the driver.
    pub fn get_for_query(&self, id: u32, kind: Kind) -> Result<Object, u32> {
        self.entries
            .get(&id)
            .filter(|e| {
                !e.superseded
                    && e.object.kind() == kind
                    && (!e.deleted || matches!(kind, Kind::Shader | Kind::Program))
            })
            .map(|e| e.object)
            .ok_or(glow::INVALID_OPERATION)
    }
    /// Idempotent deletion retains the wrapper's tombstone until its finalizer
    /// runs; this keeps a deleted wrapper from acquiring a later native object.
    pub fn mark_deleted(&mut self, id: u32, kind: Kind) -> Result<Option<Object>, u32> {
        if id == 0 {
            return Ok(None);
        }
        let entry = self
            .entries
            .get_mut(&id)
            .filter(|e| e.object.kind() == kind)
            .ok_or(glow::INVALID_OPERATION)?;
        if entry.deleted {
            return Ok(None);
        }
        entry.deleted = true;
        Ok(Some(entry.object))
    }
    /// Validate without committing a tombstone. Some native deletions (notably
    /// active transform feedback) can fail, leaving the object usable.
    pub fn for_deletion(&self, id: u32, kind: Kind) -> Result<Option<Object>, u32> {
        if id == 0 {
            return Ok(None);
        }
        let entry = self
            .entries
            .get(&id)
            .filter(|entry| entry.object.kind() == kind)
            .ok_or(glow::INVALID_OPERATION)?;
        Ok((!entry.deleted).then_some(entry.object))
    }
    pub fn forget(&mut self, id: u32) -> Option<Object> {
        let entry = self.entries.remove(&id)?;
        if entry.object.kind() == Kind::Program {
            self.locations.retain(|_, location| location.program != id);
        }
        if let Some(name) = Self::program_shader_name(entry.object) {
            if self.program_shader_names.get(&name) == Some(&id) {
                self.program_shader_names.remove(&name);
            }
        }
        (!entry.deleted).then_some(entry.object)
    }
    pub fn id_for(&self, object: Object) -> Option<u32> {
        // A driver may reuse an integer after native deletion while an old JS
        // wrapper still exists. Prefer live entries over retained tombstones.
        self.entries
            .iter()
            .filter(|(_, e)| !e.deleted)
            .chain(self.entries.iter().filter(|(_, e)| e.deleted))
            .filter(|(_, e)| !e.superseded)
            .find_map(|(&id, e)| {
                // Native wrappers are small typed names; compare the matching enum
                // variants rather than converting pointers or integer sizes.
                let same = match (e.object, object) {
                    (Object::Buffer(a), Object::Buffer(b)) => a == b,
                    (Object::Texture(a), Object::Texture(b)) => a == b,
                    (Object::Shader(a), Object::Shader(b)) => a == b,
                    (Object::Program(a), Object::Program(b)) => a == b,
                    (Object::Framebuffer(a), Object::Framebuffer(b)) => a == b,
                    (Object::Renderbuffer(a), Object::Renderbuffer(b)) => a == b,
                    (Object::VertexArray(a), Object::VertexArray(b)) => a == b,
                    (Object::Query(a), Object::Query(b)) => a == b,
                    (Object::Sampler(a), Object::Sampler(b)) => a == b,
                    (Object::TransformFeedback(a), Object::TransformFeedback(b)) => a == b,
                    (Object::Sync(a), Object::Sync(b)) => a == b,
                    _ => false,
                };
                same.then_some(id)
            })
    }
    pub fn relink(&mut self, program: u32) -> Result<(), u32> {
        let entry = self
            .entries
            .get_mut(&program)
            .filter(|e| !e.deleted && e.object.kind() == Kind::Program)
            .ok_or(glow::INVALID_OPERATION)?;
        entry.link_generation = entry
            .link_generation
            .checked_add(1)
            .ok_or(glow::OUT_OF_MEMORY)?;
        // Stale wrappers keep their unique IDs and still fail validation, but
        // repeated relinks must not retain unreachable native location records.
        self.locations
            .retain(|_, location| location.program != program);
        Ok(())
    }
    pub fn insert_location(
        &mut self,
        program: u32,
        native: glow::NativeUniformLocation,
    ) -> Result<u32, u32> {
        let generation = self
            .entries
            .get(&program)
            .filter(|e| !e.deleted && e.object.kind() == Kind::Program)
            .ok_or(glow::INVALID_OPERATION)?
            .link_generation;
        self.locations
            .try_reserve(1)
            .map_err(|_| glow::OUT_OF_MEMORY)?;
        let id = self.allocate_id()?;
        self.locations.insert(
            id,
            Location {
                native,
                program,
                generation,
            },
        );
        Ok(id)
    }
    pub fn location(
        &self,
        id: u32,
        current_program: u32,
    ) -> Result<glow::NativeUniformLocation, u32> {
        let location = self.locations.get(&id).ok_or(glow::INVALID_OPERATION)?;
        let program = self
            .entries
            .get(&location.program)
            .ok_or(glow::INVALID_OPERATION)?;
        // Deleting the current program leaves it alive until it is unbound;
        // existing locations remain valid during that interval (GLES 2.0).
        if program.superseded
            || location.program != current_program
            || location.generation != program.link_generation
        {
            return Err(glow::INVALID_OPERATION);
        }
        Ok(location.native)
    }
    pub fn forget_location(&mut self, id: u32) {
        self.locations.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;
    fn buffer(n: u32) -> Object {
        Object::Buffer(glow::NativeBuffer(NonZeroU32::new(n).unwrap()))
    }
    fn program(n: u32) -> Object {
        Object::Program(glow::NativeProgram(NonZeroU32::new(n).unwrap()))
    }
    #[test]
    fn names_are_not_reused_and_wrong_types_are_rejected() {
        let mut objects = Objects::default();
        let first = objects.insert(buffer(9)).unwrap();
        assert!(objects.get(first, Kind::Texture).is_err());
        assert!(objects.get(0, Kind::Buffer).is_err());
        assert!(objects.forget(first).is_some());
        let second = objects.insert(buffer(9)).unwrap();
        assert_ne!(first, second);
        assert!(!objects.contains(first, Kind::Buffer));
        assert!(objects.contains(second, Kind::Buffer));
    }
    #[test]
    fn deletion_is_idempotent_and_collection_cannot_double_delete() {
        let mut objects = Objects::default();
        let id = objects.insert(buffer(2)).unwrap();
        assert!(objects.mark_deleted(id, Kind::Texture).is_err());
        assert!(objects.mark_deleted(0, Kind::Buffer).unwrap().is_none());
        assert!(objects.mark_deleted(id, Kind::Buffer).unwrap().is_some());
        assert!(!objects.contains(id, Kind::Buffer));
        assert!(objects.mark_deleted(id, Kind::Buffer).unwrap().is_none());
        assert!(objects.forget(id).is_none());
        assert!(objects.forget(id).is_none());
    }
    #[test]
    fn uniform_locations_are_program_and_link_generation_specific() {
        let mut objects = Objects::default();
        let first = objects.insert(program(1)).unwrap();
        let second = objects.insert(program(2)).unwrap();
        let loc = objects
            .insert_location(first, glow::NativeUniformLocation(0))
            .unwrap();
        assert!(objects.location(loc, first).is_ok());
        assert!(objects.location(loc, second).is_err());
        objects.relink(first).unwrap();
        assert!(objects.location(loc, first).is_err());
        let fresh = objects
            .insert_location(first, glow::NativeUniformLocation(0))
            .unwrap();
        assert!(objects.location(fresh, first).is_ok());
        objects.mark_deleted(first, Kind::Program).unwrap();
        assert!(objects.location(fresh, first).is_ok());
        assert!(objects.location(fresh, 0).is_err());
        assert!(objects.get_for_query(first, Kind::Program).is_ok());
        assert!(objects
            .insert_location(first, glow::NativeUniformLocation(0))
            .is_err());
        objects.forget_location(fresh);
        assert!(objects.location(fresh, first).is_err());
    }
    #[test]
    fn native_name_reuse_prefers_the_live_wrapper() {
        let mut objects = Objects::default();
        let old = objects.insert(buffer(2)).unwrap();
        objects.mark_deleted(old, Kind::Buffer).unwrap();
        let new = objects.insert(buffer(2)).unwrap();
        assert_eq!(objects.id_for(buffer(2)), Some(new));
        assert!(objects.get_for_query(old, Kind::Buffer).is_err());
    }
    #[test]
    fn id_exhaustion_is_an_error_instead_of_an_alias() {
        let mut objects = Objects {
            next: u32::MAX,
            ..Objects::default()
        };
        assert_eq!(objects.insert(buffer(1)).unwrap_err(), glow::OUT_OF_MEMORY);
    }
    #[test]
    fn deletion_validation_does_not_commit_a_failed_native_deletion() {
        let mut objects = Objects::default();
        let id = objects.insert(buffer(1)).unwrap();
        assert!(objects.for_deletion(0, Kind::Buffer).unwrap().is_none());
        assert!(objects.for_deletion(id, Kind::Texture).is_err());
        assert!(objects.for_deletion(id, Kind::Buffer).unwrap().is_some());
        assert!(objects.contains(id, Kind::Buffer));
        objects.mark_deleted(id, Kind::Buffer).unwrap();
        assert!(objects.for_deletion(id, Kind::Buffer).unwrap().is_none());
        objects.forget(id);
        assert!(objects.for_deletion(id, Kind::Buffer).is_err());
    }
    #[test]
    fn reused_program_names_never_revive_a_deleted_wrapper_or_location() {
        let mut objects = Objects::default();
        let old = objects.insert(program(1)).unwrap();
        let location = objects
            .insert_location(old, glow::NativeUniformLocation(0))
            .unwrap();
        objects.mark_deleted(old, Kind::Program).unwrap();
        let new = objects.insert(program(1)).unwrap();
        objects.mark_deleted(new, Kind::Program).unwrap();
        assert!(objects.get_for_query(old, Kind::Program).is_err());
        assert!(objects.location(location, old).is_err());
        assert!(objects.get_for_query(new, Kind::Program).is_ok());
        assert_eq!(objects.id_for(program(1)), Some(new));
        objects.forget(old);
        assert_eq!(objects.program_shader_names.get(&(true, 1)), Some(&new));
        objects.forget(new);
        assert!(!objects.program_shader_names.contains_key(&(true, 1)));
        let shader = Object::Shader(glow::NativeShader(NonZeroU32::new(2).unwrap()));
        let old = objects.insert(shader).unwrap();
        objects.mark_deleted(old, Kind::Shader).unwrap();
        let new = objects.insert(shader).unwrap();
        assert!(objects.get_for_query(old, Kind::Shader).is_err());
        assert!(objects.get_for_query(new, Kind::Shader).is_ok());
    }
    #[test]
    fn relink_and_program_collection_reclaim_only_their_own_locations() {
        let mut objects = Objects::default();
        let first = objects.insert(program(1)).unwrap();
        let second = objects.insert(program(2)).unwrap();
        let second_location = objects
            .insert_location(second, glow::NativeUniformLocation(0))
            .unwrap();
        let mut previous = 0;
        for _ in 0..100 {
            let location = objects
                .insert_location(first, glow::NativeUniformLocation(0))
                .unwrap();
            assert!(location > previous);
            previous = location;
            objects.relink(first).unwrap();
            assert!(objects.location(location, first).is_err());
            assert_eq!(objects.locations.len(), 1);
        }
        objects.forget(first);
        assert!(objects.location(second_location, second).is_ok());
        objects.forget(second);
        assert!(objects.locations.is_empty());
        objects.forget_location(second_location); // A queued finalizer is harmless.
    }
}
