//! Real driver cases, intentionally unexecuted until the graphics validation
//! phase. The handoff must explicitly include these ignored tests; a normal
//! unit-test run is not evidence that any graphics backend works.
use crate::{
    api::{Attributes, CanvasContext},
    commands::Command,
    objects::Kind,
    resources::ResourceCommand,
    selection::Mode,
};
use crate::{
    egl::{Context, SurfaceOptions},
    selection::Backend,
};
use glow::HasContext;
use crate::queries::{Query,Value};

fn browser_context(version: u8) -> CanvasContext {
    let requested = backend();
    let mode = if requested == Backend::SwiftShader {
        Mode::Software
    } else {
        Mode::Hardware
    };
    let context = CanvasContext::create(
        version,
        8,
        8,
        Attributes {
            antialias: false,
            ..Attributes::default()
        },
        mode,
    )
    .unwrap();
    assert_eq!(context.diagnostics.backend, requested);
    context
}

#[test]
#[ignore = "mandatory real-driver drawing color changes, raw reads, presentation conversion and restored metadata"]
fn real_color_space_change_clears_storage_preserves_state_and_converts_only_browser_output() {
    use crate::color::ColorSpace::{Srgb,DisplayP3};
    for version in [1,2] {
        for owned in [false,true] {
            let mut context=browser_context(version);
            if owned {assert!(context.drawing_buffer_storage(glow::RGBA8,2,3));}
            context.command(Command::ClearColor {red:1.0,green:0.0,blue:0.0,alpha:1.0});
            context.command(Command::Clear {mask:glow::COLOR_BUFFER_BIT});
            context.error(glow::INVALID_ENUM);
            let viewport=context.query(Query::GetParameter {name:glow::VIEWPORT});
            let dimensions=(context.width,context.height,context.canvas_size);
            context.set_drawing_color_space(DisplayP3);
            assert_eq!(context.get_error(),glow::INVALID_ENUM);
            assert_eq!(context.query(Query::GetParameter {name:glow::VIEWPORT}),viewport);
            assert_eq!((context.width,context.height,context.canvas_size),dimensions);
            assert!(context.drawing_buffer_source_rgba().unwrap().iter().all(|&v|v==0));
            context.command(Command::ClearColor {red:128.0/255.0,green:64.0/255.0,blue:32.0/255.0,alpha:1.0});
            context.command(Command::Clear {mask:glow::COLOR_BUFFER_BIT});
            assert_eq!(&context.drawing_buffer_source_rgba().unwrap()[..4],&[128,64,32,255]);
            assert_eq!(&context.drawing_buffer_rgba().unwrap()[..4],&[138,59,21,255]);
            let mut page=[0;4];context.read_pixels(crate::transfers::ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::UNSIGNED_BYTE},&mut page);
            assert_eq!(page,[128,64,32,255]);
            context.dirty=false;context.set_drawing_color_space(DisplayP3);assert!(!context.dirty);
            assert_eq!(&context.drawing_buffer_source_rgba().unwrap()[..4],&page);
            context.lose();context.set_drawing_color_space(Srgb);context.unpack_color_space=DisplayP3;
            let mode=if context.diagnostics.backend==Backend::SwiftShader {Mode::Software} else {Mode::Hardware};
            assert!(context.restore(mode).unwrap());
            assert_eq!(context.drawing_color_space,Srgb);assert_eq!(context.unpack_color_space,DisplayP3);
            assert_eq!(context.get_error(),glow::NO_ERROR);
        }
    }
}

#[test]
#[ignore = "mandatory real-driver DOM/bitmap upload color conversion, NONE override and unchanged typed-array uploads"]
fn real_upload_color_space_obeys_none_and_preserves_bitmap_alpha() {
    use crate::color::ColorSpace::{Srgb,DisplayP3};
    use crate::transfers::{TextureImage,ReadPixels};
    for version in [1,2] {
        let mut context=browser_context(version);
        let texture=context.create_object(Kind::Texture,0).unwrap();
        let framebuffer=context.create_object(Kind::Framebuffer,0).unwrap();
        context.resource(ResourceCommand::BindTexture {target:glow::TEXTURE_2D,id:texture},None);
        context.resource(ResourceCommand::BindFramebuffer {target:glow::FRAMEBUFFER,id:framebuffer},None);
        let image=||TextureImage {target:glow::TEXTURE_2D,level:0,internal_format:glow::RGBA as i32,width:1,height:1,depth:1,
            format:glow::RGBA,data_type:glow::UNSIGNED_BYTE,border:0,x:0,y:0,z:0,sub_image:false,three_dimensional:false};
        for (none,bitmap,premultiplied,source,target,input,expected) in [
            (false,false,false,Srgb,DisplayP3,[255,0,0,255],[234,51,35,255]),
            (true,false,false,Srgb,DisplayP3,[255,0,0,255],[255,0,0,255]),
            (false,true,true,DisplayP3,Srgb,[128,0,0,128],[128,0,0,128]),
            (true,true,true,DisplayP3,Srgb,[7,19,231,0],[7,19,231,0]),
        ] {
            context.unpack_color_space=target;context.pixel_store(0x9243,if none {0} else {0x9244});
            context.texture_source_color(image(),1,1,&input,bitmap,source,premultiplied);
            context.resource(ResourceCommand::FramebufferTexture2D {target:glow::FRAMEBUFFER,attachment:glow::COLOR_ATTACHMENT0,texture_target:glow::TEXTURE_2D,id:texture,level:0},None);
            let mut result=[0;4];context.read_pixels(ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::UNSIGNED_BYTE},&mut result);
            assert_eq!(result,expected);assert_eq!(context.get_error(),glow::NO_ERROR);
        }
        context.unpack_color_space=DisplayP3;context.pixel_store(0x9243,0x9244);
        context.texture_image(image(),Some(&[255,0,0,255]));
        let mut result=[0;4];context.read_pixels(ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::UNSIGNED_BYTE},&mut result);
        assert_eq!(result,[255,0,0,255]);assert_eq!(context.get_error(),glow::NO_ERROR);
    }
}

#[test]
#[ignore = "mandatory real-driver drawing storage isolation, replacement failure and page framebuffer deletion"]
fn real_drawing_storage_is_private_and_failed_replacement_preserves_pixels() {
    for version in [1,2] {
        let mut context=browser_context(version);
        context.error(glow::INVALID_ENUM);
        assert!(context.drawing_buffer_storage(glow::RGBA8,4,3));
        assert_eq!(context.get_error(),glow::INVALID_ENUM);
        assert_eq!(context.query(Query::GetParameter {name:glow::VIEWPORT}),Value::Int32(vec![0,0,8,8]));
        assert_eq!((context.width,context.height,context.canvas_size),(4,3,(8,8)));
        assert_eq!(context.query(Query::GetParameter {name:glow::FRAMEBUFFER_BINDING}),Value::Null);
        context.command(Command::ClearColor {red:1.0,green:0.0,blue:0.0,alpha:1.0});
        context.dirty=false;context.command(Command::Clear {mask:glow::COLOR_BUFFER_BIT});assert!(context.dirty);
        let red=context.drawing_buffer_rgba().unwrap();assert_eq!(red.len(),48);assert!(red.chunks_exact(4).all(|p|p==[255,0,0,255]));
        let private=context.default_framebuffer();
        // Deterministically model native name reuse while an old wrapper is
        // retained; a real driver is not required to reuse a name on demand.
        let tombstone=context.objects.insert(crate::objects::Object::Framebuffer(private.unwrap())).unwrap();
        context.objects.mark_deleted(tombstone,Kind::Framebuffer).unwrap();
        assert_eq!(context.query(Query::GetParameter {name:glow::FRAMEBUFFER_BINDING}),Value::Null);
        for (format,width,height,error) in [(glow::RGB8,4,3,glow::INVALID_ENUM),(glow::RGBA8,0,3,glow::INVALID_VALUE),
            (glow::RGBA8,u32::MAX,3,glow::INVALID_VALUE)] {
            assert!(!context.drawing_buffer_storage(format,width,height));assert_eq!(context.get_error(),error);
            assert_eq!(context.default_framebuffer(),private);assert_eq!((context.width,context.height),(4,3));
            assert_eq!(context.drawing_buffer_rgba().unwrap(),red);
        }
        for command in [ResourceCommand::FramebufferRenderbuffer {target:glow::FRAMEBUFFER,attachment:glow::COLOR_ATTACHMENT0,renderbuffer_target:glow::RENDERBUFFER,id:0},
            ResourceCommand::FramebufferTexture2D {target:glow::FRAMEBUFFER,attachment:glow::COLOR_ATTACHMENT0,texture_target:glow::TEXTURE_2D,id:0,level:0}] {
            context.resource(command,None);assert_eq!(context.get_error(),glow::INVALID_OPERATION);
            assert_eq!(context.drawing_buffer_rgba().unwrap(),red);
        }
        let page=context.create_object(Kind::Framebuffer,0).unwrap();
        context.resource(ResourceCommand::BindFramebuffer {target:glow::FRAMEBUFFER,id:page},None);
        assert!(context.drawing_buffer_storage(glow::RGBA8,2,2));
        assert_eq!(context.query(Query::GetParameter {name:glow::FRAMEBUFFER_BINDING}),Value::Object {kind:Kind::Framebuffer,id:page});
        context.delete_object(page,Kind::Framebuffer);
        assert!(context.default_bound(glow::FRAMEBUFFER));
        assert_eq!(context.query(Query::CheckFramebufferStatus {target:glow::FRAMEBUFFER}),Value::UInt(glow::FRAMEBUFFER_COMPLETE));
        assert!(context.drawing_buffer_rgba().unwrap().iter().all(|&v|v==0));
        assert_eq!(context.get_error(),glow::NO_ERROR);
    }
}

#[test]
#[ignore = "mandatory real-driver half-float storage, readPixels, clear and default attachment queries"]
fn real_drawing_storage_float_output_preserves_page_reads_and_default_identity() {
    for version in [1,2] {
        let mut context=browser_context(version);
        let extension=if version==2 {"EXT_color_buffer_float"} else {"EXT_color_buffer_half_float"};
        assert_eq!(context.enable_extension(extension).as_deref(),Some(extension));
        assert!(context.drawing_buffer_storage(glow::RGBA16F,2,2));
        context.command(Command::ClearColor {red:0.25,green:0.125,blue:0.75,alpha:0.5});
        context.command(Command::Clear {mask:glow::COLOR_BUFFER_BIT});
        let bytes=context.drawing_buffer_rgba().unwrap();assert_eq!(&bytes[..4],&[128,64,255,128]);
        let mut values=[0.0_f32;4];let output=unsafe {std::slice::from_raw_parts_mut(values.as_mut_ptr().cast::<u8>(),16)};
        context.read_pixels(crate::transfers::ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::FLOAT},output);
        assert_eq!(values,[0.25,0.125,0.75,0.5]);
        let attachment=if version==2 {glow::BACK} else {glow::COLOR_ATTACHMENT0};
        let value=context.advanced(crate::advanced::Advanced::GetFramebufferAttachmentParameter {target:glow::FRAMEBUFFER,attachment,name:glow::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE});
        if version==2 {assert_eq!(value,Value::Int(glow::FRAMEBUFFER_DEFAULT as i32));}
        else {assert_eq!(value,Value::Null);assert_eq!(context.get_error(),glow::INVALID_OPERATION);}
        if version==2 {
            assert_eq!(context.advanced(crate::advanced::Advanced::GetFramebufferAttachmentParameter {target:glow::FRAMEBUFFER,attachment:glow::BACK,name:glow::FRAMEBUFFER_ATTACHMENT_RED_SIZE}),Value::Int(16));
            assert_eq!(context.advanced(crate::advanced::Advanced::GetFramebufferAttachmentParameter {target:glow::FRAMEBUFFER,attachment:glow::BACK,name:glow::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME}),Value::Null);
            assert_eq!(context.get_error(),glow::INVALID_ENUM);
            context.resource(ResourceCommand::DrawBuffers {buffers:vec![glow::NONE]},None);
            assert_eq!(context.query(Query::GetParameter {name:glow::DRAW_BUFFER0}),Value::Int(glow::NONE as i32));
        }
        context.did_present();assert!(context.drawing_buffer_rgba().unwrap().iter().all(|&v|v==0));
        assert!(context.resize(3,4));assert_eq!(context.drawing_buffer_format(),glow::RGBA16F);
        context.lose();assert!(context.drawing_storage.is_none());
        let mode=if context.diagnostics.backend==Backend::SwiftShader {Mode::Software} else {Mode::Hardware};
        assert!(context.restore(mode).unwrap());assert_eq!((context.width,context.height),(3,4));assert_eq!(context.drawing_buffer_format(),glow::RGBA8);
        assert_eq!(context.get_error(),glow::NO_ERROR);
    }
}

#[test]
#[ignore = "mandatory real-driver MSAA storage readback, copies and separate read/draw bindings"]
fn real_drawing_storage_resolves_reads_and_copies_without_changing_page_bindings() {
    for version in [1,2] {
        let requested=backend();let mode=if requested==Backend::SwiftShader {Mode::Software} else {Mode::Hardware};
        let mut context=CanvasContext::create(version,8,8,Attributes::default(),mode).unwrap();
        assert!(context.attributes.antialias,"selected validation backend must exercise multisampling");
        assert!(context.drawing_buffer_storage(glow::RGBA8,4,4));
        context.command(Command::ClearColor {red:1.0,green:0.0,blue:0.0,alpha:1.0});
        context.command(Command::Clear {mask:glow::COLOR_BUFFER_BIT});
        let page=context.create_object(Kind::Framebuffer,0).unwrap();
        if version==2 {
            context.resource(ResourceCommand::BindFramebuffer {target:glow::DRAW_FRAMEBUFFER,id:page},None);
            context.command(Command::ReadBuffer {source:glow::NONE});
            assert_eq!(context.query(Query::GetParameter {name:glow::READ_BUFFER}),Value::Int(glow::NONE as i32));
            assert_eq!(&context.drawing_buffer_rgba().unwrap()[..4],&[255,0,0,255]);
            assert_eq!(context.query(Query::GetParameter {name:glow::READ_BUFFER}),Value::Int(glow::NONE as i32));
            let mut rejected=[19;4];context.read_pixels(crate::transfers::ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::UNSIGNED_BYTE},&mut rejected);
            assert_eq!(context.get_error(),glow::INVALID_OPERATION);assert_eq!(rejected,[19;4]);
            assert_eq!(context.query(Query::GetParameter {name:glow::DRAW_FRAMEBUFFER_BINDING}),Value::Object {kind:Kind::Framebuffer,id:page});
            context.command(Command::ReadBuffer {source:glow::BACK});
        }
        let mut pixel=[0;4];context.read_pixels(crate::transfers::ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::UNSIGNED_BYTE},&mut pixel);
        assert_eq!(pixel,[255,0,0,255]);
        let texture=context.create_object(Kind::Texture,0).unwrap();
        context.resource(ResourceCommand::BindTexture {target:glow::TEXTURE_2D,id:texture},None);
        context.command(Command::CopyTexImage2D {target:glow::TEXTURE_2D,level:0,format:glow::RGBA,x:0,y:0,width:1,height:1,border:0});
        context.resource(ResourceCommand::BindFramebuffer {target:glow::FRAMEBUFFER,id:page},None);
        context.resource(ResourceCommand::FramebufferTexture2D {target:glow::FRAMEBUFFER,attachment:glow::COLOR_ATTACHMENT0,texture_target:glow::TEXTURE_2D,id:texture,level:0},None);
        pixel.fill(0);context.read_pixels(crate::transfers::ReadPixels {x:0,y:0,width:1,height:1,format:glow::RGBA,data_type:glow::UNSIGNED_BYTE},&mut pixel);
        assert_eq!(pixel,[255,0,0,255]);
        assert_eq!(context.query(Query::GetParameter {name:glow::FRAMEBUFFER_BINDING}),Value::Object {kind:Kind::Framebuffer,id:page});
        context.collect_object(page);assert!(context.default_bound(glow::FRAMEBUFFER));
        assert_eq!(context.get_error(),glow::NO_ERROR);
    }
}

#[test]
#[ignore = "mandatory real-driver bitmap transfer bounds, loss, preservation and empty surfaces"]
fn bitmap_transfer_rejects_bad_destinations_without_consuming_pixels() {
    for version in [1, 2] {
        let mut context = browser_context(version);
        context.attributes.preserve_drawing_buffer = true;
        context.command(Command::ClearColor { red: 1.0, green: 0.0, blue: 0.0, alpha: 1.0 });
        context.command(Command::Clear { mask: glow::COLOR_BUFFER_BIT });
        let mut short = [23; 4];
        assert!(!context.transfer_bitmap(&mut short));
        assert_eq!(short, [23; 4]);
        assert_eq!(&context.drawing_buffer_rgba().unwrap()[..4], &[255, 0, 0, 255]);
        context.error(glow::INVALID_ENUM);
        let mut bytes = vec![0; 8 * 8 * 4];
        assert!(context.transfer_bitmap(&mut bytes));
        assert_eq!(&bytes[..4], &[255, 0, 0, 255]);
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        assert_eq!(context.get_error(), glow::NO_ERROR);
        assert!(context.drawing_buffer_rgba().unwrap().iter().all(|&byte| byte == 0));
        assert!(context.resize(0, 2));
        assert!(context.transfer_bitmap(&mut []));
        context.lose();
        assert!(!context.transfer_bitmap(&mut []));
    }
}

#[test]
#[ignore = "mandatory real-driver reference receipts and error preservation"]
fn reference_receipts_preserve_old_errors_and_detect_duplicate_new_errors() {
    for version in [1, 2] {
        let mut context = browser_context(version);
        unsafe {
            context.driver.as_ref().unwrap().gl.bind_buffer(0, None);
        }
        let checkpoint = context.begin_reference_change();
        assert!(context.finish_reference_change(checkpoint));
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        context.error(glow::INVALID_OPERATION);
        let checkpoint = context.begin_reference_change();
        context.error(glow::INVALID_OPERATION);
        assert!(!context.finish_reference_change(checkpoint));
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
        assert_eq!(context.get_error(), glow::NO_ERROR);
        let checkpoint = context.begin_reference_change();
        context.resource(ResourceCommand::BindBuffer { target: 0, id: 0 }, None);
        assert!(!context.finish_reference_change(checkpoint));
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        let checkpoint = context.begin_reference_change();
        context.lose();
        assert!(!context.finish_reference_change(checkpoint));
        assert!(context.begin_reference_change().is_none());
        assert!(!context.finish_reference_change(None));
    }
}

#[test]
#[ignore = "mandatory real-driver active query deletion and failed begin/end handling"]
fn explicit_query_deletion_ends_each_supported_active_target() {
    let mut context = browser_context(2);
    for target in [
        glow::ANY_SAMPLES_PASSED,
        glow::ANY_SAMPLES_PASSED_CONSERVATIVE,
        glow::TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN,
    ] {
        let query = context.create_object(Kind::Query, 0).unwrap();
        let other = context.create_object(Kind::Query, 0).unwrap();
        context.resource(ResourceCommand::BeginQuery { target, id: query }, None);
        assert_eq!(context.get_error(), glow::NO_ERROR);
        assert_eq!(context.active_queries.get(&target), Some(&query));
        context.resource(ResourceCommand::BeginQuery { target, id: other }, None);
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
        assert_eq!(context.active_queries.get(&target), Some(&query));
        context.command(Command::EndQuery { target: 0 });
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        assert_eq!(context.active_queries.get(&target), Some(&query));
        context.delete_object(query, Kind::Query);
        assert_eq!(context.get_error(), glow::NO_ERROR);
        assert!(context.active_queries.is_empty());
        context.resource(ResourceCommand::BeginQuery { target, id: other }, None);
        context.command(Command::EndQuery { target });
        assert_eq!(context.get_error(), glow::NO_ERROR);
        assert!(context.active_queries.is_empty());
        context.collect_object(query);
        context.collect_object(other);
    }
}

#[test]
#[ignore = "mandatory real-driver rejected deletion and program binding during transform feedback"]
fn active_transform_feedback_rejects_deletion_and_keeps_current_program() {
    let mut context = browser_context(2);
    let program = context.create_object(Kind::Program, 0).unwrap();
    for (kind,source) in [
        (glow::VERTEX_SHADER,"#version 300 es\nout float value; void main(){value=1.;gl_Position=vec4(0.,0.,0.,1.);}"),
        (glow::FRAGMENT_SHADER,"#version 300 es\nprecision mediump float;out vec4 color;void main(){color=vec4(1.);}")
    ] {
        let shader=context.create_object(Kind::Shader,kind).unwrap();
        context.shader_source(shader,source);context.compile_shader(shader);context.attach_shader(program,shader,false);
    }
    context.resource(
        ResourceCommand::TransformFeedbackVaryings {
            program,
            varyings: vec!["value".into()],
            mode: glow::INTERLEAVED_ATTRIBS,
        },
        None,
    );
    context.link_program(program);
    context.use_program(program);
    let feedback = context.create_object(Kind::TransformFeedback, 0).unwrap();
    let buffer = context.create_object(Kind::Buffer, 0).unwrap();
    context.resource(
        ResourceCommand::BindTransformFeedback {
            target: glow::TRANSFORM_FEEDBACK,
            id: feedback,
        },
        None,
    );
    context.resource(
        ResourceCommand::BindBuffer {
            target: glow::TRANSFORM_FEEDBACK_BUFFER,
            id: buffer,
        },
        None,
    );
    context.resource(
        ResourceCommand::BufferData {
            target: glow::TRANSFORM_FEEDBACK_BUFFER,
            size: 16,
            usage: glow::DYNAMIC_COPY,
        },
        None,
    );
    context.resource(
        ResourceCommand::BindBufferBase {
            target: glow::TRANSFORM_FEEDBACK_BUFFER,
            index: 0,
            id: buffer,
        },
        None,
    );
    context.command(Command::BeginTransformFeedback { mode: glow::POINTS });
    assert_eq!(context.get_error(), glow::NO_ERROR);
    context.delete_object(feedback, Kind::TransformFeedback);
    assert_eq!(context.get_error(), glow::INVALID_OPERATION);
    assert!(!context.objects.deleted(feedback));
    context.use_program(0);
    assert_eq!(context.get_error(), glow::INVALID_OPERATION);
    assert_eq!(context.current_program, program);
    context.command(Command::EndTransformFeedback {});
    context.delete_object(feedback, Kind::TransformFeedback);
    assert!(context.objects.deleted(feedback));
    context.use_program(0);
    assert_eq!(context.current_program, 0);
    assert_eq!(context.get_error(), glow::NO_ERROR);
}

fn backend() -> Backend {
    match std::env::var("OBSCURA_WEBGL_TEST_BACKEND").as_deref() {
        Ok("metal") => Backend::Metal,
        Ok("vulkan") => Backend::Vulkan,
        Ok("swiftshader") => Backend::SwiftShader,
        _ => panic!("select OBSCURA_WEBGL_TEST_BACKEND=metal|vulkan|swiftshader explicitly"),
    }
}
fn context(version: u8) -> Context {
    Context::create(
        backend(),
        version,
        8,
        8,
        SurfaceOptions {
            alpha: true,
            depth: true,
            stencil: true,
            antialias: false,
        },
    )
    .unwrap()
}
unsafe fn pixel(context: &Context) -> [u8; 4] {
    context.make_current().unwrap();
    let mut result = [0; 4];
    context.gl.read_pixels(
        4,
        4,
        1,
        1,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut result)),
    );
    assert_eq!(context.gl.get_error(), glow::NO_ERROR);
    result
}

#[test]
#[ignore = "requires verified ANGLE bundle; mandatory explicit backend validation"]
fn real_webgl1_and_webgl2_buffers_start_zero_and_render_clear() {
    for version in [1, 2] {
        let context = context(version);
        assert!(context.proc_address(c"glGetBufferSubData").is_null(),
            "desktop GL symbols must not escape ANGLE's procedure table");
        assert!(!context.proc_address(c"glClear").is_null());
        unsafe {
            assert_eq!(pixel(&context), [0, 0, 0, 0]);
            context.gl.clear_color(0.0, 1.0, 0.0, 1.0);
            context.gl.clear(glow::COLOR_BUFFER_BIT);
            assert_eq!(pixel(&context), [0, 255, 0, 255]);
            let renderer = context.gl.get_parameter_string(glow::RENDERER);
            assert!(!renderer.is_empty());
            eprintln!("WebGL {version}, {:?}, {renderer}", backend());
        }
        // A prior read must not be required to allocate a pbuffer before its
        // first clear. Exercise cold creation and replacement independently.
        let mut cold = self::context(version);
        unsafe {
            cold.gl.clear_color(0.0,1.0,0.0,1.0);
            cold.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
            assert_eq!(pixel(&cold),[0,255,0,255]);
            cold.resize(8,8).unwrap();
            cold.gl.clear_color(1.0,0.0,1.0,1.0);
            cold.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
            assert_eq!(pixel(&cold),[255,0,255,255]);
        }
    }
}

#[test]
#[ignore = "requires verified ANGLE bundle; mandatory real shader/draw/readback validation"]
fn real_shaders_buffers_and_drawing_produce_pixels_in_both_versions() {
    for version in [1, 2] {
        let context = context(version);
        let gl = &context.gl;
        unsafe {
            let vertex = gl.create_shader(glow::VERTEX_SHADER).unwrap();
            let fragment = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
            let vs = if version == 2 {
                "#version 300 es\nin vec2 position; void main(){gl_Position=vec4(position,0.,1.);}"
            } else {
                "attribute vec2 position; void main(){gl_Position=vec4(position,0.,1.);}"
            };
            let fs = if version == 2 {
                "#version 300 es\nprecision mediump float;out vec4 color;void main(){color=vec4(1.,0.,0.,1.);}"
            } else {
                "precision mediump float;void main(){gl_FragColor=vec4(1.,0.,0.,1.);}"
            };
            for (shader, source) in [(vertex, vs), (fragment, fs)] {
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                assert!(
                    gl.get_shader_compile_status(shader),
                    "{}",
                    gl.get_shader_info_log(shader)
                );
            }
            let program = gl.create_program().unwrap();
            gl.attach_shader(program, vertex);
            gl.attach_shader(program, fragment);
            gl.bind_attrib_location(program, 0, "position");
            gl.link_program(program);
            assert!(
                gl.get_program_link_status(program),
                "{}",
                gl.get_program_info_log(program)
            );
            gl.use_program(Some(program));
            let buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            let vertices: [f32; 6] = [-1., -1., 3., -1., -1., 3.];
            let bytes = vertices
                .iter()
                .flat_map(|v| v.to_ne_bytes())
                .collect::<Vec<_>>();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 0, 0);
            gl.viewport(0, 0, 8, 8);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            assert_eq!(pixel(&context), [255, 0, 0, 255]);
            gl.delete_buffer(buffer);
            gl.delete_program(program);
            gl.delete_shader(vertex);
            gl.delete_shader(fragment);
        }
    }
}

#[test]
#[ignore = "requires verified ANGLE bundle; mandatory context ownership validation"]
fn real_contexts_do_not_share_drawing_buffers_or_gl_state() {
    let first = context(1);
    unsafe {
        first.gl.clear_color(1., 0., 0., 1.);
        first.gl.clear(glow::COLOR_BUFFER_BIT);
    }
    let second = context(1);
    unsafe {
        second.gl.clear_color(0., 0., 1., 1.);
        second.gl.clear(glow::COLOR_BUFFER_BIT);
    }
    unsafe {
        assert_eq!(pixel(&first), [255, 0, 0, 255]);
        assert_eq!(pixel(&second), [0, 0, 255, 255]);
    }
    drop(first);
    unsafe {
        assert_eq!(pixel(&second), [0, 0, 255, 255]);
    }
}

#[test]
#[ignore = "requires verified ANGLE bundle; mandatory resize and allocation validation"]
fn real_resize_clears_pixels_and_allocation_failure_retains_old_surface() {
    let mut context = context(2);
    unsafe {
        context.gl.clear_color(0., 1., 0., 1.);
        context.gl.clear(glow::COLOR_BUFFER_BIT);
    }
    assert!(context.resize(u32::MAX, u32::MAX).is_err());
    unsafe {
        assert_eq!(pixel(&context), [0, 255, 0, 255]);
    }
    context.resize(8, 8).unwrap();
    unsafe {
        assert_eq!(pixel(&context), [0, 0, 0, 0]);
    }
    context.resize(0, 0).unwrap();
    context.resize(8, 8).unwrap();
    unsafe {
        assert_eq!(pixel(&context), [0, 0, 0, 0]);
    }
}

unsafe fn whole_surface_is_zero(context: &Context, width: i32, height: i32) -> bool {
    context.make_current().unwrap();
    let mut pixels = vec![0xA5; (width * height * 4) as usize];
    context.gl.read_pixels(
        0,
        0,
        width,
        height,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut pixels)),
    );
    assert_eq!(context.gl.get_error(), glow::NO_ERROR);
    pixels.iter().all(|&v| v == 0)
}

#[test]
#[ignore = "requires verified ANGLE bundle; new and resized pbuffers must not reuse freed surface memory"]
fn real_new_and_resized_drawing_buffers_never_expose_freed_surface_memory() {
    let options = SurfaceOptions {
        alpha: true,
        depth: true,
        stencil: true,
        antialias: false,
    };
    // Intel Metal drivers hand a new pbuffer the memory of a surface freed
    // just before, same size or not. Nothing may be read before a clear.
    for (first, second, width, height) in [
        (1, 1, 8, 8),
        (1, 2, 8, 8),
        (2, 1, 8, 8),
        (2, 2, 8, 8),
        (2, 2, 16, 16),
        (2, 2, 4, 4),
    ] {
        {
            let drawn = context(first);
            unsafe {
                drawn.gl.clear_color(0.0, 1.0, 0.0, 1.0);
                drawn.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
                assert_eq!(pixel(&drawn), [0, 255, 0, 255]);
            }
        }
        let fresh = Context::create(backend(), second, width, height, options).unwrap();
        unsafe {
            assert!(
                whole_surface_is_zero(&fresh, width as i32, height as i32),
                "WebGL {first} -> {second} {width}x{height} exposed a freed drawing buffer"
            );
        }
    }
    // Replacement allocates before freeing, so resizing back reaches the
    // allocation freed by the first resize. Page state must be unchanged.
    for version in [1, 2] {
        let mut context = context(version);
        unsafe {
            context.gl.clear_color(0.0, 1.0, 0.0, 1.0);
            context.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        context.resize(16, 16).unwrap();
        unsafe {
            assert!(whole_surface_is_zero(&context, 16, 16));
            let gl = &context.gl;
            let framebuffer = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(1, 1, 1, 1);
            gl.color_mask(false, true, false, true);
            gl.depth_mask(false);
            gl.stencil_mask_separate(glow::FRONT, 3);
            gl.stencil_mask_separate(glow::BACK, 7);
            gl.clear_color(0.25, 0.5, 0.75, 0.5);
            gl.clear_depth_f32(0.25);
            gl.clear_stencil(5);
            if version == 2 {
                gl.enable(glow::RASTERIZER_DISCARD);
            }
            assert_eq!(gl.get_error(), glow::NO_ERROR);
        }
        context.resize(8, 8).unwrap();
        unsafe {
            let gl = &context.gl;
            let framebuffer = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            assert!(framebuffer.is_some());
            assert!(gl.is_enabled(glow::SCISSOR_TEST));
            let mut scissor = [0; 4];
            gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor);
            assert_eq!(scissor, [1, 1, 1, 1]);
            assert_eq!(gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK), [false, true, false, true]);
            assert!(!gl.get_parameter_bool(glow::DEPTH_WRITEMASK));
            assert_eq!(gl.get_parameter_i32(glow::STENCIL_WRITEMASK), 3);
            assert_eq!(gl.get_parameter_i32(glow::STENCIL_BACK_WRITEMASK), 7);
            let mut color = [0.0; 4];
            gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut color);
            assert_eq!(color, [0.25, 0.5, 0.75, 0.5]);
            assert_eq!(gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE), 0.25);
            assert_eq!(gl.get_parameter_i32(glow::STENCIL_CLEAR_VALUE), 5);
            // RASTERIZER_DISCARD is not an ES2 enum; querying it raises
            // INVALID_ENUM on WebGL 1 contexts.
            if version == 2 {
                assert!(gl.is_enabled(glow::RASTERIZER_DISCARD));
            }
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_framebuffer(framebuffer.unwrap());
            if version == 2 {
                gl.disable(glow::RASTERIZER_DISCARD);
            }
            gl.disable(glow::SCISSOR_TEST);
            assert!(
                whole_surface_is_zero(&context, 8, 8),
                "WebGL {version} resize back exposed a freed drawing buffer"
            );
        }
    }
}

#[test]
#[ignore = "mandatory real-driver WebGL 2 internal clears while the page keeps RASTERIZER_DISCARD enabled"]
fn real_rasterizer_discard_does_not_skip_internal_drawing_buffer_clears() {
    fn fill_green_then_discard(context: &mut CanvasContext) {
        // Clear is itself dropped while discard is enabled.
        context.command(Command::Disable { cap: glow::RASTERIZER_DISCARD });
        context.command(Command::ClearColor { red: 0.0, green: 1.0, blue: 0.0, alpha: 1.0 });
        context.command(Command::Clear { mask: glow::COLOR_BUFFER_BIT });
        assert!(context.drawing_buffer_source_rgba().unwrap().chunks_exact(4).all(|p| p == [0, 255, 0, 255]));
        context.command(Command::Enable { cap: glow::RASTERIZER_DISCARD });
    }
    fn assert_cleared_and_discard_kept(context: &mut CanvasContext, step: &str) {
        let pixels = context.drawing_buffer_source_rgba().unwrap();
        assert!(pixels.iter().all(|&v| v == 0), "{step} left drawing-buffer content");
        assert_eq!(context.query(Query::IsEnabled { cap: glow::RASTERIZER_DISCARD }), Value::Bool(true), "{step}");
        let mut clear = [0.0; 4];
        unsafe { context.driver.as_ref().unwrap().gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear); }
        assert_eq!(clear, [0.0, 1.0, 0.0, 1.0], "{step}");
        assert_eq!(context.get_error(), glow::NO_ERROR, "{step}");
    }
    for owned in [false, true] {
        let mut context = browser_context(2);
        assert!(!context.attributes.preserve_drawing_buffer);
        if owned { assert!(context.drawing_buffer_storage(glow::RGBA8, 8, 8)); }
        // Resize to a larger buffer and back reaches a just-freed allocation.
        fill_green_then_discard(&mut context);
        assert!(context.resize(16, 16));
        assert!(context.resize(8, 8));
        assert_cleared_and_discard_kept(&mut context, "resize");
        fill_green_then_discard(&mut context);
        context.did_present();
        assert_cleared_and_discard_kept(&mut context, "present");
        fill_green_then_discard(&mut context);
        let mut bitmap = vec![0_u8; 8 * 8 * 4];
        assert!(context.transfer_bitmap(&mut bitmap));
        assert!(bitmap.chunks_exact(4).all(|p| p == [0, 255, 0, 255]));
        assert_cleared_and_discard_kept(&mut context, "transfer");
        if owned {
            fill_green_then_discard(&mut context);
            assert!(context.drawing_buffer_storage(glow::RGBA8, 8, 8));
            assert_cleared_and_discard_kept(&mut context, "storage replacement");
        }
    }
    // The multisample resolve is a blit, which discard does not suppress.
    use crate::drawing_buffer::{Backing,StorageSpec};
    let context = browser_context(2);
    let driver = context.driver.as_ref().unwrap();
    let gl = &driver.gl;
    let attributes = Attributes { antialias: true, ..context.attributes.clone() };
    let spec = StorageSpec::validate(glow::RGBA8, 8, 4, 2, &attributes, &context.extensions, 4096, 4).unwrap();
    let backing = Backing::create(driver, 2, spec).unwrap();
    assert_ne!(backing.draw, backing.read);
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(backing.draw));
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.enable(glow::RASTERIZER_DISCARD);
        let read = backing.resolve(driver, 2).unwrap();
        assert!(gl.is_enabled(glow::RASTERIZER_DISCARD));
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(read));
        let mut pixels = vec![0; 8 * 4 * 4];
        gl.read_pixels(0, 0, 8, 4, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(Some(&mut pixels)));
        assert!(pixels.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.disable(glow::RASTERIZER_DISCARD);
        backing.destroy(driver);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}

#[test]
#[ignore = "requires verified ANGLE bundle; mandatory repeated-profile cleanup validation"]
fn real_repeated_context_cleanup_releases_live_gl_objects() {
    for _ in 0..64 {
        let context = context(2);
        unsafe {
            let gl = &context.gl;
            let buffer = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.buffer_data_size(glow::ARRAY_BUFFER, 1024 * 1024, glow::DYNAMIC_DRAW);
            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                256,
                256,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            // Intentionally leave objects live: destroying the owning context
            // must release them. RSS/CPU/fd measurements are a separate gate.
        }
    }
}

#[test]
#[ignore = "mandatory pinned-backend half-float rendering, component query and float readback"]
fn real_half_float_renderbuffer_reads_float_without_float_texture_extension() {
    use crate::{advanced::Advanced, queries::{Query, Value}, transfers::ReadPixels};
    for version in [1, 2] {
        let mut context = browser_context(version);
        assert!(context.supported_extensions().unwrap().iter().any(|name| name == "EXT_color_buffer_half_float"),
            "selected validation backend must expose half-float rendering to cover this case");
        let framebuffer = context.create_object(Kind::Framebuffer, 0).unwrap();
        let renderbuffer = context.create_object(Kind::Renderbuffer, 0).unwrap();
        context.resource(ResourceCommand::BindFramebuffer { target: glow::FRAMEBUFFER, id: framebuffer }, None);
        context.resource(ResourceCommand::BindRenderbuffer { target: glow::RENDERBUFFER, id: renderbuffer }, None);
        if version == 1 {
            assert_eq!(context.advanced(Advanced::GetFramebufferAttachmentParameter {
                target: glow::FRAMEBUFFER, attachment: glow::COLOR_ATTACHMENT0,
                name: glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE,
            }), Value::Null);
            assert_eq!(context.get_error(), glow::INVALID_ENUM);
        }
        assert_eq!(context.enable_extension("ext_COLOR_buffer_HALF_float").as_deref(), Some("EXT_color_buffer_half_float"));
        assert!(!context.extensions.contains("OES_texture_float"));
        context.command(Command::RenderbufferStorage { target: glow::RENDERBUFFER, format: glow::RGBA16F, width: 1, height: 1 });
        context.resource(ResourceCommand::FramebufferRenderbuffer {
            target: glow::FRAMEBUFFER, attachment: glow::COLOR_ATTACHMENT0,
            renderbuffer_target: glow::RENDERBUFFER, id: renderbuffer,
        }, None);
        assert_eq!(context.query(Query::CheckFramebufferStatus { target: glow::FRAMEBUFFER }), Value::UInt(glow::FRAMEBUFFER_COMPLETE));
        assert_eq!(context.advanced(Advanced::GetFramebufferAttachmentParameter {
            target: glow::FRAMEBUFFER, attachment: glow::COLOR_ATTACHMENT0,
            name: glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE,
        }), Value::Int(glow::FLOAT as i32));
        context.command(Command::ClearColor { red: 0.25, green: 0.5, blue: 0.75, alpha: 1.0 });
        context.command(Command::Clear { mask: glow::COLOR_BUFFER_BIT });
        assert_eq!(context.get_error(), glow::NO_ERROR);
        let request = || ReadPixels { x: 0, y: 0, width: 1, height: 1, format: glow::RGBA, data_type: glow::FLOAT };
        let mut short = [19; 15];context.read_pixels(request(), &mut short);
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);assert_eq!(short, [19; 15]);
        let mut bytes = [0; 16];context.read_pixels(request(), &mut bytes);
        assert_eq!(context.get_error(), glow::NO_ERROR);
        let values: Vec<_> = bytes.chunks_exact(4).map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap())).collect();
        assert_eq!(values, vec![0.25, 0.5, 0.75, 1.0]);
        context.advanced(Advanced::GetFramebufferAttachmentParameter {
            target: glow::FRAMEBUFFER, attachment: glow::DEPTH_STENCIL_ATTACHMENT,
            name: glow::FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE,
        });
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
    }
}

#[test]
#[ignore = "mandatory pinned-backend extension dependency, restoration and version gating"]
fn real_half_float_texture_activation_enables_rendering_and_restore_resets_extensions() {
    let mut context = browser_context(1);
    assert!(context.supported_extensions().unwrap().iter().any(|name| name == "EXT_color_buffer_half_float"));
    assert_eq!(context.enable_extension("oes_TEXTURE_half_FLOAT").as_deref(), Some("OES_texture_half_float"));
    assert!(context.extensions.contains("EXT_color_buffer_half_float"));
    assert_eq!(context.enable_extension("OES_texture_half_float").as_deref(), Some("OES_texture_half_float"));
    assert!(context.enable_extension("driver_only_extension").is_none());
    assert!(context.enable_extension("EXT_color_buffer_float").is_none());
    assert_eq!(context.get_error(), glow::NO_ERROR);
    context.lose();assert!(context.supported_extensions().is_none());
    assert!(context.enable_extension("OES_texture_half_float").is_none());
    let mode = if backend() == Backend::SwiftShader { Mode::Software } else { Mode::Hardware };
    assert_eq!(context.restore(mode).unwrap(), true);
    assert!(context.extensions.is_empty());
    assert_eq!(context.enable_extension("OES_texture_half_float").as_deref(), Some("OES_texture_half_float"));
    assert!(context.extensions.contains("EXT_color_buffer_half_float"));
    let mut two = browser_context(2);
    for name in ["OES_texture_half_float", "OES_vertex_array_object", "ANGLE_instanced_arrays"] {
        assert!(two.enable_extension(name).is_none());
    }
}

#[test]
#[ignore = "mandatory real-driver typed query errors and context-loss return values"]
fn real_query_errors_and_context_loss_preserve_return_types() {
    use crate::{advanced::Advanced, queries::{Query, Value}};
    for version in [1, 2] {
        let mut context = browser_context(version);
        context.error(glow::INVALID_OPERATION);
        assert_eq!(context.query(Query::IsEnabled { cap: 0 }), Value::Bool(false));
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        assert_eq!(context.query(Query::CheckFramebufferStatus { target: 0 }), Value::UInt(0));
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        assert_eq!(context.query(Query::GetAttribLocation { program: 0, name: "x\0y".into() }), Value::Int(-1));
        assert_eq!(context.get_error(), glow::INVALID_VALUE);
        assert_eq!(context.advanced(Advanced::GetVertexAttribOffset { index: 0, name: 0 }), Value::Number(0.0));
        assert_eq!(context.get_error(), glow::INVALID_ENUM);
        assert_eq!(context.advanced(Advanced::GetUniformBlockIndex { program: 0, name: "absent".into() }), Value::UInt(0));
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
        assert_eq!(context.advanced(Advanced::ClientWaitSync { id: 0, flags: 0, timeout: 0 }), Value::UInt(glow::WAIT_FAILED));
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
        context.lose();
        for _ in 0..2 {
            assert_eq!(context.query(Query::IsEnabled { cap: 0 }), Value::Bool(false));
            assert_eq!(context.query(Query::IsObject { kind: Kind::Buffer, id: 0 }), Value::Bool(false));
            assert_eq!(context.query(Query::CheckFramebufferStatus { target: 0 }), Value::UInt(glow::FRAMEBUFFER_UNSUPPORTED));
            assert_eq!(context.query(Query::GetAttribLocation { program: 0, name: "x\0y".into() }), Value::Int(-1));
            assert_eq!(context.query(Query::GetParameter { name: 0 }), Value::Null);
            assert_eq!(context.advanced(Advanced::GetVertexAttribOffset { index: 0, name: 0 }), Value::Number(0.0));
            assert_eq!(context.advanced(Advanced::GetUniformBlockIndex { program: 0, name: String::new() }), Value::UInt(0));
            assert_eq!(context.advanced(Advanced::GetFragDataLocation { program: 0, name: String::new() }), Value::Int(-1));
            assert_eq!(context.advanced(Advanced::ClientWaitSync { id: 0, flags: 0, timeout: 0 }), Value::UInt(glow::WAIT_FAILED));
        }
        assert_eq!(context.get_error(), 0x9242);
        assert_eq!(context.get_error(), glow::NO_ERROR);
    }
}

#[test]
#[ignore = "mandatory real-driver WebGL 2 fragment output queries and validation"]
fn real_fragment_output_queries_read_linked_programs_and_reject_invalid_requests() {
    use crate::{advanced::Advanced, queries::{Query, Value}};
    let mut old = browser_context(1);
    assert_eq!(old.advanced(Advanced::GetFragDataLocation { program: 0, name: "color".into() }), Value::Int(-1));
    assert_eq!(old.get_error(), glow::INVALID_OPERATION);
    let mut context = browser_context(2);
    let program = context.create_object(Kind::Program, 0).unwrap();
    assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: "color".into() }), Value::Int(-1));
    assert_eq!(context.get_error(), glow::INVALID_OPERATION);
    for (kind, source) in [
        (glow::VERTEX_SHADER, "#version 300 es\nvoid main(){gl_Position=vec4(0.,0.,0.,1.);}"),
        (glow::FRAGMENT_SHADER, "#version 300 es\nprecision mediump float;layout(location=2) out vec4 color;void main(){color=vec4(1.);}")
    ] {
        let shader = context.create_object(Kind::Shader, kind).unwrap();
        context.shader_source(shader, source);context.compile_shader(shader);
        assert_eq!(context.query(Query::GetShaderParameter { shader, name: glow::COMPILE_STATUS }), Value::Bool(true));
        context.attach_shader(program, shader, false);
    }
    context.link_program(program);
    assert_eq!(context.query(Query::GetProgramParameter { program, name: glow::LINK_STATUS }), Value::Bool(true));
    for (name, expected) in [("color", 2), ("absent", -1)] {
        assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: name.into() }), Value::Int(expected));
        assert_eq!(context.get_error(), glow::NO_ERROR);
    }
    assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: "x\0y".into() }), Value::Int(-1));
    assert_eq!(context.get_error(), glow::NO_ERROR);
    assert_eq!(context.advanced(Advanced::GetFragDataLocation { program: u32::MAX, name: "color".into() }), Value::Int(-1));
    assert_eq!(context.get_error(), glow::INVALID_OPERATION);
    // A valid missing block is distinct from a rejected request or lost context.
    assert_eq!(context.advanced(Advanced::GetUniformBlockIndex { program, name: "absent".into() }), Value::UInt(glow::INVALID_INDEX));
    assert_eq!(context.get_error(), glow::NO_ERROR);
    context.use_program(program);context.delete_object(program, Kind::Program);
    assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: "color".into() }), Value::Int(2));
    assert_eq!(context.get_error(), glow::NO_ERROR);
    context.use_program(0);
    assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: "color".into() }), Value::Int(-1));
    assert_eq!(context.get_error(), glow::INVALID_VALUE);
    context.lose();
    assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: "color".into() }), Value::Int(-1));
    assert_eq!(context.get_error(), 0x9242);assert_eq!(context.get_error(), glow::NO_ERROR);
}

#[test]
#[ignore = "mandatory real ANGLE owned drawing storage formats, clearing, bindings and deletion"]
fn real_owned_drawing_storage_preserves_bindings_and_renders_actual_formats() {
    use crate::drawing_buffer::{Backing,StorageSpec};
    for version in [1,2] {
        let mut context=browser_context(version);
        let extension=if version==1 { "EXT_color_buffer_half_float" } else { "EXT_color_buffer_float" };
        assert!(context.enable_extension(extension).is_some(),"required floating color storage unavailable");
        let driver=context.driver.as_ref().unwrap();let gl=&driver.gl;
        let formats=if version==1 { vec![glow::RGBA8,glow::RGBA16F] } else { vec![glow::RGBA8,glow::RGBA16F,glow::SRGB8_ALPHA8] };
        unsafe {
            let old_framebuffer=gl.create_framebuffer().unwrap();let old_renderbuffer=gl.create_renderbuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER,Some(old_framebuffer));gl.bind_renderbuffer(glow::RENDERBUFFER,Some(old_renderbuffer));
            for format in formats {
                gl.enable(glow::SCISSOR_TEST);gl.scissor(1,1,1,1);gl.color_mask(false,true,false,true);
                gl.depth_mask(false);gl.stencil_mask_separate(glow::FRONT,3);gl.stencil_mask_separate(glow::BACK,7);
                gl.clear_color(0.25,0.5,0.75,0.5);gl.clear_depth_f32(0.25);gl.clear_stencil(5);
                let limit=gl.get_parameter_i32(glow::MAX_RENDERBUFFER_SIZE) as u32;
                let spec=StorageSpec::validate(format,8,4,version,&context.attributes,&context.extensions,limit,0).unwrap();
                let backing=Backing::create(driver,version,spec).unwrap();
                assert_eq!(gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING),Some(old_framebuffer));
                assert_eq!(gl.get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING),Some(old_renderbuffer));
                assert!(gl.is_enabled(glow::SCISSOR_TEST));assert_eq!(gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK),[false,true,false,true]);
                assert!(!gl.get_parameter_bool(glow::DEPTH_WRITEMASK));assert_eq!(gl.get_parameter_i32(glow::STENCIL_WRITEMASK),3);
                assert_eq!(gl.get_parameter_i32(glow::STENCIL_BACK_WRITEMASK),7);
                assert_eq!(gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE),0.25);assert_eq!(gl.get_parameter_i32(glow::STENCIL_CLEAR_VALUE),5);
                let mut color=[0.;4];gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE,&mut color);assert_eq!(color,[0.25,0.5,0.75,0.5]);
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(backing.draw));
                let mut floating=vec![1_f32;8*4*4];let mut bytes=vec![255_u8;8*4*4];
                if format==glow::RGBA16F {
                    let destination=std::slice::from_raw_parts_mut(floating.as_mut_ptr().cast::<u8>(),floating.len()*4);
                    gl.read_pixels(0,0,8,4,glow::RGBA,glow::FLOAT,glow::PixelPackData::Slice(Some(destination)));
                    assert!(floating.iter().all(|&v|v==0.));
                } else {
                    gl.read_pixels(0,0,8,4,glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelPackData::Slice(Some(&mut bytes)));
                    assert!(bytes.iter().all(|&v|v==0));
                }
                gl.disable(glow::SCISSOR_TEST);gl.color_mask(true,true,true,true);gl.clear(glow::COLOR_BUFFER_BIT);
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(old_framebuffer));let read=backing.resolve(driver,version).unwrap();
                assert_eq!(gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING),Some(old_framebuffer));
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(read));
                if format==glow::RGBA16F {
                    let destination=std::slice::from_raw_parts_mut(floating.as_mut_ptr().cast::<u8>(),floating.len()*4);
                    gl.read_pixels(0,0,8,4,glow::RGBA,glow::FLOAT,glow::PixelPackData::Slice(Some(destination)));
                    assert_eq!(&floating[..4],&[0.25,0.5,0.75,0.5]);
                } else {
                    gl.read_pixels(0,0,8,4,glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelPackData::Slice(Some(&mut bytes)));
                    let expected=if format==glow::RGBA8 { [64_i16,128,191,128] } else { [137_i16,188,225,128] };
                    assert!(bytes[..4].iter().zip(expected).all(|(&v,e)|(i16::from(v)-e).abs()<=1),"unexpected stored color: {:?}",&bytes[..4]);
                }
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(old_framebuffer));let draw=backing.draw;let read=backing.read;
                backing.destroy(driver);assert!(!gl.is_framebuffer(draw));assert!(!gl.is_framebuffer(read));
                assert_eq!(gl.get_error(),glow::NO_ERROR);
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER,None);gl.bind_renderbuffer(glow::RENDERBUFFER,None);
            gl.delete_framebuffer(old_framebuffer);gl.delete_renderbuffer(old_renderbuffer);
        }
    }
}

#[test]
#[ignore = "mandatory real ANGLE WebGL 1/2 multisample resolve and repeated storage retirement"]
fn real_owned_drawing_storage_resolves_multisampling_without_changing_page_bindings() {
    use crate::drawing_buffer::{Backing,StorageSpec};
    for version in [1,2] {
        let context=browser_context(version);let driver=context.driver.as_ref().unwrap();let gl=&driver.gl;
        let attributes=Attributes { antialias:true,..context.attributes.clone() };
        for _ in 0..8 {
            let spec=StorageSpec::validate(glow::RGBA8,8,4,version,&attributes,&context.extensions,4096,4).unwrap();
            let backing=Backing::create(driver,version,spec).unwrap();assert_ne!(backing.draw,backing.read);
            unsafe {
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(backing.draw));gl.clear_color(1.,0.,0.,1.);gl.clear(glow::COLOR_BUFFER_BIT);
                gl.bind_framebuffer(glow::FRAMEBUFFER,None);gl.enable(glow::SCISSOR_TEST);gl.scissor(0,0,1,1);
                let read=backing.resolve(driver,version).unwrap();assert!(gl.is_enabled(glow::SCISSOR_TEST));
                assert_eq!(gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING),None);
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(read));let mut pixels=vec![0;8*4*4];
                gl.read_pixels(0,0,8,4,glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelPackData::Slice(Some(&mut pixels)));
                assert!(pixels.chunks_exact(4).all(|p|p==[255,0,0,255]));
                gl.bind_framebuffer(glow::FRAMEBUFFER,None);gl.disable(glow::SCISSOR_TEST);
                let draw=backing.draw;backing.destroy(driver);assert!(!gl.is_framebuffer(draw));assert!(!gl.is_framebuffer(read));
                assert_eq!(gl.get_error(),glow::NO_ERROR);
            }
        }
    }
}

#[test]
#[ignore = "mandatory real ANGLE WebGL 1/2 location-name boundaries and character validation"]
fn real_location_names_use_version_limits_and_reject_invalid_characters() {
    for (version, limit) in [(1, 256), (2, 1024)] {
        let mut context = browser_context(version);
        let program = context.create_object(Kind::Program, 0).unwrap();
        let attribute = format!("a{}", "x".repeat(limit - 1));
        let uniform = format!("u{}", "x".repeat(limit - 1));
        let (header, qualifier, fragment) = if version == 1 {
            ("", "attribute", "precision mediump float;void main(){gl_FragColor=vec4(1.);}")
        } else {
            ("#version 300 es\n", "in", "#version 300 es\nprecision mediump float;out vec4 color;void main(){color=vec4(1.);}")
        };
        let vertex = format!("{header}{qualifier} vec4 {attribute};uniform float {uniform};void main(){{gl_Position={attribute}*{uniform};}}");
        for (kind, source) in [(glow::VERTEX_SHADER, vertex.as_str()), (glow::FRAGMENT_SHADER, fragment)] {
            let shader = context.create_object(Kind::Shader, kind).unwrap();
            context.shader_source(shader, source);context.compile_shader(shader);
            assert_eq!(context.query(Query::GetShaderParameter {shader,name:glow::COMPILE_STATUS}),Value::Bool(true));
            context.attach_shader(program,shader,false);
        }
        context.resource(ResourceCommand::BindAttribLocation {program,index:1,name:attribute.clone()},None);
        assert_eq!(context.get_error(),glow::NO_ERROR);
        context.link_program(program);
        assert_eq!(context.query(Query::GetProgramParameter {program,name:glow::LINK_STATUS}),Value::Bool(true));
        assert_eq!(context.query(Query::GetAttribLocation {program,name:attribute}),Value::Int(1));
        assert!(matches!(context.query(Query::GetUniformLocation {program,name:uniform}),Value::UInt(id) if id != 0));
        assert_eq!(context.get_error(),glow::NO_ERROR);
        let mut invalid = vec!["x".repeat(limit + 1), "x\0y".into(), "λ".into()];
        invalid.extend(['\u{1}', '\u{7f}', '"', '$', '`', '@', '\\', '\''].map(|c| c.to_string()));
        for name in invalid {
            context.resource(ResourceCommand::BindAttribLocation {program,index:0,name:name.clone()},None);
            assert_eq!(context.get_error(),glow::INVALID_VALUE);
            assert_eq!(context.query(Query::GetAttribLocation {program,name:name.clone()}),Value::Int(-1));
            assert_eq!(context.get_error(),glow::INVALID_VALUE);
            assert_eq!(context.query(Query::GetUniformLocation {program,name}),Value::Null);
            assert_eq!(context.get_error(),glow::INVALID_VALUE);
        }
        for name in ["gl_reserved", "webgl_reserved", "_webgl_reserved"] {
            context.resource(ResourceCommand::BindAttribLocation {program,index:0,name:name.into()},None);
            assert_eq!(context.get_error(),glow::INVALID_OPERATION);
        }
    }
}

#[test]
#[ignore = "mandatory real ANGLE WebGL 2 non-location names and null-terminated UTF-8"]
fn real_webgl_two_non_location_names_keep_their_distinct_validation_rules() {
    use crate::advanced::Advanced;
    let mut context=browser_context(2);
    let program=context.create_object(Kind::Program,0).unwrap();
    let varying=format!("v{}","x".repeat(256));
    let block=format!("b{}","x".repeat(256));
    let output=format!("o{}","x".repeat(256));
    let uniform=format!("u{}","x".repeat(256));
    let vertex=format!("#version 300 es\nuniform float {uniform};uniform {block}{{float b;}};out float {varying};void main(){{{varying}={uniform}+b;gl_Position=vec4({varying},0.,0.,1.);}}");
    let fragment=format!("#version 300 es\nprecision mediump float;layout(location=1) out vec4 {output};void main(){{{output}=vec4(1.);}}");
    for (kind,source) in [(glow::VERTEX_SHADER,vertex),(glow::FRAGMENT_SHADER,fragment)] {
        let shader=context.create_object(Kind::Shader,kind).unwrap();context.shader_source(shader,&source);context.compile_shader(shader);
        assert_eq!(context.query(Query::GetShaderParameter {shader,name:glow::COMPILE_STATUS}),Value::Bool(true));
        context.attach_shader(program,shader,false);
    }
    context.resource(ResourceCommand::TransformFeedbackVaryings {program,varyings:vec![format!("{varying}\0suffix")],mode:glow::INTERLEAVED_ATTRIBS},None);
    assert_eq!(context.get_error(),glow::NO_ERROR);context.link_program(program);
    assert_eq!(context.query(Query::GetProgramParameter {program,name:glow::LINK_STATUS}),Value::Bool(true));
    assert_eq!(context.advanced(Advanced::GetTransformFeedbackVarying {program,index:0}),Value::Active {size:1,data_type:glow::FLOAT,name:varying});
    assert_eq!(context.advanced(Advanced::GetFragDataLocation {program,name:format!("{output}\0suffix")}),Value::Int(1));
    assert!(matches!(context.advanced(Advanced::GetUniformIndices {program,names:vec![format!("{uniform}\0suffix")]}),Value::UInts(v) if v.len()==1 && v[0]!=glow::INVALID_INDEX));
    assert_eq!(context.advanced(Advanced::GetUniformBlockIndex {program,name:block.clone()}),Value::UInt(0));
    assert_eq!(context.get_error(),glow::NO_ERROR);
    for name in ["absent".repeat(200),"λ".into(),"$".into()] {
        assert_eq!(context.advanced(Advanced::GetFragDataLocation {program,name:name.clone()}),Value::Int(-1));
        assert_eq!(context.advanced(Advanced::GetUniformIndices {program,names:vec![name]}),Value::UInts(vec![glow::INVALID_INDEX]));
        assert_eq!(context.get_error(),glow::NO_ERROR);
    }
    assert_eq!(context.advanced(Advanced::GetUniformBlockIndex {program,name:"b".repeat(1025)}),Value::UInt(glow::INVALID_INDEX));
    assert_eq!(context.get_error(),glow::NO_ERROR);
    for name in [format!("{block}\0suffix"),"λ".into(),"$".into()] {
        assert_eq!(context.advanced(Advanced::GetUniformBlockIndex {program,name}),Value::UInt(0));
        assert_eq!(context.get_error(),glow::INVALID_VALUE);
    }
    // Invalid varying identifiers are diagnosed at link time, not by a location-name cap.
    context.resource(ResourceCommand::TransformFeedbackVaryings {program,varyings:vec!["v".repeat(1025)],mode:glow::INTERLEAVED_ATTRIBS},None);
    assert_eq!(context.get_error(),glow::NO_ERROR);context.link_program(program);
    assert_eq!(context.query(Query::GetProgramParameter {program,name:glow::LINK_STATUS}),Value::Bool(false));
}


#[test]
#[ignore = "mandatory real ANGLE WebGL 2 failed-link query error precedence"]
fn real_webgl_two_program_queries_reject_unlinked_and_failed_relinked_programs() {
    use crate::advanced::Advanced;
    fn rejects(context: &mut CanvasContext, program: u32) {
        for index in [0, u32::MAX] {
            for query in [
                Advanced::GetTransformFeedbackVarying { program, index },
                Advanced::GetActiveUniformBlockName { program, index },
                Advanced::GetActiveUniformBlockParameter { program, index, name: glow::UNIFORM_BLOCK_DATA_SIZE },
            ] {
                assert_eq!(context.advanced(query), Value::Null);
                assert_eq!(context.get_error(), glow::INVALID_OPERATION);
                assert_eq!(context.get_error(), glow::NO_ERROR);
            }
        }
        assert_eq!(context.advanced(Advanced::GetFragDataLocation { program, name: "color".into() }), Value::Int(-1));
        assert_eq!(context.get_error(), glow::INVALID_OPERATION);
        assert_eq!(context.get_error(), glow::NO_ERROR);
    }
    let mut context = browser_context(2);
    let program = context.create_object(Kind::Program, 0).unwrap();
    rejects(&mut context, program);
    context.link_program(program);
    assert_eq!(context.query(Query::GetProgramParameter { program, name: glow::LINK_STATUS }), Value::Bool(false));
    rejects(&mut context, program);
    for (kind, source) in [
        (glow::VERTEX_SHADER, "#version 300 es\nuniform B {float b;};out float v;void main(){v=b;gl_Position=vec4(b,0.,0.,1.);}"),
        (glow::FRAGMENT_SHADER, "#version 300 es\nprecision mediump float;out vec4 color;void main(){color=vec4(1.);}"),
    ] {
        let shader = context.create_object(Kind::Shader, kind).unwrap();
        context.shader_source(shader, source); context.compile_shader(shader);
        assert_eq!(context.query(Query::GetShaderParameter { shader, name: glow::COMPILE_STATUS }), Value::Bool(true));
        context.attach_shader(program, shader, false);
    }
    context.resource(ResourceCommand::TransformFeedbackVaryings { program, varyings: vec!["v".into()], mode: glow::INTERLEAVED_ATTRIBS }, None);
    context.link_program(program);
    assert_eq!(context.query(Query::GetProgramParameter { program, name: glow::LINK_STATUS }), Value::Bool(true));
    assert_eq!(context.advanced(Advanced::GetTransformFeedbackVarying { program, index: 0 }), Value::Active { size: 1, data_type: glow::FLOAT, name: "v".into() });
    assert_eq!(context.advanced(Advanced::GetActiveUniformBlockName { program, index: 0 }), Value::String("B".into()));
    assert!(matches!(context.advanced(Advanced::GetActiveUniformBlockParameter { program, index: 0, name: glow::UNIFORM_BLOCK_DATA_SIZE }), Value::Int(size) if size >= 4));
    assert_eq!(context.get_error(), glow::NO_ERROR);
    assert_eq!(context.advanced(Advanced::GetTransformFeedbackVarying { program, index: 1 }), Value::Null);
    assert_eq!(context.get_error(), glow::INVALID_VALUE);
    context.resource(ResourceCommand::TransformFeedbackVaryings { program, varyings: vec!["absent".into()], mode: glow::INTERLEAVED_ATTRIBS }, None);
    context.link_program(program);
    assert_eq!(context.query(Query::GetProgramParameter { program, name: glow::LINK_STATUS }), Value::Bool(false));
    rejects(&mut context, program);
}

fn creation_test_mode() -> Mode {
    if backend() == Backend::SwiftShader { Mode::Software } else { Mode::Hardware }
}

#[test]
#[ignore = "real driver with injected initialization fault: first canvas make-current fails"]
fn initial_activation_failure_rejects_creation() {
    for version in [1, 2] {
        crate::creation_test_faults::ACTIVATE.with(|fault| fault.set(true));
        let result = CanvasContext::create(version, 8, 8, Attributes::default(), creation_test_mode());
        let consumed = crate::creation_test_faults::ACTIVATE.with(|fault| !fault.replace(false));
        assert!(consumed, "backend did not reach the injected canvas activation");
        if let Ok(context) = &result {
            eprintln!("INITIALIZATION_RESULT returned_ok=true lost={}", context.is_lost());
        }
        let failure = match result {
            Err(failure) => failure,
            Ok(_) => panic!("creation published a context after initial setup failed"),
        };
        assert_eq!(failure.attempts.len(), 1);
        assert_eq!(failure.attempts[0].backend, backend());
        assert_eq!(failure.attempts[0].reason, "Initial WebGL drawing-buffer activation failed");
    }
}

#[test]
#[ignore = "real driver with injected initialization fault: first drawing-buffer clear fails"]
fn initial_clear_failure_rejects_creation() {
    for version in [1, 2] {
        crate::creation_test_faults::CLEAR.with(|fault| fault.set(true));
        let mut result = CanvasContext::create(version, 8, 8, Attributes::default(), creation_test_mode());
        let consumed = crate::creation_test_faults::CLEAR.with(|fault| !fault.replace(false));
        assert!(consumed, "backend did not reach the injected clear");
        if let Ok(context) = &mut result {
            eprintln!("INITIALIZATION_RESULT returned_ok=true lost={} error={}", context.is_lost(), context.get_error());
        }
        let failure = match result {
            Err(failure) => failure,
            Ok(_) => panic!("creation published a context after initial setup failed"),
        };
        assert_eq!(failure.attempts.len(), 1);
        assert_eq!(failure.attempts[0].backend, backend());
        assert_eq!(failure.attempts[0].reason, "Initial WebGL drawing-buffer setup failed");
    }
}

#[test]
#[ignore = "control: activation failure after publication remains context loss"]
fn activation_failure_after_creation_loses_context() {
    for version in [1, 2] {
        let mut context = CanvasContext::create(version, 8, 8, Attributes::default(), creation_test_mode()).unwrap();
        assert!(!context.is_lost());
        crate::creation_test_faults::ACTIVATE.with(|fault| fault.set(true));
        context.command(Command::Clear { mask: glow::COLOR_BUFFER_BIT });
        assert!(context.is_lost());
        assert_eq!(context.get_error(), 0x9242);
        assert_eq!(context.get_error(), glow::NO_ERROR);
    }
}

fn is_swiftshader(renderer: &str) -> bool {
    renderer.contains("SwiftShader")
}
fn creation_attributes(fail_if_major_performance_caveat: bool) -> Attributes {
    Attributes {
        antialias: false,
        fail_if_major_performance_caveat,
        ..Attributes::default()
    }
}
/// Restores an environment variable when the test ends, including on panic.
/// nextest runs every test in its own process; this also keeps the change
/// out of later tests when the binary is run another way.
struct EnvOverride(&'static str, Option<std::ffi::OsString>);
impl EnvOverride {
    fn set(name: &'static str, value: Option<&std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        Self(name, previous)
    }
}
impl Drop for EnvOverride {
    fn drop(&mut self) {
        unsafe {
            match &self.1 {
                Some(value) => std::env::set_var(self.0, value),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

#[test]
#[ignore = "real driver: every automatic context is labelled with the device that backs it"]
fn auto_mode_labels_match_the_actual_device() {
    let requested = backend();
    let contexts = [1, 2, 1]
        .map(|version| CanvasContext::create(version, 8, 8, creation_attributes(false), Mode::Auto).unwrap());
    let first = contexts[0].diagnostics.backend;
    for context in &contexts {
        let d = &context.diagnostics;
        eprintln!("AUTO_CONTEXT backend={:?} renderer={} attempts={:?}", d.backend, d.renderer, d.attempts);
        assert_eq!(d.backend == Backend::SwiftShader, is_swiftshader(&d.renderer), "{d:?}");
        // The selected device must not depend on earlier contexts.
        assert_eq!(d.backend, first, "{d:?}");
    }
    if requested == Backend::Metal {
        assert_eq!(first, Backend::Metal);
    }
}

#[test]
#[ignore = "real driver: a SwiftShader display is never adopted by a hardware request"]
fn software_display_is_never_reused_for_hardware() {
    if backend() == Backend::Metal {
        // macOS has only the Metal display, shared by every context.
        let (a, b) = (context(1), context(2));
        assert_eq!(a.display_handle(), b.display_handle());
        return;
    }
    let options = SurfaceOptions {
        alpha: true,
        depth: false,
        stencil: false,
        antialias: false,
    };
    let software = Context::create(Backend::SwiftShader, 1, 8, 8, options).unwrap();
    let renderer = unsafe { software.gl.get_parameter_string(glow::RENDERER) };
    assert!(is_swiftshader(&renderer), "{renderer}");
    // One ANGLE display per process: once SwiftShader is live, a hardware
    // request neither adopts its display nor starts a second device.
    for version in [1, 2] {
        let reason = match Context::create(Backend::Vulkan, version, 8, 8, options) {
            Err(reason) => reason,
            Ok(hardware) => panic!(
                "hardware context created (same display: {}, renderer: {})",
                hardware.display_handle() == software.display_handle(),
                unsafe { hardware.gl.get_parameter_string(glow::RENDERER) }
            ),
        };
        eprintln!("HARDWARE_RESULT {reason}");
        assert!(reason.contains("SwiftShader WebGL display is already active"), "{reason}");
    }
    // Hardware attempts leave the software display usable and unchanged.
    let again = Context::create(Backend::SwiftShader, 2, 8, 8, options).unwrap();
    assert_eq!(again.display_handle(), software.display_handle());
    unsafe {
        again.make_current().unwrap();
        again.gl.clear_color(0.0, 1.0, 0.0, 1.0);
        again.gl.clear(glow::COLOR_BUFFER_BIT);
        assert_eq!(pixel(&again), [0, 255, 0, 255]);
    }
}

#[test]
#[ignore = "real driver: failIfMajorPerformanceCaveat never yields SwiftShader, before or after a software context"]
fn caveat_context_after_software_context_is_rejected() {
    let _ = backend();
    let check = |phase: &str| {
        for version in [1, 2] {
            match CanvasContext::create(version, 8, 8, creation_attributes(true), Mode::Auto) {
                Ok(context) => {
                    let d = &context.diagnostics;
                    eprintln!("CAVEAT_RESULT {phase} webgl{version} backend={:?} renderer={}", d.backend, d.renderer);
                    assert_ne!(d.backend, Backend::SwiftShader, "{d:?}");
                    assert!(!is_swiftshader(&d.renderer), "{d:?}");
                }
                Err(failure) => {
                    eprintln!("CAVEAT_RESULT {phase} webgl{version} null attempts={:?}", failure.attempts);
                    let last = failure.attempts.last().expect("a backend was attempted");
                    assert_eq!(last.backend, Backend::SwiftShader, "{failure:?}");
                    assert!(last.reason.contains("failIfMajorPerformanceCaveat"), "{failure:?}");
                }
            }
        }
    };
    check("before");
    let normal = CanvasContext::create(1, 8, 8, creation_attributes(false), Mode::Auto).unwrap();
    let mut software = None;
    if crate::selection::Platform::current() == crate::selection::Platform::Linux {
        let context = CanvasContext::create(2, 8, 8, creation_attributes(false), Mode::Software).unwrap();
        assert_eq!(context.diagnostics.backend, Backend::SwiftShader);
        assert!(is_swiftshader(&context.diagnostics.renderer));
        software = Some(context);
    }
    check("after");
    // Where automatic selection itself fell back to software, the caveat
    // request must fail however many software contexts exist.
    if normal.diagnostics.backend == Backend::SwiftShader {
        for version in [1, 2] {
            assert!(CanvasContext::create(version, 8, 8, creation_attributes(true), Mode::Auto).is_err());
        }
    }
    drop(software);
}

#[test]
#[ignore = "real driver: forced hardware rejects a CPU Vulkan ICD exposed through VK_ICD_FILENAMES"]
fn hardware_rejects_cpu_vulkan_icd() {
    if backend() == Backend::Metal {
        eprintln!("macOS selects Metal only; no Vulkan ICD is consulted");
        return;
    }
    // Expose the bundled SwiftShader ICD to the system loader before this
    // process creates any Vulkan instance, as a system lavapipe would be.
    let icd = crate::bundle::Bundle::discover()
        .and_then(|bundle| bundle.library("vk_swiftshader_icd.json"))
        .unwrap();
    let _icd = EnvOverride::set("VK_ICD_FILENAMES", Some(icd.as_os_str()));
    let _drivers = EnvOverride::set("VK_DRIVER_FILES", None);
    for version in [1, 2] {
        let failure = match CanvasContext::create(version, 8, 8, creation_attributes(false), Mode::Hardware) {
            Ok(context) => panic!("hardware accepted {:?}", context.diagnostics),
            Err(failure) => failure,
        };
        eprintln!("CPU_ICD_HARDWARE webgl{version} attempts={:?}", failure.attempts);
        assert_eq!(failure.attempts.len(), 1);
        assert_eq!(failure.attempts[0].backend, Backend::Vulkan);
        assert!(failure.attempts[0].reason.contains("unexpected CPU device"), "{failure:?}");
    }
    let auto = CanvasContext::create(1, 8, 8, creation_attributes(false), Mode::Auto).unwrap();
    let d = &auto.diagnostics;
    eprintln!("CPU_ICD_AUTO backend={:?} renderer={} attempts={:?}", d.backend, d.renderer, d.attempts);
    assert_eq!(d.backend, Backend::SwiftShader);
    assert!(is_swiftshader(&d.renderer), "{d:?}");
    assert!(d.attempts[0].reason.contains("unexpected CPU device"), "{d:?}");
    assert!(CanvasContext::create(1, 8, 8, creation_attributes(true), Mode::Auto).is_err());
}
