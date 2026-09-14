# Rigid-Body Milestone

This is a limited in-house Rust solver, not Rapier. `io-world` owns physics state
and collisions; C still receives only render transforms and geometry. Models do
not need source-code changes to participate. Collision shapes are authored
separately from meshes, animations, occupancy, and visual LODs.

## Run

```sh
make release
./build/release/io --scene assets/physics/impact.json
```

The default scene contains 72 stacked blocks, 16 launched character models, and
one static ground body. Characters are whole rigid bodies, not ragdolls. Blocks
separate because they are individual bodies; this is not mesh fracture or a
bonded structural-strength simulation. Omit `--capture-at` to watch it live.

Generate a different workload without changing engine code:

```sh
python3 tools/build_physics_demo.py --count 32 --mass 100 --speed 20 --gravity -4 --output assets/physics/custom.json
./build/release/io --scene assets/physics/custom.json
```

`--count 0` gives a wall-only stability test. The seed defaults to 7. The launch
grid, finite ground, and camera are designed for modest counts; accepting a large
count does not guarantee all bodies fit on the ground or remain visible.

A separate 16 x 16 x 16 cube contains 4,096 independent unit bricks and 16
projectiles. It is a stress workload, not a real-time performance promise:

```sh
./build/release/io --scene assets/physics/cube.json
python3 tools/build_physics_demo.py --columns 16 --rows 16 --depth 16 --brick-size 1 1 1 --count 16 --output assets/physics/cube.json
```

For exactly 10,000 individual unit blocks and no launched characters, use the
20 x 20 x 25 stack below. The floor is one additional static body. This scene
uses PGS with its default accuracy settings and sleeping enabled; it is a dense
contact stress test, not a guarantee of interactive frame rates.

```sh
./build/release/io --scene assets/physics/blocks-10k.json
python3 tools/build_physics_demo.py --columns 20 --depth 20 --rows 25 --brick-size 1 1 1 --count 0 --solver pgs --output assets/physics/blocks-10k.json
```

The 2026-09-13 release headless run over 90 ticks measured 417.95 ms mean and
464.51 ms p95 per simulation tick. All 10,000 blocks remained awake at the end;
fresh final collision geometry measured 0.04175 units maximum penetration.
Results are in `build/blocks-10k-benchmark.json`. This is physics/world CPU time,
not rendered FPS, and remains well above the 33.33 ms budget for 30 Hz.

Columns/rows/depth accept 1..100, with a 100,000-brick generation cap to reject
accidental million-body allocations. Brick-size components accept 0.1..5. The
cube uses the same engine and asset contract as the wall; no asset-specific
physics code was added. `--no-sleep` generates a reference workload with sleeping
disabled, and `--count 0 --rows 1` makes a settled-floor workload.

## Loose-Brick Blast

`assets/physics/brick-blast.json` has 1,000 individual bricks: 900 near ground
level, with gaps, and another 100 forming four separated two-layer patches.
All bricks start with seeded outward/upward velocity and spin. This is an
immediate blast-like launch using the existing body contract, not an explosion
pressure-wave or fracture system. Gravity, collisions, friction, and sleeping
take over normally; reopening the scene replays it. No character assets are used.

```sh
./build/release/io --scene assets/physics/brick-blast.json
python3 tools/build_brick_blast.py --strength 1 --seed 7
```

`--strength` accepts 0..2 and scales launch velocity and spin, not gravity or
solver accuracy. Zero gives the same layout without a launch. `--output` selects
a different scene file. The PGS solver retains its default 4 substeps and 12
iterations; the camera is framed for the default launch.

For 100,000 independent bricks (90,000 at ground level and 10,000 in low piles):

```sh
python3 tools/build_brick_blast.py --count 100000
./build/release/io --scene assets/physics/brick-blast-100000.json
```

`--count` accepts 1..100000. Larger counts expand the footprint, floor, blast
falloff radius, and camera coverage without enlarging bricks or lowering solver
accuracy. Non-default counts get separate output filenames, preserving the
original 1,000-brick scene. Launch speed remains bounded by `--strength`; this
is still a simultaneous initial-velocity launch, not a propagating pressure wave.
The 100,000-body scene is a stress test and may be very slow interactively.

The 100,000-brick release headless check on 2026-09-13 covered only 30 ticks
(one simulated second): 963.94 ms mean and 1080.01 ms p95 per simulation tick.
All 100,000 bricks remained awake; collision work averaged 665.60 ms per tick.
Results are in `build/brick-blast-100000-benchmark.json`. This is an initial
launch check, not a full-flight/landing benchmark or rendered FPS measurement.

Original 1,000-brick release headless measurement on 2026-09-13 over 240 ticks (8 simulated seconds):
16.28 ms mean, 29.46 ms p95 per simulation tick, with 772 bricks sleeping by the
end. Final fresh maximum overlap was 0.00679 units; maximum sampled contact
penetration during impacts was 0.21865 units, so this is still a discrete-contact
test with transient overlap, not continuous collision detection. Results are in
`build/brick-blast-benchmark.json`; these timings exclude rendering.

## Components and Authority

- `PhysicsBody` chooses static, dynamic, or kinematic behavior. Dynamic bodies
  carry mass, linear/angular velocity, damping, and a gravity multiplier.
- `Collider` chooses an oriented box or sphere, dimensions, local offset,
  friction, restitution, and collision masks. One collider is required per body.
- `Transform` stores a checked unit quaternion, position, scale, and interpolation
  history. Rotation and bounds work in full 3D, not just yaw.

Dynamic transforms belong to the solver. Apply an impulse with
`World::apply_impulse(item_id, impulse, world_point)`; an off-center impulse also
changes angular velocity. `set_pose` rejects physics bodies rather than allowing
two transform authorities. `set_gravity_scale` changes an individual dynamic
body's response; `set_physics_settings` changes global gravity/accuracy.

Kinematic bodies follow `set_kinematic_target`, or a configured route. They push
dynamic bodies but are not blocked by contacts. They are not a collision-aware
character controller. Static bodies cannot move. Runtime body-mode changes,
teleports, attachment/removal, and respawning are not exposed yet.

Collider dimensions and offsets are **physical local units, independent of
render scale**. The offset also locates the center of mass relative to the item
anchor. For a unit-box mesh spanning 0..1 with visual scale `[2, 4, 6]`, use box
half-extents `[1, 2, 3]` and offset `[1, 2, 3]`. Changing an LOD never changes mass
or collision dimensions. Velocity describes the center of mass, so an offset
anchor moves around it during rotation. The world's visibility index still uses
occupancy/render bounds, not collider bounds; physics has its own broad phase.

## Scene Contract

Existing version-1 scenes remain valid. Optional top-level settings:

```json
"physics": {
  "gravity": [0, 0, -9.81], "substeps": 4, "iterations": 12,
  "cell_size": 2, "warm_start": true,
  "sleep": {
    "enabled": true, "linear_threshold": 0.12,
    "angular_threshold": 0.15, "idle_seconds": 1
  }
}
```

Optional fields on an item:

```json
"physics_body": {
  "type": "dynamic", "mass": 70,
  "velocity": [0, 16, 4], "angular_velocity": [0.4, 0.2, 0.1],
  "gravity_scale": 1, "linear_damping": 0, "angular_damping": 0.05
},
"collider": {
  "shape": {"type": "box", "half_extents": [0.4, 0.3, 0.9]},
  "offset": [0, 0, 0.9], "friction": 0.5, "restitution": 0.1,
  "memberships": 1, "filter": 4294967295
}
```

Use `{"type":"static"}` or `{"type":"kinematic"}` instead for those body
modes; they reject dynamic-only fields. Sphere shape is
`{"type":"sphere","radius":0.5}`. Masks must allow the pair in both
directions. A zero filter disables contact. Physics records reject unknown
fields and variants. Body and collider must be supplied together; a route on a
physics item requires kinematic mode.

Optional `rotation_xyzw` sets orientation instead of `yaw_degrees`; it cannot be
combined with nonzero yaw or a route. Valid nonzero quaternions are normalized.
Velocity is world units/second, angular velocity radians/second. Interpret one
world unit as a meter and mass as kilograms for the default gravity convention.

Validated numeric limits: mass 0.01..1,000,000; collider dimensions 0.01..10,000;
offset components +/-10,000; body positions +/-1,000,000; velocity components
+/-10,000; angular velocity components +/-1,000; friction 0..2; restitution 0..1;
damping 0..100; gravity multiplier -10..10; global gravity components +/-1,000;
substeps 1..32; iterations 1..64. These are rejection bounds, not promises of
accuracy across extreme mass ratios, speeds, sizes, or world coordinates.
Collision cell size accepts 0.1..1,000 world units. Sleep linear/angular speed
thresholds accept 0..10; idle time accepts 0.1..60 seconds. Nested settings reject
unknown fields. `PhysicsBody::is_sleeping()` exposes state without allowing
callers to force an unsupported structure to sleep.

## Tick and Contact Policy

Physics activity is independent of camera visibility. Attached
animation/route state also advances off-screen. Non-physics items retain the
existing camera-region pause policy. This is not a general background AI
scheduler. Settled physics bodies can sleep regardless of distance.

The app defaults to 30 Hz, configurable with scene `simulation.tick_hz` or the
native `--tick-hz` override (integer 4..1000). The fixed timestep and scheduling
period are both derived from that rate; actual worker throughput can be lower.
At 144 Hz, the whole-tick budget is about 6.94 ms. Four physics substeps remain
the default, so this requests 576 physics substeps per simulated second, not
fewer solver iterations. Raising the target increases work per wall-clock second
when the worker can keep up; it is not a solver speed optimization.

The native worker preserves fixed steps under overload and lets simulation time
lag, without blocking the renderer on the solve. Inline/capture mode derives its
bounded catch-up allowance from the configured rate. Runtime rate changes are
not supported. Headless physics benchmarks and worker probes use the scene rate
and report `tick_hz` and simulated seconds. Native render benchmarks report
`simulation_tick_hz`; their existing `fixed_dt` remains the render-driver update
interval, not the physics timestep. Historical measurements below used 30 Hz;
compare results at equal simulated durations and matching rates/settings.

Within
each substep the solver integrates active velocities and rotation, queries a
persistent 3D spatial hash, tests sphere/box and box/box contacts, then iterates
normal/friction impulses. Bounds spanning more than 512 cells use a large-body
fallback instead of filling the grid or dropping possible collisions. Only
active dynamics and moving kinematics initiate queries; sleeping bodies remain
indexed so an active neighbor can hit them. Cross-cell pairs are deduplicated.
Box contacts use separating axes and clipped face manifolds. Rendering
interpolates positions and rotations; C's packet layout is unchanged.

Dynamic contact chains form conservative simulation islands. Each dynamic body
accumulates quiet time while every sampled substep stays below the configured
linear/angular speed thresholds. An island sleeps only when all its bodies are
quiet, and it either touches a fixed/stationary kinematic support or has zero
effective gravity throughout. This support heuristic is not structural analysis.
Sleeping zeroes residual velocities and skips integration and contact solving;
the retained graph and collision index preserve wake-up relationships.

An explicit impulse or gravity change wakes a body. Wake-up propagates through
its cached dynamic contact chain before the next solve. Moving a kinematic
support wakes its dependents even when it moves away rather than into them.
New impacts wake sleeping groups and add their queries in the same substep.
Static ground and stationary kinematic supports do not bridge unrelated piles.
All active bodies still use synchronized fixed steps: there is no arbitrary
radius cutoff, delayed collision queue, or async task per brick.

The graph is conservative across a tick's substeps, so a transient connection
can keep extra bodies awake until the next tick. Large connected or jittering
stacks may never meet the settling threshold. Physics sleeping does not freeze
attached gameplay animation. New world assemblies start awake even if cloned
from a sleeping body; runtime component removal/teleport APIs remain unavailable
and must invalidate support relationships if added later.

`World::contacts()` exposes the latest tick's solved contact points and normal
impulses. Normals point from item `a` to `b`. Events repeat across substeps and
include resting contact impulses; they are **not collision-begin notifications**.
Sleeping contacts emit no fresh impulses until awakened.
Gameplay should aggregate and threshold them before applying damage. There is no
automatic collision-to-health mapping. Existing depletion responses stop routes
and animation as configured, but do not cancel dynamic momentum or gravity.

Physics solves in scratch state. Unsupported/nonfinite results pause physics and
populate `World::physics_error()` without committing partial physical poses.
Other gameplay updates are not rolled back. Restart the scene to recover.
Direct world callers must provide positive finite ticks no greater than 0.25 s;
small fixed ticks are required for useful accuracy.

## Warm Starting

Warm starting is enabled by default; iterations remain at 12. The physics runtime
retains solved normal and friction impulses from the immediately preceding
substep. Matching uses the ordered stable body-ID pair, contact positions in
both bodies' center-of-mass local frames, and a normal-direction check. The local
position tolerance is 10% of the smaller collider's minimum half-extent/radius,
capped at 0.02 units; normal dot product must be at least 0.98. Matches are
nearest-first and one-to-one, not merely one entry per body pair. This is
geometric matching, not persistent mesh feature IDs.

A match initializes the accumulated impulse AND applies that impulse to the
current body velocities before the normal solver passes. Cached friction is
projected into the current tangent plane and clamped to the current friction
cone. Iterations can subtract an excessive guess. Friction is reclamped when a
normal correction shrinks the cone even if instantaneous slip is zero.

Only positive finite solved normal impulses and finite tangent impulses are
cached. Unmatched contacts start at zero. Entries expire after one missing
substep; changed substep duration invalidates the cache rather than rescaling.
Changed global settings clear it, and a changed gravity multiplier invalidates
that body's entries. Sleeping contacts naturally expire from this cache without
removing their separate support/wake graph. There is no LRU or fixed memory cap:
live entries scale with the latest solved contacts, and hash capacity can retain
its allocation high-water mark. A solver error does not publish its scratch cache.

Compare warm/cold and iteration counts without source changes:

```sh
python3 tools/build_physics_demo.py --count 0 --no-sleep --no-warm-start --output build/cold12.json
python3 tools/build_physics_demo.py --count 0 --no-sleep --iterations 4 --output build/warm4.json
cargo run --release --offline --bin io-physics-bench -- build/cold12.json 300
cargo run --release --offline --bin io-physics-bench -- build/warm4.json 300
```

The generator accepts `--warm-start`/`--no-warm-start` and `--iterations 1..64`.
The JSON equivalent is `physics.warm_start: false`. Cache hit/miss totals count
contact points, not body pairs. `peak_cached_contacts` counts the largest cache
observed after any substep. `warm_start_mean_ms` includes restoring/applying and
storing impulses; it is a subset of resolution time, not an additional phase.
Peak/final contact penetration are measured during contact generation before
solving, not an exact global maximum overlap after the step.

Single release runs of the 72-brick, six-high wall with sleeping disabled, 300
ticks (10 simulated seconds):

| Mode | Mean tick | Maximum final displacement | Peak penetration |
| --- | --- | --- | --- |
| Cold, 12 iterations | 3.556 ms | 0.1735 units | 0.01361 units |
| Warm, 12 iterations | 3.252 ms | 0.0293 units | 0.00828 units |
| Warm, 4 iterations | 1.645 ms | 0.0636 units | 0.01545 units |

Warm contact match rates were approximately 81% and 76% respectively. Four
iterations are a workload-specific experiment, not the new default: lower
iteration counts can trade accuracy for speed. A six-high stack regression
checks ten seconds with four iterations and sleeping disabled. Other tests
cover stale geometry/IDs/timesteps, one-to-one matches, impulse application,
negative corrections, and cache expiration/invalidation.

For the 4,096-brick cube plus 16 projectiles over 90 ticks, the same comparison
measured 231.813 ms/tick cold at 12 iterations, 225.742 ms warm at 12, and
140.023 ms warm at 4. Peak penetration was respectively 0.35681, 0.17087, and
0.23693 units; final-substep penetration was 0.04633, 0.00743, and 0.04070.
All dynamic bodies remained awake. Warm starting improves convergence, but the
cube is still far from real-time, including at four iterations. The interactive
app's existing multi-tick catch-up loop is unchanged and can compound stalls;
these are CPU simulation timings, not window FPS. No iteration defaults or
physical contacts were reduced to claim a higher frame rate.

## Prepared Contacts and Data Size

`physics/solver.rs` prepares each contact once per substep, after contact
generation and before warm starting. It caches both center-of-mass lever arms
and the normal effective-mass denominator. Body poses stay fixed during the
iterative velocity solve, so these quantities are reused across passes.
Friction's slip direction still changes each pass; its effective mass is
recomputed using the original scalar calculation. Normal division is retained.

Inertia has an exact isotropic fast path: when all three local inverse-inertia
values are equal, `R * (sI) * R^-1 = sI`. Cubes and spheres therefore use vector
scaling instead of quaternion rotations for angular impulse response. This is
based on inertia values, not asset names or an approximate equality threshold.
Anisotropic bodies retain the original quaternion calculation.

More aggressive world-inertia/contact-response matrix caches were rejected
after floating-point reassociation exceeded an existing low-iteration stack
tolerance. No tolerance, iteration default, tick rate, sleeping policy, or
catch-up behavior was weakened to obtain the final result.

Physics values remain `f32`, and stable world IDs remain `u64`. Solver-local
body indices use a checked `u32` newtype: unrepresentable indices return an error
instead of truncating. On the development 64-bit target the constraint record
shrinks from 64 to 56 bytes, without quantization or packed/unaligned access.
The preparation record contains seven floats (28 bytes). There is no cached
matrix and no half-precision physics.

This is still a **CPU-for-memory tradeoff**, not an overall memory reduction:
combined live contact records use 84 bytes versus the previous 64.
Preparation vector capacity is reused across substeps/ticks, but every live
entry is rebuilt for current poses. Capacity retains its high-water allocation.

New benchmark fields `preparation_mean_ms` and `iteration_mean_ms` are subsets
of resolution time, alongside warm starting and event emission.
`contact_record_bytes` and `prepared_record_bytes` report actual type sizes.
`peak_contact_working_bytes` measures the largest combined live contact payload
in any substep; `prepared_capacity_bytes` reports retained preparation capacity.
These are not total process memory: constraint spare capacity, body scratch,
warm-start cache, broad phase, events, and allocator overhead are not included
in the live-payload measurement.

Release cube measurements on the development Mac (2026-09-13), 4,096 bricks,
16 projectiles, 90 ticks, warm starting and the same 12 iterations:

| Run | Before mean tick | Final mean tick |
| --- | --- | --- |
| 1 | 226.413 ms | 150.108 ms |
| 2 | 222.807 ms | 149.657 ms |

Whole-tick time is about 33% lower; resolution falls from 167-171 ms to 94-95 ms,
including about 0.57 ms preparation. All 4,112 dynamics stay awake. The cache
alone, without the isotropic path, measured 224.538 ms: most of this workload's
gain comes from avoiding unnecessary inertia rotations, not merely allocating
data ahead of time. The anisotropic 72-brick wall (300 ticks, no sleeping)
measured 3.216 ms before and 3.330 ms after, with identical reported contact and
displacement results: no speedup there. Other collider distributions may benefit
much less.

The isotropic identity changes floating-point rounding and trajectories:
peak/final penetration changes from 0.17087/0.00743 to 0.17329/0.02044 units.
This is a same-scene comparison, not identical-contact replay or evidence of
improved contact accuracy. Peak live contact payload is 2,064,972 bytes, with
917,504 bytes of retained preparation capacity. Roughly 150 ms/tick still
cannot keep up with 30 Hz; this is not window FPS.

Tests compare prepared normal responses and iterative passes with the scalar
solve order using exact equality for rotated anisotropic bodies,
static/dynamic/kinematic pairs, warm impulses, and multiple friction values.
Separate checks cover orientation-independent isotropic inertia, immovable
pairs, record sizes, index overflow, scratch reuse with changed poses, and the
existing ten-second stack regressions in debug and release builds.

## Measurement and Limits

For opt-in per-iteration correction histograms and scratch replay checks, see
[Contact Work Diagnostics](contact-diagnostics.md). Instrumented timings include
substantial observation overhead; the normal benchmark remains uninstrumented.

Run CPU simulation without a window or rendering:

```sh
cargo run --release --offline --bin io-physics-bench -- assets/physics/impact.json 300
```

The JSON reports whole-tick and solver mean/p95 time, candidate/contact counts,
and final maximum displacement from starting positions. The default 300 ticks
represent 10 simulated seconds; loading is excluded, but there is no warmup or
repeat aggregation. `moving_bodies` counts dynamic/kinematic bodies, not awake
bodies. Pair/contact counts sum across substeps, not unique pairs. These CPU
times are not GPU time or rendered FPS. Use the existing rendering benchmark
separately when measuring total frame cost.

Additional counters expose final awake/sleeping dynamic bodies and islands,
peak/final unique regional pair checks (including rejected AABB/mask checks),
total dynamic body-substep integrations, and propagated wake-ups. Explicit
impulse/settings wake calls are not included in the propagated-wake counter.
`tail_simulation_mean_ms` covers the final up-to-30 ticks, not sorted samples.
`collision_mean_ms` includes index updates, pair queries, narrow phase, and wake
propagation during contact discovery. `resolution_mean_ms` covers preparation,
warm starting, iterative contact solving, and event emission. Other bookkeeping
remains in solver/tick time; these measurements are not an exhaustive breakdown.

Pre-warm-start single-run release observations on the development Mac (2026-09-13):

| Workload | Mean tick | Final 30-tick mean | Final sleeping |
| --- | --- | --- | --- |
| 256 floor bricks, sleep enabled, 300 ticks | 0.855 ms | 0.049 ms | 256 |
| Same scene, sleep disabled, 300 ticks | 7.768 ms | 7.864 ms | 0 |
| 4,096-brick cube + 16 projectiles, 90 ticks | 231.513 ms | 255.859 ms | 0 |

The floor runs had identical final maximum displacement (0.006542 units), with
0 versus 7,232 regional pair checks on the final tick. In the cube run, collision
discovery averaged 56.521 ms and contact resolution 168.851 ms. Its three simulated
seconds completed without numeric errors, but it was not interactive and did not
settle. The older six-high wall also remained awake after ten seconds. Sleeping
is deliberately not forced on a jittering structure to manufacture a faster
result. These are illustrative single runs, not cross-machine guarantees.

There is no continuous collision detection (fast/thin bodies can tunnel),
joints, ragdolls, mesh/capsule
colliders, gyroscopic torque integration, fracture, or parallel solving.
Dense occupied cells and many large fallback bodies can still cost quadratic
work. Scratch body copies, graph traversal, bounds scans, and world bookkeeping
still visit retained bodies; sleeping does not make total work independent of
world size. The spatial index and prepared-contact capacity persist, but other
scratch buffers are allocated per tick/substep. Threaded island jobs are not
implemented in this pass. This milestone establishes component
ownership and a tested simple solver, not a production large-world physics
performance guarantee.
