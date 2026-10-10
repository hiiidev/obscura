//! Driver-free checks run in normal nextest. The ignored cases are mandatory
//! backend-specific validation and must be selected explicitly in the handoff.
use crate::runtime::ObscuraJsRuntime;
use obscura_dom::parse_html;
use serde_json::json;
fn page() -> ObscuraJsRuntime {
    let mut runtime = ObscuraJsRuntime::with_base_url("https://graphics.example/");
    runtime.set_dom(parse_html(
        "<html><body style='margin:0'><canvas id='c' width='8' height='8'></canvas></body></html>",
    ));
    runtime.set_url("https://graphics.example/");
    runtime.run_page_init();
    runtime
}
fn require_driver() {
    let mode = std::env::var("OBSCURA_WEBGL_BACKEND")
        .expect("select hardware or software explicitly for real-driver tests");
    assert!(matches!(mode.as_str(), "hardware" | "software"));
    assert!(std::path::Path::new(
        &std::env::var("OBSCURA_WEBGL_LIB_DIR").expect("pinned ANGLE bundle required")
    )
    .join("bundle.json")
    .is_file());
}

#[test]
fn color_conversion_op_is_bounded_and_needs_no_graphics_context() {
    let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const pixels=new Uint8Array([255,0,0,255]),ops=__obscura_test_ops;
      const success=ops.op_webgl_convert_color('srgb','display-p3',false,pixels);
      const before=Array.from(pixels);
      const invalid=ops.op_webgl_convert_color('p3','srgb',false,pixels);
      const short=ops.op_webgl_convert_color('srgb','srgb',false,new Uint8Array(3));
      return [success,before,invalid,Array.from(pixels),short];
    })()"#).unwrap(),json!([true,[234,51,35,255],false,[234,51,35,255],false]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
}

#[test]
#[ignore = "mandatory real-driver P3 canvas display, retained bitmap gamut and matching-space texture upload"]
fn real_p3_canvas_and_bitmap_keep_raw_gamut_until_the_destination_conversion() {
    require_driver();let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const result=[];
      for(const version of [1,2]){
        const canvas=new OffscreenCanvas(1,1),gl=canvas.getContext(version===1?'webgl':'webgl2',{antialias:false,preserveDrawingBuffer:true});
        if(!gl)throw Error('WebGL unavailable');gl.drawingBufferColorSpace='display-p3';
        gl.clearColor(128/255,64/255,32/255,1);gl.clear(gl.COLOR_BUFFER_BIT);
        const visible=new OffscreenCanvas(1,1),ctx=visible.getContext('2d');ctx.drawImage(canvas,0,0);
        const converted=Array.from(ctx.getImageData(0,0,1,1).data);
        gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);const bitmap=canvas.transferToImageBitmap();
        ctx.drawImage(bitmap,0,0);const displayed=Array.from(ctx.getImageData(0,0,1,1).data);
        const destination=new OffscreenCanvas(1,1).getContext(version===1?'webgl':'webgl2',{antialias:false});
        if(!destination)throw Error('Destination WebGL unavailable');destination.unpackColorSpace='display-p3';
        const texture=destination.createTexture(),fbo=destination.createFramebuffer();destination.bindTexture(destination.TEXTURE_2D,texture);
        destination.texImage2D(destination.TEXTURE_2D,0,destination.RGBA,destination.RGBA,destination.UNSIGNED_BYTE,bitmap);
        destination.bindFramebuffer(destination.FRAMEBUFFER,fbo);destination.framebufferTexture2D(destination.FRAMEBUFFER,destination.COLOR_ATTACHMENT0,destination.TEXTURE_2D,texture,0);
        const raw=new Uint8Array(4);destination.readPixels(0,0,1,1,destination.RGBA,destination.UNSIGNED_BYTE,raw);
        result.push([converted,displayed,Array.from(raw),gl.drawingBufferColorSpace,destination.unpackColorSpace,destination.getError()]);bitmap.close();
      }
      return result;
    })()"#).unwrap(),json!([
        [[138,59,21,255],[255,0,0,255],[255,0,0,255],"display-p3","display-p3",0],
        [[138,59,21,255],[255,0,0,255],[255,0,0,255],"display-p3","display-p3",0]
    ]));
}

#[test]
#[ignore = "mandatory real-driver custom drawing storage dimensions in PNG and OffscreenCanvas bitmaps"]
fn real_drawing_storage_serializes_actual_dimensions_and_transfers_pixels() {
    require_driver();
    let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const result=[];
      for(const version of [1,2]){
        const canvas=document.createElement('canvas');canvas.width=8;canvas.height=8;
        const gl=canvas.getContext(version===1?'webgl':'webgl2',{antialias:false,preserveDrawingBuffer:true});
        if(!gl)throw Error('WebGL unavailable');
        gl.drawingBufferStorage(gl.RGBA8,3,2);gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);
        const png=atob(canvas.toDataURL().split(',')[1]);
        const size=offset=>[0,1,2,3].reduce((v,i)=>v*256+png.charCodeAt(offset+i),0);
        gl.drawingBufferStorage(gl.RGBA8,0,2);const error=gl.getError();
        result.push([canvas.width,canvas.height,gl.drawingBufferWidth,gl.drawingBufferHeight,gl.drawingBufferFormat,size(16),size(20),error,gl.getParameter(gl.FRAMEBUFFER_BINDING)]);
        const offscreen=new OffscreenCanvas(8,8),other=offscreen.getContext(version===1?'webgl':'webgl2',{antialias:false,preserveDrawingBuffer:true});
        if(!other)throw Error('Offscreen WebGL unavailable');
        other.drawingBufferStorage(other.RGBA8,2,3);other.clearColor(0,1,0,1);other.clear(other.COLOR_BUFFER_BIT);
        const bitmap=offscreen.transferToImageBitmap(),copy=new OffscreenCanvas(2,3),target=copy.getContext('2d');
        target.drawImage(bitmap,0,0);const pixel=Array.from(target.getImageData(0,0,1,1).data);
        const blank=new Uint8Array(4);other.readPixels(0,0,1,1,other.RGBA,other.UNSIGNED_BYTE,blank);
        result.push([bitmap.width,bitmap.height,pixel,Array.from(blank),other.getError()]);bitmap.close();
      }
      return result;
    })()"#).unwrap(),json!([
        [8,8,3,2,32856,3,2,1281,null],[2,3,[0,255,0,255],[0,0,0,0],0],
        [8,8,3,2,32856,3,2,1281,null],[2,3,[0,255,0,255],[0,0,0,0],0]
    ]));
}

#[test]
fn deferred_graphics_cleanup_retries_at_a_synchronous_runtime_boundary() {
    let mut runtime = page();
    let cleanup = crate::webgl_ops::deferred_cleanup(&runtime.js_runtime).unwrap();
    let owner = runtime.state.clone();
    let guard = owner.borrow_mut();
    cleanup.request(std::rc::Rc::downgrade(&owner), guard.document_generation, u32::MAX);
    drop(runtime.runtime());
    assert_eq!(cleanup.pending_count(), 1);
    drop(guard);
    let op_state = runtime.js_runtime.op_state();
    let op_guard = op_state.borrow_mut();
    drop(runtime.runtime());
    assert_eq!(cleanup.pending_count(), 1);
    drop(op_guard);
    drop(runtime.runtime());
    assert_eq!(cleanup.pending_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn deferred_graphics_cleanup_retries_after_async_poll_and_cancellation() {
    let mut runtime = page();
    let cleanup = crate::webgl_ops::deferred_cleanup(&runtime.js_runtime).unwrap();
    let owner = runtime.state.clone();
    for cancel in [false,true] {
        let guard = owner.borrow_mut();
        cleanup.request(std::rc::Rc::downgrade(&owner), guard.document_generation, u32::MAX);
        let future = super::entered_runtime_future(&mut runtime.js_runtime, |_runtime| std::future::ready(()));
        assert_eq!(cleanup.pending_count(), 1);
        drop(guard);
        if cancel { drop(future); } else { future.await; }
        assert_eq!(cleanup.pending_count(), 0);
    }
    let guard = owner.borrow_mut();
    cleanup.request(std::rc::Rc::downgrade(&owner), guard.document_generation, u32::MAX);
    let mut future = Box::pin(super::entered_runtime_future(&mut runtime.js_runtime,
        |_runtime| std::future::pending::<()>()));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(future.as_mut(),cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    assert_eq!(cleanup.pending_count(), 1);
    drop(guard);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(future.as_mut(),cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    assert_eq!(cleanup.pending_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real V8 collection while document state is borrowed"]
async fn real_collected_context_survives_busy_cleanup_then_retires_without_navigation() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate("(()=>{const c=new OffscreenCanvas(2,2);if(!c.getContext('webgl'))throw Error('WebGL unavailable');})()").unwrap();
    let cleanup = crate::webgl_ops::deferred_cleanup(&runtime.js_runtime).unwrap();
    let owner = runtime.state.clone();
    let guard = owner.borrow_mut();
    assert_eq!(guard.webgl.entries.len(), 1);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            {
                let mut entered = runtime.runtime();
                entered.v8_isolate().clear_kept_objects();
                entered.v8_isolate().low_memory_notification();
            }
            super::entered_runtime_future(&mut runtime.js_runtime, |entered|
                entered.run_event_loop(deno_core::PollEventLoopOptions::default())).await.unwrap();
            if cleanup.pending_count() == 1 { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("collected context did not retain its contended cleanup request");
    assert_eq!(guard.webgl.entries.len(), 1);
    drop(guard);
    drop(runtime.runtime());
    assert_eq!(cleanup.pending_count(), 0);
    assert!(owner.borrow().webgl.entries.is_empty());
}

#[test]
fn missing_graphics_libraries_return_null_without_disabling_canvas_2d() {
    // nextest runs each V8 test in a fresh process. Configure before creating
    // the isolate, without changing another test's native loader cache.
    unsafe {
        std::env::set_var(
            "OBSCURA_WEBGL_LIB_DIR",
            "/obscura-test-missing-graphics-bundle",
        );
        std::env::set_var("OBSCURA_WEBGL_BACKEND", "auto");
    }
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{const canvas=document.getElementById('c');const gl=canvas.getContext('webgl');const gl2=canvas.getContext('webgl2');const ctx=canvas.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);return [gl,gl2,Array.from(ctx.getImageData(0,0,1,1).data),canvas.toDataURL().startsWith('data:image/png;base64,')];})()"#).unwrap(),json!([null,null,[255,0,0,255],true]));
}

#[test]
fn stale_canvas_epochs_reject_webgl_and_placeholder_creation_before_backend_loading() {
    let mut runtime = page();
    runtime.execute_script("<fixture-setup>", "window.oldCanvas=document.getElementById('c');window.offscreen=new OffscreenCanvas(1,1);window.oldEpoch=__obscura_test_ops.op_canvas_document_epoch(0)").unwrap();
    runtime.set_dom(parse_html("<canvas id=c width=1 height=1></canvas>"));runtime.run_page_init();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const ops=__obscura_test_ops,node=document.getElementById('c')._nid;
      return [oldCanvas.getContext('webgl'),offscreen.getContext('webgl'),offscreen.getContext('webgl2'),
        ops.op_webgl_create(0,oldEpoch,node,1,1,1,{}).status,
        ops.op_canvas_placeholder(0,oldEpoch,{kind:'register',node,width:1,height:1},new Uint8Array(0)).status];
    })()"#).unwrap(),json!([null,null,null,"failed","failed"]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
    assert!(runtime.state.borrow().webgl_surfaces.is_empty());
}

#[test]
fn frame_teardown_releases_cpu_backing_stores_even_with_retained_native_state() {
    let mut runtime = page();
    let frame = crate::frame::FrameRealm::new(&mut runtime,72,0,"https://graphics.example/frame",
        "<canvas id=c width=1 height=1></canvas>").unwrap();
    frame.execute_script(&mut runtime,"document.getElementById('c').getContext('2d').fillRect(0,0,1,1)").unwrap();
    let retained = {
        let op_state = runtime.js_runtime.op_state();let ops = op_state.borrow();
        crate::ops::frame_state(&ops,72)
    };
    assert_eq!(retained.borrow().canvas_surfaces.len(),1);
    assert!(runtime.state.borrow().canvas_surfaces.is_empty());
    assert_ne!(retained.borrow().canvas_epoch,0);
    drop(frame);
    assert!(retained.borrow().canvas_surfaces.is_empty());
    assert_eq!(retained.borrow().canvas_epoch,0);
}
#[test]
fn invalid_backend_selection_returns_null_and_preserves_dictionary_errors() {
    unsafe {
        std::env::set_var("OBSCURA_WEBGL_BACKEND", "not-a-backend");
    }
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{const canvas=document.getElementById('c');let invalid=false;try{canvas.getContext('webgl',{powerPreference:'invalid'});}catch(e){invalid=e instanceof TypeError;}return [canvas.getContext('webgl'),invalid,!!canvas.getContext('2d')];})()"#).unwrap(),json!([null,true,true]));
}
#[test]
fn webgl_interface_constructors_and_borrowed_methods_reject_invalid_receivers() {
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{const errors=[];for(const call of [()=>new WebGLRenderingContext(),()=>new WebGL2RenderingContext(),()=>new WebGLBuffer(),()=>WebGLRenderingContext.prototype.getError.call({})]){try{call();errors.push(false);}catch(e){errors.push(e instanceof TypeError);}}return [errors,typeof WebGLRenderingContext.prototype.bindVertexArray,typeof WebGL2RenderingContext.prototype.bindVertexArray,WebGLRenderingContext.COLOR_BUFFER_BIT,Object.getOwnPropertyDescriptor(WebGLRenderingContext.prototype,'clear').enumerable];})()"#).unwrap(),json!([[true,true,true,true],"undefined","function",16384,true]));
}

#[test]
fn standalone_cpu_canvas_moves_real_pixels_without_registering_dom_storage() {
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const canvas=new OffscreenCanvas(2,1);const ctx=canvas.getContext('2d');
      canvas._nid=document.getElementById('c')._nid;canvas._ctx={};
      ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);ctx.save();ctx.fillStyle='#0000ff';
      const bitmap=canvas.transferToImageBitmap();const blank=Array.from(ctx.getImageData(0,0,2,1).data);
      const copy=new OffscreenCanvas(2,1),target=copy.getContext('2d');target.drawImage(bitmap,0,0);bitmap.close();
      ctx.restore();const style=ctx.fillStyle;canvas.width=2;
      return [blank,Array.from(target.getImageData(0,0,2,1).data),style,ctx.fillStyle,ctx.canvas===canvas,
        canvas.getContext('2d')===ctx,canvas.getContext('webgl'),Object.keys(ctx).includes('_buf')];
    })()"#).unwrap(),json!([[0,0,0,0,0,0,0,0],[255,0,0,255,0,0,0,0],"#ff0000","#000000",true,true,null,false]));
    let state = runtime.state.borrow();
    assert!(state.canvas_surfaces.is_empty());
    assert!(state.webgl.entries.is_empty());
    assert!(state.webgl_surfaces.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_canvas_serializes_snapshot_and_preserves_png_fallback_type() {
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      window.offscreenResult=null;const canvas=new OffscreenCanvas(1,1),ctx=canvas.getContext('2d');
      ctx.fillStyle='#00ff00';ctx.fillRect(0,0,1,1);
      const promise=canvas.convertToBlob({type:'image/jpeg'});ctx.clearRect(0,0,1,1);
      promise.then(async blob=>{
        const bitmap=await createImageBitmap(blob),copy=new OffscreenCanvas(1,1),target=copy.getContext('2d');
        target.drawImage(bitmap,0,0);bitmap.close();window.offscreenResult=[blob.type,
          Array.from(target.getImageData(0,0,1,1).data),Array.from(ctx.getImageData(0,0,1,1).data)];
      }).catch(error=>{window.offscreenResult=error.name+':'+error.message;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
        .await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("offscreenResult").unwrap(), json!(["image/png",[0,255,0,255],[0,0,0,0]]));
    assert!(runtime.state.borrow().canvas_surfaces.is_empty());
}

#[test]
fn standalone_missing_backend_keeps_cpu_fallback_available() {
    unsafe {
        std::env::set_var("OBSCURA_WEBGL_LIB_DIR", "/obscura-test-missing-graphics-bundle");
        std::env::set_var("OBSCURA_WEBGL_BACKEND", "auto");
    }
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const canvas=new OffscreenCanvas(1,1),first=canvas.getContext('webgl'),second=canvas.getContext('webgl2');
      const ctx=canvas.getContext('2d',{alpha:false});ctx.clearRect(0,0,1,1);
      return [first,second,ctx.getContextAttributes().alpha,Array.from(ctx.getImageData(0,0,1,1).data)];
    })()"#).unwrap(),json!([null,null,false,[0,0,0,255]]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
    assert!(runtime.state.borrow().canvas_surfaces.is_empty());
}

#[test]
#[ignore = "mandatory real ANGLE standalone transfer, compositor isolation and GL state preservation"]
fn real_offscreen_gl_transfer_clears_pixels_preserving_graphics_state() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      window.offscreenContexts=[];
      for(const version of [1,2]){
        const canvas=new OffscreenCanvas(2,2),gl=canvas.getContext(version===1?'webgl':'webgl2',{preserveDrawingBuffer:true,antialias:false});
        if(!gl)throw Error('Standalone WebGL unavailable');
        gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);
        window.offscreenContexts.push({canvas,gl});
      }
    })()"#).unwrap();
    {
        let mut state = runtime.state.borrow_mut();
        assert_eq!(state.webgl.entries.len(), 2);
        assert!(state.webgl.entries.values().all(|entry| entry.node.is_none()));
        crate::webgl_ops::prepare_surfaces(&mut state);
        crate::webgl_ops::did_present(&mut state);
        assert!(state.webgl_surfaces.is_empty());
        assert!(state.webgl.entries.values().all(|entry| entry.context.dirty));
    }
    assert_eq!(runtime.evaluate(r#"offscreenContexts.map(({canvas,gl})=>{
      const framebuffer=gl.createFramebuffer();gl.bindFramebuffer(gl.FRAMEBUFFER,framebuffer);
      gl.enable(gl.SCISSOR_TEST);gl.scissor(1,1,1,1);gl.colorMask(false,true,false,true);gl.clearColor(0,1,0,1);
      const bitmap=canvas.transferToImageBitmap(),target=new OffscreenCanvas(2,2).getContext('2d');
      target.drawImage(bitmap,0,0);bitmap.close();
      const binding=gl.getParameter(gl.FRAMEBUFFER_BINDING)===framebuffer;
      const mask=Array.from(gl.getParameter(gl.COLOR_WRITEMASK));
      const clear=Array.from(gl.getParameter(gl.COLOR_CLEAR_VALUE));
      const scissor=gl.isEnabled(gl.SCISSOR_TEST);gl.bindFramebuffer(gl.FRAMEBUFFER,null);
      const pixel=new Uint8Array(4);gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
      return [Array.from(target.getImageData(0,0,1,1).data),Array.from(pixel),binding,mask,clear,scissor,gl.getError()];
    })"#).unwrap(),json!([
        [[255,0,0,255],[0,0,0,0],true,[false,true,false,true],[0,1,0,1],true,0],
        [[255,0,0,255],[0,0,0,0],true,[false,true,false,true],[0,1,0,1],true,0]
    ]));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real V8 standalone-context GC and repeated-profile native cleanup"]
async fn real_offscreen_context_is_collected_without_document_navigation() {
    require_driver();
    let mut runtime = page();
    for _ in 0..3 {
        runtime.evaluate(r#"(()=>{
          const canvas=new OffscreenCanvas(4,4);const gl=canvas.getContext('webgl');
          if(!gl)throw Error('Standalone WebGL unavailable');
          gl.clearColor(0,0,1,1);gl.clear(gl.COLOR_BUFFER_BIT);
        })()"#).unwrap();
        assert_eq!(runtime.state.borrow().webgl.entries.len(), 1);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                {
                    let mut entered = runtime.runtime();
                    entered.v8_isolate().clear_kept_objects();
                    entered.v8_isolate().low_memory_notification();
                }
                runtime.run_event_loop_bounded(10).await.unwrap();
                if runtime.state.borrow().webgl.entries.is_empty() { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("standalone context retained after its canvas was collected");
        assert!(runtime.state.borrow().canvas_surfaces.is_empty());
        assert!(runtime.state.borrow().webgl_surfaces.is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real V8 GC, bound-object retention and native resource cleanup"]
async fn real_webgl_gc_releases_unbound_resources_without_collecting_bound_buffers() {
    require_driver();
    let mut runtime = page();
    assert_eq!(
        runtime
            .evaluate(
                r#"(()=>{
      const gl=document.getElementById('c').getContext('webgl2');
      if(!gl)throw Error('WebGL unavailable');window.resourceGL=gl;
      for(let i=0;i<128;i++){
        const buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buffer);
        gl.bufferData(gl.ARRAY_BUFFER,65536,gl.STATIC_DRAW);
        if(i===127)window.boundResource=new WeakRef(buffer);
      }
      return gl.getError();
    })()"#
            )
            .unwrap().as_f64(),
        Some(0.0)
    );
    async fn collect(runtime: &mut ObscuraJsRuntime, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                {
                    let mut entered = runtime.runtime();
                    // Model an actual job boundary before asking V8 to collect.
                    entered.v8_isolate().clear_kept_objects();
                    entered.v8_isolate().low_memory_notification();
                }
                runtime.run_event_loop_bounded(10).await.unwrap();
                let count = {
                    let state = runtime.state.borrow();
                    let context = &state.webgl.entries.values().next().unwrap().context;
                    (1..=128)
                        .filter(|&id| {
                            context
                                .objects
                                .contains(id, obscura_webgl::objects::Kind::Buffer)
                        })
                        .count()
                };
                if count == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("native WebGL resources were not collected within the test deadline");
    }
    collect(&mut runtime, 1).await;
    assert_eq!(
        runtime
            .evaluate(
                "resourceGL.getParameter(resourceGL.ARRAY_BUFFER_BINDING)===boundResource.deref()"
            )
            .unwrap(),
        json!(true)
    );
    runtime
        .evaluate("resourceGL.bindBuffer(resourceGL.ARRAY_BUFFER,null)")
        .unwrap();
    collect(&mut runtime, 0).await;
    assert_eq!(
        runtime
            .evaluate("boundResource.deref()===undefined && resourceGL.getError()===0")
            .unwrap(),
        json!(true)
    );
}
#[test]
#[ignore = "mandatory on pinned ANGLE Metal, Linux SwiftShader and Linux hardware candidates"]
fn real_webgl_one_and_two_draw_shaders_and_serialize_canvas_pixels() {
    require_driver();
    let mut runtime = page();
    let result=runtime.evaluate(r#"(()=>{
      const results=[];
      for(const version of [1,2]){
        const canvas=document.createElement('canvas');canvas.width=8;canvas.height=8;
        const gl=canvas.getContext(version===1?'webgl':'webgl2',{antialias:false,preserveDrawingBuffer:true});
        if(!gl)throw new Error('real WebGL '+version+' unavailable');
        const vs=gl.createShader(gl.VERTEX_SHADER),fs=gl.createShader(gl.FRAGMENT_SHADER),program=gl.createProgram();
        gl.shaderSource(vs,version===1?'attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}':'#version 300 es\nin vec2 p;void main(){gl_Position=vec4(p,0.,1.);}');
        gl.shaderSource(fs,version===1?'precision mediump float;void main(){gl_FragColor=vec4(1.,0.,0.,1.);}':'#version 300 es\nprecision mediump float;out vec4 color;void main(){color=vec4(1.,0.,0.,1.);}');
        for(const shader of [vs,fs]){gl.compileShader(shader);if(!gl.getShaderParameter(shader,gl.COMPILE_STATUS))throw new Error(gl.getShaderInfoLog(shader));gl.attachShader(program,shader);}
        gl.bindAttribLocation(program,0,'p');gl.linkProgram(program);if(!gl.getProgramParameter(program,gl.LINK_STATUS))throw new Error(gl.getProgramInfoLog(program));
        gl.useProgram(program);const buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buffer);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,3,-1,-1,3]),gl.STATIC_DRAW);
        gl.vertexAttribPointer(0,2,gl.FLOAT,false,0,0);gl.enableVertexAttribArray(0);gl.drawArrays(gl.TRIANGLES,0,3);
        const pixel=new Uint8Array(4);gl.readPixels(2,2,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
        const expected=document.createElement('canvas');expected.width=8;expected.height=8;const ctx=expected.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,8,8);
        results.push([version,Array.from(pixel),canvas.toDataURL()===expected.toDataURL(),gl.getError(),canvas.getContext('2d'),Object.keys(gl),Object.keys(buffer)]);
        gl.deleteBuffer(buffer);gl.deleteShader(vs);gl.deleteShader(fs);gl.deleteProgram(program);
      }return results;
    })()"#).unwrap();
    assert_eq!(
        result,
        json!([
            [1, [255, 0, 0, 255], true, 0, null, [], []],
            [2, [255, 0, 0, 255], true, 0, null, [], []]
        ])
    );
    // Without runtime stealth both versions report the driver identity. With
    // it, a document reports its profile's seeded Windows GPU and Chrome
    // version strings, stable across versions, contexts and repeated reads.
    let plain = webgl_identity_rows(&mut runtime);
    let (driver_vendor, driver_renderer) = webgl_driver_identity(&runtime);
    for (row, version) in plain.iter().zip([1, 2, 1, 2]) {
        assert_eq!(row["unmasked"], json!([driver_vendor, driver_renderer]), "{row}");
        assert_eq!(row["strings"], json!(["WebKit", "WebKit WebGL",
            if version == 1 { "WebGL 1.0 (OpenGL ES 2.0)" } else { "WebGL 2.0 (OpenGL ES 3.0)" },
            if version == 1 { "WebGL GLSL ES 1.00" } else { "WebGL GLSL ES 3.00" }]), "{row}");
    }
    runtime.set_stealth(true);
    runtime.set_platform("Win32", "Windows", "15.0.0");
    runtime.set_dom(parse_html("<html><body><canvas id='c' width='8' height='8'></canvas></body></html>"));
    runtime.run_page_init();
    let masked = webgl_identity_rows(&mut runtime);
    let (_, driver_renderer) = webgl_driver_identity(&runtime);
    let profile = masked[0]["unmasked"].clone();
    let (vendor, renderer) = (profile[0].as_str().unwrap(), profile[1].as_str().unwrap());
    let gpu = renderer.strip_prefix("ANGLE (").and_then(|rest| rest.split(',').next()).unwrap_or_default();
    assert!(matches!(gpu, "NVIDIA" | "Intel" | "AMD"), "{renderer}");
    assert!(renderer.ends_with(" Direct3D11 vs_5_0 ps_5_0, D3D11)"), "{renderer}");
    assert_eq!(vendor, format!("Google Inc. ({gpu})"));
    assert_ne!(renderer, driver_renderer);
    for (row, version) in masked.iter().zip([1, 2, 1, 2]) {
        assert_eq!(row["unmasked"], profile, "{row}");
        assert_eq!(row["strings"], json!(["WebKit", "WebKit WebGL",
            if version == 1 { "WebGL 1.0 (OpenGL ES 2.0 Chromium)" } else { "WebGL 2.0 (OpenGL ES 3.0 Chromium)" },
            if version == 1 {
                "WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)"
            } else {
                "WebGL GLSL ES 3.00 (OpenGL ES GLSL ES 3.0 Chromium)"
            }]), "{row}");
    }
    // The extension stays listed, and the disabled-extension query keeps its
    // null result and INVALID_ENUM in both modes.
    for row in plain.iter().chain(masked.iter()) {
        assert_eq!(row["hidden"], json!([null, 1280]), "{row}");
        assert_eq!(row["listed"], true, "{row}");
        assert_eq!(row["repeated"], row["unmasked"], "{row}");
        assert_eq!(row["error"], 0, "{row}");
    }
}
fn webgl_identity_rows(runtime: &mut ObscuraJsRuntime) -> Vec<serde_json::Value> {
    let rows = runtime.evaluate(r#"(()=>[1,2,1,2].map(version=>{
      const canvas=document.createElement('canvas');canvas.width=2;canvas.height=2;
      const gl=canvas.getContext(version===1?'webgl':'webgl2');
      if(!gl)throw new Error('real WebGL '+version+' unavailable');
      const hidden=[gl.getParameter(0x9246),gl.getError()];
      const listed=gl.getSupportedExtensions().includes('WEBGL_debug_renderer_info');
      const ext=gl.getExtension('WEBGL_debug_renderer_info');
      const read=()=>[gl.getParameter(ext.UNMASKED_VENDOR_WEBGL),gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)];
      const unmasked=read();
      return {hidden,listed,unmasked,repeated:read(),
        strings:[gl.VENDOR,gl.RENDERER,gl.VERSION,gl.SHADING_LANGUAGE_VERSION].map(name=>gl.getParameter(name)),
        error:gl.getError()};
    }))()"#).unwrap();
    rows.as_array().unwrap().clone()
}
fn webgl_driver_identity(runtime: &ObscuraJsRuntime) -> (String, String) {
    let state = runtime.state.borrow();
    let mut identities = state.webgl.entries.values()
        .map(|entry| (entry.context.diagnostics.vendor.clone(), entry.context.diagnostics.renderer.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(identities.len(), 1, "{identities:?}");
    identities.pop_first().unwrap()
}
#[test]
#[ignore = "mandatory real-driver buffer bounds, ownership, texture and FBO validation"]
fn real_webgl_resources_reject_cross_context_and_short_transfer_views() {
    require_driver();
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const a=document.createElement('canvas').getContext('webgl2',{antialias:false});const b=document.createElement('canvas').getContext('webgl2',{antialias:false});if(!a||!b)throw Error('WebGL unavailable');
      const buffer=a.createBuffer();b.bindBuffer(b.ARRAY_BUFFER,buffer);const cross=b.getError();
      a.bindBuffer(a.ARRAY_BUFFER,buffer);a.bufferData(a.ARRAY_BUFFER,new Uint8Array([1,2,3,4]),a.STATIC_DRAW);a.bufferSubData(a.ARRAY_BUFFER,4,new Uint8Array([8]));const bounds=a.getError();
      const read=new Uint8Array(4);a.getBufferSubData(a.ARRAY_BUFFER,0,read);
      const view=new Uint8Array([9,9,9,9]);a.getBufferSubData(a.ARRAY_BUFFER,1,view,1,2);
      a.getBufferSubData(a.ARRAY_BUFFER,4,new Uint8Array(0));
      const retained=a.getParameter(a.ARRAY_BUFFER_BINDING)===buffer;
      const texture=a.createTexture();a.bindTexture(a.TEXTURE_2D,texture);a.texImage2D(a.TEXTURE_2D,0,a.RGBA8,2,2,0,a.RGBA,a.UNSIGNED_BYTE,new Uint8Array(3));const short=a.getError();
      a.texImage2D(a.TEXTURE_2D,0,a.RGBA8,2,2,0,a.RGBA,a.UNSIGNED_BYTE,null);
      const fbo=a.createFramebuffer();a.bindFramebuffer(a.FRAMEBUFFER,fbo);a.framebufferTexture2D(a.FRAMEBUFFER,a.COLOR_ATTACHMENT0,a.TEXTURE_2D,texture,0);
      const attached=a.getFramebufferAttachmentParameter(a.FRAMEBUFFER,a.COLOR_ATTACHMENT0,a.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME)===texture;
      a.clearColor(0,1,0,1);a.clear(a.COLOR_BUFFER_BIT);const pixel=new Uint8Array(4);a.readPixels(0,0,1,1,a.RGBA,a.UNSIGNED_BYTE,pixel);
      const result=[cross,bounds,short,Array.from(read),attached,Array.from(pixel),a.getError(),Array.from(view),retained];
      a.deleteFramebuffer(fbo);a.deleteTexture(texture);a.deleteBuffer(buffer);return result;
    })()"#).unwrap(),json!([1282,1281,1282,[1,2,3,4],true,[0,255,0,255],0,[9,2,3,9],true]));
}
#[test]
#[ignore = "mandatory real-driver resize, viewport preservation and default-buffer clear validation"]
fn real_webgl_resize_keeps_viewport_and_clears_pixels() {
    require_driver();
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{const c=document.getElementById('c'),g=c.getContext('webgl',{antialias:false});if(!g)throw Error('WebGL unavailable');g.viewport(1,2,3,4);g.clearColor(1,0,0,1);g.clear(g.COLOR_BUFFER_BIT);c.width=c.width;const p=new Uint8Array(4);g.readPixels(0,0,1,1,g.RGBA,g.UNSIGNED_BYTE,p);const viewport=Array.from(g.getParameter(g.VIEWPORT));c.width=0;c.height=0;return [Array.from(p),viewport,g.drawingBufferWidth,g.drawingBufferHeight,c.toDataURL(),g.getError()];})()"#).unwrap(),json!([[0,0,0,0],[1,2,3,4],1,1,"data:,",0]));
}
#[test]
#[ignore = "mandatory real-driver repeated-profile resource destruction; measure RSS externally too"]
fn real_webgl_contexts_drop_on_repeated_document_replacement() {
    require_driver();
    let mut runtime = page();
    let mut previous_id=0;
    for _ in 0..32 {
        assert_eq!(runtime.evaluate(r#"(()=>{const c=document.getElementById('c'),g=c.getContext('webgl2');if(!g)throw Error('WebGL unavailable');for(let i=0;i<16;i++){const b=g.createBuffer();g.bindBuffer(g.ARRAY_BUFFER,b);g.bufferData(g.ARRAY_BUFFER,65536,g.STATIC_DRAW);}return g.getError();})()"#).unwrap().as_f64(),Some(0.0));
        let current_id=*runtime.state.borrow().webgl.entries.keys().next().unwrap();
        assert!(current_id>previous_id);previous_id=current_id;
        runtime.set_dom(parse_html(
            "<html><body><canvas id='c' width='8' height='8'></canvas></body></html>",
        ));
        assert!(runtime.state.borrow().webgl.entries.is_empty());
        assert!(runtime.state.borrow().webgl_surfaces.is_empty());
        runtime.run_page_init();
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory native compositor loss delivery without further page GL calls"]
async fn native_compositor_loss_delivers_once_and_restoration_rearms_notifications() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      const canvas=document.getElementById('c');window.gl=canvas.getContext('webgl');
      if(!gl)throw Error('WebGL unavailable');window.lossExtension=gl.getExtension('WEBGL_lose_context');
      window.contextEvents=[];
      canvas.addEventListener('webglcontextlost',e=>{contextEvents.push([e.type,e instanceof WebGLContextEvent,gl.drawingBufferWidth]);e.preventDefault();});
      canvas.addEventListener('webglcontextrestored',e=>contextEvents.push([e.type,e instanceof WebGLContextEvent,gl.drawingBufferWidth]));
    })()"#).unwrap();
    for round in 0..2 {
        {
            let mut state = runtime.state.borrow_mut();
            state
                .webgl
                .entries
                .values_mut()
                .next()
                .unwrap()
                .context
                .lose();
            crate::webgl_ops::prepare_surfaces(&mut state);
            crate::webgl_ops::prepare_surfaces(&mut state);
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
            .await
            .unwrap()
            .unwrap();
        let expected = if round == 0 {
            json!([["webglcontextlost", true, 0]])
        } else {
            json!([
                ["webglcontextlost", true, 0],
                ["webglcontextrestored", true, 8],
                ["webglcontextlost", true, 0]
            ])
        };
        assert_eq!(runtime.evaluate("contextEvents").unwrap(), expected);
        if round == 0 {
            runtime.evaluate("lossExtension.restoreContext()").unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
                .await
                .unwrap()
                .unwrap();
        }
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory queued loss cancellation on document replacement"]
async fn queued_loss_from_a_replaced_document_does_not_reach_the_new_document() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{window.retiredEvents=0;const canvas=document.getElementById('c');if(!canvas.getContext('webgl'))throw Error('WebGL unavailable');canvas.addEventListener('webglcontextlost',()=>retiredEvents++);})()"#).unwrap();
    {
        let mut state = runtime.state.borrow_mut();
        state
            .webgl
            .entries
            .values_mut()
            .next()
            .unwrap()
            .context
            .lose();
        crate::webgl_ops::prepare_surfaces(&mut state);
    }
    runtime.set_dom(parse_html(
        "<html><body><canvas id='c' width='4' height='4'></canvas></body></html>",
    ));
    runtime.run_page_init();
    runtime.execute_script("<fixture-setup>", "window.newGL=document.getElementById('c').getContext('webgl');if(!newGL)throw Error('new context unavailable')").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        runtime
            .evaluate("[retiredEvents,newGL.isContextLost(),newGL.drawingBufferWidth]")
            .unwrap(),
        json!([0, false, 4])
    );
    assert_eq!(runtime.state.borrow().webgl.entries.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory child-frame loss routing and teardown"]
async fn frame_loss_delivery_is_scoped_and_teardown_discards_pending_events() {
    require_driver();
    let mut runtime = page();
    runtime.execute_script("<fixture-setup>", "window.topLosses=0;document.getElementById('c').addEventListener('webglcontextlost',()=>topLosses++);window.lossMessages=[];addEventListener('message',e=>lossMessages.push([e.data,e.origin,e.isTrusted]))").unwrap();
    let frame = crate::frame::FrameRealm::new(
        &mut runtime,
        91,
        0,
        "https://graphics.example/frame",
        "<canvas id='c'></canvas>",
    )
    .unwrap();
    frame.execute_script(&mut runtime,"const canvas=document.getElementById('c');const gl=canvas.getContext('webgl');if(!gl)throw Error('frame WebGL unavailable');window.frameLosses=0;canvas.addEventListener('webglcontextlost',()=>{frameLosses++;parent.postMessage('webglcontextlost','*')});gl.getExtension('WEBGL_lose_context').loseContext();").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
        .await
        .unwrap()
        .unwrap();
    // Observe the event in its own realm. The inherited RemoteWindow stub
    // transports messages but does not forward arbitrary parent properties.
    assert_eq!(frame.evaluate(&mut runtime, "frameLosses").unwrap(), json!(1));
    assert_eq!(runtime.evaluate("topLosses").unwrap().as_f64(), Some(0.0));
    let messages = runtime.take_pending_frame_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!((messages[0].source_frame_id, messages[0].target_frame_id), (91, 0));
    // Host messages contain a structured-clone envelope. Deliver it through
    // the same bridge as the browser and assert the page-visible payload.
    runtime.execute_script("<frame-message>", &format!(
        "globalThis.__obscura_deliverMessage({}, {}, {});",
        serde_json::to_string(&messages[0].data_json).unwrap(),
        serde_json::to_string(&messages[0].origin).unwrap(),
        messages[0].source_frame_id,
    )).unwrap();
    assert_eq!(runtime.evaluate("lossMessages").unwrap(),
        json!([["webglcontextlost", "https://graphics.example", true]]));
    let retired = crate::frame::FrameRealm::new(
        &mut runtime,
        92,
        0,
        "https://graphics.example/retired",
        "<canvas id='c'></canvas>",
    )
    .unwrap();
    retired.execute_script(&mut runtime,"const canvas=document.getElementById('c');const gl=canvas.getContext('webgl');if(!gl)throw Error('frame WebGL unavailable');window.frameLosses=0;canvas.addEventListener('webglcontextlost',()=>{frameLosses++;parent.postMessage('webglcontextlost','*')});gl.getExtension('WEBGL_lose_context').loseContext();").unwrap();
    let retained_state = {
        let op_state = runtime.js_runtime.op_state();
        let op_state = op_state.borrow();
        crate::ops::frame_state(&op_state, 92)
    };
    assert_eq!(retained_state.borrow().webgl.entries.len(), 1);
    drop(retired);
    assert!(retained_state.borrow().webgl.entries.is_empty());
    assert!(retained_state.borrow().webgl_surfaces.is_empty());
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frame.evaluate(&mut runtime, "frameLosses").unwrap(), json!(1));
    assert_eq!(runtime.evaluate("topLosses").unwrap().as_f64(), Some(0.0));
    assert!(runtime.take_pending_frame_messages().is_empty());
    assert_eq!(runtime.evaluate("lossMessages").unwrap(),
        json!([["webglcontextlost", "https://graphics.example", true]]));
}

#[test]
#[ignore = "mandatory real-driver WebGL 2 pixel-buffer offset and readback validation"]
fn real_webgl_two_pixel_buffers_keep_byte_offsets_and_bounds() {
    require_driver();
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const c=document.getElementById('c'),g=c.getContext('webgl2',{antialias:false});if(!g)throw Error('WebGL unavailable');
      const unpack=g.createBuffer();g.bindBuffer(g.PIXEL_UNPACK_BUFFER,unpack);g.bufferData(g.PIXEL_UNPACK_BUFFER,new Uint8Array([9,9,9,9,12,34,56,255]),g.STATIC_DRAW);
      const texture=g.createTexture();g.bindTexture(g.TEXTURE_2D,texture);g.texImage2D(g.TEXTURE_2D,0,g.RGBA8,1,1,0,g.RGBA,g.UNSIGNED_BYTE,4);
      const fbo=g.createFramebuffer();g.bindFramebuffer(g.FRAMEBUFFER,fbo);g.framebufferTexture2D(g.FRAMEBUFFER,g.COLOR_ATTACHMENT0,g.TEXTURE_2D,texture,0);
      const pack=g.createBuffer();g.bindBuffer(g.PIXEL_PACK_BUFFER,pack);g.bufferData(g.PIXEL_PACK_BUFFER,12,g.STREAM_READ);g.readPixels(0,0,1,1,g.RGBA,g.UNSIGNED_BYTE,8);
      const result=new Uint8Array(12);g.getBufferSubData(g.PIXEL_PACK_BUFFER,0,result);
      const error=g.getError();g.readPixels(0,0,1,1,g.RGBA,g.UNSIGNED_BYTE,12);const bounds=g.getError();
      g.deleteFramebuffer(fbo);g.deleteTexture(texture);g.deleteBuffer(pack);g.deleteBuffer(unpack);
      return [Array.from(result),error,bounds];
    })()"#).unwrap(),json!([[0,0,0,0,0,0,0,0,12,34,56,255],0,1282]));
}

#[test]
#[ignore = "mandatory real-driver DOM-source conversion and pixel-store restoration"]
fn real_webgl_dom_uploads_convert_pixels_and_restore_unpack_state() {
    require_driver();
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const g=document.getElementById('c').getContext('webgl2',{antialias:false});if(!g)throw Error('WebGL unavailable');
      const texture=g.createTexture();g.bindTexture(g.TEXTURE_2D,texture);
      g.pixelStorei(g.UNPACK_ALIGNMENT,8);g.pixelStorei(g.UNPACK_ROW_LENGTH,99);
      g.pixelStorei(g.UNPACK_SKIP_PIXELS,1);g.pixelStorei(g.UNPACK_SKIP_ROWS,1);
      const image=new ImageData(new Uint8ClampedArray([255,0,0,255,0,255,0,255,0,0,255,255,64,32,16,128]),2);
      g.pixelStorei(g.UNPACK_PREMULTIPLY_ALPHA_WEBGL,true);
      g.texImage2D(g.TEXTURE_2D,0,g.RGBA8,1,1,0,g.RGBA,g.UNSIGNED_BYTE,image);
      const state=[g.getParameter(g.UNPACK_ALIGNMENT),g.getParameter(g.UNPACK_ROW_LENGTH),g.getParameter(g.UNPACK_SKIP_PIXELS),g.getParameter(g.UNPACK_SKIP_ROWS),g.getParameter(g.UNPACK_PREMULTIPLY_ALPHA_WEBGL)];
      const fbo=g.createFramebuffer();g.bindFramebuffer(g.FRAMEBUFFER,fbo);g.framebufferTexture2D(g.FRAMEBUFFER,g.COLOR_ATTACHMENT0,g.TEXTURE_2D,texture,0);
      const pixels=new Uint8Array(4);g.readPixels(0,0,1,1,g.RGBA,g.UNSIGNED_BYTE,pixels);const ok=g.getError();
      // A failing source conversion must leave all unpack state unchanged.
      g.texImage2D(g.TEXTURE_2D,0,g.RGBA8,3,3,0,g.RGBA,g.UNSIGNED_BYTE,image);const invalid=g.getError();
      const after=[g.getParameter(g.UNPACK_ALIGNMENT),g.getParameter(g.UNPACK_ROW_LENGTH),g.getParameter(g.UNPACK_SKIP_PIXELS),g.getParameter(g.UNPACK_SKIP_ROWS),g.getParameter(g.UNPACK_PREMULTIPLY_ALPHA_WEBGL)];
      g.deleteFramebuffer(fbo);g.deleteTexture(texture);return [Array.from(pixels),state,ok,invalid,after];
    })()"#).unwrap(),json!([[32,16,8,128],[8,99,1,1,true],0,1282,[8,99,1,1,true]]));
}

#[test]
#[ignore = "mandatory real-driver image decoding and CORS permission validation"]
fn real_webgl_image_upload_uses_cached_response_permission() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate("document.body.insertAdjacentHTML('beforeend',\"<img id='image' src='https://graphics.example/source.png'>\")").unwrap();
    // Construct the fixture with the engine's existing PNG serializer.
    let data_url = runtime.evaluate("(()=>{const c=document.createElement('canvas');c.width=1;c.height=1;const x=c.getContext('2d');x.fillStyle='#ff0000';x.fillRect(0,0,1,1);return c.toDataURL();})()").unwrap();
    use base64::Engine;
    let bytes: std::sync::Arc<[u8]> = base64::engine::general_purpose::STANDARD
        .decode(data_url.as_str().unwrap().split_once(',').unwrap().1)
        .unwrap()
        .into();
    runtime
        .state
        .borrow_mut()
        .render_resources
        .seed_image_shared_with_origin(
            "https://graphics.example/source.png".into(),
            obscura_render::ImageRequestProfile::NoCorsInclude,
            bytes.clone(),
            true,
        );
    assert_eq!(runtime.evaluate(r#"(()=>{
      const g=document.getElementById('c').getContext('webgl',{antialias:false});if(!g)throw Error('WebGL unavailable');window.imageGL=g;
      const texture=g.createTexture();g.bindTexture(g.TEXTURE_2D,texture);g.texImage2D(g.TEXTURE_2D,0,g.RGBA,g.RGBA,g.UNSIGNED_BYTE,document.getElementById('image'));
      const fbo=g.createFramebuffer();g.bindFramebuffer(g.FRAMEBUFFER,fbo);g.framebufferTexture2D(g.FRAMEBUFFER,g.COLOR_ATTACHMENT0,g.TEXTURE_2D,texture,0);
      const pixels=new Uint8Array(4);g.readPixels(0,0,1,1,g.RGBA,g.UNSIGNED_BYTE,pixels);return [Array.from(pixels),g.getError()];
    })()"#).unwrap(),json!([[255,0,0,255],0]));
    // The same URL can later produce an unreadable response. It must not
    // inherit permission or produce a graphics upload using the old image.
    runtime
        .state
        .borrow_mut()
        .render_resources
        .seed_image_shared_with_origin(
            "https://graphics.example/source.png".into(),
            obscura_render::ImageRequestProfile::NoCorsInclude,
            bytes,
            false,
        );
    assert_eq!(runtime.evaluate(r#"(()=>{try{const g=window.imageGL;g.texImage2D(g.TEXTURE_2D,0,g.RGBA,g.RGBA,g.UNSIGNED_BYTE,document.getElementById('image'));return 'unexpected';}catch(e){return e.name;}})()"#).unwrap(),json!("SecurityError"));
}

#[test]
#[ignore = "mandatory real-driver SVG image upload and alpha validation"]
fn real_webgl_svg_upload_rasterizes_shapes_and_preserves_unpack_alpha() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate("document.body.insertAdjacentHTML('beforeend',\"<img id='svg' src='https://graphics.example/source.svg'>\")").unwrap();
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='2' height='1'><path fill='red' d='M0 0h1v1H0z'/><path fill='blue' fill-opacity='.5' d='M1 0h1v1H1z'/></svg>";
    runtime.state.borrow_mut().render_resources.seed_image_shared_with_origin(
        "https://graphics.example/source.svg".into(),
        obscura_render::ImageRequestProfile::NoCorsInclude,
        std::sync::Arc::from(svg.as_slice()), true,
    );
    assert_eq!(runtime.evaluate(r#"(()=>{
      return ['webgl','webgl2'].map(type=>{
        const canvas=document.createElement('canvas');const gl=canvas.getContext(type,{antialias:false});
        if(!gl)throw Error('WebGL unavailable');const texture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,texture);
        gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,document.getElementById('svg'));
        const fbo=gl.createFramebuffer();gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,texture,0);
        const pixels=new Uint8Array(8);gl.readPixels(0,0,2,1,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
        const straight=pixels[0]===255&&pixels[3]===255&&pixels[6]===255&&pixels[7]>=127&&pixels[7]<=128;
        gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,true);
        gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,document.getElementById('svg'));
        gl.readPixels(0,0,2,1,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
        return [straight,pixels[6]===pixels[7],gl.getError()];
      });
    })()"#).unwrap(),json!([[true,true,0],[true,true,0]]));
}

#[tokio::test(flavor = "current_thread")]
async fn imagebitmap_blob_decoding_and_close_need_no_graphics_driver() {
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      window.bitmapResult=null;
      const svg="<svg xmlns='http://www.w3.org/2000/svg' width='2' height='1'><rect width='2' height='1' fill='red'/></svg>";
      createImageBitmap(new Blob([svg],{type:'image/svg+xml'})).then(image=>{
        const before=[image instanceof ImageBitmap,image.width,image.height,Object.keys(image).length];
        image.close();image.close();window.bitmapResult=[before,image.width,image.height];
      },e=>{window.bitmapResult=e.name+':'+e.message;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("bitmapResult").unwrap(), json!([[true,2,1,0],0,0]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn imagebitmap_rejects_invalid_blob_and_source_states_without_graphics() {
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      window.bitmapErrors=[];
      const cases=[()=>createImageBitmap(new Blob(['not an image'])),
        ()=>createImageBitmap(new ImageData(1,1),0,0,0,1),
        ()=>createImageBitmap(new ImageData(1,1),{resizeWidth:0}),
        ()=>createImageBitmap(document.createElement('video'))];
      Promise.all(cases.map(fn=>fn().then(()=> 'unexpected',e=>e.name))).then(result=>{window.bitmapErrors=result;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("bitmapErrors").unwrap(),json!(["InvalidStateError","RangeError","InvalidStateError","InvalidStateError"]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real-driver ImageBitmap upload options and cleanup"]
async fn real_webgl_imagebitmap_upload_ignores_repeated_flip_and_premultiply() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      window.bitmapPixels=null;
      createImageBitmap(new ImageData(new Uint8ClampedArray([255,0,0,255,0,0,255,128]),1),{premultiplyAlpha:'none'}).then(image=>{
        const gl=document.getElementById('c').getContext('webgl2',{antialias:false});if(!gl)throw Error('WebGL unavailable');
        const texture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,texture);
        gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,true);gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,true);
        gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,image);
        const fbo=gl.createFramebuffer();gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,texture,0);
        const pixels=new Uint8Array(8);gl.readPixels(0,0,1,2,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
        const error=gl.getError();image.close();
        gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,image);
        window.bitmapPixels=[Array.from(pixels),error,gl.getError(),image.width,image.height];
      }).catch(e=>{window.bitmapPixels=e.name+':'+e.message;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("bitmapPixels").unwrap(),json!([[255,0,0,255,0,0,255,128],0,0x0501,0,0]));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real-driver tainted ImageBitmap creation and upload rejection"]
async fn real_webgl_imagebitmap_retains_cross_origin_image_taint() {
    require_driver();
    let mut runtime=page();
    runtime.evaluate("document.body.insertAdjacentHTML('beforeend',\"<img id='tainted' src='https://other.example/image.svg'>\")").unwrap();
    let bytes=b"<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><rect width='1' height='1' fill='red'/></svg>";
    runtime.state.borrow_mut().render_resources.seed_image_shared_with_origin(
        "https://other.example/image.svg".into(),obscura_render::ImageRequestProfile::NoCorsInclude,std::sync::Arc::from(bytes.as_slice()),false);
    runtime.evaluate(r#"(()=>{
      window.bitmapTaint=null;createImageBitmap(document.getElementById('tainted')).then(image=>{
        const gl=document.getElementById('c').getContext('webgl');if(!gl)throw Error('WebGL unavailable');
        try{gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,image);window.bitmapTaint='unexpected';}
        catch(e){window.bitmapTaint=[image.width,image.height,e.name,gl.getError()];}finally{image.close();}
      }).catch(e=>{window.bitmapTaint=e.name+':'+e.message;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("bitmapTaint").unwrap(),json!([1,1,"SecurityError",0]));
}

#[tokio::test(flavor = "current_thread")]
async fn imagebitmap_draws_real_pixels_to_private_two_dimensional_canvas_storage() {
    let mut runtime=page();
    runtime.evaluate(r#"(()=>{
      window.bitmapDraw=null;const input=new ImageData(new Uint8ClampedArray([255,0,0,255,0,0,255,128]),2);
      createImageBitmap(input).then(image=>{
        const canvas=document.createElement('canvas');canvas.width=2;canvas.height=1;const context=canvas.getContext('2d');
        context.drawImage(image,0,0);image.close();window.bitmapDraw=[Array.from(context.getImageData(0,0,2,1).data),context._buf===undefined];
      }).catch(e=>{window.bitmapDraw=e.name+':'+e.message;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2),runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("bitmapDraw").unwrap(),json!([[255,0,0,255,0,0,255,128],true]));
}

#[tokio::test(flavor = "current_thread")]
async fn bitmap_taint_propagates_through_canvas_copies_and_resets_only_with_pixels() {
    let mut runtime=page();
    runtime.evaluate("document.body.insertAdjacentHTML('beforeend',\"<img id='tainted' src='https://other.example/image.svg'>\")").unwrap();
    let bytes=b"<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><rect width='1' height='1' fill='red'/></svg>";
    runtime.state.borrow_mut().render_resources.seed_image_shared_with_origin(
        "https://other.example/image.svg".into(),obscura_render::ImageRequestProfile::NoCorsInclude,std::sync::Arc::from(bytes.as_slice()),false);
    runtime.evaluate(r#"(()=>{
      window.canvasTaint=null;createImageBitmap(document.getElementById('tainted')).then(image=>{
        const canvas=document.createElement('canvas');canvas.width=1;canvas.height=1;const context=canvas.getContext('2d');context.drawImage(image,0,0);
        const copy=document.createElement('canvas');copy.width=1;copy.height=1;const target=copy.getContext('2d');target.drawImage(canvas,0,0);
        const check=fn=>{try{fn();return 'unexpected';}catch(e){return e.name;}};
        const result=[check(()=>context.getImageData(0,0,1,1)),check(()=>canvas.toDataURL()),check(()=>copy.toDataURL()),context._buf===undefined];
        context.clearRect(0,0,1,1);result.push(check(()=>context.getImageData(0,0,1,1)));
        canvas.width=1;result.push(Array.from(context.getImageData(0,0,1,1).data));window.canvasTaint=result;image.close();
      }).catch(e=>{window.canvasTaint=e.name+':'+e.message;});
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2),runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("canvasTaint").unwrap(),json!(["SecurityError","SecurityError","SecurityError",true,"SecurityError",[0,0,0,0]]));
}

#[tokio::test(flavor = "current_thread")]
async fn placeholder_cpu_presentation_updates_layout_and_attributes_at_frame_boundary() {
    let mut runtime = page();
    runtime.set_viewport(16.0, 16.0);
    assert_eq!(runtime.evaluate(r#"(()=>{
      const c=document.getElementById('c');c.width=2;c.height=1;c.style.display='block';
      Object.defineProperty(c,'_ctx',{get(){throw Error('page-owned context field');},configurable:true});
      c.toDataURL();window.placeholder=c;window.offscreen=c.transferControlToOffscreen();
      window.attributeRecords=[];const observer=new MutationObserver(records=>attributeRecords.push(...records));
      observer.observe(c,{attributes:true});
      window.offscreen2d=offscreen.getContext('2d');offscreen.width=4;offscreen.height=2;
      offscreen2d.fillStyle='#ff0000';offscreen2d.fillRect(0,0,4,2);
      const before=c.getBoundingClientRect();return [before.width,before.height,c.width,c.height];
    })()"#).unwrap(), json!([2,1,2,1]));
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate(r#"(()=>{const rect=placeholder.getBoundingClientRect();return [rect.width,rect.height,
      placeholder.getAttribute('width'),placeholder.getAttribute('height'),attributeRecords.length];})()"#).unwrap(), json!([4,2,"4","2",2]));
    let png = runtime.screenshot_unprepared_with_retained_resources((16.0,16.0),
        Some("https://graphics.example/"), (0.0,0.0), Default::default(), [255;4]).unwrap();
    let mut pixels = vec![0;16*16*4];
    assert!(obscura_render::image_pixels::decode_image_rgba(&png, &mut pixels));
    assert_eq!(&pixels[(16+2)*4..(16+2)*4+4], &[255,0,0,255]);
    assert_eq!(&pixels[(16+5)*4..(16+5)*4+4], &[255,255,255,255]);
    assert_eq!(runtime.evaluate(r#"(()=>{
      const pixels=placeholder.toDataURL(),before=Array.from(offscreen2d.getImageData(0,0,1,1).data);window.presentedBeforeZero=pixels;
      placeholder.setAttribute('width','90');
      return [placeholder.width,offscreen.width,placeholder.toDataURL()===pixels,
        Array.from(offscreen2d.getImageData(0,0,1,1).data),before];
    })()"#).unwrap(),json!([90,4,true,[255,0,0,255],[255,0,0,255]]));
    runtime.execute_script("<fixture-setup>", "offscreen.width=0").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate(r#"(()=>{const r=placeholder.getBoundingClientRect();return [r.width,r.height,
      placeholder.width,placeholder.toDataURL()===presentedBeforeZero,attributeRecords.length];})()"#).unwrap(), json!([90,2,90,true,3]));
    assert_eq!(runtime.state.borrow().webgl_surfaces.len(), 1);
    runtime.set_dom(parse_html("<body></body>"));
    assert!(runtime.state.borrow().webgl_surfaces.is_empty());
    assert!(runtime.state.borrow().canvas_surfaces.is_empty());
}

async fn check_empty_placeholder_frames(modes: &[&str]) {
    let mut runtime = page();
    for mode in modes {
        for axis in ["width", "height"] {
            runtime.evaluate(&format!(r#"(()=>{{
              const c=document.getElementById('c');c.width=2;c.height=1;
              const off=c.transferControlToOffscreen(),ctx=off.getContext('{mode}',{{antialias:false,preserveDrawingBuffer:true}});
              if(!ctx)throw Error('Context unavailable');
              const records=[];new MutationObserver(batch=>records.push(...batch.map(r=>r.attributeName))).observe(c,{{attributes:true}});
              window.trial={{c,off,ctx,records,axis:'{axis}',mode:'{mode}'}};
              off.width=4;off.height=2;
              if('{mode}'==='2d'){{ctx.fillStyle='#ff0000';ctx.fillRect(0,0,4,2);}}
              else{{ctx.clearColor(1,0,0,1);ctx.clear(ctx.COLOR_BUFFER_BIT);}}
            }})()"#)).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
            assert_eq!(runtime.evaluate(r#"(()=>{const {c,records}=trial;trial.png=c.toDataURL();return c.width===4&&c.height===2&&records.join(',')==='width,height';})()"#).unwrap(),json!(true));
            assert_eq!(runtime.state.borrow().webgl_surfaces.len(), 1);
            let original = runtime.state.borrow().webgl_surfaces.values().next().unwrap().clone();
            assert_eq!((original.0, original.1, original.2.len()), (4, 2, 32));
            runtime.execute_script("<fixture-zero>", "trial.c.setAttribute(trial.axis,'90');trial.off[trial.axis]=0").unwrap();
            for draw_while_zero in [false, true] {
                if draw_while_zero {
                    runtime.execute_script("<fixture-zero-draw>", "if(trial.mode==='2d'){trial.ctx.fillStyle='#0000ff';trial.ctx.fillRect(0,0,4,2);}else{trial.ctx.clearColor(0,0,1,1);trial.ctx.clear(trial.ctx.COLOR_BUFFER_BIT);}").unwrap();
                }
                tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
                assert_eq!(runtime.evaluate(r#"(()=>{const {c,off,png,records,axis,ctx,mode}=trial;return off[axis]===0&&c[axis]===90&&c.toDataURL()===png&&records.join(',')==='width,height,'+axis&&(mode==='2d'||ctx.getError()===0);})()"#).unwrap(),json!(true));
                assert_eq!(runtime.state.borrow().webgl_surfaces.values().next().unwrap(), &original);
            }
            runtime.execute_script("<fixture-resume>", "trial.off[trial.axis]=3").unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
            assert_eq!(runtime.evaluate(r#"(()=>{const {c,off,png,records,axis}=trial;return c.width===off.width&&c.height===off.height&&c[axis]===3&&c.toDataURL()!==png&&records.join(',')==='width,height,'+axis+',width,height';})()"#).unwrap(),json!(true));
            let restored = runtime.state.borrow().webgl_surfaces.values().next().unwrap().clone();
            assert_eq!((restored.0,restored.1), if axis=="width" {(3,2)} else {(4,3)});
            assert!(restored.2.iter().all(|value| *value==0));
            runtime.execute_script("<fixture-blue>", "if(trial.mode==='2d'){trial.ctx.fillStyle='#0000ff';trial.ctx.fillRect(0,0,trial.off.width,trial.off.height);}else{trial.ctx.clearColor(0,0,1,1);trial.ctx.clear(trial.ctx.COLOR_BUFFER_BIT);}").unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
            assert_eq!(runtime.evaluate("trial.records.length===5").unwrap(),json!(true));
            assert!(runtime.state.borrow().webgl_surfaces.values().next().unwrap().2.chunks_exact(4).all(|rgba| rgba==[0,0,255,255]));
            runtime.set_dom(parse_html("<canvas id=c width=4 height=3></canvas>"));
            runtime.run_page_init();
            assert!(runtime.state.borrow().webgl_surfaces.is_empty());
            assert!(runtime.state.borrow().webgl.entries.is_empty());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn empty_cpu_placeholder_frames_preserve_pixels_and_resume_on_nonzero_resize() {
    check_empty_placeholder_frames(&["2d"]).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory actual WebGL zero-sized placeholder retention and resumption"]
async fn real_empty_webgl_placeholder_frames_preserve_pixels_and_resume_on_nonzero_resize() {
    require_driver();
    check_empty_placeholder_frames(&["webgl", "webgl2"]).await;
}

#[tokio::test(flavor = "current_thread")]
async fn placeholder_blobs_use_a_presented_snapshot_and_private_dom_context_ownership() {
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{window.c=document.getElementById('c');c.width=1;c.height=1;
      window.off=c.transferControlToOffscreen();window.ctx=off.getContext('2d');
      ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);})()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    runtime.evaluate(r#"(()=>{window.blobPixels=null;c.toBlob(async blob=>{
      const bitmap=await createImageBitmap(blob);const target=document.createElement('canvas');
      target.width=1;target.height=1;target._ctx={pageOwned:true};const context=target.getContext('2d');
      context.drawImage(bitmap,0,0);let rejected=false;try{target.transferControlToOffscreen();}catch(e){rejected=e.name==='InvalidStateError';}
      blobPixels=[blob.type,Array.from(context.getImageData(0,0,1,1).data),target._ctx.pageOwned,rejected];
    },'image/jpeg');ctx.fillStyle='#0000ff';ctx.fillRect(0,0,1,1);})()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("blobPixels").unwrap(), json!(["image/png",[255,0,0,255],true,true]));
}

#[tokio::test(flavor = "current_thread")]
async fn placeholder_navigation_and_frame_teardown_release_snapshots_and_pending_tasks() {
    let mut runtime = page();
    let frame = crate::frame::FrameRealm::new(&mut runtime, 95, 0,
        "https://graphics.example/frame", "<canvas id=c width=2 height=1></canvas>").unwrap();
    frame.execute_script(&mut runtime,"const off=document.getElementById('c').transferControlToOffscreen();const ctx=off.getContext('2d');ctx.fillRect(0,0,2,1);").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    let retained = {
        let op_state = runtime.js_runtime.op_state();
        let op_state = op_state.borrow();
        crate::ops::frame_state(&op_state,95)
    };
    assert_eq!(retained.borrow().webgl_surfaces.len(),1);
    assert!(runtime.state.borrow().webgl_surfaces.is_empty());
    runtime.evaluate("window.latePlaceholderBlob=0").unwrap();
    frame.execute_script(&mut runtime,"off.width=3;ctx.fillRect(0,0,3,1);document.getElementById('c').toBlob(()=>parent.latePlaceholderBlob++);").unwrap();
    drop(frame);
    assert!(retained.borrow().webgl_surfaces.is_empty());
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert!(retained.borrow().webgl_surfaces.is_empty());
    assert_eq!(runtime.evaluate("latePlaceholderBlob").unwrap().as_f64(), Some(0.0));
    for _ in 0..3 {
        runtime.execute_script("<fixture-setup>", "window.oldPlaceholder=document.getElementById('c');window.oldOffscreen=oldPlaceholder.transferControlToOffscreen();oldOffscreen.getContext('2d').fillRect(0,0,1,1)").unwrap();
        runtime.set_dom(parse_html("<canvas id=c width=3 height=2></canvas>"));
        runtime.run_page_init();
        tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
        assert!(runtime.state.borrow().webgl_surfaces.is_empty());
        assert_eq!(runtime.evaluate("[document.getElementById('c')===oldPlaceholder,!!document.getElementById('c').getContext('2d')]").unwrap(),json!([false,true]));
        runtime.set_dom(parse_html("<canvas id=c width=3 height=2></canvas>"));
        runtime.run_page_init();
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory actual WebGL 1/2 placeholder presentation and drawing-buffer preservation"]
async fn real_placeholder_webgl_presents_once_and_preserves_last_frame_after_loss() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{window.presentations=[];
      for(const version of [1,2])for(const preserveDrawingBuffer of [false,true]){
        const c=document.createElement('canvas');c.width=2;c.height=2;document.body.appendChild(c);
        const off=c.transferControlToOffscreen();const gl=off.getContext(version===1?'webgl':'webgl2',{preserveDrawingBuffer,antialias:false});
        if(!gl)throw Error('WebGL unavailable');gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);
        presentations.push({c,off,gl,preserveDrawingBuffer});
      }})()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.state.borrow().webgl_surfaces.len(),4);
    assert!(runtime.state.borrow().webgl.entries.values().all(|e| e.node.is_none()&&!e.context.dirty));
    assert_eq!(runtime.evaluate(r#"presentations.map(item=>{
      const {c,gl,preserveDrawingBuffer}=item,copy=new OffscreenCanvas(2,2),ctx=copy.getContext('2d');ctx.drawImage(c,0,0);
      const pixels=new Uint8Array(4);gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
      item.png=c.toDataURL();c._ctx={};const expected=preserveDrawingBuffer?[255,0,0,255]:[0,0,0,0];
      const result=[Array.from(ctx.getImageData(0,0,1,1).data),Array.from(pixels).every((v,i)=>v===expected[i]),gl.getError()];
      gl.getExtension('WEBGL_lose_context').loseContext();return result;
    })"#).unwrap(),json!([[[255,0,0,255],true,0],[[255,0,0,255],true,0],[[255,0,0,255],true,0],[[255,0,0,255],true,0]]));
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("presentations.map(({c,png})=>c.toDataURL()===png)").unwrap(),json!([true,true,true,true]));
    runtime.set_dom(parse_html("<body></body>"));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
    assert!(runtime.state.borrow().webgl_surfaces.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn placeholder_missing_backend_retains_transfer_ownership_and_cpu_fallback() {
    unsafe {
        std::env::set_var("OBSCURA_WEBGL_LIB_DIR", "/obscura-test-missing-graphics-bundle");
        std::env::set_var("OBSCURA_WEBGL_BACKEND", "auto");
    }
    let mut runtime = page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      window.placeholder=document.getElementById('c');placeholder.width=1;placeholder.height=1;
      window.offscreen=placeholder.transferControlToOffscreen();
      const first=offscreen.getContext('webgl'),second=offscreen.getContext('webgl2');
      const ctx=offscreen.getContext('2d');ctx.fillStyle='#00ff00';ctx.fillRect(0,0,1,1);
      const rejected=fn=>{try{fn();return false;}catch(e){return e.name==='InvalidStateError';}};
      return [first,second,rejected(()=>placeholder.getContext('2d')),
        rejected(()=>placeholder.transferControlToOffscreen()),rejected(()=>{placeholder.width=2;})];
    })()"#).unwrap(),json!([null,null,true,true,true]));
    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const ctx=new OffscreenCanvas(1,1).getContext('2d');ctx.drawImage(placeholder,0,0);
      return Array.from(ctx.getImageData(0,0,1,1).data);
    })()"#).unwrap(),json!([0,255,0,255]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
    assert_eq!(runtime.state.borrow().webgl_surfaces.len(),1);
    runtime.set_dom(parse_html("<body></body>"));
    assert!(runtime.state.borrow().webgl_surfaces.is_empty());
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real V8 GC and WebGL placeholder snapshot ownership"]
async fn real_placeholder_keeps_presented_pixels_after_its_gl_owner_is_collected() {
    require_driver();
    let mut runtime = page();
    for version in [1,2] {
        runtime.evaluate(&format!(r#"(()=>{{
          window.placeholder=document.getElementById('c');placeholder.width=2;placeholder.height=2;
          window.offscreen=placeholder.transferControlToOffscreen();
          const gl=offscreen.getContext('{}',{{antialias:false}});
          if(!gl)throw Error('WebGL unavailable');gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);
        }})()"#,if version==1 { "webgl" } else { "webgl2" })).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop()).await.unwrap().unwrap();
        assert_eq!(runtime.state.borrow().webgl.entries.len(),1);
        assert_eq!(runtime.state.borrow().webgl_surfaces.len(),1);
        runtime.execute_script("<fixture-setup>", "window.presentedPNG=placeholder.toDataURL();window.offscreen=null").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                {
                    let mut entered = runtime.runtime();
                    entered.v8_isolate().clear_kept_objects();
                    entered.v8_isolate().low_memory_notification();
                }
                runtime.run_event_loop_bounded(10).await.unwrap();
                if runtime.state.borrow().webgl.entries.is_empty() { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("HTML placeholder retained its offscreen GL owner");
        assert_eq!(runtime.state.borrow().webgl_surfaces.len(),1);
        assert_eq!(runtime.evaluate(r#"(()=>{
          const ctx=new OffscreenCanvas(2,2).getContext('2d');ctx.drawImage(placeholder,0,0);
          return [placeholder.toDataURL()===presentedPNG,Array.from(ctx.getImageData(0,0,1,1).data)];
        })()"#).unwrap(),json!([true,[255,0,0,255]]));
        runtime.set_dom(parse_html("<canvas id=c></canvas>"));
        runtime.run_page_init();
        assert!(runtime.state.borrow().webgl_surfaces.is_empty());
    }
}

#[test]
#[ignore = "mandatory real ANGLE retained-wrapper queries after document retirement"]
fn real_retired_webgl_wrappers_keep_loss_returns_without_native_entries() {
    require_driver();
    let mut runtime = page();
    runtime.evaluate(r#"(()=>{
      window.retiredContexts=[1,2].map(version=>{
        const canvas=new OffscreenCanvas(2,2),gl=canvas.getContext(version===1?'webgl':'webgl2');
        if(!gl)throw Error('WebGL unavailable');
        return {gl,program:gl.createProgram(),version};
      });
    })()"#).unwrap();
    assert_eq!(runtime.state.borrow().webgl.entries.len(), 2);
    runtime.set_dom(parse_html("<canvas id=c width=2 height=2></canvas>"));runtime.run_page_init();
    assert!(runtime.state.borrow().webgl.entries.is_empty());
    assert_eq!(runtime.evaluate(r#"retiredContexts.map(({gl,program,version})=>[
      gl.getError(),gl.getError(),gl.isContextLost(),gl.isEnabled(gl.BLEND),
      gl.checkFramebufferStatus(0),gl.getVertexAttribOffset(0,0),gl.getAttribLocation(program,'position'),
      gl.getParameter(gl.VERSION),gl.getContextAttributes(),gl.getSupportedExtensions(),gl.createBuffer(),
      version===2?[gl.getFragDataLocation(program,'color'),gl.getUniformBlockIndex(program,'Block')]:null,
      gl.getError()])"#).unwrap(), json!([
        [37442,0,true,false,36061,0,-1,null,null,null,null,null,0],
        [37442,0,true,false,36061,0,-1,null,null,null,null,[-1,0],0]
    ]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
}

#[test]
fn image_data_private_color_state_and_canvas_conversion_need_no_graphics_context() {
    let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const source=new ImageData(new Uint8ClampedArray([128,64,32,255]),1,1,{colorSpace:'display-p3'});
      const keys=[Object.getOwnPropertyNames(source),Object.getOwnPropertySymbols(source).length];
      const canvas=new OffscreenCanvas(1,1),ctx=canvas.getContext('2d');ctx.putImageData(source,0,0);
      const output=ctx.getImageData(0,0,1,1),blank=ctx.createImageData(source);
      Object.defineProperty(source,'data',{get(){throw Error('page data getter');}});
      Object.defineProperty(source,'colorSpace',{get(){throw Error('page color getter');}});
      ctx.putImageData(source,0,0);
      return [keys,Array.from(output.data),output instanceof ImageData,blank.colorSpace,Array.from(blank.data),
        Object.getOwnPropertyDescriptor(ImageData.prototype,'data').set===undefined,
        Reflect.ownKeys(source).includes('obscura.graphics.image_data'),Array.from(ctx.getImageData(0,0,1,1).data)];
    })()"#).unwrap(),json!([[[],0],[138,59,21,255],true,"display-p3",[0,0,0,0],true,false,[138,59,21,255]]));
    assert!(runtime.state.borrow().webgl.entries.is_empty());
}

#[test]
fn image_data_private_brand_survives_a_same_origin_realm_boundary() {
    let mut runtime=page();
    let frame=crate::frame::FrameRealm::new(&mut runtime,71,0,"https://graphics.example/frame","<html><body></body></html>").unwrap();
    frame.execute_script(&mut runtime, "window.foreignPixels=new ImageData(new Uint8ClampedArray([128,64,32,255]),1,1,{colorSpace:'display-p3'});Object.defineProperty(foreignPixels,'data',{get(){throw Error('page getter');}})").unwrap();
    // Consume the child's actual published object. The inherited child
    // `parent` messaging stub does not publish arbitrary parent properties.
    assert_eq!(runtime.evaluate(r#"(()=>{
      const child=globalThis.__obscura_frameObjects[71].window,image=child.foreignPixels;
      const foreignRealm=child.ImageData!==ImageData && image instanceof child.ImageData && !(image instanceof ImageData);
      const ctx=new OffscreenCanvas(1,1).getContext('2d');ctx.putImageData(image,0,0);
      const getter=Object.getOwnPropertyDescriptor(ImageData.prototype,'colorSpace').get;
      return [foreignRealm,getter.call(image),Array.from(ctx.getImageData(0,0,1,1).data),
        Object.getOwnPropertyNames(image),Object.getOwnPropertySymbols(image).length];
    })()"#).unwrap(),json!([true,"display-p3",[138,59,21,255],["data"],0]));
}

#[test]
#[ignore = "mandatory real-driver ImageData brand, raw source gamut and unpack color conversion"]
fn real_image_data_texture_upload_preserves_private_source_metadata() {
    require_driver();let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const results=[];
      for(const type of ['webgl','webgl2']){
        const gl=new OffscreenCanvas(1,1).getContext(type,{antialias:false});if(!gl)throw Error('missing graphics');
        const texture=gl.createTexture(),fbo=gl.createFramebuffer();gl.bindTexture(gl.TEXTURE_2D,texture);gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);
        const image=new ImageData(new Uint8ClampedArray([255,0,0,255]),1,1,{colorSpace:'srgb'});
        Object.defineProperty(image,'colorSpace',{value:'display-p3'});Object.defineProperty(image,'width',{value:999});gl.unpackColorSpace='display-p3';
        gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,image);
        gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,texture,0);
        const output=new Uint8Array(4);gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,output);results.push([Array.from(output),gl.getError()]);
      }
      return results;
    })()"#).unwrap(),json!([[[234,51,35,255],0],[[234,51,35,255],0]]));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real-driver placeholder source gamut, display conversion and retained snapshots after loss"]
async fn real_placeholder_color_preserves_source_gamut_across_presentation_and_loss() {
    require_driver();let mut runtime=page();
    runtime.evaluate(r#"(()=>{window.widePlaceholders=[];
      for(const type of ['webgl','webgl2']){
        const canvas=document.createElement('canvas');canvas.width=1;canvas.height=1;document.body.appendChild(canvas);
        const offscreen=canvas.transferControlToOffscreen(),gl=offscreen.getContext(type,{antialias:false});if(!gl)throw Error('Missing WebGL');
        gl.drawingBufferColorSpace='display-p3';gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);widePlaceholders.push({canvas,gl,type});
      }
    })()"#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2),runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate(r#"widePlaceholders.map(item=>{
      const {canvas,gl,type}=item,destination=new OffscreenCanvas(1,1).getContext(type,{antialias:false});if(!destination)throw Error('Missing destination');
      const texture=destination.createTexture(),fbo=destination.createFramebuffer();destination.bindTexture(destination.TEXTURE_2D,texture);destination.bindFramebuffer(destination.FRAMEBUFFER,fbo);
      destination.unpackColorSpace='display-p3';destination.texImage2D(destination.TEXTURE_2D,0,destination.RGBA,destination.RGBA,destination.UNSIGNED_BYTE,canvas);
      destination.framebufferTexture2D(destination.FRAMEBUFFER,destination.COLOR_ATTACHMENT0,destination.TEXTURE_2D,texture,0);
      const raw=new Uint8Array(4);destination.readPixels(0,0,1,1,destination.RGBA,destination.UNSIGNED_BYTE,raw);
      const ctx=new OffscreenCanvas(1,1).getContext('2d');ctx.drawImage(canvas,0,0);item.png=canvas.toDataURL();
      gl.getExtension('WEBGL_lose_context').loseContext();
      return [Array.from(raw),Array.from(ctx.getImageData(0,0,1,1).data),destination.getError()];
    })"#).unwrap(),json!([[[255,0,0,255],[255,0,0,255],0],[[255,0,0,255],[255,0,0,255],0]]));
    tokio::time::timeout(std::time::Duration::from_secs(2),runtime.run_event_loop()).await.unwrap().unwrap();
    assert_eq!(runtime.evaluate("widePlaceholders.map(item=>item.canvas.toDataURL()===item.png)").unwrap(),json!([true,true]));
    assert!(runtime.state.borrow().webgl_surfaces.values().all(|(_,_,pixels)|pixels==&[255,0,0,255]));
    runtime.set_dom(parse_html("<body></body>"));assert!(runtime.state.borrow().webgl_surfaces.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn placeholder_publication_waits_for_a_frame_and_preserves_author_attributes_until_damage() {
    let mut runtime=page();
    runtime.execute_script("<placeholder-frame-fixture>",r#"
      window.frameResults=null;
      (async()=>{
        const results=[];
        for(const mode of ['none','context-only','draw']){
          const c=document.createElement('canvas');c.width=13;c.height=7;document.body.appendChild(c);
          const off=c.transferControlToOffscreen(),records=[];
          const observer=new MutationObserver(batch=>records.push(...batch.map(r=>[r.attributeName,r.oldValue])));
          observer.observe(c,{attributes:true,attributeOldValue:true});
          const ctx=mode==='none'?null:off.getContext('2d');
          const checkpoints=[];
          const snapshot=()=>[c.width,c.height,off.width,off.height,records.slice()];
          off.width=30;off.height=10;if(mode==='draw')ctx.fillRect(0,0,1,1);
          checkpoints.push(snapshot());await Promise.resolve();checkpoints.push(snapshot());
          await new Promise(resolve=>setTimeout(resolve,0));checkpoints.push(snapshot());
          await new Promise(requestAnimationFrame);checkpoints.push(snapshot());
          c.setAttribute('width','90');await Promise.resolve();checkpoints.push(snapshot());
          if(ctx)ctx.fillRect(0,0,1,1);
          await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);checkpoints.push(snapshot());
          if(ctx){off.width=30;await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);}
          checkpoints.push(snapshot());
          let setter;try{c.width=91;setter='allowed';}catch(e){setter=e.name;}
          results.push([mode,checkpoints,setter]);observer.disconnect();
        }
        frameResults=results;
      })().catch(error=>{frameResults=String(error);});
    "#).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3),runtime.run_event_loop()).await.unwrap().unwrap();
    let mut expected=Vec::new();
    for mode in ["none","context-only","draw"] {
        let initial=json!([13,7,30,10,[]]);
        let first=if mode=="none" {initial.clone()} else {json!([30,10,30,10,[["width","13"],["height","7"]]])};
        let author=if mode=="none" {json!([90,7,30,10,[["width","13"]]])}
          else {json!([90,10,30,10,[["width","13"],["height","7"],["width","30"]]])};
        let redrawn=if mode=="none" {author.clone()}
          else {json!([30,10,30,10,[["width","13"],["height","7"],["width","30"],["width","90"],["height","10"]]])};
        expected.push(json!([mode,[initial.clone(),initial.clone(),initial,first,author,redrawn.clone(),redrawn],"InvalidStateError"]));
    }
    assert_eq!(runtime.evaluate("frameResults").unwrap(),json!(expected));
}

#[test]
#[ignore = "mandatory real-driver bufferData overload conversion and exact range dispatch"]
fn real_buffer_data_overloads_convert_numeric_values_and_preserve_buffer_sources() {
    require_driver();let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const check=(ok,message)=>{if(!ok)throw Error(message);};
      for(const version of [1,2]){
        const canvas=document.createElement('canvas'),gl=canvas.getContext(version===1?'webgl':'webgl2');
        check(!!gl,'context');
        const buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,buffer);
        for(const [value,size] of [[4,4],[5.8,5],['4',4],['5.8',5],[[42],42],[[42,64],0],[{},0],[true,1],[NaN,0],[Infinity,0],[{valueOf(){return 6;}},6]]){
          gl.bufferData(gl.ARRAY_BUFFER,value,gl.STATIC_DRAW);
          check(gl.getError()===0,'numeric error');check(gl.getBufferParameter(gl.ARRAY_BUFFER,gl.BUFFER_SIZE)===size,'numeric size');
        }
        const failure=new RangeError('conversion');let caught=null;
        try{gl.bufferData(gl.ARRAY_BUFFER,{valueOf(){throw failure;}},gl.STATIC_DRAW);}catch(error){caught=error;}
        check(caught===failure,'coercion exception');
        for(const value of [Symbol(),4n]){let type=null;try{gl.bufferData(gl.ARRAY_BUFFER,value,gl.STATIC_DRAW);}catch(error){type=error.name;}check(type==='TypeError','numeric TypeError');}
        for(const value of [null,undefined,-4]){gl.bufferData(gl.ARRAY_BUFFER,value,gl.STATIC_DRAW);check(gl.getError()===gl.INVALID_VALUE,'null/negative');}
        const bytes=new Uint8Array([9,8,7,6]);
        const sources=[bytes.buffer,bytes,new DataView(bytes.buffer)];
        if(typeof SharedArrayBuffer==='function'){
          const shared=new SharedArrayBuffer(4);new Uint8Array(shared).set(bytes);sources.push(shared,new Uint8Array(shared),new DataView(shared));
        }
        for(const source of sources){
          gl.bufferData(gl.ARRAY_BUFFER,source,gl.STATIC_DRAW);
          check(gl.getBufferParameter(gl.ARRAY_BUFFER,gl.BUFFER_SIZE)===4,'source size');
          gl.bufferSubData(gl.ARRAY_BUFFER,0,source);check(gl.getError()===0,'source upload');
          if(version===2){const output=new Uint8Array(4);gl.getBufferSubData(gl.ARRAY_BUFFER,0,output);check(output.join(',')==='9,8,7,6','source bytes');}
        }
        if(version===1){
          for(const source of [4,'4',bytes.buffer,bytes]){gl.bufferData(gl.ARRAY_BUFFER,source,gl.STATIC_DRAW,1,1);check(gl.getBufferParameter(gl.ARRAY_BUFFER,gl.BUFFER_SIZE)===4,'ignored range');}
        }else{
          for(const source of [4,'4',bytes.buffer,null,undefined]){let type=null;try{gl.bufferData(gl.ARRAY_BUFFER,source,gl.STATIC_DRAW,undefined);}catch(error){type=error.name;}check(type==='TypeError','range requires view');}
          gl.bufferData(gl.ARRAY_BUFFER,bytes,gl.STATIC_DRAW,1,2);const output=new Uint8Array(2);gl.getBufferSubData(gl.ARRAY_BUFFER,0,output);check(output.join(',')==='8,7','range bytes');
        }
        if(typeof SharedArrayBuffer==='function'){
          const shared=new Uint8Array(new SharedArrayBuffer(48));shared.fill(19);
          if(version===2){
            gl.bufferData(gl.ARRAY_BUFFER,bytes,gl.STATIC_DRAW);
            gl.getBufferSubData(gl.ARRAY_BUFFER,0,shared,4,4);
            check(shared.slice(4,8).join(',')==='9,8,7,6'&&shared[3]===19&&shared[8]===19,'shared buffer copyback range');
            shared.fill(19);gl.getBufferSubData(gl.ARRAY_BUFFER,99,shared,4,4);
            check(gl.getError()===gl.INVALID_VALUE&&shared.every(n=>n===19),'rejected shared buffer read');
            gl.pixelStorei(gl.PACK_ROW_LENGTH,3);gl.pixelStorei(gl.PACK_SKIP_PIXELS,1);gl.pixelStorei(gl.PACK_SKIP_ROWS,1);
          }
          gl.clearColor(1,0,0,1);gl.clear(gl.COLOR_BUFFER_BIT);gl.enable(0xdead);
          gl.readPixels(0,0,2,2,gl.RGBA,gl.UNSIGNED_BYTE,shared);
          check(gl.getError()===gl.INVALID_ENUM&&gl.getError()===0,'shared read preserves previous error');
          const start=version===2?16:0,stride=version===2?12:8;
          for(let i=0;i<48;i++){
            const row=i>=start&&i<start+8?i-start:i>=start+stride&&i<start+stride+8?i-start-stride:-1;
            check(shared[i]===(row<0?19:row%4===0||row%4===3?255:0),'shared packed pixel range');
          }
          shared.fill(19);gl.readPixels(0,0,2,2,0xdead,gl.UNSIGNED_BYTE,shared);
          check(gl.getError()!==0&&shared.every(n=>n===19),'rejected shared pixel read');
          const incomplete=gl.createFramebuffer();gl.bindFramebuffer(gl.FRAMEBUFFER,incomplete);
          gl.readPixels(0,0,2,2,gl.RGBA,gl.UNSIGNED_BYTE,shared);
          check(gl.getError()===gl.INVALID_FRAMEBUFFER_OPERATION&&shared.every(n=>n===19),'native rejected shared pixel read');
          gl.bindFramebuffer(gl.FRAMEBUFFER,null);gl.deleteFramebuffer(incomplete);
          if(version===2){gl.pixelStorei(gl.PACK_ROW_LENGTH,0);gl.pixelStorei(gl.PACK_SKIP_PIXELS,0);gl.pixelStorei(gl.PACK_SKIP_ROWS,0);}
        }
        check(gl.getError()===0,'final GL error');gl.deleteBuffer(buffer);gl.getExtension('WEBGL_lose_context').loseContext();
      }
      return true;
    })()"#).unwrap(),json!(true));
}

#[test]
#[ignore = "mandatory real-driver WebGL1 extension and WebGL2 core instancing with unused attributes"]
fn real_instanced_draws_use_version_correct_entry_points_and_keep_attribute_validation() {
    require_driver();let mut runtime=page();
    assert_eq!(runtime.evaluate(r#"(()=>{
      const check=(ok,message)=>{if(!ok)throw Error(message);};
      for(const version of [1,2]){
        const canvas=document.createElement('canvas');canvas.width=canvas.height=4;
        const gl=canvas.getContext(version===1?'webgl':'webgl2',{antialias:false,preserveDrawingBuffer:true});check(!!gl,'context');
        const program=gl.createProgram(),shaders=[];
        for(const [type,source] of [[gl.VERTEX_SHADER,version===1?'attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}':'#version 300 es\nin vec2 p;void main(){gl_Position=vec4(p,0.,1.);}'],[gl.FRAGMENT_SHADER,version===1?'precision mediump float;void main(){gl_FragColor=vec4(1.,0.,0.,1.);}':'#version 300 es\nprecision mediump float;out vec4 color;void main(){color=vec4(1.,0.,0.,1.);}']]){
          const shader=gl.createShader(type);gl.shaderSource(shader,source);gl.compileShader(shader);check(gl.getShaderParameter(shader,gl.COMPILE_STATUS),gl.getShaderInfoLog(shader));gl.attachShader(program,shader);shaders.push(shader);
        }
        gl.bindAttribLocation(program,0,'p');gl.linkProgram(program);check(gl.getProgramParameter(program,gl.LINK_STATUS),gl.getProgramInfoLog(program));gl.useProgram(program);
        const vertex=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,vertex);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,3,-1,-1,3]),gl.STATIC_DRAW);gl.vertexAttribPointer(0,2,gl.FLOAT,false,0,0);gl.enableVertexAttribArray(0);
        const index=gl.createBuffer();gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,index);gl.bufferData(gl.ELEMENT_ARRAY_BUFFER,new Uint16Array([999,0,1,2]),gl.STATIC_DRAW);
        const ext=version===1?gl.getExtension('ANGLE_instanced_arrays'):null;check(version===2||!!ext,'instancing extension');
        const divisor=(i,n)=>version===1?ext.vertexAttribDivisorANGLE(i,n):gl.vertexAttribDivisor(i,n);
        divisor(0,1);check(gl.getVertexAttrib(0,0x88fe)===1,'divisor set');divisor(0,0);check(gl.getVertexAttrib(0,0x88fe)===0,'divisor reset');check(gl.getError()===0,'divisor error');
        const draws=[()=>version===1?ext.drawArraysInstancedANGLE(gl.TRIANGLES,0,3,1):gl.drawArraysInstanced(gl.TRIANGLES,0,3,1),()=>version===1?ext.drawElementsInstancedANGLE(gl.TRIANGLES,3,gl.UNSIGNED_SHORT,2,1):gl.drawElementsInstanced(gl.TRIANGLES,3,gl.UNSIGNED_SHORT,2,1)];
        const unused=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,unused);gl.vertexAttribPointer(1,2,gl.FLOAT,false,0,0);
        for(const enabled of [false,true]){
          if(enabled)gl.enableVertexAttribArray(1);
          for(const draw of draws){
            gl.clearColor(0,0,0,0);gl.clear(gl.COLOR_BUFFER_BIT);draw();check(gl.getError()===0,'instanced draw error');
            const pixel=new Uint8Array(4);gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);check(pixel.join(',')==='255,0,0,255','instanced pixels');
          }
        }
        divisor(gl.getParameter(gl.MAX_VERTEX_ATTRIBS),0);check(gl.getError()===gl.INVALID_VALUE,'invalid divisor index');
        const bad=()=>version===1?ext.drawArraysInstancedANGLE(gl.TRIANGLES,0,-1,1):gl.drawArraysInstanced(gl.TRIANGLES,0,-1,1);bad();check(gl.getError()===gl.INVALID_VALUE,'negative count');
        gl.disableVertexAttribArray(1);gl.deleteBuffer(unused);gl.deleteBuffer(index);gl.deleteBuffer(vertex);gl.useProgram(null);gl.deleteProgram(program);for(const shader of shaders)gl.deleteShader(shader);
        check(gl.getError()===0,'cleanup');gl.getExtension('WEBGL_lose_context').loseContext();
      }
      return true;
    })()"#).unwrap(),json!(true));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "mandatory real-driver failIfMajorPerformanceCaveat after a normal context, within and across documents"]
async fn real_caveat_context_after_a_normal_context_follows_the_backing_device() {
    require_driver();
    // Automatic selection is the mode that falls back to SwiftShader on a
    // GPU-less host. nextest runs each test in its own process, and the
    // previous value is restored even if an assertion fails.
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(value) => std::env::set_var("OBSCURA_WEBGL_BACKEND", value),
                    None => std::env::remove_var("OBSCURA_WEBGL_BACKEND"),
                }
            }
        }
    }
    let _restore = Restore(std::env::var_os("OBSCURA_WEBGL_BACKEND"));
    unsafe {
        std::env::set_var("OBSCURA_WEBGL_BACKEND", "auto");
    }
    const PROBE: &str = r#"window.events=[];window.keep=[];
      window.probe=(label,kind,caveat)=>{const c=document.createElement('canvas');c.width=c.height=2;
        c.addEventListener('webglcontextcreationerror',e=>events.push([label,e.type,e instanceof WebGLContextEvent,typeof e.statusMessage==='string'&&e.statusMessage.length>0]));
        const gl=c.getContext(kind,{antialias:false,failIfMajorPerformanceCaveat:caveat});if(gl)keep.push(gl);
        return [label,gl?gl.getContextAttributes().failIfMajorPerformanceCaveat:null];};"#;
    // Every native context in the current document runs on one device.
    fn software(runtime: &ObscuraJsRuntime) -> bool {
        let state = runtime.state.borrow();
        let devices = state.webgl.entries.values()
            .map(|entry| (format!("{:?}", entry.context.diagnostics.backend), entry.context.diagnostics.renderer.contains("SwiftShader")))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(devices.len(), 1, "{devices:?}");
        let (backend, renderer) = devices.into_iter().next().unwrap();
        assert_eq!(backend == "SwiftShader", renderer, "label must match the device");
        renderer
    }
    async fn settle(runtime: &mut ObscuraJsRuntime) {
        tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
            .await
            .unwrap()
            .unwrap();
    }
    let expected = |software: bool, rows: &[(&str, bool)]| {
        let results = rows.iter().map(|&(label, caveat)| {
            json!([label, if !caveat { json!(false) } else if software { json!(null) } else { json!(true) }])
        }).collect::<Vec<_>>();
        let events = rows.iter().filter(|&&(_, caveat)| caveat && software)
            .map(|&(label, _)| json!([label, "webglcontextcreationerror", true, true]))
            .collect::<Vec<_>>();
        (json!(results), json!(events))
    };
    // One document: a normal context, then caveat contexts.
    let mut runtime = page();
    runtime.execute_script("<fixture-setup>", PROBE).unwrap();
    let first = runtime.evaluate("[probe('a-normal','webgl',false),probe('a-caveat-1','webgl',true),probe('a-caveat-2','webgl2',true)]").unwrap();
    let software_device = software(&runtime);
    settle(&mut runtime).await;
    let rows = [("a-normal", false), ("a-caveat-1", true), ("a-caveat-2", true)];
    assert_eq!((first, runtime.evaluate("events").unwrap()), expected(software_device, &rows));
    // A second document in the same process: caveat contexts before and
    // after its own normal context. The first document's contexts are gone,
    // but the process-wide displays remain initialized.
    runtime.set_dom(parse_html("<html><body><canvas id='c' width='8' height='8'></canvas></body></html>"));
    runtime.run_page_init();
    assert!(runtime.state.borrow().webgl.entries.is_empty());
    runtime.execute_script("<fixture-setup>", PROBE).unwrap();
    let second = runtime.evaluate("[probe('b-caveat-1','webgl',true),probe('b-caveat-2','webgl2',true),probe('b-normal','webgl',false),probe('b-caveat-3','webgl',true)]").unwrap();
    assert_eq!(software(&runtime), software_device);
    settle(&mut runtime).await;
    let rows = [("b-caveat-1", true), ("b-caveat-2", true), ("b-normal", false), ("b-caveat-3", true)];
    assert_eq!((second, runtime.evaluate("events").unwrap()), expected(software_device, &rows));
}
