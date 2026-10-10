# Native graphics render repro

This local fixture captures WebGL 1/2 clear, shader triangle, textured quad,
alpha and resize output at 800×600 DPR 1. It retains raw RGBA, canvas PNG and
browser screenshots. A second scene checks transferred-canvas presentation,
author dimensions without changing bitmap pixels, and an SVG with no root
size (300×150). Timers and mutation observers remain active.

Install Playwright in a Python environment. Build the CLI with `render,webgl`
and optionally `stealth`, and prepare the pinned graphics bundle documented in
`graphics/README.md`. Start an owned loopback CDP server with explicit private
network access:

```sh
OBSCURA_WEBGL_LIB_DIR=/absolute/verified/bundle OBSCURA_WEBGL_BACKEND=software   target/release/obscura --stealth --allow-private-network serve   --host 127.0.0.1 --port 9333 --storage-dir /absolute/fresh/profile
python3 render-repros/webgl/run.py --endpoint http://127.0.0.1:9333 --out /absolute/fresh/native
python3 render-repros/webgl/run.py --out /absolute/fresh/chromium
```

Use `hardware` for the verified macOS Metal bundle. Omit `--stealth` only for
a separately labelled build/runtime configuration. Stop the owned server after
capture. The runner closes its contexts but does not stop an attached server.
Record binary/source/bundle hashes and exact build and launch commands alongside
outputs. Run each configuration twice into distinct directories.

An exact baseline without WebGL must use `--expect unavailable`: both GL context
requests must return null while Canvas2D still draws red. Do not substitute this
for candidate graphics acceptance. Baseline and candidate require separate
build targets. The baseline is not expected to render the new GL scenes.

The numeric checks validate readback and ownership, not screenshot fidelity.
Inspect paired browser screenshots: the quad has four distinct quadrants; the
triangle center is green over blue; red alpha blends over white; resize remains
blue; the author-resized placeholder retains red pixels and the SVG is blue at
300×150. Compare PNG content and screenshot placement before acceptance. Color
checks allow up to two byte values per channel for rasterization and conversion
rounding. Stealth applies no WebGL or canvas pixel perturbation, so this
tolerance is not fingerprint noise, and the harness never disables randomness.
The placeholder height records the known fractional CSSOM versus device-pixel
layout difference rather than claiming exact geometry parity.
This is bounded render evidence, not Khronos, IDL, fingerprint-distribution or
performance conformance. Generated output belongs outside the repository.
