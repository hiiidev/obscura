# ANGLE integration status for hiiidev/obscura

The source and pinned build recipe here are adapted from
[ice-zeus/obscura PR #18](https://github.com/ice-zeus/obscura/pull/18),
Apache-2.0.

This is the **first-stage port** of the native ANGLE backend into
feat/angle-metal-webgl-arm64-candidate. Bringing in
crates/obscura-webgl and its vendor-verified dependency recipe does **not**
yet make HTMLCanvasElement.getContext("webgl") functional in the app.
The downstream V8 bindings, Canvas surface ownership/paint path and feature
gating must be adapted to this repository's existing FrameRealm/Worker and
BrowserContext changes. The current webgl_software.js experiment is not
part of the native ANGLE path and must not be shipped.

Mac requirements: ANGLE Metal, a correctly pinned local
libEGL.dylib / libGLESv2.dylib bundle, and successful native
clear/triangle/readPixels screenshot tests. On Mac, the fork explicitly does
**not** provide SwiftShader software fallback.

See [graphics/README.md](../graphics/README.md) for the source-build recipe
and [graphics/VALIDATION.md](../graphics/VALIDATION.md) for acceptance checks.
Never publish a release until those checks and native Playwright tests pass.
