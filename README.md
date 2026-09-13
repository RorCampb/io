# io

A native large-world prototype. Rust owns world state, cameras, visibility, and
simulation. C owns the SDL window and OpenGL resource submission.

## Build and Run

Requires Rust, Apple's command-line tools, and SDL2 installed under
`/opt/homebrew`. The renderer requests OpenGL 4.1 core on macOS.

```sh
make run
```

For an optimized build:

```sh
make release
./build/release/io
```

## Controls

For a large mixed scene, run `./build/release/io --scene assets/district/huge.json`.
It contains 80,446 items across 1,024 blocks, including 8,192 moving characters,
7,280 houses, 29,108 trees, and market props. See [Huge District](docs/huge-district.md)
for generation and benchmark commands.

- Left-drag: orbit and tilt around the camera target.
- Right/middle-drag or arrow keys: move the camera target across the map.
- Scroll or `+` / `-`: zoom.
- `[` / `]`: decrease/increase render and simulation distance.
- `N`: duplicate the active camera and select the copy.
- `Tab`: cycle cameras. Other cameras retain their settings.
- `R`: restore the active camera's starting view and follow target.
- `F`: toggle camera following; panning detaches the camera.
- `G`: toggle the debug grid.
- `Q` / `A`, `W` / `S`, `E` / `D`: adjust subdivisions per world unit on A/B/C.
- `Esc`: exit.

The title shows the active camera, visible/total items, spatial-query candidates,
active simulated characters, target coordinates, and render distance.

## World and Placement

The default demo contains a street, houses, lamps, trees, and an animated runner:
39 items in a 10,000 x 10,000 x 256 world. Instances share reusable models; empty
cells allocate no item state. Assets, placements, the runner's route, and camera
settings are defined in `assets/street-kit/demo.json`, not engine source.
World dimensions, item positions, and item sizes use stable world units.
Changing subdivisions changes grid detail without moving or resizing
items. The floor grid adapts its displayed spacing to zoom and is generated
locally rather than for the entire map. C subdivisions are retained as metadata
for future placement/snapping; they do not change the floor grid.

An item has identity and placement plus optional rendering, durability, movement,
animation, and depletion-response components. See [Item Components](docs/item-components.md)
for the runtime contracts and legacy scene adapter.
Its transform is `translation * rotation * scale`. The anchor locates the
model's authored local origin. Bounds account for rotation
and extent when selecting visible items, including items whose anchors are
outside a query region. Bounds are not yet a collision/placement validator.

## Cameras and Visibility

Cameras are independent orthographic views. Render distance is a world-unit
radius around the camera's target, not a perspective far plane. It is
configurable per camera through the API and clamped to 8-20,000 world units.
Zoom changes the visible region independently of that radius.

Rust first queries a sparse spatial index, then checks candidate bounds against
distance and the camera's projected viewport. The index uses 32-unit buckets as
an internal lookup aid; this is not a visibility limit. Items crossing bucket
boundaries are deduplicated, and very large bounds use a separate overflow list.

The older 40,400-item box world remains a test fixture, not the default scene.
Spatial-query workload counts are not a frame-rate benchmark.

## Simulation and Retained State

Rust runs a fixed 30 Hz simulation, advancing route movement and animation clocks.
Configured animation events queue gameplay effects; the queue is applied after
all selected items update. Health and positions remain in world state even when
the item is not exposed to a renderer. See [Animation Effects](docs/animation-effects.md)
for the trait boundary, JSON configuration, and a tested damage example.

The union of all cameras' distance regions, plus followed items, determines active items. An
item in overlapping regions is updated once per tick. Each defined camera
currently contributes even when it is not selected in the single window.
Items outside all camera regions pause unless followed; returning resumes their
retained clocks without simulating the missed time. Events from an active source
can still affect an inactive target. Independent background simulation scheduling
is not implemented yet.

Elapsed-time input is capped at 0.25 seconds per call and at eight ticks to
avoid an unbounded catch-up after a stall. Simulation selection is cached until
camera regions or the spatial index change. Moving items update the spatial index
and invalidate that selection.

World state and cameras are retained in memory for the session, not saved to
disk. Scene JSON is loaded at startup; assets are not streamed or hot-reloaded.

## Rendering Boundary

```text
SDL events -> input actions -> Rust world/cameras
                                  |
                 spatial query + visibility + appearance/LOD
                                  |
                     model IDs + transforms + camera matrix
                                  |
                        C GPU buffers + instanced draw
```

Rust exposes an `IoFrame` containing visible instances, a camera matrix, joint
matrices, and local debug-grid vertices. C does not choose render distances, visibility, LOD, or
simulation regions. It resolves the model IDs supplied by Rust, uploads each
model once, and batches instances sharing a model into one instanced draw.
Changed frame packets update instance/grid buffers; unchanged packets reuse them.

Appearances separate game objects from concrete meshes. Each camera selects an
LOD using projected size and hysteresis; visual states can choose different LOD
groups without changing placement or animation clocks. See
[Render Variants](docs/render-variants.md) for the schema, rig compatibility rules,
and a demo with low-detail houses, lamps, trees, and an animated runner:

```sh
./build/io --scene assets/street-kit/variants-demo.json
```

Instance records, joint matrices, and debug-grid vertices use the C
`DynamicBuffer` helper. Capacity is allocated on first use, grows geometrically,
and does not shrink during normal rendering. Changed packets orphan the old
store with `glBufferData(..., NULL, GL_STREAM_DRAW)` and upload just the used
bytes with `glBufferSubData`. Orphaning may allocate fresh driver storage even
when capacity is unchanged; this is not persistent mapping or a guarantee of
stall-free uploads. Empty instance/grid streams upload nothing. An empty joint
stream uses one identity matrix to keep the texture buffer valid.

Shutdown logs report each stream's capacity, peak used bytes, total uploaded
bytes, upload calls, capacity growths, and orphanings. These are logical buffer
and transfer counts, not measurements of physical GPU memory or frame speed.
`Renderer.last_upload_bytes` reports the latest draw's uploads (zero when its
packet is already cached). Each view's packet uses Rust's global frame serial;
switching views replaces the streams as needed. No additional CPU frame copy
or GPU handle is exposed to Rust. Skeletal buffer capacity respects the device's
texture-buffer limit.

Vertices are transformed on the GPU. A geometry shader expands line segments
to triangles for consistent screen-space width, with edge coverage in the
fragment shader and MSAA where available. The window uses native Retina pixel
dimensions, separately from its logical viewport. The renderer now draws solid
triangle models with depth testing; the debug grid remains wire-based. Material
base colors and emission are supported, with simple directional lighting and
GPU skeletal skinning. Textures, shadows, and full PBR materials are not implemented.

## Blender Models

For reusable, tool-independent asset delivery, use [Asset Packages](docs/asset-packages.md).
Packages declare meshes, clips, appearances, and capability requirements; scenes
import them by namespace without engine changes. Validate without opening a window:

```sh
cargo run --bin io-validate -- package assets/street-kit/package.json
cargo run --bin io-validate -- scene assets/street-kit/package-demo.json
```

Export models and skeletal clips from Blender as glTF 2.0 (`.glb`). Add an asset
name/file entry to the scene JSON and reference that name from an item. Asset paths
are relative to the scene file. Item positions are relative to the scene's origin;
the camera target is also relative to that origin. Imported authored dimensions are
preserved, with Y-up converted to Z-up; item scale defaults to one. Scene loading
reports unsupported assets and unknown references rather than substituting cubes.

Select another scene without changing source:

```sh
make
./build/io --scene assets/street-kit/effects-demo.json
```

`IO_SCENE` also selects a scene. Restart after edits: there is one asset registry
per process. Named skeletal clips support linear and step keys, looping playback,
and independent per-item clocks. One-shot death playback holds the final pose.
Clip blending, general action controllers,
cubic-spline keys, morph animation, and physics are not implemented yet.
The original `assets/demo.glb` donut and legacy unit-box-normalizing importer are
retained, but are not the current scene-loading path.

The current GPU API follows the
[OpenGL instanced vertex specification](https://www.khronos.org/opengl/wiki/Vertex_Buffer_Objects).
The macOS build uses the
[OpenGL 4.1 core profile](https://developer.apple.com/documentation/scenekit/scnrenderingapi/openglcore41).

## Workspace

| Crate | Responsibility | Local dependencies |
| --- | --- | --- |
| `crates/io-types` | Shared vectors and bounds | None |
| `crates/io-world` | Items, world units/subdivisions, spatial queries, retained simulation state | `io-types` |
| `crates/io-assets` | Shared meshes, glTF import, versioned asset delivery contract | `io-types` |
| `io` (root) | App coordination, cameras, visible frame packets, demo setup, C ABI | All three |

An `Item` lives in `io-world`; its optional `Renderable` component references an
appearance and visual state using typed IDs from `io-types`. The application resolves them through `src/model.rs`
and `src/appearance.rs`, which load meshes and appearance definitions from scene
data. `src/demo.rs` validates placements and resolves appearance, clip, and
effect-target names. These modules do not know about street
assets or the demo layout.

The world uses stable world units; subdivisions describe placement/grid detail.
The current asset importer preserves authored mesh-local dimensions; item transforms
place and scale those coordinates in world space. Cameras stay outside the world
crate so multiple views can observe the same retained state.

Item components hold state; reusable skeletons and animation clips belong with assets.
The `Effect` trait defines gameplay behavior in `io-world`, with `Damage` as the
first implementation. Animation event bindings live with each animation player,
not in mesh data or the renderer. This is an incremental component-based design,
not a full ECS or a general physics/combat system.

Other application modules:

- `src/camera.rs`: camera target, orientation, zoom, distance, and view matrix.
- `src/appearance.rs`: visual states, LOD groups, thresholds, and asset compatibility.
- `src/projection.rs`: per-camera LOD selection, resolved instances/palettes, and local grid generation.
- `src/app.rs`: actions, camera collection, frame caches, and simulation scheduling.
- `src/lib.rs` / `include/io.h`: the C ABI.
- `csrc/input.c`: input translation; `csrc/main.c`: native lifecycle and loop.
- `csrc/renderer.c`: GPU resources, shaders, and draw submission.

The core Rust modules forbid unsafe code. The FFI wrappers declare pointer
contracts as `unsafe extern`; C must pass live handles and writable outputs.
Frame arrays are borrowed until the next mutable app call or destruction.
Model arrays are immutable for the process lifetime. Never free either from C.
`io_app_item_state` returns an owned copy whether or not the item is visible.
See the [Rust Contract Audit](docs/rust-contract-audit.md) for constructor/mutation
guarantees, remaining trust boundaries, and why numeric validation still uses
conditions alongside exhaustive enum dispatch.

## Verification

For repeatable performance and load sweeps, see [Benchmarking](docs/benchmarking.md).
To watch each scene as it runs:

```sh
python3 tools/benchmark_engine.py --cases animated --counts 25 100 250 --watch
```

Enter advances to the next run after inspection; Esc stops the sweep. Preview
and inspection time are excluded from measurements.

```sh
make test
make gpu-test
make smoke
cargo clippy --workspace --all-targets -- -D warnings
```

`make test` runs every crate's tests plus the C ABI/input test. `make run` and
`make release` still produce the same native executable; crates link into one
Rust static library. Source and manifest changes in member crates trigger rebuilds.

`make gpu-test` needs native GUI access and uses a hidden OpenGL window. It reads
back GPU buffer contents and checks geometric growth, smaller/empty/repopulated
frames, cached packets, switching view packets, overflow rejection, and recovery
after a partial upload failure. It is a correctness test, not a performance benchmark.

Tests cover stable subdivisions, bounds, spatial queries, camera independence,
zoom/distance selection, paused state retention, simulation deduplication, and
the C packet layout/input path. The native smoke test exercises GPU drawing and
camera changes, reads back pixels, resizes the window, and exits. It writes
`build/smoke.ppm` for visual inspection. GUI access is required for that test.
