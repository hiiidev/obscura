# ANGLE integration status for hiiidev/obscura

The source and pinned build recipe here are adapted from
[ice-zeus/obscura PR #18](https://github.com/ice-zeus/obscura/pull/18),
Apache-2.0.

This is the **V8/Canvas integration candidate** for the native ANGLE backend in
feat/angle-metal-webgl-arm64-candidate. Bringing in
crates/obscura-webgl and its vendor-verified dependency recipe does **not**
guarantee a working WebGL context without the validated ANGLE Metal native bundle.
The feature webgl activates the Rust/V8 bindings and Canvas pixel pipeline,
but real context creation, shader compilation, draw/readPixels, resource
lifetimes and Playwright behavior all require native Metal bundle testing.
Image-origin handling currently fails closed on non-data: image URLs until
response provenance is integrated. OffscreenCanvas is also not yet enabled.
The webgl_software.js prototype is not in the native ANGLE build.

Mac requirements: ANGLE Metal, a correctly pinned local
libEGL.dylib / libGLESv2.dylib bundle, and successful native
clear/triangle/readPixels screenshot tests. On Mac, the fork explicitly does
**not** provide SwiftShader software fallback.

See [graphics/README.md](../graphics/README.md) for the source-build recipe
and [graphics/VALIDATION.md](../graphics/VALIDATION.md) for acceptance checks.
Never publish a release until those checks and native Playwright tests pass.
