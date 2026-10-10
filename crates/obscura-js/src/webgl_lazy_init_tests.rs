//! Native graphics initialization is lazy: a document that never requests a
//! WebGL context must not discover, verify or load the ANGLE bundle, nor create
//! an EGL display. The first context request initializes once per process and
//! later contexts, documents and realms reuse that state.
//!
//! Kept apart from `webgl_tests`, whose ignored cases form the pinned native
//! driver inventory. nextest runs every test in a fresh process, which these
//! process-wide assertions rely on.
use crate::runtime::ObscuraJsRuntime;
use obscura_dom::parse_html;
use obscura_webgl::egl::{native_initializations, NativeInitializations};
use serde_json::json;

const GRAPHICS_LIBRARIES: [&str; 4] = ["libEGL", "libGLESv2", "libvulkan", "libvk_swiftshader"];

fn page(seed: u32, stealth: bool) -> ObscuraJsRuntime {
    let mut runtime = ObscuraJsRuntime::with_base_url("https://graphics.example/");
    runtime.set_fingerprint_seed(seed);
    runtime.set_stealth(stealth);
    runtime.set_dom(parse_html(
        "<html><body style='margin:0'><canvas id='c' width='8' height='8'></canvas><img src='data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%222%22 height=%222%22/%3E'></body></html>",
    ));
    runtime.set_url("https://graphics.example/");
    runtime.run_page_init();
    runtime
}

/// Graphics libraries mapped into this process (Linux). Other platforms rely
/// on the native initialization counters alone.
fn mapped_graphics_libraries() -> Vec<String> {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return Vec::new();
    };
    let mut found: Vec<String> = maps
        .lines()
        .filter_map(|line| line.split_whitespace().nth(5))
        .filter(|path| GRAPHICS_LIBRARIES.iter().any(|name| path.contains(name)))
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
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

/// Everything an ordinary page may do with canvases, images and the WebGL
/// interface objects without requesting a WebGL context.
const ORDINARY_PAGE: &str = r#"(()=>{
  const out={};
  out.interfaces=[typeof WebGLRenderingContext,typeof WebGL2RenderingContext,typeof WebGLContextEvent,
    typeof WebGLBuffer,typeof OffscreenCanvas,typeof ImageData,typeof createImageBitmap];
  out.prototype=[Object.getOwnPropertyNames(WebGLRenderingContext.prototype).length>100,
    typeof WebGL2RenderingContext.prototype.bindVertexArray,WebGLRenderingContext.COLOR_BUFFER_BIT,
    Function.prototype.toString.call(WebGLRenderingContext.prototype.getParameter).includes('[native code]'),
    WebGLRenderingContext.prototype instanceof Object];
  try{new WebGLRenderingContext();out.illegal=false;}catch(e){out.illegal=e instanceof TypeError;}
  const canvas=document.getElementById('c'),ctx=canvas.getContext('2d');
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,4,4);ctx.font='10px sans-serif';
  out.canvas2d=[Array.from(ctx.getImageData(0,0,1,1).data),canvas.toDataURL().startsWith('data:image/png;base64,'),
    ctx.measureText('obscura').width>0,canvas.getContext('2d')===ctx,canvas.getContext('webgl')===null];
  const kinds={};
  for(const kind of ['bitmaprenderer','webgpu','unknown-context']){
    const c=document.createElement('canvas');
    try{const value=c.getContext(kind);kinds[kind]=value===null?null:typeof value;}catch(e){kinds[kind]=e.name;}
  }
  out.otherContexts=kinds;
  for(let i=0;i<64;i++){const c=document.createElement('canvas');c.width=c.height=4;c.getContext('2d').fillRect(0,0,1,1);}
  const offscreen=new OffscreenCanvas(4,2),octx=offscreen.getContext('2d');
  octx.fillStyle='#00ff00';octx.fillRect(0,0,4,2);
  const bitmap=offscreen.transferToImageBitmap();ctx.drawImage(bitmap,0,0);bitmap.close();
  out.offscreen=[Array.from(ctx.getImageData(0,0,1,1).data),new ImageData(2,2).data.length];
  out.gpu=typeof navigator.gpu?.requestAdapter;
  window.asyncResult=null;
  Promise.all([navigator.gpu.requestAdapter(),createImageBitmap(new ImageData(1,1)),offscreen.convertToBlob()])
    .then(([adapter,image,blob])=>{window.asyncResult=[adapter,image.width,blob.type];image.close();})
    .catch(e=>{window.asyncResult=e.name+':'+e.message;});
  return out;
})()"#;

fn ordinary_page_expectation() -> serde_json::Value {
    json!({
        "interfaces": ["function","function","function","function","function","function","function"],
        "prototype": [true,"function",16384,true,true],
        "illegal": true,
        "canvas2d": [[255,0,0,255],true,true,true,true],
        "offscreen": [[0,255,0,255],16],
        "gpu": "function",
    })
}

#[tokio::test(flavor = "current_thread")]
async fn pages_without_webgl_never_initialize_native_graphics() {
    assert_eq!(native_initializations(), NativeInitializations::default());
    for (seed, stealth) in [(7, true), (8, false)] {
        let mut runtime = page(seed, stealth);
        let mut value = runtime.evaluate(ORDINARY_PAGE).unwrap();
        let other_contexts = value.as_object_mut().unwrap().remove("otherContexts");
        assert_eq!(value, ordinary_page_expectation());
        assert_eq!(other_contexts.unwrap()["webgpu"], json!(null));
        tokio::time::timeout(std::time::Duration::from_secs(2), runtime.run_event_loop())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.evaluate("asyncResult").unwrap(), json!([null, 1, "image/png"]));
        // Child realm with its own canvas.
        let frame = crate::frame::FrameRealm::new(
            &mut runtime,
            41,
            0,
            "https://graphics.example/frame",
            "<canvas id=c width=2 height=2></canvas>",
        )
        .unwrap();
        frame
            .execute_script(&mut runtime, "document.getElementById('c').getContext('2d').fillRect(0,0,1,1)")
            .unwrap();
        drop(frame);
        // Paint/presentation path, which prepares WebGL surfaces when any exist.
        let png = runtime
            .screenshot_unprepared_with_retained_resources(
                (16.0, 16.0),
                Some("https://graphics.example/"),
                (0.0, 0.0),
                Default::default(),
                [255; 4],
            )
            .unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        // A navigation tears the document's graphics registry down.
        runtime.set_dom(parse_html("<html><body><canvas></canvas></body></html>"));
        runtime.run_page_init();
        assert!(runtime.state.borrow().webgl.entries.is_empty());
        assert_eq!(native_initializations(), NativeInitializations::default());
        assert_eq!(mapped_graphics_libraries(), Vec::<String>::new());
    }
    // The hook is not vacuous: the first context request starts native
    // initialization, whether or not this host has a usable graphics bundle.
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        let mut runtime = page(9, true);
        let result = runtime
            .evaluate("(()=>{const gl=document.getElementById('c').getContext('webgl');return gl===null?'null':'context';})()")
            .unwrap();
        assert!(result == json!("null") || result == json!("context"), "{result}");
        assert_eq!(native_initializations().library_loads, 1);
    }
}

#[test]
#[ignore = "real-driver lazy initialization: first context, once-per-process reuse and unchanged stealth identity"]
fn real_first_context_initializes_once_and_keeps_stealth_identity() {
    require_driver();
    const IDENTITY: &str = r#"(version=>{
      const canvas=document.createElement('canvas');canvas.width=canvas.height=4;
      const gl=canvas.getContext(version===2?'webgl2':'webgl',{antialias:false,preserveDrawingBuffer:true});
      if(!gl)throw Error('WebGL unavailable');
      gl.clearColor(0.2,0.4,0.6,1);gl.clear(gl.COLOR_BUFFER_BIT);
      const px=new Uint8Array(16*4);gl.readPixels(0,0,4,4,gl.RGBA,gl.UNSIGNED_BYTE,px);
      const ext=gl.getExtension('WEBGL_debug_renderer_info');
      return {vendor:gl.getParameter(ext.UNMASKED_VENDOR_WEBGL),renderer:gl.getParameter(ext.UNMASKED_RENDERER_WEBGL),
        version:gl.getParameter(gl.VERSION),glsl:gl.getParameter(gl.SHADING_LANGUAGE_VERSION),
        maskedVendor:gl.getParameter(gl.VENDOR),maskedRenderer:gl.getParameter(gl.RENDERER),
        pixel:Array.from(px.slice(0,4)),flat:px.every((v,i)=>v===px[i%4]),error:gl.getError()};
    })"#;
    let identity = |runtime: &mut ObscuraJsRuntime, version: u8| {
        runtime.evaluate(&format!("{IDENTITY}({version})")).unwrap()
    };
    let none = NativeInitializations::default();
    let once = NativeInitializations { library_loads: 1, display_initializations: 1 };

    let mut runtime = page(7, true);
    let mut ordinary = runtime.evaluate(ORDINARY_PAGE).unwrap();
    ordinary.as_object_mut().unwrap().remove("otherContexts");
    assert_eq!(ordinary, ordinary_page_expectation());
    assert_eq!(native_initializations(), none);
    assert_eq!(mapped_graphics_libraries(), Vec::<String>::new());

    let first = identity(&mut runtime, 1);
    assert_eq!(native_initializations(), once, "the first context initializes once");
    assert_eq!(first["pixel"], json!([51, 102, 153, 255]));
    assert_eq!(first["flat"], json!(true));
    assert_eq!(first["error"], json!(0));
    assert_eq!(first["version"], json!("WebGL 1.0 (OpenGL ES 2.0 Chromium)"));
    assert_eq!(first["glsl"], json!("WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)"));
    let (vendor, renderer) = (first["vendor"].as_str().unwrap(), first["renderer"].as_str().unwrap());
    assert!(vendor.starts_with("Google Inc. ("), "{vendor}");
    assert!(renderer.starts_with("ANGLE ("), "{renderer}");
    if cfg!(target_os = "linux") {
        assert!(!mapped_graphics_libraries().is_empty());
    }

    // Every later context, version, document and runtime reuses the display.
    let second = identity(&mut runtime, 2);
    assert_eq!((&second["vendor"], &second["renderer"]), (&first["vendor"], &first["renderer"]));
    assert_eq!(second["version"], json!("WebGL 2.0 (OpenGL ES 3.0 Chromium)"));
    assert_eq!(second["pixel"], json!([51, 102, 153, 255]));
    let offscreen = runtime
        .evaluate("(()=>{const gl=new OffscreenCanvas(2,2).getContext('webgl');const e=gl.getExtension('WEBGL_debug_renderer_info');return [gl.getParameter(e.UNMASKED_VENDOR_WEBGL),gl.getParameter(e.UNMASKED_RENDERER_WEBGL)];})()")
        .unwrap();
    assert_eq!(offscreen, json!([vendor, renderer]));
    runtime.set_dom(parse_html("<html><body></body></html>"));
    runtime.run_page_init();
    let navigated = identity(&mut runtime, 1);
    assert_eq!((&navigated["vendor"], &navigated["renderer"]), (&first["vendor"], &first["renderer"]));
    drop(runtime);
    let mut same_profile = page(7, true);
    let reused = identity(&mut same_profile, 1);
    assert_eq!((&reused["vendor"], &reused["renderer"]), (&first["vendor"], &first["renderer"]));
    assert_eq!(native_initializations(), once, "later contexts reuse the process display");
    drop(same_profile);

    // Runtime stealth off still reports the driver, so the stealth strings
    // above are the profile's, not the host's.
    let mut plain = page(7, false);
    let driver = identity(&mut plain, 1);
    assert_ne!(driver["renderer"], first["renderer"]);
    assert_eq!(native_initializations(), once);
}
