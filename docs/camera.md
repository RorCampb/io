# Hybrid Camera

The dungeon now uses [collision-bounded orbit](#collision-bounded-orbit): player
angles and distance, fixed FOV, no automatic room steering. The earlier
[room-rig experiment](editor.md#geometry-orbit-prototype) remains an optional
authoring mode for comparison, not the dungeon's active camera.
Authored rigs can also transition `fov_degrees` independently of zoom. This widens
the view at a fixed fully-perspective eye distance rather than cancelling the lens
change with a dolly; its contribution fades out at the orthographic endpoint.

The camera can keep the distant isometric/tabletop view and continuously acquire
perspective as the user zooms in. Existing scenes remain orthographic unless
they opt in. World units, transforms, physics, animation and gameplay are unchanged.

## Try It

From the repository root:

```sh
make release
./build/release/io --scene assets/camera/zoom.json
```

The preview places the imported Mixamo character between repeated pillars so
depth and convergence are easy to compare. It requires the previously imported
`assets/action-adventure` package. The character walks in place; this is a camera
preview, not a playable dungeon.

- Scroll or +/- to zoom. Perspective starts above zoom 1.5 and is complete at 5.
- Left-drag to orbit/tilt, independently of zoom.
- On MacBook trackpads, two-finger movement orbits and pinch zooms in all scenes.
- Right/middle-drag or arrows to pan across the map.
- R restores the initial view. N creates a separate camera; Tab cycles cameras.

These are the existing non-game controls. Game scenes retain right-drag/arrow
orbit and camera-relative WASD. There is no macOS Touch Bar-specific integration.

## Scene Contract

Add a `projection` field to a scene's `camera`:

```json
"projection": {
  "type": "zoom_perspective",
  "start_zoom": 1.5,
  "end_zoom": 5,
  "vertical_fov_degrees": 45,
  "near_clip": 0.05,
  "smoothing_seconds": 0.16
}
```

All five parameters are required for this mode. Values must be finite:
start/end are within 0.025..16 with at least 0.01 separation, FOV is 20..100
degrees, near clip is 0.001..1 world units, and smoothing is 0..2 seconds.
Unknown fields are rejected. Omit projection or use `{"type":"orthographic"}`
to retain the previous immediate orthographic zoom.

Smoothing is an exponential time constant, not a fixed transition duration.
Zero disables temporal easing. Scroll adjusts a bounded desired zoom; each
presentation update eases log-zoom toward it, independently of simulation ticks.
Reversing scroll reverses the transition without queuing another animation.
Direct scene/map zoom assignments and reset remain immediate.

## Projection and Ownership

`src/camera.rs` owns pose, zoom intent and validated projection settings.
`src/camera_projection.rs` prepares the shared geometry calculations. The frame
builder prepares one view per frame, not one frustum per item. C still consumes
the existing 4x4 matrix; no FFI layout change or new simulation messages are needed.

For camera-local coordinates, z points toward the eye. If h is the target-plane
half-height, b is a smoothstep blend over log-zoom, and f is the configured FOV:

```text
k = b * tan(f / 2) / h
w = 1 - k * z
screen_x = x / (half_width * w)
screen_y = y / (half_height * w)
```

At b=0, w=1 gives orthographic projection. At b=1 this is ordinary perspective;
the implied eye is `target + forward / k`. At every blend value the target plane
keeps the scale prescribed by zoom, avoiding a size jump from changing modes.
There is no discrete projection toggle to chatter at a boundary, so no additional
hysteresis state is needed. Orbit pitch does not automatically change with zoom.

Near/far depth mapping, screen labels, picking rays, six-plane bounds tests and
screen-size LOD use the same view. LOD accounts for depth and off-axis projection
conservatively; a sphere crossing the eye plane requests maximum detail.
Projectile point sprites divide their size by homogeneous w, and wire lines are
clipped at the near plane before screen-space expansion.

`render_distance` remains a target-centered world/simulation radius, not eye
distance. Optional `coverage: {"type":"viewport","min_z":...,"max_z":...}`
expands only the render query to conservatively cover the visible height band.
This uses the frustum/slab intersection, including shallow angles. Geometry
outside the authored band still needs wider bounds. Far depth extent retains the
legacy conservative depth budget derived from orthographic coverage and base
radius.

Depth precision needs a separate bound during the transition: a 5 cm near plane
at a virtual eye hundreds of units away wastes almost all the depth buffer and
causes z-fighting even between non-coplanar surfaces. With depth budget D, the
front plane is bounded by `D / (1 + k * D)` in target-relative camera coordinates.
This approaches the original orthographic front plane continuously at k=0 and
keeps useful depth precision around the target as perspective develops.
`near_clip` is the minimum eye clearance, not an exact near-plane distance;
the precision bound can increase that clearance. The configured minimum is
capped at half the implied eye distance for extremely small viewports, keeping
the target visible. Depth coefficients use double-precision intermediates before
producing the existing float matrix. Screen framing and world geometry do not
change, but geometry closer than the adaptive near plane is clipped.

## Scope and Verification

The [dungeon prototype](dungeon.md) adds world-owned interior volumes and an
optional collision-bounded orbit. It is not a general camera path planner,
portal system or roof cutaway system.
Panning retains target-plane scaling; it is not cursor-anchored surface dragging.

`make test` covers schema rejection, projection/matrix agreement, near/behind-eye
clipping, picking order, LOD depth selection, blend boundaries, reversible
frame-rate-independent easing, independent cameras and height-band coverage.
A dense zoom sweep verifies that surfaces separated by 1 cm at the target retain
at least 16 distinct 24-bit depth-buffer levels at three viewport aspect ratios.
`make PROFILE=release gpu-test` includes a native camera test with real skinned
geometry and depth-projected spell sprites. The camera test uses 4x MSAA and
checks path/floor occlusion against a path-only reference at 120 eased zoom frames,
in both draw orders. This test reproduced the original transition failure.
Captures are written to `build/camera-{ortho,blend,perspective,return,transition}.ppm`;
a failing depth comparison also writes `build/camera-depth-failure.ppm`.
The other GPU suites retain their single-sample HUD pixel checks. This is
correctness verification, not a new performance benchmark.

## Collision-Bounded Orbit

Optional scene configuration (requires a `zoom_perspective` lens):

```json
"orbit": {
  "target_height_fraction": 0.7,
  "min_distance": 0.4,
  "max_distance": 35,
  "clearance": 0.12,
  "response_seconds": 0.18,
  "lookahead_seconds": 0
}
```

This opts into a finite, perspective orbit instead of the hybrid projection.
It reuses the lens FOV, near clearance and zoom easing settings; hybrid
start/end thresholds are not used in this mode. The usual distant isometric
orientation remains available, but the projection is not orthographic.
`orbit` takes runtime precedence over optional retained `rigs`; editor saves
preserve this choice. Those rig previews do not drive gameplay while orbit is set.

Zoom specifies a preferred eye-to-target distance, clamped to the configured
range. Scroll intent is also bounded so reversing at a limit responds immediately.
Collisions do not change that intent. Given the orbit direction, the world performs
a conservative sphere sweep from the target toward the requested eye, excluding
the followed item. The padded radius also encloses the near-plane rectangle,
including viewport aspect ratio. Finite orbit uses the configured near plane,
not the hybrid projection's receding-eye depth bound. Oriented boxes
are expanded in local space; spheres use their expanded radius. Decorative
meshes without colliders do not obstruct the camera. Collision layers currently
do not filter this query: every collider except the followed item's is solid.

The query uses immutable world snapshots and their spatial index. Spatial bounds
include colliders independently of mesh/occupancy extents, including invisible,
offset and oversized blockers. Heightfields use a conservative maximum-slope
clearance and prefix intersection tests, so narrow ridges are not skipped.
Steep terrain elsewhere in the field may make that clearance overly cautious.

For presentation interval dt and response time tau, `alpha = 1 - exp(-dt/tau)`.
The radius moves by `alpha * (goal - radius)` and is then clamped to the current
safe distance. A short predicted target position can start contraction early;
predictions through walls are discarded. Outward recovery is smooth, but sudden
obstructions must clamp immediately rather than letting smoothing cross a wall.
The same collision-resolved eye drives rendering, picking, culling and LOD.
Each camera retains its own requested zoom and response state.
In radial-only mode manual orbit sweeps the current eye around the target, with
two-degree arc segments and a boundary search. On wall contact it projects the
unused eye displacement onto the collider's actual world-space contact plane,
then normalizes that tangent to preserve the remaining travel distance. This is
surface sliding, not separate yaw/pitch clamping. Further sweeps stop at corners;
head-on movement with no tangent does not invent an arbitrary direction. Sliding
may change actual distance to the player as the eye follows the wall instead of
a circle, but requested zoom and the abdomen target are unchanged. The requested
maximum radius and line of sight still constrain movement. Ceilings, floors,
terrain hits without reliable normals and initial overlaps retain the existing
stop behavior; overhead-camera work is deferred. Following a moving target still
uses radial retraction. The optional validated
`target_height_fraction` (0.1..0.9) positions the pivot relative to the followed
character's current body height; 0.7 targets the upper abdomen.
Window resizing preserves the requested world-space distance. Pitch is bounded
to -85..85 degrees without a forced above-ground angle; collision limits the eye.

SDL's MacBook trackpad touch mode supplies finger events, not synthetic mouse
clicks. Input tracks one device, coalesces the two contacts once per event batch,
classifies centroid movement as orbit or span change as pinch after a small
threshold. That choice stays locked until contacts change, preventing accidental
zoom during rotation. Lift and replace fingers to switch gestures. A third
finger pauses the gesture; changing contacts rebases it, and focus loss resets
it. Mouse wheel and +/- remain zoom alternatives. No new game commands or FFI
structures are involved.

Optional `avoidance` searches nearby yaw/pitch angles when the preferred ray
loses more than 5% of its radius. It trades additional usable distance against
deviation from the requested view; goal hysteresis discourages switching sides.
The search is bounded to a small angular grid, not a full world/path search.
Yaw/pitch ease toward the selected direction subject to a turn-rate limit; the
intermediate direction is swept again before resolving radius. This lets the
camera lower or move to the side, then expand back along a passage rather than
remaining at the distance imposed by the original side-wall ray.

Manual orbit temporarily bypasses automatic steering while input is active and
for 0.3 seconds after it stops. A new gesture starts from the visible angle,
not an unseen pre-avoidance angle. Collision limits still apply during manual
control. Zoom always updates the requested radius. Clear preferred rays restore
the requested angle smoothly; neither steering nor radius clipping changes zoom
intent. Omit `avoidance` to retain the radial-only behavior.

The minimum radius is a user preference, not permission to intersect an obstacle.
An already blocked pivot collapses toward the target; a 1 mm projection floor
keeps matrices finite, not a guarantee of useful framing or a collision-free
solution when the target itself is invalid. The local angular search is not
obstacle-following path planning; it can still pull close when none of the nearby
angles fit. Roof fading and first-person fallback are not implemented.

With interior guidance enabled, orbit mode keeps the pitch/follow-height easing
but ignores the legacy interior zoom/yaw constraint. Manual yaw and pitch offsets
remain responsive. The physical roof collider, not a mesh name or scene-specific
condition, sets the radial limit.
