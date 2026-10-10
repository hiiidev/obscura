# Optional ANGLE graphics

The `webgl` Cargo feature adds native WebGL 1 and WebGL 2 contexts through
ANGLE. It implies `render`; it does not change the existing default feature
sets. A build without this feature retains the previous context behavior.

This implementation is a validation candidate. Native graphics, rendering
conformance, resource costs and platform compatibility have not yet been
validated. Compiling Rust or parsing the bindings does not validate ANGLE.
See [VALIDATION.md](VALIDATION.md) for the required gates.

## Build and install the graphics bundle

Install the native build prerequisites described by
[ANGLE](https://chromium.googlesource.com/angle/angle/+/main/doc/DevSetup.md).
Build on each destination operating system and architecture. A Mac bundle
cannot validate Linux, and a software backend does not validate hardware.

```sh
python3 graphics/build_dependencies.py \
  --work-dir /absolute/path/to/graphics-build-work \
  --output /absolute/path/to/graphics-bundle \
  --jobs 2
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 \
  cargo build --locked --release -p obscura-cli --bins --features webgl,stealth
```

Both paths belong to the recipe. The work directory must be empty or already
owned by the same dependency pins. The output must not exist and must not
contain, or be inside, the work directory. Failed build/staging evidence is
retained; the recipe does not overwrite an existing bundle. The dependency
lock pins ANGLE, its matching SwiftShader revision and depot_tools. The
result includes library hashes, upstream licenses and build arguments.
Linux also includes the exact built `libvulkan.so.1`: this pinned ANGLE loads
Vulkan from its own module directory, including when selecting SwiftShader.
A system Vulkan loader does not satisfy this bundle dependency.

Set `OBSCURA_WEBGL_LIB_DIR` to this bundle's absolute directory. Alternatively,
install it as `webgl/` next to the Obscura executable. The loader verifies the
manifest's platform, architecture, pins and each loaded library's hash. It
does not substitute a system GLES library when verification fails.

## Backend policy

Set `OBSCURA_WEBGL_BACKEND` before starting a browser process:

| Value | macOS | Linux |
| --- | --- | --- |
| `auto` (default) | ANGLE Metal | ANGLE Vulkan hardware, then bundled SwiftShader on initialization failure |
| `hardware` | ANGLE Metal only | ANGLE Vulkan hardware only |
| `software` | Unsupported | Bundled SwiftShader only |

Linux uses the offscreen Vulkan platform without X11, Wayland or GBM. A usable
GPU and driver are required for the hardware path. GPU-less Linux uses CPU
rendering; it has additional CPU and memory costs that must be measured.
`failIfMajorPerformanceCaveat: true` excludes software fallback. Context or
configuration initialization failures participate in fallback; success means
a usable native context was created, not merely that a display was found.

Invalid policy, missing libraries or exhausted backends cause `getContext()`
to return `null` and produce a context-creation error event. Creation failures
and attempted backends are logged by the host. Debug logging records the
selected backend and actual renderer. Diagnostic driver strings are not a
promise that a GPU matches the emulated browser/device identity. With runtime
stealth, page-visible `WEBGL_debug_renderer_info` values come from the
document's seeded GPU pool for the stealth platform, and `VERSION` and
`SHADING_LANGUAGE_VERSION` use Chrome's format. Limits, extensions, shader
precision and pixels still come from the actual backend and are not masked.

Native libraries and displays are initialized lazily and reused within the
process. Their discovery is cached; install or change a bundle before launch,
then restart the process. Changing environment variables in a running page is
not a backend-switch API.

## Canvas behavior and ownership

The bindings validate resource ownership, version gates, enums, typed-array
bounds and image origins before issuing native operations. Contexts and native
objects are document-owned. Resize, loss, restoration, deletion, GC and frame
teardown retain their browser-visible state and ownership rules. Failed
allocation must not publish a partially constructed drawing buffer.

Dirty HTML canvases contribute pixels at presentation/capture boundaries.
Standalone OffscreenCanvas contexts do not trigger document-paint readbacks.
PNG serialization, screenshots, ImageBitmap transfer and transferred HTML
placeholders use explicit snapshots. There is no graphics polling loop.
`preserveDrawingBuffer` controls presentation clearing; bitmap transfer consumes
the buffer independently of that option.

sRGB and Display P3 are supported on the implemented context/source paths.
Page `readPixels` and numeric texture uploads preserve numeric values. Browser
image output is RGBA8 sRGB. A P3 transferred placeholder retains a raw source
copy and an sRGB display copy under the snapshot budget. The extra conversion,
copy and temporary allocations require separate CPU/RSS measurements.

## Current limits

- Encoded image uploads use bounded RGBA8 decoding at encoded dimensions,
  including only the first raster frame. Full ICC/EXIF, animation/video and
  HDR browser-image handling are not implemented. Video uploads are rejected.
- RGBA8 ImageData has private metadata and cross-realm ownership checks.
  `rgba-float16` ImageData is rejected; native half-float drawing storage and
  floating-point reads do not imply float ImageData or HDR serialization.
- Main-realm OffscreenCanvas and ImageBitmap paths are implemented. General
  worker/structured-clone graphics transfer and complete foreign-realm
  canvas/ImageBitmap source handling are not claimed.
- Supported extensions are the browser allowlist intersected with actual
  driver/version support. Compression extensions are not advertised;
  compressed uploads reject unsupported formats. Internal ANGLE extensions
  used for browser storage are not automatically exposed to pages.
- Desynchronized presentation and power-preference selection are not promised.
  Actual attributes report the supported configuration.
- Context creation is bounded to 16 contexts per document. Transfers and
  retained placeholder storage are bounded. Images have encoded/decoded size
  limits. Resource-limit failures are part of validation, not proof of leaks.
- The EGL pbuffer remains allocated alongside custom drawing storage. Measure
  that cost, P3 copies and software rendering before acceptance.
- Generic stale non-canvas DOM wrappers and existing Canvas2D limitations are
  not fixed by the new canvas ownership guard.

These limitations must remain visible in comparisons and release decisions.
A working triangle, completed scrape or stealth-feature build does not prove
full graphics conformance or resolve website detection.
