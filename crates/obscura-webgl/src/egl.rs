//! ANGLE EGL ownership. Displays/libraries are lazily shared for process life;
//! contexts and their draw surfaces belong to one thread and one canvas.
use crate::{bundle::Bundle, selection::Backend};
use libloading::Library;
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    marker::PhantomData,
    ptr,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};

// Process-wide counts of native graphics initialization attempts. Pages and
// processes that never request a WebGL context must leave both at zero.
static LIBRARY_LOADS: AtomicUsize = AtomicUsize::new(0);
static DISPLAY_INITIALIZATIONS: AtomicUsize = AtomicUsize::new(0);

/// How often this process has started native graphics initialization.
///
/// `library_loads` counts attempts to discover, verify and load the pinned
/// ANGLE bundle (at most one per process, successful or not).
/// `display_initializations` counts attempts to create and initialize an EGL
/// display for a backend that has no live display yet, including attempts
/// that fail and fall back. Both stay zero until the first context request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeInitializations {
    pub library_loads: usize,
    pub display_initializations: usize,
}

pub fn native_initializations() -> NativeInitializations {
    NativeInitializations {
        library_loads: LIBRARY_LOADS.load(Ordering::Acquire),
        display_initializations: DISPLAY_INITIALIZATIONS.load(Ordering::Acquire),
    }
}

type Handle = *mut c_void;
type Int = i32;
const NONE: Int = 0x3038;
const TRUE: Int = 1;
const FALSE: Int = 0;

macro_rules! egl_functions {
    ($($field:ident: $name:literal ($($arg:ty),*) -> $result:ty;)*) => {
        struct Functions { $( $field: unsafe extern "C" fn($($arg),*) -> $result, )* }
        impl Functions {
            unsafe fn load(library: &Library) -> Result<Self,String> {
                Ok(Self { $( $field: *library.get::<unsafe extern "C" fn($($arg),*) -> $result>(concat!($name,"\0").as_bytes()).map_err(|e|e.to_string())?, )* })
            }
        }
    }
}
egl_functions! {
    get_proc_address: "eglGetProcAddress" (*const c_char) -> *const c_void;
    initialize: "eglInitialize" (Handle,*mut Int,*mut Int) -> u32;
    terminate: "eglTerminate" (Handle) -> u32;
    bind_api: "eglBindAPI" (u32) -> u32;
    choose_config: "eglChooseConfig" (Handle,*const Int,*mut Handle,Int,*mut Int) -> u32;
    get_config_attrib: "eglGetConfigAttrib" (Handle,Handle,Int,*mut Int) -> u32;
    create_context: "eglCreateContext" (Handle,Handle,Handle,*const Int) -> Handle;
    destroy_context: "eglDestroyContext" (Handle,Handle) -> u32;
    create_pbuffer: "eglCreatePbufferSurface" (Handle,Handle,*const Int) -> Handle;
    destroy_surface: "eglDestroySurface" (Handle,Handle) -> u32;
    make_current: "eglMakeCurrent" (Handle,Handle,Handle,Handle) -> u32;
    swap_buffers: "eglSwapBuffers" (Handle,Handle) -> u32;
    query_string: "eglQueryString" (Handle,Int) -> *const c_char;
    get_error: "eglGetError" () -> Int;
}

struct Libraries {
    _egl: Library,
    _gles: Library,
    functions: Functions,
    bundle: Bundle,
}
impl Libraries {
    fn open() -> Result<Self, String> {
        LIBRARY_LOADS.fetch_add(1, Ordering::AcqRel);
        let bundle = Bundle::discover()?;
        if cfg!(target_os = "linux") {
            // ANGLE opens this dependency internally from its module directory.
            // Verify it before loading native code, just like EGL and GLES.
            bundle.verify_vulkan_loader()?;
        }
        let suffix = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let egl_path = bundle.library(&format!("libEGL.{suffix}"))?;
        let gles_path = bundle.library(&format!("libGLESv2.{suffix}"))?;
        // Only host-controlled, pinned and verified native libraries are loaded.
        unsafe {
            let gles = Library::new(gles_path).map_err(|e| format!("GLES library: {e}"))?;
            let egl = Library::new(egl_path).map_err(|e| format!("EGL library: {e}"))?;
            let functions = Functions::load(&egl)?;
            Ok(Self {
                _egl: egl,
                _gles: gles,
                functions,
                bundle,
            })
        }
    }
    fn error(&self, operation: &str) -> String {
        format!("{operation} failed (EGL 0x{:04x})", unsafe {
            (self.functions.get_error)()
        })
    }
    unsafe fn symbol(&self, name: &CStr) -> *const c_void {
        // dlsym on a GLES library may search its dependencies and return a
        // desktop GL function from the system driver. ANGLE's own procedure
        // table returns only supported entry points from the pinned build.
        (self.functions.get_proc_address)(name.as_ptr())
    }
}

struct Display {
    backend: Backend,
    libraries: Arc<Libraries>,
    handle: usize,
}
impl Display {
    fn handle(&self) -> Handle {
        self.handle as Handle
    }
    fn acquire(backend: Backend) -> Result<Arc<Self>, String> {
        static LIBRARIES: OnceLock<Result<Arc<Libraries>, String>> = OnceLock::new();
        static DISPLAYS: OnceLock<Mutex<HashMap<Backend, Arc<Display>>>> = OnceLock::new();
        let libraries = LIBRARIES
            .get_or_init(|| Libraries::open().map(Arc::new))
            .clone()?;
        let mut displays = DISPLAYS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "graphics display cache poisoned")?;
        if let Some(display) = displays.get(&backend) {
            return Ok(display.clone());
        }
        // ANGLE's Vulkan backend keeps Vulkan entry points in process-wide
        // tables and reloads them only on some calls. A second live device
        // would run another device's entry points, so keep one display.
        if let Some(active) = active_other(displays.keys().copied(), backend) {
            return Err(format!("the {active:?} WebGL display is already active in this process"));
        }
        DISPLAY_INITIALIZATIONS.fetch_add(1, Ordering::AcqRel);
        if backend == Backend::SwiftShader {
            libraries.bundle.library("libvk_swiftshader.so")?;
            libraries.bundle.library("vk_swiftshader_icd.json")?;
        }
        let attributes = display_attributes(backend);
        unsafe {
            type GetDisplay = unsafe extern "C" fn(u32, Handle, *const Int) -> Handle;
            let name = c"eglGetPlatformDisplayEXT";
            let address = (libraries.functions.get_proc_address)(name.as_ptr());
            if address.is_null() {
                return Err("ANGLE has no eglGetPlatformDisplayEXT".into());
            }
            let get_display: GetDisplay = std::mem::transmute(address);
            let handle = get_display(0x3202, ptr::null_mut(), attributes.as_ptr());
            if handle.is_null() {
                return Err(libraries.error("get ANGLE display"));
            }
            // An initialized ANGLE display ignores new attributes and
            // eglInitialize succeeds again. Never adopt, initialize or
            // terminate a display that another backend owns.
            let owned = displays.values().map(|d| (d.backend, d.handle));
            if let Some(owner) = alias_owner(owned, backend, handle as usize) {
                return Err(format!(
                    "ANGLE returned the initialized {owner:?} display for {backend:?}"
                ));
            }
            if (libraries.functions.initialize)(handle, ptr::null_mut(), ptr::null_mut()) == 0 {
                return Err(libraries.error("initialize ANGLE display"));
            }
            let extensions = (libraries.functions.query_string)(handle, 0x3055);
            let extensions = if extensions.is_null() {
                ""
            } else {
                CStr::from_ptr(extensions).to_str().unwrap_or("")
            };
            // Browser memory safety relies on robust buffer access and zeroed
            // resources. Never silently run against a non-WebGL GLES context.
            for required in [
                "EGL_ANGLE_create_context_webgl_compatibility",
                "EGL_ANGLE_robust_resource_initialization",
                "EGL_ANGLE_create_context_client_arrays",
                "EGL_ANGLE_create_context_extensions_enabled",
                "EGL_EXT_create_context_robustness",
            ] {
                if !extensions.split_ascii_whitespace().any(|e| e == required) {
                    (libraries.functions.terminate)(handle);
                    return Err(format!("ANGLE display lacks {required}"));
                }
            }
            // The Vulkan loader can expose CPU devices (SwiftShader, lavapipe)
            // to the hardware request. Only accept the device each backend
            // names; any query failure rejects the display.
            #[cfg(target_os = "linux")]
            if let Err(reason) = physical_device(&libraries, handle)
                .and_then(|device| device_policy(backend, device))
            {
                (libraries.functions.terminate)(handle);
                return Err(reason);
            }
            let display = Arc::new(Self {
                backend,
                libraries,
                handle: handle as usize,
            });
            displays.insert(backend, display.clone());
            Ok(display)
        }
    }
}

fn display_attributes(backend: Backend) -> Vec<Int> {
    let renderer = if backend == Backend::Metal {
        0x3489
    } else {
        0x3450
    };
    let device = if backend == Backend::SwiftShader {
        0x3487
    } else {
        0x320A
    };
    let mut attributes = vec![0x3203, renderer, 0x3209, device];
    if backend != Backend::Metal {
        // Explicit offscreen Vulkan platform, independent of DISPLAY/Wayland.
        attributes.extend_from_slice(&[0x348F, 0x31DD]);
        // ANGLE caches platform displays by key, and the device type is not
        // part of that key. EGL_PLATFORM_ANGLE_DISPLAY_KEY_ANGLE keeps the
        // hardware and SwiftShader displays separate.
        let key = if backend == Backend::SwiftShader { 2 } else { 1 };
        attributes.extend_from_slice(&[0x34DC, key]);
    }
    attributes.push(NONE);
    attributes
}

/// Another backend whose display this process already uses, if any.
fn active_other(active: impl IntoIterator<Item = Backend>, backend: Backend) -> Option<Backend> {
    active.into_iter().find(|&other| other != backend)
}

/// The backend that already owns `handle`, if it is not `backend`.
fn alias_owner(
    owned: impl IntoIterator<Item = (Backend, usize)>,
    backend: Backend,
    handle: usize,
) -> Option<Backend> {
    owned
        .into_iter()
        .find(|&(owner, owned)| owned == handle && owner != backend)
        .map(|(owner, _)| owner)
}

/// VkPhysicalDeviceProperties identity of the device behind a display.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct PhysicalDevice {
    vendor: u32,
    device: u32,
    kind: i32,
    name: String,
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const SWIFTSHADER_DEVICE: (u32, u32) = (0x1AE0, 0xC0DE);

/// Hardware must be a real GPU (integrated, discrete or virtual). The
/// software backend must be the bundled SwiftShader device.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn device_policy(backend: Backend, device: PhysicalDevice) -> Result<(), String> {
    let swiftshader = (device.vendor, device.device) == SWIFTSHADER_DEVICE;
    let accepted = match backend {
        Backend::Vulkan => matches!(device.kind, 1..=3) && !swiftshader,
        Backend::SwiftShader => swiftshader,
        Backend::Metal => true,
    };
    if accepted {
        return Ok(());
    }
    let kind = match device.kind {
        0 => "other",
        1 => "integrated GPU",
        2 => "discrete GPU",
        3 => "virtual GPU",
        4 => "CPU",
        _ => "unknown",
    };
    Err(format!(
        "ANGLE {backend:?} display uses an unexpected {kind} device {:?} (vendor 0x{:04x}, device 0x{:04x})",
        device.name, device.vendor, device.device
    ))
}

/// Read the Vulkan physical device ANGLE initialized for `display`, through
/// the bundled loader that ANGLE already loaded from the verified bundle.
#[cfg(target_os = "linux")]
unsafe fn physical_device(libraries: &Libraries, display: Handle) -> Result<PhysicalDevice, String> {
    type Query = unsafe extern "C" fn(Handle, Int, *mut isize) -> u32;
    type GetInstanceProcAddr = unsafe extern "C" fn(Handle, *const c_char) -> *const c_void;
    type GetProperties = unsafe extern "C" fn(Handle, *mut DeviceProperties);
    #[repr(C)]
    #[allow(dead_code)]
    struct DeviceProperties {
        api_version: u32,
        driver_version: u32,
        vendor_id: u32,
        device_id: u32,
        device_type: i32,
        device_name: [c_char; 256],
        pipeline_cache_uuid: [u8; 16],
        // VkPhysicalDeviceLimits and sparse properties (824 bytes in all).
        rest: [u64; 128],
    }
    // glibc and musl on every Linux architecture.
    const RTLD_NOLOAD: std::os::raw::c_int = 0x4;
    let query = |name: &CStr| {
        let address = libraries.symbol(name);
        (!address.is_null()).then(|| std::mem::transmute::<*const c_void, Query>(address))
    };
    let (Some(query_display), Some(query_device)) =
        (query(c"eglQueryDisplayAttribEXT"), query(c"eglQueryDeviceAttribEXT"))
    else {
        return Err("ANGLE has no EGL device query".into());
    };
    let mut device = 0;
    if query_display(display, 0x322C, &mut device) == 0 || device == 0 {
        return Err(libraries.error("query ANGLE display device"));
    }
    let (mut instance, mut physical) = (0, 0);
    for (name, value) in [(0x34A9, &mut instance), (0x34AB, &mut physical)] {
        if query_device(device as Handle, name, value) == 0 || *value == 0 {
            return Err(libraries.error("query ANGLE Vulkan device"));
        }
    }
    // ANGLE opens the first loader name present in its module directory.
    let name = if libraries.bundle.directory.join("libvulkan.so").exists() {
        "libvulkan.so"
    } else {
        "libvulkan.so.1"
    };
    let path = libraries.bundle.library(name)?;
    // Only reuse the loader ANGLE already mapped; never load another one.
    let loader = libloading::os::unix::Library::open(
        Some(&path),
        libloading::os::unix::RTLD_NOW | RTLD_NOLOAD,
    )
    .map_err(|e| format!("Vulkan loader is not loaded: {e}"))?;
    let get_proc: GetInstanceProcAddr = *loader
        .get::<GetInstanceProcAddr>(b"vkGetInstanceProcAddr\0")
        .map_err(|e| e.to_string())?;
    let address = get_proc(instance as Handle, c"vkGetPhysicalDeviceProperties".as_ptr());
    if address.is_null() {
        return Err("Vulkan loader has no vkGetPhysicalDeviceProperties".into());
    }
    let get_properties = std::mem::transmute::<*const c_void, GetProperties>(address);
    let mut properties: DeviceProperties = std::mem::zeroed();
    get_properties(physical as Handle, &mut properties);
    let last = properties.device_name.len() - 1;
    properties.device_name[last] = 0;
    Ok(PhysicalDevice {
        vendor: properties.vendor_id,
        device: properties.device_id,
        kind: properties.device_type,
        name: CStr::from_ptr(properties.device_name.as_ptr())
            .to_string_lossy()
            .into_owned(),
    })
}
fn context_attributes(version: u8) -> Result<Vec<Int>, String> {
    let major = match version {
        1 => 2,
        2 => 3,
        _ => return Err("unsupported WebGL version".into()),
    };
    Ok(vec![
        0x3098, major, 0x30FB, 0, // exact ES version; ES3.1 is not WebGL2
        0x3483, FALSE, // EGL_CONTEXT_OPENGL_BACKWARDS_COMPATIBLE_ANGLE
        0x33AC, TRUE, // EGL_CONTEXT_WEBGL_COMPATIBILITY_ANGLE
        0x3452, FALSE, // never permit client-side vertex array pointers
        0x3453, TRUE, // initialize resources before page data can read them
        0x345F, FALSE, // enable extensions only after WebGL getExtension validation
        0x30BF, TRUE, // EGL_CONTEXT_OPENGL_ROBUST_ACCESS_EXT
        0x3138, 0x31BF, // lose context on device reset
        NONE,
    ])
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceOptions {
    pub alpha: bool,
    pub depth: bool,
    pub stencil: bool,
    pub antialias: bool,
}

/// No Send/Sync: makeCurrent state and GL object names never leave the owning
/// browser thread. Deleting a profile drops every context before its runtime.
pub struct Context {
    display: Arc<Display>,
    config: usize,
    context: usize,
    surface: usize,
    pub gl: glow::Context,
    pub samples: u32,
    pub depth_bits: u32,
    pub stencil_bits: u32,
    width: u32,
    height: u32,
    _thread: PhantomData<Rc<()>>,
}

// The borrowed function table remains valid through the display/library owner.
// Keeping handle ownership independent of dynamic loading lets unit tests drive
// every partial-allocation failure without pretending to provide a GL renderer.
struct PendingContext<'a> {
    functions: &'a Functions,
    display: Handle,
    context: Handle,
    surface: Handle,
}
impl Drop for PendingContext<'_> {
    fn drop(&mut self) {
        unsafe {
            release_handles(self.functions, self.display, self.context, self.surface);
        }
    }
}
unsafe fn release_handles(f: &Functions, display: Handle, context: Handle, surface: Handle) {
    if !context.is_null() {
        (f.make_current)(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        (f.destroy_context)(display, context);
    }
    if !surface.is_null() {
        (f.destroy_surface)(display, surface);
    }
}
/// Zero the current default framebuffer without changing page-visible state.
/// Pinned ANGLE Metal never robust-initializes pbuffer color: SurfaceMtl sets
/// mColorTextureInitialized to true and OffscreenSurfaceMtl never resets it
/// after allocating a color texture. Drivers can hand a new pbuffer the
/// memory of a surface that was just freed, so clear it explicitly.
unsafe fn zero_default_framebuffer(gl: &glow::Context) {
    use glow::HasContext;
    let es3 = gl.version().major >= 3;
    let target = if es3 {
        glow::DRAW_FRAMEBUFFER
    } else {
        glow::FRAMEBUFFER
    };
    // FRAMEBUFFER_BINDING is the draw binding in ES3; the read binding is
    // never changed here.
    let framebuffer = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
    let scissor = gl.is_enabled(glow::SCISSOR_TEST);
    // ES3 drops Clear while RASTERIZER_DISCARD is enabled.
    let discard = es3 && gl.is_enabled(glow::RASTERIZER_DISCARD);
    let mask = gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK);
    let depth_mask = gl.get_parameter_bool(glow::DEPTH_WRITEMASK);
    let front = gl.get_parameter_i32(glow::STENCIL_WRITEMASK) as u32;
    let back = gl.get_parameter_i32(glow::STENCIL_BACK_WRITEMASK) as u32;
    let mut color = [0.0; 4];
    gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut color);
    let depth = gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE);
    let stencil = gl.get_parameter_i32(glow::STENCIL_CLEAR_VALUE);
    gl.bind_framebuffer(target, None);
    // Draw-buffer selection belongs to the default framebuffer itself.
    let draw = if es3 {
        let previous = gl.get_parameter_i32(glow::DRAW_BUFFER0) as u32;
        gl.draw_buffers(&[glow::BACK]);
        Some(previous)
    } else {
        None
    };
    gl.disable(glow::SCISSOR_TEST);
    if discard {
        gl.disable(glow::RASTERIZER_DISCARD);
    }
    gl.color_mask(true, true, true, true);
    gl.depth_mask(true);
    gl.stencil_mask(u32::MAX);
    gl.clear_color(0.0, 0.0, 0.0, 0.0);
    gl.clear_depth_f32(1.0);
    gl.clear_stencil(0);
    gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
    gl.clear_color(color[0], color[1], color[2], color[3]);
    gl.clear_depth_f32(depth);
    gl.clear_stencil(stencil);
    gl.color_mask(mask[0], mask[1], mask[2], mask[3]);
    gl.depth_mask(depth_mask);
    gl.stencil_mask_separate(glow::FRONT, front);
    gl.stencil_mask_separate(glow::BACK, back);
    if discard {
        gl.enable(glow::RASTERIZER_DISCARD);
    }
    if scissor {
        gl.enable(glow::SCISSOR_TEST);
    }
    if let Some(draw) = draw {
        gl.draw_buffers(&[draw]);
    }
    gl.bind_framebuffer(target, framebuffer);
}
unsafe fn allocate_context<'a>(
    f: &'a Functions,
    display: Handle,
    config: Handle,
    context_attrs: &[Int],
    surface_attrs: &[Int],
    backend: Backend,
    error: impl Fn(&str) -> String,
) -> Result<PendingContext<'a>, String> {
    let context = (f.create_context)(display, config, ptr::null_mut(), context_attrs.as_ptr());
    if context.is_null() {
        return Err(error("create WebGL-compatible context"));
    }
    let mut pending = PendingContext {
        functions: f,
        display,
        context,
        surface: ptr::null_mut(),
    };
    pending.surface = (f.create_pbuffer)(display, config, surface_attrs.as_ptr());
    if pending.surface.is_null() {
        return Err(error("allocate WebGL drawing buffer"));
    }
    if (f.make_current)(display, pending.surface, pending.surface, context) == 0 {
        return Err(error("activate WebGL context"));
    }
    // Pinned ANGLE Metal allocates pbuffer attachments lazily. Its robust
    // initialization path can clear them before allocation. A pbuffer swap
    // allocates those attachments without exposing or reading their contents.
    // Keep robust initialization enabled for both the context and surface.
    if backend == Backend::Metal && (f.swap_buffers)(display, pending.surface) == 0 {
        return Err(error("initialize Metal WebGL drawing buffer"));
    }
    Ok(pending)
}
unsafe fn replace_surface(
    f: &Functions,
    display: Handle,
    config: Handle,
    context: Handle,
    previous: Handle,
    attrs: &[Int],
    backend: Backend,
    error: impl Fn(&str) -> String,
) -> Result<Handle, String> {
    let replacement = (f.create_pbuffer)(display, config, attrs.as_ptr());
    if replacement.is_null() {
        return Err(error("resize WebGL drawing buffer"));
    }
    if (f.make_current)(display, replacement, replacement, context) == 0 {
        let reason = error("activate resized WebGL buffer");
        (f.destroy_surface)(display, replacement);
        return Err(reason);
    }
    if backend == Backend::Metal && (f.swap_buffers)(display, replacement) == 0 {
        let mut reason = error("initialize resized Metal WebGL buffer");
        // The replacement is current, but the old surface is still owned.
        // Restore it before discarding the failed allocation. If restoration
        // also fails, release the current binding and report both failures.
        if (f.make_current)(display, previous, previous, context) == 0 {
            reason.push_str("; ");
            reason.push_str(&error("restore previous WebGL buffer"));
            (f.make_current)(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        }
        (f.destroy_surface)(display, replacement);
        return Err(reason);
    }
    (f.destroy_surface)(display, previous);
    Ok(replacement)
}

pub fn backing_size(width: u32, height: u32) -> Result<(u32, u32), String> {
    // A zero-size HTML canvas still has a valid GL context with a 1x1 backing
    // store. Canvas serialization remains empty for a zero-size canvas.
    let size = (width.max(1), height.max(1));
    if size.0 > 32767 || size.1 > 32767 || u64::from(size.0) * u64::from(size.1) > 16_777_216 {
        Err("WebGL drawing buffer allocation exceeds the canvas limit".into())
    } else {
        Ok(size)
    }
}

// Disabled buffers must stay absent; requested depth/stencil/MSAA are
// preferences. Hardware that lacks a preferred combination may use fewer.
fn config_score(options: SurfaceOptions, sizes: [i32; 4]) -> Option<i32> {
    let [alpha, depth, stencil, samples] = sizes;
    if sizes.iter().any(|n| *n < 0)
        || (options.alpha && alpha < 8)
        || (!options.alpha && alpha != 0)
        || (!options.depth && depth != 0)
        || (!options.stencil && stencil != 0)
        || (!options.antialias && samples != 0)
    {
        return None;
    }
    Some(
        i32::from(options.depth && depth >= 16) * 100
            + i32::from(options.stencil && stencil >= 8) * 100
            + if options.antialias { samples.min(4) } else { 0 },
    )
}

impl Context {
    pub fn create(
        backend: Backend,
        version: u8,
        width: u32,
        height: u32,
        options: SurfaceOptions,
    ) -> Result<Self, String> {
        let attributes = context_attributes(version)?;
        let (width, height) = backing_size(width, height)?;
        let display = Display::acquire(backend)?;
        unsafe {
            let f = &display.libraries.functions;
            let d = display.handle();
            if (f.bind_api)(0x30A0) == 0 {
                return Err(display.libraries.error("bind OpenGL ES API"));
            }
            // EGL size requests are minima: asking for zero alpha/depth does
            // not exclude a larger buffer. Inspect each candidate explicitly.
            let config_attrs = [
                0x3033,
                1,
                0x3040,
                if version == 2 { 0x0040 } else { 0x0004 },
                0x3024,
                8,
                0x3023,
                8,
                0x3022,
                8,
                NONE,
            ];
            let mut count = 0;
            if (f.choose_config)(d, config_attrs.as_ptr(), ptr::null_mut(), 0, &mut count) == 0
                || !(1..=4096).contains(&count)
            {
                return Err(display.libraries.error("enumerate WebGL configurations"));
            }
            let mut configs = vec![ptr::null_mut(); count as usize];
            let capacity = count;
            if (f.choose_config)(
                d,
                config_attrs.as_ptr(),
                configs.as_mut_ptr(),
                capacity,
                &mut count,
            ) == 0
            {
                return Err(display.libraries.error("read WebGL configurations"));
            }
            let mut selected = None;
            for &candidate in configs.iter().take(count.max(0) as usize) {
                let mut values = [0; 4];
                if ![0x3021, 0x3025, 0x3026, 0x3031]
                    .iter()
                    .zip(&mut values)
                    .all(|(&name, value)| (f.get_config_attrib)(d, candidate, name, value) != 0)
                {
                    continue;
                }
                if let Some(score) = config_score(options, values) {
                    if selected.as_ref().is_none_or(|(best, _, _)| score > *best) {
                        selected = Some((score, candidate, values));
                    }
                }
            }
            let Some((_, config, config_sizes)) = selected else {
                return Err(
                    "no EGL configuration honors WebGL alpha/depth/stencil/antialias exclusions"
                        .into(),
                );
            };
            let surface_attrs = [
                0x3057,
                width as Int,
                0x3056,
                height as Int,
                0x3453,
                TRUE,
                NONE,
            ];
            let mut pending = allocate_context(f, d, config, &attributes, &surface_attrs, backend, |op| {
                display.libraries.error(op)
            })?;
            let gl = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                glow::Context::from_loader_function_cstr(|name| display.libraries.symbol(name))
            }))
            .map_err(|_| "ANGLE GLES entry points are incomplete")?;
            if backend == Backend::Metal {
                zero_default_framebuffer(&gl);
                // A fresh context has no page errors to consume. Returning
                // here drops `pending`, which releases the context and surface.
                if glow::HasContext::get_error(&gl) != glow::NO_ERROR {
                    return Err("initialize Metal WebGL drawing buffer".into());
                }
            }
            let mut samples = 0;
            if (f.get_config_attrib)(d, config, 0x3031, &mut samples) == 0 {
                return Err(display.libraries.error("query drawing buffer samples"));
            }
            let surface = pending.surface;
            let context = pending.context;
            pending.context = ptr::null_mut();
            pending.surface = ptr::null_mut();
            drop(pending);
            Ok(Self {
                display,
                config: config as usize,
                context: context as usize,
                surface: surface as usize,
                gl,
                samples: samples.max(0) as u32,
                depth_bits: config_sizes[1].max(0) as u32,
                stencil_bits: config_sizes[2].max(0) as u32,
                width,
                height,
                _thread: PhantomData,
            })
        }
    }
    pub fn make_current(&self) -> Result<(), String> {
        #[cfg(test)]
        if crate::creation_test_faults::ACTIVATE.with(|fault| fault.replace(false)) {
            return Err("injected first Canvas activation failure".into());
        }
        unsafe {
            let f = &self.display.libraries.functions;
            if (f.make_current)(
                self.display.handle(),
                self.surface as Handle,
                self.surface as Handle,
                self.context as Handle,
            ) == 0
            {
                Err(self.display.libraries.error("activate WebGL context"))
            } else {
                Ok(())
            }
        }
    }

    /// ANGLE exposes a few GLES entry points that glow does not wrap. Callers
    /// supply a static symbol and its exact GLES ABI, never a page-provided name.
    pub(crate) unsafe fn entry<T: Copy>(&self, name: &std::ffi::CStr) -> Result<T, u32> {
        let address = self.display.libraries.symbol(name);
        if address.is_null() || std::mem::size_of::<T>() != std::mem::size_of_val(&address) {
            return Err(glow::INVALID_OPERATION);
        }
        Ok(std::mem::transmute_copy(&address))
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let (width, height) = backing_size(width, height)?;
        // Even assigning identical dimensions resets the drawing buffer.
        unsafe {
            let f = &self.display.libraries.functions;
            let d = self.display.handle();
            let attrs = [
                0x3057,
                width as Int,
                0x3056,
                height as Int,
                0x3453,
                TRUE,
                NONE,
            ];
            let replacement = replace_surface(
                f,
                d,
                self.config as Handle,
                self.context as Handle,
                self.surface as Handle,
                &attrs,
                self.display.backend,
                |op| self.display.libraries.error(op),
            )?;
            self.surface = replacement as usize;
            if self.display.backend == Backend::Metal {
                // The replacement is current. Page errors stay queued.
                zero_default_framebuffer(&self.gl);
            }
            self.width = width;
            self.height = height;
            Ok(())
        }
    }
    pub fn proc_address(&self, name: &CStr) -> *const c_void {
        unsafe { self.display.libraries.symbol(name) }
    }
    #[cfg(test)]
    pub(crate) fn display_handle(&self) -> usize {
        self.display.handle
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            let f = &self.display.libraries.functions;
            let d = self.display.handle();
            release_handles(f, d, self.context as Handle, self.surface as Handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Clone, Copy, Default, PartialEq)]
    enum Failure {
        #[default]
        None,
        Context,
        Surface,
        Activate,
        Prime,
        PrimeAndRestore,
        Destroy,
    }
    #[derive(Default)]
    struct FakeEgl {
        failure: Failure,
        calls: Vec<(&'static str, usize)>,
        current_surfaces: Vec<usize>,
    }
    thread_local! { static FAKE:RefCell<FakeEgl>=RefCell::new(FakeEgl::default()); }
    fn reset(failure: Failure) {
        FAKE.with(|f| {
            *f.borrow_mut() = FakeEgl {
                failure,
                calls: Vec::new(),
                current_surfaces: Vec::new(),
            }
        });
    }
    fn calls() -> Vec<(&'static str, usize)> {
        FAKE.with(|f| f.borrow().calls.clone())
    }
    fn record(name: &'static str, handle: usize) -> Failure {
        FAKE.with(|f| {
            let mut f = f.borrow_mut();
            f.calls.push((name, handle));
            f.failure
        })
    }
    unsafe extern "C" fn create_context(_: Handle, _: Handle, _: Handle, _: *const Int) -> Handle {
        if record("create_context", 10) == Failure::Context {
            ptr::null_mut()
        } else {
            10usize as Handle
        }
    }
    unsafe extern "C" fn create_surface(_: Handle, _: Handle, _: *const Int) -> Handle {
        if record("create_surface", 20) == Failure::Surface {
            ptr::null_mut()
        } else {
            20usize as Handle
        }
    }
    unsafe extern "C" fn make_current(_: Handle, surface: Handle, _: Handle, context: Handle) -> u32 {
        let failure = record("make_current", context as usize);
        FAKE.with(|f| f.borrow_mut().current_surfaces.push(surface as usize));
        u32::from(context.is_null() || (failure != Failure::Activate
            && !(failure == Failure::PrimeAndRestore && surface as usize == 30)))
    }
    unsafe extern "C" fn swap_buffers(_: Handle, surface: Handle) -> u32 {
        let failure = record("swap_buffers", surface as usize);
        u32::from(!matches!(failure, Failure::Prime | Failure::PrimeAndRestore))
    }
    unsafe extern "C" fn destroy_context(_: Handle, context: Handle) -> u32 {
        u32::from(record("destroy_context", context as usize) != Failure::Destroy)
    }
    unsafe extern "C" fn destroy_surface(_: Handle, surface: Handle) -> u32 {
        u32::from(record("destroy_surface", surface as usize) != Failure::Destroy)
    }
    unsafe extern "C" fn get_proc_address(_: *const c_char) -> *const c_void {
        ptr::null()
    }
    unsafe extern "C" fn initialize(_: Handle, _: *mut Int, _: *mut Int) -> u32 {
        0
    }
    unsafe extern "C" fn terminate(_: Handle) -> u32 {
        0
    }
    unsafe extern "C" fn bind_api(_: u32) -> u32 {
        0
    }
    unsafe extern "C" fn choose_config(
        _: Handle,
        _: *const Int,
        _: *mut Handle,
        _: Int,
        _: *mut Int,
    ) -> u32 {
        0
    }
    unsafe extern "C" fn get_config_attrib(_: Handle, _: Handle, _: Int, _: *mut Int) -> u32 {
        0
    }
    unsafe extern "C" fn query_string(_: Handle, _: Int) -> *const c_char {
        ptr::null()
    }
    unsafe extern "C" fn get_error() -> Int {
        0x3003
    }
    fn functions() -> Functions {
        Functions {
            get_proc_address,
            initialize,
            terminate,
            bind_api,
            choose_config,
            get_config_attrib,
            create_context,
            destroy_context,
            create_pbuffer: create_surface,
            destroy_surface,
            make_current,
            swap_buffers,
            query_string,
            get_error,
        }
    }
    unsafe fn allocate(f: &Functions) -> Result<PendingContext<'_>, String> {
        allocate_context(
            f,
            1usize as Handle,
            2usize as Handle,
            &[NONE],
            &[NONE],
            Backend::Vulkan,
            str::to_owned,
        )
    }
    #[test]
    fn partial_context_allocation_releases_each_acquired_handle_once() {
        let f = functions();
        for (failure, message, expected) in [
            (
                Failure::Context,
                "create WebGL-compatible context",
                vec![("create_context", 10)],
            ),
            (
                Failure::Surface,
                "allocate WebGL drawing buffer",
                vec![
                    ("create_context", 10),
                    ("create_surface", 20),
                    ("make_current", 0),
                    ("destroy_context", 10),
                ],
            ),
            (
                Failure::Activate,
                "activate WebGL context",
                vec![
                    ("create_context", 10),
                    ("create_surface", 20),
                    ("make_current", 10),
                    ("make_current", 0),
                    ("destroy_context", 10),
                    ("destroy_surface", 20),
                ],
            ),
        ] {
            reset(failure);
            assert_eq!(unsafe { allocate(&f) }.err().as_deref(), Some(message));
            assert_eq!(calls(), expected);
        }
    }
    #[test]
    fn failures_after_activation_and_unwinding_still_release_both_handles() {
        let f = functions();
        reset(Failure::None);
        let result = std::panic::catch_unwind(|| unsafe {
            let _pending = allocate(&f).unwrap();
            // Mirrors a missing GLES symbol or another error after activation.
            panic!("injected loader failure");
        });
        assert!(result.is_err());
        assert_eq!(
            calls(),
            vec![
                ("create_context", 10),
                ("create_surface", 20),
                ("make_current", 10),
                ("make_current", 0),
                ("destroy_context", 10),
                ("destroy_surface", 20)
            ]
        );
    }
    #[test]
    fn committed_handles_are_destroyed_by_the_final_owner_only() {
        let f = functions();
        reset(Failure::None);
        let mut pending = unsafe { allocate(&f) }.unwrap();
        let (context, surface) = (pending.context, pending.surface);
        pending.context = ptr::null_mut();
        pending.surface = ptr::null_mut();
        drop(pending);
        assert_eq!(calls().len(), 3);
        unsafe {
            release_handles(&f, 1usize as Handle, context, surface);
        }
        assert_eq!(
            &calls()[3..],
            &[
                ("make_current", 0),
                ("destroy_context", 10),
                ("destroy_surface", 20)
            ]
        );
    }
    #[test]
    fn resize_keeps_the_old_surface_until_replacement_activation_succeeds() {
        let f = functions();
        for (failure, expected, result) in [
            (Failure::Surface, vec![("create_surface", 20)], false),
            (
                Failure::Activate,
                vec![
                    ("create_surface", 20),
                    ("make_current", 10),
                    ("destroy_surface", 20),
                ],
                false,
            ),
            (
                Failure::None,
                vec![
                    ("create_surface", 20),
                    ("make_current", 10),
                    ("destroy_surface", 30),
                ],
                true,
            ),
        ] {
            reset(failure);
            let replacement = unsafe {
                replace_surface(
                    &f,
                    1usize as Handle,
                    2usize as Handle,
                    10usize as Handle,
                    30usize as Handle,
                    &[NONE],
                    Backend::Vulkan,
                    str::to_owned,
                )
            };
            assert_eq!(replacement.is_ok(), result);
            assert_eq!(calls(), expected);
            if let Ok(surface) = replacement {
                assert_eq!(surface as usize, 20);
            }
        }
    }
    #[test]
    fn metal_pbuffer_initialization_precedes_gl_and_releases_failed_allocations() {
        let f = functions();
        for failure in [Failure::None, Failure::Prime] {
            reset(failure);
            let result = unsafe { allocate_context(&f, 1usize as Handle, 2usize as Handle,
                &[NONE], &[NONE], Backend::Metal, str::to_owned) };
            assert_eq!(result.is_ok(), failure == Failure::None);
            if failure == Failure::Prime {
                assert_eq!(result.as_ref().err().map(String::as_str),
                    Some("initialize Metal WebGL drawing buffer"));
            }
            drop(result);
            assert_eq!(calls(), vec![("create_context",10),("create_surface",20),
                ("make_current",10),("swap_buffers",20),("make_current",0),
                ("destroy_context",10),("destroy_surface",20)]);
        }
    }
    #[test]
    fn metal_resize_initializes_before_retiring_old_surface_and_rolls_back_failures() {
        let f = functions();
        for failure in [Failure::None, Failure::Prime, Failure::PrimeAndRestore] {
            reset(failure);
            let result = unsafe { replace_surface(&f, 1usize as Handle, 2usize as Handle,
                10usize as Handle, 30usize as Handle, &[NONE], Backend::Metal, str::to_owned) };
            assert_eq!(result.is_ok(), failure == Failure::None);
            let mut expected = vec![("create_surface",20),("make_current",10),("swap_buffers",20)];
            let mut surfaces = vec![20];
            if failure == Failure::None {
                assert_eq!(result.unwrap() as usize,20);
                expected.push(("destroy_surface",30));
            } else {
                let reason = result.unwrap_err();
                assert!(reason.starts_with("initialize resized Metal WebGL buffer"));
                expected.push(("make_current",10));
                surfaces.push(30);
                if failure == Failure::PrimeAndRestore {
                    assert!(reason.contains("restore previous WebGL buffer"));
                    expected.push(("make_current",0));
                    surfaces.push(0);
                }
                expected.push(("destroy_surface",20));
            }
            assert_eq!(calls(), expected);
            FAKE.with(|f| assert_eq!(f.borrow().current_surfaces, surfaces));
        }
    }
    #[test]
    fn cleanup_handles_empty_surface_only_and_failed_driver_destruction() {
        let f = functions();
        reset(Failure::None);
        unsafe {
            release_handles(&f, 1usize as Handle, ptr::null_mut(), ptr::null_mut());
        }
        assert!(calls().is_empty());
        unsafe {
            release_handles(&f, 1usize as Handle, ptr::null_mut(), 20usize as Handle);
        }
        assert_eq!(calls(), vec![("destroy_surface", 20)]);
        reset(Failure::Destroy);
        unsafe {
            release_handles(&f, 1usize as Handle, 10usize as Handle, 20usize as Handle);
        }
        assert_eq!(
            calls(),
            vec![
                ("make_current", 0),
                ("destroy_context", 10),
                ("destroy_surface", 20)
            ]
        );
    }
    #[test]
    fn config_selection_honors_disabled_buffers_and_falls_back_for_preferences() {
        let options = SurfaceOptions {
            alpha: false,
            depth: false,
            stencil: false,
            antialias: false,
        };
        assert_eq!(config_score(options, [0, 0, 0, 0]), Some(0));
        for sizes in [[8, 0, 0, 0], [0, 16, 0, 0], [0, 0, 8, 0], [0, 0, 0, 4]] {
            assert!(config_score(options, sizes).is_none());
        }
        let preferred = SurfaceOptions {
            alpha: true,
            depth: true,
            stencil: true,
            antialias: true,
        };
        assert!(config_score(preferred, [8, 24, 8, 4]) > config_score(preferred, [8, 0, 0, 0]));
        assert!(config_score(preferred, [8, 0, 0, 0]).is_some());
        assert!(config_score(preferred, [0, 24, 8, 4]).is_none());
    }
    #[test]
    fn backing_dimensions_are_checked_without_overflow_and_zero_is_supported() {
        assert_eq!(backing_size(0, 0).unwrap(), (1, 1));
        assert_eq!(backing_size(4096, 4096).unwrap(), (4096, 4096));
        for (w, h) in [(4097, 4096), (32768, 1), (1, 32768), (u32::MAX, u32::MAX)] {
            assert!(backing_size(w, h).is_err());
        }
    }
    #[test]
    fn both_versions_require_webgl_validation_robustness_and_disabled_client_arrays() {
        for (version, es) in [(1, 2), (2, 3)] {
            let a = context_attributes(version).unwrap();
            assert_eq!(&a[..2], &[0x3098, es]);
            let pairs = a[..a.len() - 1].chunks_exact(2).collect::<Vec<_>>();
            for required in [
                &[0x33AC, TRUE][..],
                &[0x3452, FALSE],
                &[0x3453, TRUE],
                &[0x345F, FALSE],
                &[0x30BF, TRUE],
            ] {
                assert!(pairs.contains(&required));
            }
        }
        assert!(context_attributes(0).is_err());
        assert!(context_attributes(3).is_err());
    }
    #[test]
    fn linux_backends_are_explicitly_surfaceless_and_software_is_requested_directly() {
        for backend in [Backend::Vulkan, Backend::SwiftShader] {
            let a = display_attributes(backend);
            assert!(a.windows(2).any(|p| p == [0x348F, 0x31DD]));
        }
        assert!(display_attributes(Backend::SwiftShader)
            .windows(2)
            .any(|p| p == [0x3209, 0x3487]));
        assert!(display_attributes(Backend::Metal)
            .windows(2)
            .any(|p| p == [0x3203, 0x3489]));
    }
    #[test]
    fn linux_backends_request_distinct_angle_display_keys_and_metal_none() {
        let key = |backend| {
            let a = display_attributes(backend);
            assert_eq!(a.last(), Some(&NONE));
            let pairs = a[..a.len() - 1].chunks_exact(2).collect::<Vec<_>>();
            let keys = pairs.iter().filter(|p| p[0] == 0x34DC).map(|p| p[1]).collect::<Vec<_>>();
            assert!(keys.len() <= 1, "{a:?}");
            keys.first().copied()
        };
        let (vulkan, swiftshader) = (key(Backend::Vulkan), key(Backend::SwiftShader));
        assert_eq!((vulkan, swiftshader), (Some(1), Some(2)));
        assert_eq!(key(Backend::Metal), None);
        // Only the key and the requested device type differ between them.
        let strip = |backend| {
            display_attributes(backend)
                .chunks(2)
                .filter(|p| p[0] != 0x34DC && p[0] != 0x3209)
                .flatten()
                .copied()
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(Backend::Vulkan), strip(Backend::SwiftShader));
    }
    #[test]
    fn a_process_never_initializes_a_second_backend_display() {
        assert_eq!(active_other([], Backend::Vulkan), None);
        assert_eq!(active_other([Backend::Vulkan], Backend::Vulkan), None);
        assert_eq!(active_other([Backend::SwiftShader], Backend::Vulkan), Some(Backend::SwiftShader));
        assert_eq!(active_other([Backend::Vulkan], Backend::SwiftShader), Some(Backend::Vulkan));
    }
    #[test]
    fn a_display_owned_by_another_backend_is_never_adopted() {
        let owned = [(Backend::SwiftShader, 0x10), (Backend::Metal, 0x30)];
        assert_eq!(alias_owner(owned, Backend::Vulkan, 0x10), Some(Backend::SwiftShader));
        assert_eq!(alias_owner(owned, Backend::SwiftShader, 0x30), Some(Backend::Metal));
        // A fresh handle, or the backend's own handle, is not an alias.
        assert_eq!(alias_owner(owned, Backend::Vulkan, 0x20), None);
        assert_eq!(alias_owner(owned, Backend::SwiftShader, 0x10), None);
        assert_eq!(alias_owner([], Backend::Vulkan, 0x10), None);
    }
    #[test]
    fn hardware_requires_a_gpu_device_and_software_requires_swiftshader() {
        let device = |vendor, device, kind| PhysicalDevice {
            vendor,
            device,
            kind,
            name: "fixture".into(),
        };
        let swiftshader = device(0x1AE0, 0xC0DE, 4);
        for kind in [1, 2, 3] {
            assert!(device_policy(Backend::Vulkan, device(0x10DE, 0x2184, kind)).is_ok());
            // SwiftShader stays software even if it reported a GPU type.
            assert!(device_policy(Backend::Vulkan, device(0x1AE0, 0xC0DE, kind)).is_err());
        }
        for kind in [0, 4, 5, -1] {
            let reason = device_policy(Backend::Vulkan, device(0x10005, 0, kind)).unwrap_err();
            assert!(reason.starts_with("ANGLE Vulkan display uses an unexpected"), "{reason}");
        }
        let reason = device_policy(Backend::Vulkan, swiftshader.clone()).unwrap_err();
        assert!(reason.contains("CPU device \"fixture\" (vendor 0x1ae0, device 0xc0de)"), "{reason}");
        assert!(device_policy(Backend::SwiftShader, swiftshader).is_ok());
        for other in [device(0x1AE0, 0xC0DF, 4), device(0x10005, 0, 4), device(0x10DE, 0x2184, 2)] {
            assert!(device_policy(Backend::SwiftShader, other).is_err());
        }
    }
}
