# Graphics validation handoff

All behavioral gates below are pending. The implementation phase authors
source, tests and cases; it does not run platform validation. Execute the
frozen candidate only in the later validation phase. Keep every failed log,
input, source/binary hash and reproduction. Code or test corrections return
to the implementation owner, then receive a new frozen revision and retest.
Do not merge, deploy or infer website-detection improvements from these gates.

## Source identity and build isolation

Record the exact base and candidate commits, complete source manifests,
Cargo.lock hash, graphics dependency lock and native bundle hash. Export only
tracked candidate files. Never include unrelated local files or another PR.
Build the pristine base and each candidate in separate fresh target directories.
If a directory has previously been used, remove only its owned build artifacts
before its first build; archive timestamps are not proof of Cargo freshness.

Record OS, architecture, CPU, GPU/driver, Rust/Cargo/nextest/Node/Python versions,
viewport, DPR, feature flags and all backend variables. Set `CARGO_BUILD_JOBS=2`
and `CARGO_INCREMENTAL=0` for reproducibility. Do not copy native libraries
between operating systems or architectures. Build pins with the recipe in
[README.md](README.md); retain the source checkout revisions and manifest.

Use `cargo nextest`, never `cargo test`, for native engine tests. Tests using
V8 require a separate process. The commands below assume the checkout is the
current directory and `CARGO_TARGET_DIR` names a fresh candidate-owned target.

## Authored tests and explicit driver selection

```sh
python3 -m unittest discover -s graphics -p test_build_dependencies.py
node --test crates/obscura-js/tests/webgl_bindings.cjs \
  crates/obscura-js/tests/offscreen_bindings.cjs
cargo nextest list --locked --release --workspace \
  --features obscura-cli/webgl,obscura-cli/stealth \
  --ignore-default-filter --run-ignored all --message-format json > test-inventory.json
cargo nextest run --locked --release --workspace \
  --features obscura-cli/webgl,obscura-cli/stealth \
  --ignore-default-filter --no-fail-fast
```

Compare the compiled inventory with the source test inventory before accepting
results. Candidate-only regressions must exist only on the candidate; never
use a candidate binary as the baseline. Record ignored tests and the reason
for every exclusion. The normal run above does not execute driver tests.

The native extension request boundary must compile and its existing
`extensions::tests::native_extension_entry_and_string_failures_do_not_publish_capabilities`
case must run on Linux aarch64 (unsigned `c_char`) and a signed-`c_char` target,
such as macOS or Linux x86_64. Its typed callback must accept `CString::as_ptr()`
without a hardcoded signed-byte pointer. Keep separate target directories and
record the actual Rust target triple for both results.

The dependency recipe explicitly runs the pinned depot_tools `ensure_bootstrap`
with automatic checkout updates disabled, verifies its revision and tracked
files afterward, and checks the Python wrapper used by GN. Retain bootstrap
stdout/stderr and toolchain architecture evidence. Bootstrap failure, a moved
pin, modified tracked tools or an unusable Python wrapper must stop before
ANGLE sync/build and must never publish a bundle. A bootstrap pass does not
prove host compiler, GN or native graphics compatibility.

Linux must also install the native architecture's sysroot using the installer
and checksum metadata from the exact ANGLE dependency checkout before GN.
Record the installer/metadata hashes and selected tarball SHA-256 in the bundle.
Exercise ARM64 and x86_64 selection, installer failure and the macOS no-op;
never disable GN's sysroot requirement to make a failing build proceed.

For each native backend, set `OBSCURA_WEBGL_LIB_DIR` to its verified bundle.
Set `OBSCURA_WEBGL_TEST_BACKEND=metal|vulkan|swiftshader` for native crate
fixtures and `OBSCURA_WEBGL_BACKEND=hardware|software` for browser fixtures.
Use `metal/hardware` on Mac, `vulkan/hardware` on Linux hardware and
`swiftshader/software` on GPU-less Linux. Then run all explicitly ignored
graphics fixtures in a separate pass:

```sh
cargo nextest list --locked --release -p obscura-webgl -p obscura-js \
  --features obscura-js/webgl --run-ignored only --ignore-default-filter \
  -E 'test(/^driver_tests::/) | test(/^runtime::webgl_tests::/)' \
  --message-format json > graphics-driver-inventory.json
cargo nextest run --locked --release -p obscura-webgl -p obscura-js \
  --features obscura-js/webgl --run-ignored only --ignore-default-filter \
  -E 'test(/^driver_tests::/) | test(/^runtime::webgl_tests::/)' \
  --test-threads 1 --no-fail-fast
```

Every driver fixture in the frozen source inventory must be selected. A zero
count, missing fixture or skipped driver case is a failed gate. Existing
unrelated opt-in tests require their own documented configuration; they must
not be relabeled as driver coverage.

## Required backend campaigns

1. **Mac Metal:** forced hardware and auto, both WebGL versions. Record the
   actual backend and renderer. Software is unsupported on Mac.
2. **Linux without GPU access:** use an isolated VM/container/device policy
   with `/dev/dri` and other GPU devices unavailable. Remove DISPLAY and
   WAYLAND_DISPLAY. Record device mounts, permissions and the denied-device
   condition; merely unsetting DISPLAY is insufficient. Auto must attempt
   hardware, preserve its failure reason, then render through SwiftShader.
   Repeat forced software. Forced hardware must return null. Both versions,
   rendering, context loss and repeated-profile cleanup must run here.
3. **Linux hardware:** run separately with a supported GPU/driver. Auto and
   hardware must select the hardware backend and render. Software results and
   Mac results cannot establish this path. If no suitable host exists, retain
   the gate as pending and make no Linux-hardware compatibility claim.
4. **Both backends unavailable:** in fresh processes use copies of the bundle
   with missing EGL/GLES, wrong hashes, wrong pins/platform, missing SwiftShader
   or ICD, missing required symbols/extensions and explicit failed context
   creation. On GPU-less Linux, auto must return null after retaining both
   reasons; Canvas2D and the browser process remain usable. Never modify the
   shared validated bundle for a fault case.

## Changed behavior and failure-path cases

The following is an authored conformance contract. Expand each named operation
family over both supported versions and backends. For every validation guard,
exercise an accepted value and each rejected class; for each state dispatch,
exercise every supported branch and its default/unsupported branch. Record the
actual error, returned type, resource ownership and unchanged state after
rejection. Boundary tests must use below/at/above values, not just a valid case.
The source-linked campaign coverage map records individual changed locations.
Measured branch coverage remains pending until execution and instrumentation.

- **DOM-NOMODULE:** parser, dynamic and frame scripts; boolean attribute values,
  module/import-map/classic types, inline/external, async/defer, before/after
  connection and removal after preparation. Require one initialization, no
  legacy fetch, valid mutation ordering, observer records, connected state and
  NotFoundError. Preserve the native tree's cycle guards. The reduced baseline
  error establishes duplicate removal, not proven native-tree corruption.
- **PRIVATE-STATE:** main realm, same-origin child and replaced navigation;
  direct access, descriptor, own-key and symbol enumeration; absent state,
  page-created former names and hostile setters. Exercise fulfilled/rejected
  promises, synchronous/async CDP evaluation, supplied function declarations,
  mouse down/up/click targets, viewport and screen updates. Page properties
  cannot read or overwrite the host state. A sealing/allocation failure must
  fail initialization without publishing the temporary handoff.
- **BUNDLE-RECIPE:** clean/existing/foreign/dirty workspaces; exact/different
  revisions, malformed ownership marker, invalid jobs/platform, overlapping
  work/output and existing output. Exercise failed fetch/gclient/GN/build,
  absent/duplicate outputs, revision mismatch, copy/license/manifest/rename
  failure. Require no final bundle before every input is verified; retain
  failed staging and do not overwrite unrelated files. The offline recipe
  tests cover decisions; actual native builds remain a platform gate.
- **BUNDLE-LOAD:** explicit/default directory; absent manifest, malformed JSON,
  wrong schema/pins/OS/arch, unknown filename, escaping paths/symlinks, missing
  file, unreadable file and checksum mismatch. Require a controlled error
  before loading unverified bytes. Native symbols, null extension strings,
  required extension absence and poisoned cache are separate fault cases.
- **BACKEND-EGL:** supported/unsupported platforms, auto/hardware/software,
  caveat true/false, every hardware initialization failure, successful hardware
  without software creation and exhausted fallback. Exercise config count
  zero/negative/over-limit, unreadable config attributes, eligible/ineligible
  alpha/depth/stencil/MSAA configs, create/make-current/query failures and
  resize rollback. Inject failure after each acquired handle and destructor
  failures; require exact-once release and preserved previous surface.
- **RESOURCE-IDENTITY:** each object kind, null/foreign/wrong-type/deleted/stale
  handles, native-name reuse, browser-ID exhaustion, attached/deferred shader
  and program deletion, relinked locations, active queries/transform feedback,
  bound containers and finalizers. Failed native deletion cannot mark a live
  wrapper deleted; predicates retain their boolean/error-preserving rules.
- **COMMANDS:** every Command and ResourceCommand dispatch arm: state setters,
  clears, drawing, indexed/instanced draws, vertex attributes, bindings,
  attachments, copies, invalidation, sync/query/feedback/sampler operations.
  Check WebGL1/2 gates, unsupported desktop enums, negative sizes/counts,
  offset/stride/alignment/overflow and incomplete/unlinked state. Preserve
  page FBOs, masks, scissor and active texture. Unsupported extension tokens
  must reject until enabled. Compare observable state with the pinned WebGL
  conformance suite; native GL acceptance alone is not the browser oracle.
- **UNIFORMS-QUERIES:** every scalar/vector/matrix/sampler/bool shape and query
  return type, row/column counts, transpose, short/empty/oversized lists,
  location/program ownership, relink generations and loss. Query invalid
  indices/enums, missing versus unlinked resources, native-name tombstones,
  default framebuffer and hidden internal attachments. Test location limits
  256/1024 with below/at/above lengths, all ASCII classes, non-ASCII, NUL and
  reserved prefixes; WebGL2 non-location names use their distinct rules.
- **PIXEL-TRANSFERS:** typed arrays, PBO offsets and DOM/image/bitmap sources;
  each format/type including packed/float/half, null data, subimages, 2D/3D,
  pack/unpack row/image lengths, skips, alignment, zero sizes, checked integer
  overflow and insufficient source/destination. Preserve padding, source
  arrays, bindings and pixel-store state. Detached/shared/resizable/wrong
  typed arrays, stale buffers and reentrant coercion must not reach unsafe
  native memory. Test loss/errors without partial writes.
- **DRAWING-STORAGE:** owned/default/page FBOs, RGBA8/sRGB/RGBA16F, depth/stencil,
  MSAA resolve, alpha modes, logical BACK/NONE, read/draw selection, attachment
  queries, copy/blit/invalidation and PBO readback. Inject each partial
  allocation/reserve/activation/completeness failure; preserve old storage
  until replacement succeeds. Verify actual storage dimensions versus canvas
  intrinsic dimensions, zero canvas versus explicit zero allocation, resize,
  loss and restore. Account for retained pbuffer and temporary float buffers.
- **EXTENSIONS:** each allowlisted extension and required companion, absent,
  supported, requestable, enabled, rejected activation and failed dependency.
  Check repeated/case-insensitive queries, constants/descriptors, extension-only
  entry points/tokens, version promotion and restoration. Rejected activation
  must not update the browser-enabled set or leak driver-private extensions.
- **IMAGE-ORIGIN:** exact image bytes, request profile and full redirect chain;
  same-origin, CORS success/failure, data/opaque origins, unloaded/evicted images
  and replacements. Decoding never performs an extra resource request. A tainted
  source cannot become readable after a cache hit, bitmap conversion or transfer.
- **IMAGE-DECODE:** supported raster/SVG/SVGZ, malformed and unsupported input,
  dimension/decoded-byte/encoded-byte/decompression/node/depth/recursion limits,
  wrong destination size, embedded data resources, blocked external files and
  fonts. Test transparent RGB/alpha and no destination modification on failure.
  Include ICC/EXIF, animated images and video as explicitly retained limitations;
  do not mark unsupported media as conformance passes.
- **COLOR-IMAGEDATA:** sRGB/P3, invalid enums, alpha/premultiplication modes,
  raw numeric versus browser output, NONE conversion, extended float values,
  same/changed color spaces and allocation failure. ImageData constructors,
  dimensions, typed-array offsets, detached/shared/resizable buffers, private
  metadata, page shadows, cross-realm objects, dirty rectangles and coercion
  that resizes/taints/detaches are distinct cases. Float16 rejection stays visible.
- **BITMAP-OFFSCREEN:** branded/fake/closed sources; crop sign and bounds,
  resize dimensions/quality, flip/alpha/color options, Blob decode/rejection,
  taint, zero size, allocation failure, promise task completion and navigation
  cancellation. Transfer consumes only a successfully copied bitmap. Failed
  conversion or destination validation preserves source ownership and pixels.
  Record foreign-realm and worker transfer limitations separately.
- **PLACEHOLDER-PRESENTATION:** one transfer per HTML canvas, private ownership,
  intrinsic/CSS size and layout constraints, container queries, hidden/reveal,
  object-fit, zero axes, lost/retired source, stale revision and failed snapshot.
  Verify raw P3 versus display sRGB, budget accounting for both, committed-frame
  color after context changes, coalescing and no retry polling. Inject reserve,
  source-map and conversion-copy failures; no half-published frame or consumed
  failed drawing buffer. Require cleanup on replacement, GC and navigation.
- **LOSS-CLEANUP:** first/repeated loss, typed defaults, one loss error, event
  cancellation/restoration, queued events after frame removal, owner-generation
  mismatch, busy native state, synchronous/async/cancelled runtime boundaries,
  repeated GC and profile teardown. Weak callbacks cannot retain the document;
  temporary contention defers cleanup without spinning or dropping requests.
- **PRESENTATION-ORDER:** readPixels, toDataURL, bitmap transfer and screenshot
  before/after posted tasks and rendering opportunities, in main/child frames
  and offscreen placeholders. Require the specified clearing and ownership
  behavior with preserveDrawingBuffer on/off. Failed captures do not consume
  the drawing buffer; idle canvases do not repeatedly read back.
- **STEALTH:** exact baseline/candidate with runtime `--stealth`, HTTP/TLS,
  navigator/descriptors, canvas/WebGL/audio/font/geometry and timing/observers.
  Compare random surfaces by consistency and distributions without fixing or
  disabling randomness. Retain actual GPU/software versus emulated-device
  inconsistencies. Website success does not establish fingerprint compatibility.

For V8 private-slot failures and allocator failures not exposed to pages, use
a fault-injection build or debugger at the named allocation site. Preserve the
unmodified release candidate as the behavior baseline. The injection manifest
must name the exact source line, injected return/failure occurrence and expected
cleanup state. Do not induce uncontrolled machine-wide OOM or call a missing
symbol through a null function pointer. Fault-driver results do not replace
real-driver rendering or repeated-profile measurements.

## Full engine, rendering and performance gates

After the candidate is frozen, run the full relevant render+stealth and render
engine suites and release CLI builds, plus no-render and no-render+stealth
DOM/JS/browser/CDP suites and CLI builds. Keep each target directory isolated.
Run the upstream obstacle course from a recorded exact companion revision.
Keep all baseline failures and reference-fixture exceptions; do not waive a
candidate-only regression. Required release build examples are in root AGENTS.md.

At 800x600, DPR 1, fixed identity and animation/settle boundary, compare real
WebGL1/2 clear, shader triangle, textured quad, alpha, resize and loss fixtures.
Save actual readPixels, PNG, screenshot and CDP output. Confirm nonblank
successful navigations, then inspect geometry, orientation, colors, clipping,
alpha and resources. Run the deterministic and broad top/bottom rendering
harnesses with paired baseline/candidate and Chromium references:

```sh
OBSCURA_BIN=/absolute/candidate/obscura BASELINE_BIN=/absolute/base/obscura \
  render-repros/run.sh /absolute/evidence/fixtures
OBSCURA_BIN=/absolute/candidate/obscura \
  render-repros/representative-suite/run.sh /absolute/evidence/top
OBSCURA_BIN=/absolute/candidate/obscura \
  render-repros/representative-suite/run.sh /absolute/evidence/bottom bottom
```

Run the pinned Khronos WebGL conformance revision from dependencies.lock.json,
including supported extensions and IDL/API validation. Report subtest counts,
not whole-file percentages. Never label skipped or unsupported cases passed.

For performance, interleave exact baseline, each independent candidate and the
combined candidate using frozen inputs. Keep JS, images, rendering, timers and
observers enabled. Use 1 and 10 profiles, cold/warm backend phases and a fixed
post-load observation window. Record p50/p95 latency, total process CPU time,
peak/steady RSS, file descriptors and readback counts. Repeat 100 and 1,000
profile lifecycles after warmup, retaining resource-growth slopes and raw data.
Separate software and hardware results. Include pages that do not request WebGL
and pages that actively draw; an earlier load event is not proof of less work.

No live Google/proxy/solver campaign is required by this implementation handoff.
If later authorized, it has its own fresh-SID campaign and no IP verification.
It cannot replace deterministic rendering or CPU comparisons.
