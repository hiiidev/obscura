//! Only advertise extensions for which both the browser binding and ANGLE
//! supply behavior. Driver-only extensions are never exposed wholesale.
use crate::{api::CanvasContext, queries::Value};
use glow::HasContext;
use std::ffi::{c_char, CStr, CString};
use std::collections::HashSet;

struct Extension {
    web: &'static str,
    driver: &'static str,
    version: u8,
}
const EXTENSIONS: &[Extension] = &[
    Extension {
        web: "OES_element_index_uint",
        driver: "GL_OES_element_index_uint",
        version: 1,
    },
    Extension {
        web: "OES_standard_derivatives",
        driver: "GL_OES_standard_derivatives",
        version: 1,
    },
    Extension {
        web: "OES_texture_float",
        driver: "GL_OES_texture_float",
        version: 1,
    },
    Extension {
        web: "OES_texture_float_linear",
        driver: "GL_OES_texture_float_linear",
        version: 0,
    },
    Extension {
        web: "OES_texture_half_float",
        driver: "GL_OES_texture_half_float",
        version: 1,
    },
    Extension {
        web: "OES_texture_half_float_linear",
        driver: "GL_OES_texture_half_float_linear",
        version: 1,
    },
    Extension {
        web: "OES_vertex_array_object",
        driver: "GL_OES_vertex_array_object",
        version: 1,
    },
    Extension {
        web: "ANGLE_instanced_arrays",
        driver: "GL_ANGLE_instanced_arrays",
        version: 1,
    },
    Extension {
        web: "EXT_texture_filter_anisotropic",
        driver: "GL_EXT_texture_filter_anisotropic",
        version: 0,
    },
    Extension {
        web: "EXT_color_buffer_float",
        driver: "GL_EXT_color_buffer_float",
        version: 2,
    },
    Extension {
        web: "EXT_color_buffer_half_float",
        driver: "GL_EXT_color_buffer_half_float",
        version: 0,
    },
    Extension {
        web: "WEBGL_depth_texture",
        driver: "GL_ANGLE_depth_texture",
        version: 1,
    },
];
fn available_extensions(version: u8, available: &std::collections::HashSet<String>) -> Vec<String> {
    let mut names = vec!["WEBGL_lose_context".into(), "WEBGL_debug_renderer_info".into()];
    names.extend(EXTENSIONS.iter().filter(|extension| {
        (extension.version == 0 || extension.version == version)
            && available.contains(extension.driver)
            // WebGL 1 half-float rendering requires half-float texture support.
            && (version != 1 || extension.web != "EXT_color_buffer_half_float"
                || available.contains("GL_OES_texture_half_float"))
    }).map(|extension| extension.web.to_owned()));
    names
}
// Keep native discovery/activation behind the same boundary used by failure
// fixtures. An unavailable entry point or null string must not enable a browser
// capability, and a failed dependency must remain retryable.
trait ExtensionAccess {
    fn initial_active(&self) -> &HashSet<String>;
    fn extension_string(&self, name: u32) -> Option<String>;
    fn request(&self, name: &str) -> bool;
}
type GetExtensionString = unsafe extern "system" fn(u32) -> *const u8;
type RequestExtension = unsafe extern "system" fn(*const c_char);
fn read_extension_string(get: Option<GetExtensionString>, name: u32) -> Option<String> {
    let pointer = unsafe { get?(name) };
    (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer.cast()) }.to_string_lossy().into_owned())
}
fn request_driver_extension(request: Option<RequestExtension>, name: &str) -> bool {
    let Some(request) = request else { return false; };
    let Ok(name) = CString::new(name) else { return false; };
    unsafe { request(name.as_ptr()); }
    true
}
impl ExtensionAccess for crate::egl::Context {
    fn initial_active(&self) -> &HashSet<String> {
        self.gl.supported_extensions()
    }
    fn extension_string(&self, name: u32) -> Option<String> {
        read_extension_string(unsafe { self.entry::<GetExtensionString>(c"glGetString") }.ok(), name)
    }
    fn request(&self, name: &str) -> bool {
        request_driver_extension(unsafe { self.entry::<RequestExtension>(c"glRequestExtensionANGLE") }.ok(), name)
    }
}
fn driver_extensions(driver: &impl ExtensionAccess) -> HashSet<String> {
    let mut names = driver.initial_active().clone();
    // ANGLE removes activated tokens from its requestable list; glow's startup
    // snapshot cannot represent extensions activated after context creation.
    if let Some(active) = driver.extension_string(glow::EXTENSIONS) {
        names.extend(active.split_ascii_whitespace().map(str::to_owned));
    }
    if let Some(requestable) = driver.extension_string(0x93A8) { // GL_REQUESTABLE_EXTENSIONS_ANGLE
        names.extend(requestable.split_ascii_whitespace().map(str::to_owned));
    }
    names
}
pub(crate) fn enable_internal_extension(driver: &crate::egl::Context, name: &str) -> bool {
    activate_driver_extension(driver, name)
}
fn activate_driver_extension(driver: &impl ExtensionAccess, name: &str) -> bool {
    if driver.initial_active().contains(name) { return true; }
    if driver.extension_string(glow::EXTENSIONS)
        .is_some_and(|active| active.split_ascii_whitespace().any(|token| token == name)) { return true; }
    if !driver_extensions(driver).contains(name) || !driver.request(name) { return false; }
    driver.extension_string(glow::EXTENSIONS)
        .is_some_and(|active| active.split_ascii_whitespace().any(|token| token == name))
}
fn activate_extension(
    driver: &impl ExtensionAccess,
    supported: &[String],
    enabled: &mut HashSet<String>,
    name: &str,
) -> Option<String> {
    let canonical = supported.iter().find(|v| v.eq_ignore_ascii_case(name))?.clone();
    if enabled.contains(&canonical) {
        return Some(canonical);
    }
    // ANGLE's ES2 RGBA16F renderbuffer support requires native half-float
    // texture support too. Activate that driver dependency without publishing
    // the browser's OES_texture_half_float capability before it is requested.
    if canonical == "EXT_color_buffer_half_float"
        && supported.iter().any(|name| name == "OES_texture_half_float")
        && !activate_driver_extension(driver, "GL_OES_texture_half_float")
    {
        return None;
    }
    if let Some(extension) = EXTENSIONS.iter().find(|e| e.web == canonical) {
        if !activate_driver_extension(driver, extension.driver) { return None; }
    }
    if canonical == "OES_texture_half_float"
        && supported.iter().any(|name| name == "EXT_color_buffer_half_float")
        && activate_extension(driver, supported, enabled, "EXT_color_buffer_half_float").is_none()
    {
        return None;
    }
    enabled.insert(canonical.clone());
    Some(canonical)
}
impl CanvasContext {
    pub fn supported_extensions(&mut self) -> Option<Vec<String>> {
        if !self.activate() {
            return None;
        }
        Some(available_extensions(self.version, &driver_extensions(self.driver.as_ref().unwrap())))
    }
    pub fn enable_extension(&mut self, name: &str) -> Option<String> {
        let supported = self.supported_extensions()?;
        activate_extension(self.driver.as_ref().unwrap(), &supported, &mut self.extensions, name)
    }
    pub(crate) fn extension_parameter(&self, name: u32) -> Option<Value> {
        match name {
            0x9245 if self.extensions.contains("WEBGL_debug_renderer_info") => {
                Some(Value::String(self.diagnostics.vendor.clone()))
            }
            0x9246 if self.extensions.contains("WEBGL_debug_renderer_info") => {
                Some(Value::String(self.diagnostics.renderer.clone()))
            }
            0x84FF if self.extensions.contains("EXT_texture_filter_anisotropic") => {
                Some(Value::Float(unsafe {
                    self.driver.as_ref().unwrap().gl.get_parameter_f32(name)
                }))
            }
            glow::VERTEX_ARRAY_BINDING if self.extensions.contains("OES_vertex_array_object") => {
                Some(self.object_value(unsafe {
                    self.driver
                        .as_ref()
                        .unwrap()
                        .gl
                        .get_parameter_vertex_array(name)
                        .map(crate::objects::Object::VertexArray)
                }))
            }
            0x8B8B if self.version == 2 || self.extensions.contains("OES_standard_derivatives") => {
                Some(Value::Int(unsafe {
                    self.driver.as_ref().unwrap().gl.get_parameter_i32(name)
                }))
            }
            0x9240 => Some(Value::Bool(self.unpack.flip_y)),
            0x9241 => Some(Value::Bool(self.unpack.premultiply_alpha)),
            0x9243 => Some(Value::UInt(if self.unpack.colorspace_none {
                0
            } else {
                0x9244
            })),
            _ => None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct DriverFixture {
        initial: HashSet<String>,
        requestable: Option<String>,
        current: RefCell<Option<HashSet<String>>>,
        requests: RefCell<Vec<String>>,
        missing_request: bool,
        rejected: RefCell<HashSet<String>>,
    }
    impl DriverFixture {
        fn available(names: &[&str]) -> Self {
            Self { requestable: Some(names.join(" ")),
                current: RefCell::new(Some(HashSet::new())), ..Self::default() }
        }
    }
    impl ExtensionAccess for DriverFixture {
        fn initial_active(&self) -> &HashSet<String> { &self.initial }
        fn extension_string(&self, name: u32) -> Option<String> {
            if name == 0x93A8 { return self.requestable.clone(); }
            assert_eq!(name, glow::EXTENSIONS);
            self.current.borrow().as_ref().map(|names| names.iter().cloned().collect::<Vec<_>>().join(" "))
        }
        fn request(&self, name: &str) -> bool {
            self.requests.borrow_mut().push(name.into());
            if self.missing_request { return false; }
            if !self.rejected.borrow().contains(name) {
                if let Some(current) = self.current.borrow_mut().as_mut() { current.insert(name.into()); }
            }
            true
        }
    }
    fn enable(fixture: &DriverFixture, version: u8, enabled: &mut HashSet<String>, name: &str) -> Option<String> {
        activate_extension(fixture, &available_extensions(version, &driver_extensions(fixture)), enabled, name)
    }
    #[test]
    fn internal_extensions_use_live_driver_state_without_publishing_web_flags() {
        let mut fixture=DriverFixture::available(&["GL_ANGLE_framebuffer_blit"]);
        assert!(activate_driver_extension(&fixture,"GL_ANGLE_framebuffer_blit"));
        assert_eq!(fixture.requests.borrow().len(),1);
        fixture.requestable=None;fixture.missing_request=true;
        assert!(activate_driver_extension(&fixture,"GL_ANGLE_framebuffer_blit"));
        assert_eq!(fixture.requests.borrow().len(),1);
        assert!(!activate_driver_extension(&fixture,"GL_ANGLE_framebuffer"));
        assert!(!available_extensions(1,&driver_extensions(&fixture)).iter().any(|name|name.contains("framebuffer")));
        fixture.initial.insert("GL_OES_rgb8_rgba8".into());
        assert!(activate_driver_extension(&fixture,"GL_OES_rgb8_rgba8"));
    }
    #[test]
    fn internal_extension_failures_remain_unavailable_and_retryable() {
        let mut fixture=DriverFixture::available(&["GL_ANGLE_framebuffer_blit"]);
        fixture.missing_request=true;
        assert!(!activate_driver_extension(&fixture,"GL_ANGLE_framebuffer_blit"));
        fixture.missing_request=false;fixture.rejected.borrow_mut().insert("GL_ANGLE_framebuffer_blit".into());
        assert!(!activate_driver_extension(&fixture,"GL_ANGLE_framebuffer_blit"));
        fixture.rejected.borrow_mut().clear();*fixture.current.borrow_mut()=None;
        assert!(!activate_driver_extension(&fixture,"GL_ANGLE_framebuffer_blit"));
        *fixture.current.borrow_mut()=Some(HashSet::new());
        assert!(activate_driver_extension(&fixture,"GL_ANGLE_framebuffer_blit"));
    }
    #[test]
    fn native_extension_entry_and_string_failures_do_not_publish_capabilities() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static REQUESTS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "system" fn get(name: u32) -> *const u8 {
            if name == glow::EXTENSIONS { c"GL_OES_texture_float".as_ptr().cast() } else { std::ptr::null() }
        }
        unsafe extern "system" fn request(name: *const c_char) {
            if unsafe { CStr::from_ptr(name) }.to_bytes() == b"GL_OES_texture_float" {
                REQUESTS.fetch_add(1, Ordering::SeqCst);
            }
        }
        // Compile this boundary on both signed- and unsigned-c_char targets.
        let request: RequestExtension = request;
        assert_eq!(read_extension_string(None, glow::EXTENSIONS), None);
        assert_eq!(read_extension_string(Some(get), 0x93A8), None);
        assert_eq!(read_extension_string(Some(get), glow::EXTENSIONS).as_deref(), Some("GL_OES_texture_float"));
        REQUESTS.store(0, Ordering::SeqCst);
        assert!(!request_driver_extension(None, "GL_OES_texture_float"));
        assert!(!request_driver_extension(Some(request), "GL_OES_texture_float\0suffix"));
        assert_eq!(REQUESTS.load(Ordering::SeqCst), 0);
        assert!(request_driver_extension(Some(request), "GL_OES_texture_float"));
        assert_eq!(REQUESTS.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn extension_discovery_keeps_active_names_when_requestable_string_is_missing() {
        let mut fixture = DriverFixture::available(&["GL_OES_texture_float", "GL_private"]);
        fixture.initial.insert("GL_OES_texture_float".into());
        fixture.initial.insert("GL_OES_element_index_uint".into());
        assert_eq!(driver_extensions(&fixture).len(), 3);
        fixture.requestable = None;
        assert_eq!(driver_extensions(&fixture), fixture.initial);
        let mut enabled = HashSet::new();
        // An already active extension needs no request entry or current string.
        fixture.missing_request = true;*fixture.current.borrow_mut() = None;
        assert_eq!(enable(&fixture, 1, &mut enabled, "oes_TEXTURE_FLOAT").as_deref(), Some("OES_texture_float"));
        assert!(fixture.requests.borrow().is_empty());
    }
    #[test]
    fn activated_extensions_remain_available_after_leaving_the_requestable_list() {
        let mut fixture = DriverFixture::available(&["GL_OES_texture_float"]);
        let mut enabled = HashSet::new();
        assert!(enable(&fixture, 1, &mut enabled, "OES_texture_float").is_some());
        fixture.requestable = None;
        fixture.missing_request = true;
        assert!(driver_extensions(&fixture).contains("GL_OES_texture_float"));
        assert_eq!(enable(&fixture, 1, &mut enabled, "oes_TEXTURE_float").as_deref(), Some("OES_texture_float"));
        assert_eq!(*fixture.requests.borrow(), ["GL_OES_texture_float"]);
    }
    #[test]
    fn half_float_renderbuffer_native_dependency_does_not_publish_texture_api() {
        let fixture = DriverFixture::available(&["GL_OES_texture_half_float", "GL_EXT_color_buffer_half_float"]);
        let mut enabled = HashSet::new();
        fixture.rejected.borrow_mut().insert("GL_OES_texture_half_float".into());
        assert!(enable(&fixture, 1, &mut enabled, "EXT_color_buffer_half_float").is_none());
        assert!(enabled.is_empty());
        assert_eq!(*fixture.requests.borrow(), ["GL_OES_texture_half_float"]);
        fixture.rejected.borrow_mut().clear();
        assert!(enable(&fixture, 1, &mut enabled, "EXT_color_buffer_half_float").is_some());
        assert_eq!(enabled, HashSet::from(["EXT_color_buffer_half_float".into()]));
        assert_eq!(*fixture.requests.borrow(), ["GL_OES_texture_half_float", "GL_OES_texture_half_float", "GL_EXT_color_buffer_half_float"]);
        let two = DriverFixture::available(&["GL_EXT_color_buffer_half_float"]);
        let mut enabled = HashSet::new();
        assert!(enable(&two, 2, &mut enabled, "EXT_color_buffer_half_float").is_some());
        assert_eq!(*two.requests.borrow(), ["GL_EXT_color_buffer_half_float"]);
    }
    #[test]
    fn extension_version_unknown_and_browser_only_requests_never_touch_native_activation() {
        let fixture = DriverFixture::available(&["GL_OES_texture_float", "GL_EXT_color_buffer_float", "GL_private"]);
        let mut enabled = HashSet::new();
        for (version,name) in [(2,"OES_texture_float"),(1,"EXT_color_buffer_float"),(1,"private"),(2,"")] {
            assert!(enable(&fixture, version, &mut enabled, name).is_none());
        }
        for name in ["WEBGL_lose_context","WEBGL_debug_renderer_info"] {
            assert_eq!(enable(&fixture, 1, &mut enabled, &name.to_ascii_lowercase()).as_deref(), Some(name));
            assert_eq!(enable(&fixture, 1, &mut enabled, name).as_deref(), Some(name));
        }
        assert_eq!(enabled.len(), 2);assert!(fixture.requests.borrow().is_empty());
    }
    #[test]
    fn failed_extension_activation_is_not_cached_and_successful_retry_is_cached() {
        for fault in ["missing_entry","missing_string","rejected"] {
            let mut fixture = DriverFixture::available(&["GL_OES_texture_float"]);
            match fault {
                "missing_entry" => fixture.missing_request = true,
                "missing_string" => *fixture.current.borrow_mut() = None,
                _ => { fixture.rejected.borrow_mut().insert("GL_OES_texture_float".into()); }
            }
            let mut enabled = HashSet::new();
            assert!(enable(&fixture, 1, &mut enabled, "OES_texture_float").is_none());
            assert!(enabled.is_empty());assert_eq!(fixture.requests.borrow().len(), 1);
            fixture.missing_request = false;fixture.rejected.borrow_mut().clear();
            *fixture.current.borrow_mut() = Some(HashSet::new());
            assert_eq!(enable(&fixture, 1, &mut enabled, "oes_texture_float").as_deref(), Some("OES_texture_float"));
            assert_eq!(fixture.requests.borrow().len(), 2);
            fixture.missing_request = true;
            assert_eq!(enable(&fixture, 1, &mut enabled, "OES_texture_float").as_deref(), Some("OES_texture_float"));
            assert_eq!(fixture.requests.borrow().len(), 2);
        }
    }
    #[test]
    fn half_float_dependency_failure_keeps_parent_retryable_without_erasing_other_extensions() {
        let fixture = DriverFixture::available(&["GL_OES_texture_half_float","GL_EXT_color_buffer_half_float"]);
        fixture.rejected.borrow_mut().insert("GL_EXT_color_buffer_half_float".into());
        let mut enabled = HashSet::from(["WEBGL_lose_context".into()]);
        assert!(enable(&fixture, 1, &mut enabled, "OES_texture_half_float").is_none());
        assert_eq!(enabled, HashSet::from(["WEBGL_lose_context".into()]));
        assert_eq!(*fixture.requests.borrow(), ["GL_OES_texture_half_float","GL_EXT_color_buffer_half_float"]);
        assert!(fixture.current.borrow().as_ref().unwrap().contains("GL_OES_texture_half_float"));
        fixture.rejected.borrow_mut().clear();
        assert_eq!(enable(&fixture, 1, &mut enabled, "OES_texture_half_float").as_deref(), Some("OES_texture_half_float"));
        assert!(enabled.contains("EXT_color_buffer_half_float"));assert!(enabled.contains("WEBGL_lose_context"));
        let count = fixture.requests.borrow().len();
        assert!(enable(&fixture, 1, &mut enabled, "ext_color_buffer_half_float").is_some());
        assert!(enable(&fixture, 1, &mut enabled, "oes_texture_half_float").is_some());
        assert_eq!(fixture.requests.borrow().len(), count);
    }
    #[test]
    fn failed_primary_extension_never_activates_companion_or_accepts_a_token_prefix() {
        let fixture = DriverFixture::available(&["GL_OES_texture_half_float","GL_EXT_color_buffer_half_float"]);
        fixture.rejected.borrow_mut().insert("GL_OES_texture_half_float".into());
        fixture.current.borrow_mut().as_mut().unwrap().insert("GL_OES_texture_half_float_linear".into());
        let mut enabled = HashSet::new();
        assert!(enable(&fixture, 1, &mut enabled, "OES_texture_half_float").is_none());
        assert!(enabled.is_empty());
        assert_eq!(*fixture.requests.borrow(), ["GL_OES_texture_half_float"]);
    }
    #[test]
    fn half_float_texture_without_rendering_support_has_no_companion_activation() {
        let fixture = DriverFixture::available(&["GL_OES_texture_half_float"]);
        let mut enabled = HashSet::new();
        assert!(enable(&fixture, 1, &mut enabled, "OES_texture_half_float").is_some());
        assert_eq!(enabled, HashSet::from(["OES_texture_half_float".into()]));
        assert_eq!(*fixture.requests.borrow(), ["GL_OES_texture_half_float"]);
        assert!(enable(&fixture, 1, &mut enabled, "EXT_color_buffer_half_float").is_none());
    }
    #[test]
    fn extension_availability_filters_version_driver_and_half_float_dependency() {
        let mut available = std::collections::HashSet::from([
            "GL_EXT_color_buffer_half_float".to_string(),
            "GL_EXT_color_buffer_float".to_string(),
            "GL_OES_vertex_array_object".to_string(),
            "GL_driver_private_extension".to_string(),
        ]);
        let one = available_extensions(1, &available);
        assert!(one.iter().any(|name| name == "OES_vertex_array_object"));
        assert!(!one.iter().any(|name| name.contains("color_buffer")));
        let two = available_extensions(2, &available);
        assert!(two.iter().any(|name| name == "EXT_color_buffer_half_float"));
        assert!(two.iter().any(|name| name == "EXT_color_buffer_float"));
        assert!(!two.iter().any(|name| name == "OES_vertex_array_object"));
        assert!(!two.iter().any(|name| name.contains("private")));
        available.insert("GL_OES_texture_half_float".to_string());
        assert!(available_extensions(1, &available).iter().any(|name| name == "EXT_color_buffer_half_float"));
        assert_eq!(available_extensions(1, &Default::default()), vec!["WEBGL_lose_context", "WEBGL_debug_renderer_info"]);
    }
    #[test]
    fn advertised_driver_extensions_have_unique_web_names_and_version_rules() {
        let mut names = std::collections::HashSet::new();
        for extension in EXTENSIONS {
            assert!(names.insert(extension.web));
            assert!(extension.driver.starts_with("GL_"));
            assert!(extension.version <= 2);
        }
        assert_eq!(
            EXTENSIONS
                .iter()
                .find(|e| e.web == "OES_texture_float")
                .unwrap()
                .version,
            1
        );
        assert_eq!(
            EXTENSIONS
                .iter()
                .find(|e| e.web == "EXT_color_buffer_float")
                .unwrap()
                .version,
            2
        );
    }
}
