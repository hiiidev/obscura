//! Browser-owned framebuffer storage. Allocation is transactional: incomplete
//! replacements release every acquired object and restore the page's bindings.
//! The owning canvas makes its ANGLE context current before allocation/release.
use crate::{api::Attributes, egl, extensions::enable_internal_extension, pixels::MAX_TRANSFER_BYTES};
use glow::HasContext;

#[derive(Clone, Copy, Debug)]
pub(crate) struct StorageSpec {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub depth: bool,
    pub stencil: bool,
}
impl StorageSpec {
    pub(crate) fn validate(
        format: u32, width: u32, height: u32, version: u8, attributes: &Attributes,
        extensions: &std::collections::HashSet<String>, max_size: u32, samples: u32,
    ) -> Result<Self, u32> {
        if width == 0 || height == 0 || width > max_size || height > max_size { return Err(glow::INVALID_VALUE); }
        if !attributes.alpha { return Err(glow::INVALID_OPERATION); }
        let allowed = format == glow::RGBA8
            || format == glow::SRGB8_ALPHA8 && (version == 2 || extensions.contains("EXT_sRGB"))
            || format == glow::RGBA16F && extensions.contains(if version == 2 {
                "EXT_color_buffer_float"
            } else { "EXT_color_buffer_half_float" });
        if !allowed { return Err(glow::INVALID_ENUM); }
        let (backing_width, backing_height) = egl::backing_size(width, height).map_err(|_| glow::OUT_OF_MEMORY)?;
        let samples = if attributes.antialias { samples } else { 0 };
        if samples > i32::MAX as u32 { return Err(glow::OUT_OF_MEMORY); }
        let color = if format == glow::RGBA16F { 8_u64 } else { 4 };
        let depth = match (attributes.depth, attributes.stencil) { (true,true) => 4, (true,false) => 2, (false,true) => 1, _ => 0 };
        // Include both multisample storage and its resolved color attachment.
        let per_pixel = (color + depth) * u64::from(samples.max(1)) + if samples > 0 { color } else { 0 };
        if u64::from(backing_width) * u64::from(backing_height) * per_pixel > MAX_TRANSFER_BYTES as u64 {
            return Err(glow::OUT_OF_MEMORY);
        }
        Ok(Self { format, width: backing_width, height: backing_height, samples,
            depth: attributes.depth, stencil: attributes.stencil })
    }
    fn depth_storage(self) -> Option<(u32,u32)> {
        match (self.depth,self.stencil) {
            (true,true) => Some((glow::DEPTH24_STENCIL8,glow::DEPTH_STENCIL_ATTACHMENT)),
            (true,false) => Some((glow::DEPTH_COMPONENT16,glow::DEPTH_ATTACHMENT)),
            (false,true) => Some((glow::STENCIL_INDEX8,glow::STENCIL_ATTACHMENT)),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owned<F,R> { Framebuffer(F), Renderbuffer(R) }
trait StorageDriver {
    type Framebuffer: Copy + Eq;
    type Renderbuffer: Copy + Eq;
    type Bindings;
    fn bindings(&self) -> Self::Bindings;
    fn restore(&self, bindings: &Self::Bindings);
    fn framebuffer(&self) -> Result<Self::Framebuffer,u32>;
    fn renderbuffer(&self) -> Result<Self::Renderbuffer,u32>;
    fn bind(&self, framebuffer: Self::Framebuffer);
    fn storage(&self, buffer: Self::Renderbuffer, format: u32, samples: u32, width: u32, height: u32) -> Result<(),u32>;
    fn attach(&self, attachment: u32, buffer: Self::Renderbuffer);
    fn complete(&self) -> Result<(),u32>;
    fn initialize(&self, depth: bool, stencil: bool) -> Result<(),u32>;
    fn check(&self) -> Result<(),u32>;
    fn delete(&self, object: Owned<Self::Framebuffer,Self::Renderbuffer>);
}
struct Pending<'a,D:StorageDriver> {
    driver: &'a D,
    bindings: D::Bindings,
    owned: Vec<Owned<D::Framebuffer,D::Renderbuffer>>,
}
impl<D:StorageDriver> Drop for Pending<'_,D> {
    fn drop(&mut self) {
        self.driver.restore(&self.bindings);
        for object in self.owned.drain(..).rev() { self.driver.delete(object); }
    }
}
impl<D:StorageDriver> Pending<'_,D> {
    fn framebuffer(&mut self) -> Result<D::Framebuffer,u32> {
        let id=self.driver.framebuffer()?;self.owned.push(Owned::Framebuffer(id));
        self.driver.check()?;self.driver.bind(id);self.driver.check()?;Ok(id)
    }
    fn attachment(&mut self, point: u32, format: u32, samples: u32, spec: StorageSpec) -> Result<(),u32> {
        let id=self.driver.renderbuffer()?;self.owned.push(Owned::Renderbuffer(id));self.driver.check()?;
        self.driver.storage(id,format,samples,spec.width,spec.height)?;
        // The ES2 packed-depth extension attaches the same image to both
        // points; DEPTH_STENCIL_ATTACHMENT itself is an ES3 convenience token.
        let points=if point==glow::DEPTH_STENCIL_ATTACHMENT { &[glow::DEPTH_ATTACHMENT,glow::STENCIL_ATTACHMENT][..] } else { &[point][..] };
        for &point in points { self.driver.attach(point,id);self.driver.check()?; }
        Ok(())
    }
}
#[must_use = "Retire storage with its owning context current, or destroy the owning context"]
pub(crate) struct Backing<F=glow::NativeFramebuffer,R=glow::NativeRenderbuffer> {
    pub draw: F,
    pub read: F,
    pub spec: StorageSpec,
    owned: Vec<Owned<F,R>>,
    blit: Option<Blit>,
}
fn allocate<D:StorageDriver>(driver: &D, spec: StorageSpec) -> Result<Backing<D::Framebuffer,D::Renderbuffer>,u32> {
    let mut pending=Pending { driver, bindings:driver.bindings(), owned:Vec::with_capacity(5) };
    let draw=pending.framebuffer()?;
    pending.attachment(glow::COLOR_ATTACHMENT0,spec.format,spec.samples,spec)?;
    if let Some((format,point))=spec.depth_storage() { pending.attachment(point,format,spec.samples,spec)?; }
    driver.complete()?;driver.initialize(spec.depth,spec.stencil)?;
    let read=if spec.samples>0 {
        let id=pending.framebuffer()?;
        pending.attachment(glow::COLOR_ATTACHMENT0,spec.format,0,spec)?;
        driver.complete()?;driver.initialize(false,false)?;id
    } else { draw };
    Ok(Backing { draw,read,spec,owned:std::mem::take(&mut pending.owned),blit:None })
}
impl<F:Copy+Eq,R:Copy+Eq> Backing<F,R> {
    fn release<D:StorageDriver<Framebuffer=F,Renderbuffer=R>>(self, driver: &D) {
        for object in self.owned.into_iter().rev() { driver.delete(object); }
    }
}
type MultisampleStorage = unsafe extern "system" fn(u32,i32,u32,i32,i32);
type Blit = unsafe extern "system" fn(i32,i32,i32,i32,i32,i32,i32,i32,u32,u32);
struct Native<'a> {
    context: &'a egl::Context,
    version: u8,
    multisample: Option<MultisampleStorage>,
    blit: Option<Blit>,
}
impl<'a> Native<'a> {
    fn new(context: &'a egl::Context, version: u8, spec: StorageSpec) -> Result<Self,u32> {
        let mut result=Self { context,version,multisample:None,blit:None };
        if version == 1 {
            let mut extensions=vec!["GL_OES_rgb8_rgba8"];
            if spec.depth && spec.stencil { extensions.push("GL_OES_packed_depth_stencil"); }
            if spec.samples>0 { extensions.extend(["GL_ANGLE_framebuffer_multisample","GL_ANGLE_framebuffer_blit"]); }
            if extensions.iter().any(|name| !enable_internal_extension(context,name)) {
                return Err(glow::INVALID_OPERATION);
            }
            if spec.samples>0 {
                result.multisample=Some(unsafe { context.entry(c"glRenderbufferStorageMultisampleANGLE") }?);
                result.blit=Some(unsafe { context.entry(c"glBlitFramebufferANGLE") }?);
            }
        }
        Ok(result)
    }
}
impl StorageDriver for Native<'_> {
    type Framebuffer=glow::NativeFramebuffer;
    type Renderbuffer=glow::NativeRenderbuffer;
    type Bindings=(Option<glow::NativeFramebuffer>,Option<glow::NativeFramebuffer>,Option<glow::NativeRenderbuffer>);
    fn bindings(&self) -> Self::Bindings {
        let gl=&self.context.gl;
        unsafe {
            let draw=gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let read=if self.version==2 { gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING) } else { draw };
            (draw,read,gl.get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING))
        }
    }
    fn restore(&self, bindings: &Self::Bindings) {
        let gl=&self.context.gl;
        unsafe {
            if self.version==2 {
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER,bindings.0);gl.bind_framebuffer(glow::READ_FRAMEBUFFER,bindings.1);
            } else { gl.bind_framebuffer(glow::FRAMEBUFFER,bindings.0); }
            gl.bind_renderbuffer(glow::RENDERBUFFER,bindings.2);
        }
    }
    fn framebuffer(&self) -> Result<Self::Framebuffer,u32> { unsafe { self.context.gl.create_framebuffer() }.map_err(|_| glow::OUT_OF_MEMORY) }
    fn renderbuffer(&self) -> Result<Self::Renderbuffer,u32> { unsafe { self.context.gl.create_renderbuffer() }.map_err(|_| glow::OUT_OF_MEMORY) }
    fn bind(&self, framebuffer: Self::Framebuffer) { unsafe { self.context.gl.bind_framebuffer(glow::FRAMEBUFFER,Some(framebuffer)); } }
    fn storage(&self, buffer: Self::Renderbuffer, format: u32, samples: u32, width: u32, height: u32) -> Result<(),u32> {
        let gl=&self.context.gl;
        unsafe {
            gl.bind_renderbuffer(glow::RENDERBUFFER,Some(buffer));
            if samples==0 { gl.renderbuffer_storage(glow::RENDERBUFFER,format,width as i32,height as i32); }
            else if let Some(storage)=self.multisample { storage(glow::RENDERBUFFER,samples as i32,format,width as i32,height as i32); }
            else { gl.renderbuffer_storage_multisample(glow::RENDERBUFFER,samples as i32,format,width as i32,height as i32); }
        }
        self.check()
    }
    fn attach(&self, point: u32, buffer: Self::Renderbuffer) {
        unsafe { self.context.gl.framebuffer_renderbuffer(glow::FRAMEBUFFER,point,glow::RENDERBUFFER,Some(buffer)); }
    }
    fn complete(&self) -> Result<(),u32> {
        let status=unsafe { self.context.gl.check_framebuffer_status(glow::FRAMEBUFFER) };
        self.check()?;
        if status==glow::FRAMEBUFFER_COMPLETE { Ok(()) } else { Err(glow::OUT_OF_MEMORY) }
    }
    fn initialize(&self, depth: bool, stencil: bool) -> Result<(),u32> {
        let gl=&self.context.gl;
        unsafe {
            let scissor=gl.is_enabled(glow::SCISSOR_TEST);let mask=gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK);
            // WebGL 2 drops Clear while RASTERIZER_DISCARD is enabled.
            let discard=self.version==2 && gl.is_enabled(glow::RASTERIZER_DISCARD);
            let depth_mask=gl.get_parameter_bool(glow::DEPTH_WRITEMASK);
            let front=gl.get_parameter_i32(glow::STENCIL_WRITEMASK) as u32;let back=gl.get_parameter_i32(glow::STENCIL_BACK_WRITEMASK) as u32;
            let mut color=[0.;4];gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE,&mut color);
            let depth_value=gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE);let stencil_value=gl.get_parameter_i32(glow::STENCIL_CLEAR_VALUE);
            gl.disable(glow::SCISSOR_TEST);if discard { gl.disable(glow::RASTERIZER_DISCARD); }
            gl.color_mask(true,true,true,true);gl.depth_mask(true);gl.stencil_mask(u32::MAX);
            gl.clear_color(0.,0.,0.,0.);gl.clear_depth_f32(1.);gl.clear_stencil(0);
            gl.clear(glow::COLOR_BUFFER_BIT | if depth { glow::DEPTH_BUFFER_BIT } else { 0 } | if stencil { glow::STENCIL_BUFFER_BIT } else { 0 });
            gl.clear_color(color[0],color[1],color[2],color[3]);gl.clear_depth_f32(depth_value);gl.clear_stencil(stencil_value);
            gl.color_mask(mask[0],mask[1],mask[2],mask[3]);gl.depth_mask(depth_mask);
            gl.stencil_mask_separate(glow::FRONT,front);gl.stencil_mask_separate(glow::BACK,back);
            if discard { gl.enable(glow::RASTERIZER_DISCARD); }
            if scissor { gl.enable(glow::SCISSOR_TEST); }
        }
        self.check()
    }
    fn check(&self) -> Result<(),u32> {
        let error=unsafe { self.context.gl.get_error() };
        if error==glow::NO_ERROR { Ok(()) } else { Err(error) }
    }
    fn delete(&self, object: Owned<Self::Framebuffer,Self::Renderbuffer>) {
        unsafe { match object {
            Owned::Framebuffer(id)=>self.context.gl.delete_framebuffer(id),
            Owned::Renderbuffer(id)=>self.context.gl.delete_renderbuffer(id),
        } }
    }
}
impl Backing {
    // Caller drains preexisting driver errors into its browser error queue.
    // This layer returns new allocation/driver errors without consuming them
    // as evidence that an unrelated old page operation failed.
    pub(crate) fn create(context: &egl::Context, version: u8, spec: StorageSpec) -> Result<Self,u32> {
        let native=Native::new(context,version,spec)?;
        let mut backing=allocate(&native,spec)?;backing.blit=native.blit;Ok(backing)
    }
    pub(crate) fn destroy(self, context: &egl::Context) {
        // Deletion requires no extension entry points. The owning context is
        // current; a lost context instead destroys all its native names at once.
        self.release(&Native { context,version:2,multisample:None,blit:None });
    }
    pub(crate) fn resolve(&self, context: &egl::Context, version: u8) -> Result<glow::NativeFramebuffer,u32> {
        if self.draw==self.read { return Ok(self.read); }
        let driver=Native { context,version,multisample:None,blit:self.blit };
        let bindings=driver.bindings();let gl=&context.gl;
        unsafe {
            let scissor=gl.is_enabled(glow::SCISSOR_TEST);gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER,Some(self.draw));gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER,Some(self.read));
            let read_buffer=if version==2 { let value=gl.get_parameter_i32(glow::READ_BUFFER);gl.read_buffer(glow::COLOR_ATTACHMENT0);Some(value) } else { None };
            let draw_buffer=if version==2 { let value=gl.get_parameter_i32(glow::DRAW_BUFFER0);gl.draw_buffers(&[glow::COLOR_ATTACHMENT0]);Some(value) } else { None };
            let w=self.spec.width as i32;let h=self.spec.height as i32;
            if let Some(blit)=driver.blit { blit(0,0,w,h,0,0,w,h,glow::COLOR_BUFFER_BIT,glow::NEAREST); }
            else { gl.blit_framebuffer(0,0,w,h,0,0,w,h,glow::COLOR_BUFFER_BIT,glow::NEAREST); }
            if let Some(value)=read_buffer { gl.read_buffer(value as u32); }
            if let Some(value)=draw_buffer { gl.draw_buffers(&[value as u32]); }
            if scissor { gl.enable(glow::SCISSOR_TEST); }
        }
        driver.restore(&bindings);driver.check()?;Ok(self.read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::{Cell,RefCell}, collections::HashSet};
    fn spec(samples: u32, depth: bool, stencil: bool) -> StorageSpec {
        StorageSpec { format:glow::RGBA8,width:8,height:4,samples,depth,stencil }
    }
    #[test]
    fn storage_format_validation_obeys_alpha_version_and_extension_gates() {
        for version in [1,2] {
            for alpha in [false,true] {
                for half in [false,true] {
                    for float in [false,true] {
                        for srgb in [false,true] {
                            let attributes=Attributes { alpha,..Attributes::default() };
                            let mut extensions=HashSet::new();
                            if half { extensions.insert("EXT_color_buffer_half_float".into()); }
                            if float { extensions.insert("EXT_color_buffer_float".into()); }
                            if srgb { extensions.insert("EXT_sRGB".into()); }
                            for (format,allowed) in [(glow::RGBA8,true),(glow::SRGB8_ALPHA8,version==2||srgb),
                                (glow::RGBA16F,if version==2 { float } else { half }),(glow::RGB8,false)] {
                                let result=StorageSpec::validate(format,8,4,version,&attributes,&extensions,4096,0);
                                if !alpha { assert_eq!(result.unwrap_err(),glow::INVALID_OPERATION); }
                                else if !allowed { assert_eq!(result.unwrap_err(),glow::INVALID_ENUM); }
                                else { assert_eq!(result.unwrap().format,format); }
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn storage_limits_account_for_samples_depth_resolve_and_zero_sized_canvases() {
        let mut attributes=Attributes::default();let extensions=HashSet::new();
        let request=|w,h,limit,samples,a:&Attributes| StorageSpec::validate(glow::RGBA8,w,h,2,a,&extensions,limit,samples);
        for (w,h) in [(0,0),(0,4),(4,0)] { assert_eq!(request(w,h,4096,0,&attributes).unwrap_err(),glow::INVALID_VALUE); }
        assert_eq!(request(4097,1,4096,0,&attributes).unwrap_err(),glow::INVALID_VALUE);
        assert_eq!(request(1,4097,4096,0,&attributes).unwrap_err(),glow::INVALID_VALUE);
        assert_eq!(request(u32::MAX,1,u32::MAX,0,&attributes).unwrap_err(),glow::OUT_OF_MEMORY);
        assert_eq!(request(4096,4097,u32::MAX,0,&attributes).unwrap_err(),glow::OUT_OF_MEMORY);
        assert!(request(4096,4096,4096,0,&attributes).is_ok());
        assert_eq!(request(4096,4096,4096,4,&attributes).unwrap_err(),glow::OUT_OF_MEMORY);
        assert_eq!(request(1,1,4096,u32::MAX,&attributes).unwrap_err(),glow::OUT_OF_MEMORY);
        attributes.antialias=false;assert_eq!(request(8,4,4096,4,&attributes).unwrap().samples,0);
    }
    #[test]
    fn storage_depth_and_stencil_requests_keep_disabled_attachments_absent() {
        for (depth,stencil,expected) in [(false,false,None),(true,false,Some((glow::DEPTH_COMPONENT16,glow::DEPTH_ATTACHMENT))),
            (false,true,Some((glow::STENCIL_INDEX8,glow::STENCIL_ATTACHMENT))),
            (true,true,Some((glow::DEPTH24_STENCIL8,glow::DEPTH_STENCIL_ATTACHMENT)))] {
            assert_eq!(spec(0,depth,stencil).depth_storage(),expected);
        }
    }
    #[derive(Default)]
    struct Fake {
        next:Cell<u32>, step:Cell<usize>, failure:Cell<Option<usize>>, panic_at:Cell<Option<usize>>,
        live:RefCell<HashSet<u32>>, deleted:RefCell<Vec<u32>>, bindings:Cell<(u32,u32)>,
        allocations:RefCell<Vec<(u32,u32,u32,u32)>>, clears:RefCell<Vec<(bool,bool)>>,
        attachments:RefCell<Vec<(u32,u32)>>,
    }
    impl Fake {
        fn new() -> Self { Self { bindings:Cell::new((900,901)),..Self::default() } }
        fn advance(&self) -> Result<(),u32> {
            let step=self.step.get()+1;self.step.set(step);
            assert_ne!(self.panic_at.get(),Some(step),"injected allocation panic");
            if self.failure.get()==Some(step) { Err(glow::OUT_OF_MEMORY) } else { Ok(()) }
        }
        fn object(&self) -> Result<u32,u32> {
            self.advance()?;let id=self.next.get()+1;self.next.set(id);assert!(self.live.borrow_mut().insert(id));Ok(id)
        }
    }
    impl StorageDriver for Fake {
        type Framebuffer=u32;type Renderbuffer=u32;type Bindings=(u32,u32);
        fn bindings(&self) -> Self::Bindings { self.bindings.get() }
        fn restore(&self,b:&Self::Bindings) { self.bindings.set(*b); }
        fn framebuffer(&self) -> Result<u32,u32> { self.object() }
        fn renderbuffer(&self) -> Result<u32,u32> { self.object() }
        fn bind(&self,id:u32) { self.bindings.set((id,self.bindings.get().1)); }
        fn storage(&self,id:u32,format:u32,samples:u32,w:u32,h:u32) -> Result<(),u32> {
            self.bindings.set((self.bindings.get().0,id));self.allocations.borrow_mut().push((format,samples,w,h));self.advance()
        }
        fn attach(&self,point:u32,buffer:u32) { self.attachments.borrow_mut().push((point,buffer)); }
        fn complete(&self) -> Result<(),u32> { self.advance() }
        fn initialize(&self,depth:bool,stencil:bool) -> Result<(),u32> { self.clears.borrow_mut().push((depth,stencil));self.advance() }
        fn check(&self) -> Result<(),u32> { self.advance() }
        fn delete(&self,object:Owned<u32,u32>) {
            let (Owned::Framebuffer(id)|Owned::Renderbuffer(id))=object;
            assert!(self.live.borrow_mut().remove(&id),"object released more than once");self.deleted.borrow_mut().push(id);
        }
    }
    #[test]
    fn every_partial_storage_failure_releases_resources_and_restores_bindings() {
        for samples in [0,4] {
            for (depth,stencil) in [(false,false),(true,false),(false,true),(true,true)] {
                let request=spec(samples,depth,stencil);let success=Fake::new();
                let result=allocate(&success,request).unwrap();let steps=success.step.get();result.release(&success);
                for fail in 1..=steps {
                    let driver=Fake::new();driver.failure.set(Some(fail));
                    assert!(matches!(allocate(&driver,request),Err(glow::OUT_OF_MEMORY)));
                    assert!(driver.live.borrow().is_empty(),"leak at operation {fail}");
                    assert_eq!(driver.bindings.get(),(900,901));
                    assert_eq!(driver.deleted.borrow().len(),driver.next.get() as usize);
                }
            }
        }
    }
    #[test]
    fn unwinding_during_storage_allocation_also_releases_every_acquired_handle() {
        let request=spec(4,true,true);let success=Fake::new();let backing=allocate(&success,request).unwrap();
        let steps=success.step.get();backing.release(&success);
        for step in 1..=steps {
            let driver=Fake::new();driver.panic_at.set(Some(step));
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| allocate(&driver,request))).is_err());
            assert!(driver.live.borrow().is_empty());assert_eq!(driver.bindings.get(),(900,901));
        }
    }
    #[test]
    fn successful_storage_owns_distinct_resolve_storage_only_when_multisampled() {
        for samples in [0,4] {
            let driver=Fake::new();let backing=allocate(&driver,spec(samples,true,true)).unwrap();
            assert_eq!(backing.draw==backing.read,samples==0);assert_eq!(driver.bindings.get(),(900,901));
            assert_eq!(driver.live.borrow().len(),if samples==0 { 3 } else { 5 });
            assert_eq!(driver.clears.borrow()[0],(true,true));
            let attachments=driver.attachments.borrow();
            assert_eq!(attachments[1].0,glow::DEPTH_ATTACHMENT);assert_eq!(attachments[2].0,glow::STENCIL_ATTACHMENT);
            assert_eq!(attachments[1].1,attachments[2].1);drop(attachments);
            if samples>0 {
                assert_eq!(driver.clears.borrow()[1],(false,false));
                assert_eq!(driver.allocations.borrow()[2],(glow::RGBA8,0,8,4));
            }
            backing.release(&driver);assert!(driver.live.borrow().is_empty());
            assert_eq!(driver.deleted.borrow().len(),driver.next.get() as usize);
        }
    }
}
