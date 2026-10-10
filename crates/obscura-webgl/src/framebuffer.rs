//! Logical default framebuffer isolation from browser-owned native attachments.
use crate::{api::CanvasContext, drawing_buffer::{Backing,StorageSpec}, queries::Value};
use glow::HasContext;

pub(crate) fn default_buffer_token(value: u32, owned: bool) -> Result<u32,u32> {
    match value {
        glow::NONE => Ok(glow::NONE),
        glow::BACK => Ok(if owned { glow::COLOR_ATTACHMENT0 } else { glow::BACK }),
        _ => Err(glow::INVALID_OPERATION),
    }
}
pub(crate) fn default_invalidation(attachments: &[u32], owned: bool) -> Result<Vec<u32>,u32> {
    attachments.iter().map(|&value| {
        let mapped=match value { glow::COLOR=>glow::COLOR_ATTACHMENT0,glow::DEPTH=>glow::DEPTH_ATTACHMENT,
            glow::STENCIL=>glow::STENCIL_ATTACHMENT,_=>return Err(glow::INVALID_ENUM) };
        Ok(if owned { mapped } else { value })
    }).collect()
}

// Readback and copy operations temporarily bind the resolved image. No page
// bindings or read-buffer selections survive this guard, including on unwind.
pub(crate) struct ReadBinding<'a> {
    gl: &'a glow::Context,
    target: u32,
    previous: Option<glow::NativeFramebuffer>,
    selection: Option<u32>,
}
impl Drop for ReadBinding<'_> {
    fn drop(&mut self) { unsafe {
        if let Some(selection)=self.selection { self.gl.read_buffer(selection); }
        self.gl.bind_framebuffer(self.target,self.previous);
    } }
}
impl CanvasContext {
    pub(crate) fn graphics_error(&mut self,error:u32) {
        if error==glow::CONTEXT_LOST { self.lose(); } else { self.error(error); }
    }
    pub(crate) fn copy_from_read_buffer(&self, command: &crate::commands::Command) -> Result<(),u32> {
        use crate::commands::Command::*;
        let _read=self.resolved_read(false)?;
        let gl=&self.driver.as_ref().unwrap().gl;
        unsafe { match *command {
            CopyTexImage2D {target,level,format,x,y,width,height,border}=>gl.copy_tex_image_2d(target,level,format,x,y,width,height,border),
            CopyTexSubImage2D {target,level,xoffset,yoffset,x,y,width,height}=>gl.copy_tex_sub_image_2d(target,level,xoffset,yoffset,x,y,width,height),
            CopyTexSubImage3D {target,level,xoffset,yoffset,zoffset,x,y,width,height}=>gl.copy_tex_sub_image_3d(target,level,xoffset,yoffset,zoffset,x,y,width,height),
            _=>unreachable!("copy command validated before dispatch"),
        } }
        Ok(())
    }
    pub fn drawing_buffer_format(&self) -> u32 {
        self.drawing_storage.as_ref().map_or(if self.attributes.alpha { glow::RGBA8 } else { glow::RGB8 },|s|s.spec.format)
    }
    pub(crate) fn default_framebuffer(&self) -> Option<glow::NativeFramebuffer> {
        self.drawing_storage.as_ref().map(|s|s.draw)
    }
    pub(crate) fn default_bound(&self, target: u32) -> bool {
        let gl=&self.driver.as_ref().expect("current canvas").gl;
        let binding=if self.version==2 && target==glow::READ_FRAMEBUFFER { glow::READ_FRAMEBUFFER_BINDING } else { glow::FRAMEBUFFER_BINDING };
        unsafe { gl.get_parameter_framebuffer(binding)==self.default_framebuffer() }
    }
    pub(crate) fn rebind_default_after_deletion(&self) {
        let Some(default)=self.default_framebuffer() else { return; };
        let gl=&self.driver.as_ref().expect("current canvas").gl;
        unsafe {
            if self.version==2 {
                for (target,binding) in [(glow::DRAW_FRAMEBUFFER,glow::DRAW_FRAMEBUFFER_BINDING),(glow::READ_FRAMEBUFFER,glow::READ_FRAMEBUFFER_BINDING)] {
                    if gl.get_parameter_framebuffer(binding).is_none() { gl.bind_framebuffer(target,Some(default)); }
                }
            } else if gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING).is_none() { gl.bind_framebuffer(glow::FRAMEBUFFER,Some(default)); }
        }
    }
    pub fn drawing_buffer_storage(&mut self, format: u32, width: u32, height: u32) -> bool {
        if !self.activate() { return false; }
        self.retain_driver_errors();
        if self.is_lost() { return false; }
        let driver=self.driver.as_ref().unwrap();
        let max_size=unsafe { driver.gl.get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE).max(0) as u32 };
        let result=StorageSpec::validate(format,width,height,self.version,&self.attributes,&self.extensions,max_size,driver.samples)
            .and_then(|spec| Backing::create(driver,self.version,spec));
        let replacement=match result {
            Ok(storage)=>storage,
            Err(error)=>{ if error==glow::CONTEXT_LOST { self.lose(); } else { self.error(error); } return false; }
        };
        // Allocate before publishing. Both page bindings remain unchanged when
        // allocation fails, and an unbound page FBO must not become the default.
        let old=self.default_framebuffer();let gl=&driver.gl;
        unsafe {
            let draw=gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let read=if self.version==2 { gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING) } else { draw };
            gl.bind_framebuffer(glow::FRAMEBUFFER,Some(replacement.draw));
            if self.version==2 {
                gl.draw_buffers(&[default_buffer_token(self.default_draw_buffer,true).unwrap()]);
                gl.read_buffer(default_buffer_token(self.default_read_buffer,true).unwrap());
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER,if draw==old { Some(replacement.draw) } else { draw });
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER,if read==old { Some(replacement.draw) } else { read });
            } else { gl.bind_framebuffer(glow::FRAMEBUFFER,if draw==old { Some(replacement.draw) } else { draw }); }
        }
        if let Some(previous)=self.drawing_storage.replace(replacement) { previous.destroy(driver); }
        self.width=width;self.height=height;self.dirty=true;true
    }
    pub(crate) fn resolved_read(&self, force_default: bool) -> Result<Option<ReadBinding<'_>>,u32> {
        let driver=self.driver.as_ref().expect("current canvas");let gl=&driver.gl;
        let target=if self.version==2 { glow::READ_FRAMEBUFFER } else { glow::FRAMEBUFFER };
        if !force_default && !self.default_bound(target) { return Ok(None); }
        if self.drawing_storage.is_none() && !force_default { return Ok(None); }
        let binding=if self.version==2 { glow::READ_FRAMEBUFFER_BINDING } else { glow::FRAMEBUFFER_BINDING };
        let previous=unsafe { gl.get_parameter_framebuffer(binding) };
        let resolved=self.drawing_storage.as_ref().map(|s|s.resolve(driver,self.version)).transpose()?;
        let selected=if force_default { glow::BACK } else { self.default_read_buffer };
        let selected=default_buffer_token(selected,resolved.is_some())?;
        unsafe {
            gl.bind_framebuffer(target,resolved);
            let selection=if self.version==2 {
                let previous=gl.get_parameter_i32(glow::READ_BUFFER) as u32;
                gl.read_buffer(selected);Some(previous)
            } else { None };
            Ok(Some(ReadBinding { gl,target,previous,selection }))
        }
    }
    pub(crate) fn default_attachment_parameter(&self, attachment: u32, name: u32) -> Result<Value,u32> {
        if self.version==1 { return Err(glow::INVALID_OPERATION); }
        if ![glow::BACK,glow::DEPTH,glow::STENCIL].contains(&attachment) { return Err(glow::INVALID_ENUM); }
        let missing=attachment==glow::DEPTH && !self.attributes.depth || attachment==glow::STENCIL && !self.attributes.stencil;
        if missing { return if name==glow::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE { Ok(Value::Int(glow::NONE as i32)) } else { Err(glow::INVALID_OPERATION) }; }
        if name==glow::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE { return Ok(Value::Int(glow::FRAMEBUFFER_DEFAULT as i32)); }
        if !matches!(name,glow::FRAMEBUFFER_ATTACHMENT_RED_SIZE|glow::FRAMEBUFFER_ATTACHMENT_GREEN_SIZE|glow::FRAMEBUFFER_ATTACHMENT_BLUE_SIZE
            |glow::FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE|glow::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE|glow::FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE
            |glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE|glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING) { return Err(glow::INVALID_ENUM); }
        let spec=self.drawing_storage.as_ref().map(|s|s.spec);let driver=self.driver.as_ref().unwrap();
        let color=attachment==glow::BACK;let float=self.drawing_buffer_format()==glow::RGBA16F;
        let value=match name {
            glow::FRAMEBUFFER_ATTACHMENT_RED_SIZE|glow::FRAMEBUFFER_ATTACHMENT_GREEN_SIZE|glow::FRAMEBUFFER_ATTACHMENT_BLUE_SIZE=>if color { if float {16} else {8} } else {0},
            glow::FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE=>if color && self.attributes.alpha { if float {16} else {8} } else {0},
            glow::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE=>if attachment==glow::DEPTH { spec.map_or(driver.depth_bits,|s|if s.stencil {24} else {16}) as i32 } else {0},
            glow::FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE=>if attachment==glow::STENCIL {8} else {0},
            glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE=>(if color && float {glow::FLOAT} else if attachment==glow::STENCIL {glow::UNSIGNED_INT} else {glow::UNSIGNED_NORMALIZED}) as i32,
            glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING=>(if color && self.drawing_buffer_format()==glow::SRGB8_ALPHA8 {glow::SRGB} else {glow::LINEAR}) as i32,
            _=>unreachable!(),
        };
        Ok(Value::Int(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_buffer_selection_never_accepts_private_attachment_tokens() {
        for owned in [false,true] {
            assert_eq!(default_buffer_token(glow::BACK,owned),Ok(if owned {glow::COLOR_ATTACHMENT0} else {glow::BACK}));
            assert_eq!(default_buffer_token(glow::NONE,owned),Ok(glow::NONE));
            for token in [glow::COLOR_ATTACHMENT0,glow::FRONT,glow::DEPTH,1] { assert_eq!(default_buffer_token(token,owned),Err(glow::INVALID_OPERATION)); }
        }
    }
    #[test]
    fn default_invalidation_maps_only_default_attachments() {
        for owned in [false,true] {
            assert!(default_invalidation(&[],owned).unwrap().is_empty());
            let values=[glow::COLOR,glow::DEPTH,glow::STENCIL,glow::COLOR];
            assert_eq!(default_invalidation(&values,owned).unwrap(),if owned {vec![glow::COLOR_ATTACHMENT0,glow::DEPTH_ATTACHMENT,glow::STENCIL_ATTACHMENT,glow::COLOR_ATTACHMENT0]} else {values.to_vec()});
            for token in [glow::BACK,glow::COLOR_ATTACHMENT0,glow::DEPTH_ATTACHMENT,0] { assert_eq!(default_invalidation(&[glow::COLOR,token],owned),Err(glow::INVALID_ENUM)); }
        }
    }
}
