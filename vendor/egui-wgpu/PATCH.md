# Vendored egui-wgpu 0.36.2

Upstream: <https://github.com/emilk/egui/tree/main/crates/egui-wgpu> (MIT OR Apache-2.0),
used through `[patch.crates-io]` in the workspace `Cargo.toml`. Drop this directory and the
patch entry when upstream gets an equivalent switch.

## Why

On a software rasterizer (WARP, "Microsoft Basic Render Driver": no graphics driver, a virtual
machine, a remote session) the hardware `textureSample` path costs ~130 ms per window-sized
quad on a 1080p window, whatever the shader does around it (measured with a bare egui example:
`textureLoad` instead of `textureSample` makes the same frame ten times faster, 7 times with
manual bilinear filtering). A full PhotoCraft window then took 350 ms per frame.

## What changed (all marked "software" in the code)

* `src/egui.wgsl`: pipeline-overridable constants `SW_SAMPLING`, `SW_NEAREST`, `SW_REPEAT`
  and `sample_texture_software`, which samples with `textureLoad`s (nearest, or manual
  bilinear; clamp or repeat). Off by default: the hardware path is untouched.
* `src/renderer.rs`: `RendererOptions::software_sampling`; with it on, four pipelines are
  built (one per nearest/linear × clamp/repeat variant) and `render` switches between them per
  texture (`sampling_variant`).
* `src/lib.rs`: `RenderState::create` turns it on for an adapter of `DeviceType::Cpu`;
  `EGUI_WGPU_SOFTWARE_SAMPLING=0` / `=1` forces the choice.

Pixel comparison against the hardware path on the same WARP device: max channel difference 1
(UI, transparent document with its repeating checkerboard).
