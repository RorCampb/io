# Typed Movement Stages

## Observation-Based Urgency

The playground's optional `obstacle_observations.reaction` policy estimates
translating obstacle velocity from timestamped **visible** samples. It is not a
read of hidden physics velocity. Losing visual evidence resets the estimate;
gaps over 0.5 seconds and apparent teleports over 100 m/s discard it. Attention
must reach the notice threshold before a sample drives a reaction. This policy
currently uses discovered scenery tracks, not explicit pursuit tracks or NPCs.

`ReactionStage` is a pure plugin stage: predict an expanded moving obstacle box
against up to eight accepted-route segments at the actor's current speed. It
uses no geometry queries and returns the earliest predicted conflict/priority.
Receding obstacles and lateral misses need not be urgent. This is approximate
(filtered constant velocity, current bounds, coarse route instead of a future
local curve), not a new physics solver or future-obstacle route search.

```json
"reaction": {
  "minimum_obstacle_speed": 0.25,
  "horizon_seconds": 2.0,
  "brake_seconds": 0.6,
  "cooldown_seconds": 0.5,
  "lease_seconds": 0.35,
  "margin": 0.25
}
```

Omitting `reaction` retains the old policy. Static geometry remains with surface
planning, attention-correlated notices and immediate clearance: a ramp's enclosing
box is not an imminent collision with its walkable surface. The speed threshold
defines a moving-obstacle policy, not an asset/scene exception. Keep the lease
long enough for the bounded observer sampling cadence; large populations can
sample later than the nominal interval.

Core `NavigationRequest::Prioritize { ticket, priority, seconds }` validates the
objective ticket and a finite 0.05..2 second lease without replacing/canceling
the objective. The plugin separately issues coalesced `Reconsider` requests at a
bounded cadence or when urgency increases. Priority also reaches the optional
`TraversalExecutor::navigation_priority` hook; custom executors may ignore it or
supply their own response. The supplied locomotion executor refreshes feasibility
checks sooner for elevated urgency and brakes with its configured acceleration
for urgent conflicts. Leases expire automatically. Physical limits, live checks
and route ownership remain unchanged. Predictive sidestep search is not added.

Inline admission and background work reserve one in four slices for ordinary
round-robin progress; other slices favor urgency. Independent admission cursors
avoid starving populations divisible by four. In-flight priority changes do not
restart searches. The worker retains 32 slots, does not evict another job for
admission, and cannot preempt a provider within an expansion. Priority is not a
hard real-time completion guarantee; planning still uses snapshot geometry.

### Visual Test

```sh
python3 tools/build_reaction_demo.py
make PROFILE=release all
./build/release/io --scene assets/dungeon/reactions.json
```

Three lanes have crossing fixtures at 1, 3 and 6 m/s. Existing fixture safety may
pause a barrier when an actor occupies its swept volume; collision-free results
alone do not prove anticipation. Separate tests check prediction timing, misses,
hidden samples, lease expiry and braking.

Compact read-only meters above each dynamically observed NPC show cyan `E`:
current visual evidence (view direction/range/occlusion), and yellow `A`:
accumulated attention. Both describe the same highest-attention tracked Item, not
independent maxima or sums. No tracks means zero. Their width is 1.4 world units
projected at head height: zooming out shrinks them, hides tiny text, then culls
bars under five pixels. They are overlays, not occlusion-tested scene geometry
or draggable tuning controls. Presentation capacity is 1024 rows (512 pairs), not
a world/NPC limit. When optional character discovery is enabled, meters prefer
a character above the notice threshold over scenery; otherwise strongest attention
wins. Explicit attention demos retain tracked-target anchors with
the same world-scaled sizing. Rust/C layout changed together; rebuild the native
host when updating the library.

## Rolling Local Trajectories

The supplied `io-locomotion` plugin optionally rounds the next same-action,
level-ground corner while retaining the existing coarse route. No new navigation
crate, motor, renderer interface, or thread was introduced. Enable per agent:

```json
"steering": {
  "trajectory": {
    "preview_seconds": 0.8,
    "max_distance": 3.0,
    "corner_distance": 1.2,
    "lateral_acceleration": 4.0,
    "refresh_seconds": 0.2,
    "samples": 12
  }
}
```

Omit `trajectory` (or use `null`) for previous waypoint steering. The stress-128
scene and newly generated stress agents enable it. No new live editor panel exists.

`TrajectoryRefiner::propose(RefinementInput)` is the replaceable, pure plugin
strategy. It receives position/velocity, the next corner/continuation and settings,
and returns optional cubic control points. `CornerRefiner` supplies entry-heading
and exit-edge tangents. It does not search routes.

`TrajectoryStage: Stage<TrajectoryRequest>` owns per-actor local progress. It checks
one short sampled chord per tick through existing body clearance/support primitives,
retaining the coarse route while preparing. Failed proposals can retry from the
new position after the refresh interval. Accepted samples produce a rolling steering
target and curvature-based speed preference. Progress is monotonic and velocity
changes are bounded by the steering rates; live safety can still stop immediately.

The existing Motor applies motion. Every actual displacement receives live collision
and support checks: sampled curves are not cross-tick permission to cross geometry.
A blocked curve does not steer backwards to its bypassed coarse corner. Rounded
corner completion uses the existing explicit `Progress::Complete` acknowledgement.
Cancellation or changes to route/action/profile/world identity/settings discard
local state. Actual velocity feedback survives route handoffs; cloned state owns
independent sample arrays and progress.

Preview distance scales with current speed and braking distance, capped by
`max_distance`. Periodic short scenery-feasibility checks are staggered by Item ID.
They can return generic `Progress::Reconsider`: request a replacement while keeping
a still-executable route. The coordinator retains existing ticket/coalescing and
background/inline behavior. This is physical anticipation, not semantic observation;
it does not update attention memory or grant knowledge of an unseen target.

Developers use `TrajectoryControl::set_trajectory_refiner` to replace the strategy;
retain the returned `NavigationTicket`. Invalid settings leave the executor intact.
The stage rejects nonfinite/out-of-range proposals or endpoints not on the intended
next segment; a custom generator cannot bypass collision validation.

Limits: the default generator leaves vertical transitions, reversals, stance changes
and discrete jump/stair actions alone. Curves are sampled polyline approximations,
not analytic swept-spline proofs. This is corner shaping and early reconsideration,
not a general local detour/crowd optimizer; truly blocked movement can still wait
for a background route. Animation cadence has not been changed in this milestone.

Debug traces show local samples in yellow and accepted coarse routes in cyan/orange.
The CPU probe reports `trajectory` counts: proposals, checks, accepted/rejected
curves, completed corners and anticipations. They describe preparation separately
from executed body motion. Tests exercise actual safe detours and bounded velocity
changes at 30/60/144 Hz, early reconsideration, invalid custom strategies, geometric
tangents, fallback conditions and clone isolation.

## Stage Contract

`io_game::stage::Stage<Input>` is a synchronous plugin composition contract, not
a thread, worker, event queue, global registry or additional movement controller.
There is no new crate. The developer owns the input/output/error types and state.

```rust
pub trait Stage<Input> {
    type Output;
    type Error;
    fn run(&mut self, input: Input) -> Result<Self::Output, Self::Error>;
}
```

`Then(A, B)` (or `a.then(b)`) requires B's input to match A's output and both errors
to match. A failure stops downstream execution; it does not roll back side effects
performed by arbitrary developer stages. Inputs can borrow world/configuration and
scratch buffers. Worker queues remain responsible for thread safety and stale-result
validation; a Stage does not make borrowed data cross-thread or cross-process safe.

## Existing Contracts Remain

The route provider still defines nodes, profiles, eligible connections and actions.
The traversal executor still interprets actions and owns execution. The supplied
locomotion plugin composes stages internally; it is not baked into `Stage` itself.

The public-API example in `plugins/io-locomotion/tests/stage_contract.rs` defines
its own `Advance | Sneak` actions and standing/sneaking properties. A developer
`Prepare` stage accepts `Waypoint<Action>` plus those properties and produces a
`WalkRequest`. It then composes:

```rust
let mut pipeline = Prepare.then(SupportStage).then(ActorClearanceStage);
```

The same fixture rejects standing under a beam and accepts sneaking. Neither action
name nor its properties are added to the engine. A flying plugin can define entirely
different trajectory stages; it need not use upright-body support at all.

## Route Preparation Pipeline

The existing `Stage` trait now also drives route preparation in `io-traversal`:

```text
Snapshot search -> RouteRefinement -> ticket/attempt check -> RouteHandoff
                [planning worker]                       [simulation tick]
    -> RouteFollower -> plugin executor -> immediate live collision checks
```

No new crate, worker, queue, route format, or renderer buffer is introduced.
`navigation/refinement.rs` owns the two preparation/handoff stages. The existing
`planning.rs` schedules refinement after search using the same immutable world,
actor profile, objective ticket and attempt ID. `coordinator.rs` alone accepts the
result into the live actor. Snapshot publications do not own the worker.

`RouteRefinement<P>: Stage<RefineRequest<P>>` returns `Pending` or `Complete`.
Each invocation makes at most one `P::segment_clear` call. It tries bounded shortcuts
within the configured horizon, then validates whole collinear runs before merging
them. This avoids stopping at every grid or refinement point. Action changes and
explicit-completion actions are preserved. Failed shortcuts retain the original
planned edges; no point-ray substitute or collider bypass is used. The supplied
surface provider excludes transient actors from this scenery preparation.

The stage retains the original route until completion, then atomically installs an
immutable `Arc<[Waypoint<P::Action>]>` in the job's existing Agent. Its pending cursor
checks route identity, progress, actor, profile, start, horizon and geometry revision.
Worker calls always use a pinned `WorldSnapshot`; callers of the public stage must
likewise keep their read-only world stable during an in-progress preparation.

`RouteHandoff: Stage<HandoffRequest<P>>` checks profile/revision compatibility and
delegates the current-position connector to the existing provider. Discrete actions
keep exact takeoff requirements. Ticket and attempt validation remains with the
coordinator/queue, not a second ownership system. Accepted continuous routes may
survive unrelated revisions only under the existing `LiveChecked` contract.

Existing valid execution continues while a replacement is prepared. A first route
is published after refinement, not as separate raw/refined publications. New goals,
cancellation, capability changes and stale completions retain their existing guards.
The worker's round-robin and cancellation checks apply between refinement calls too.

The follower no longer searches shortcuts every tick. It selects prepared targets,
skips exact collinear continuation without world queries, and retains the live
near-corner connector check. The executor still checks the actual next movement,
including actors. A persistent scenery obstruction now triggers a blocked route
after a safe turn-from-rest attempt; a temporary actor obstruction yields. This
does not implement crowd reservations or guarantee no traffic deadlocks.

`Navigation::refine_route` is the explicit synchronous convenience API for inline
mode and deterministic tests. It uses the same stage. Direct users replacing the
old `look_ahead` call should prepare once, then use `advance` and `prepared_target`.
Do not call a fresh geometry-refinement implementation from each execution tick.

`Stats::refinement_queries` counts preparation geometry work; `steering_queries`
counts live handoff/corner/immediate steering work. The paced worker probe records
both in its navigation statistics. Query counts from in-flight/cancelled jobs are
not included in the simulation's accumulated completed-result statistics.

This removes repeated preparation, not every geometric cost. Immediate steering
and motor validation remain separate. No cubic-spline trajectory, turn-radius
controller, continuous refinement of an already-followed route, or general crowd
avoidance was added. Tight corners can still require deliberate braking.

## Locomotion Data Flow

```text
WalkRequest (world, actor, start, planar displacement, body, scratch)
    -> SupportStage
SupportedTrajectory (borrowed world/body provenance, accepted polyline, walk result)
    -> ActorClearanceStage
ValidatedMovement (validated prefix and reached flag)
    -> existing Motor / world mutation path
```

`SupportStage` uses the existing support walker with characters excluded. The new
`io_world::trace_character_walk` records each accepted segment, including recursive
refinements at slope creases. Unsuccessful refinement cannot leave stray points
beyond the accepted prefix. The ordinary non-recording walker retains its API and
does not allocate a trajectory.

`ActorClearanceStage` reads that same polyline. It gathers character candidates
once over its conservative bounds and sweeps against their current upright bodies.
It does **not** repeat terrain, scenery or support calculations. Initial overlaps
are checked, including zero-displacement requests. An actor-less shared route query
intentionally uses support only, keeping transient actors out of shared knowledge.

The trajectory's provenance is private: subsequent supplied stages cannot substitute
a different world, body or exclusion. Its lifetime borrows the world and scratch,
preventing ordinary mutation while it is still being validated. Public read-only
access lets other stages inspect the trajectory without recomputing it. This relies
on the existing trusted, read-only `WorldView` contract, not a sandbox for plugins.

`WalkRequest` is a planar displacement intent: the support calculation supplies its
vertical trajectory, not a teleport to an arbitrary goal height. `reached` describes
that planar request. Route providers must still compare the returned height with
their desired 3D endpoint; the supplied surface provider does so.

`reached == false` means only a prefix was supported; it is not arrival or full
request clearance. An invalid/unsupported starting position does not produce a
validated movement. A validated result is not a persistent permission: after consuming
it, the movement owner must apply it before changing the relevant world state.

## Actual Integration and Boundaries

- Surface-provider shortcut and live-segment checks now use one support calculation
  followed by actor-only validation, replacing the old double walk.
- Ordinary motor walking uses the same stages with an owned reusable scratch buffer.
  Published motor clones start with empty scratch, not copies of temporary paths.
- If an actor blocks the proposed path, the motor retains the old full-world walking
  refinement as a fallback to find a safe contact prefix, then its existing sliding
  behavior. Recalculation in this blocked case is intentional.
- Player and surface-NPC ordinary walking share this motor. Jump/step/gravity paths
  retain their existing contracts and implementation.
- Background route preparation and live movement remain separate requests. The
  cross-caller handoffs below reuse compatible geometry inside those boundaries;
  a worker route is never a permission to move through the current world.
- Tick duration and overload policy are unchanged. This is not a claim that the
  whole engine now runs as a DAG.

## Cross-Caller Handoffs (2026-10-02)

The existing stages now feed their results across the supplied planner/executor/
motor boundaries rather than only between support and actor checks within one call.

- `RouteProvider::connection_clear` receives the exact source location and
  generated connection. Its default delegates to `live_clear`, preserving custom
  providers. The surface plugin retains supported polylines with its existing
  edge cache. World identity, navigation revision, body shape and exact endpoints
  must match before `ActorClearanceStage` can reuse that geometry. Every call still
  checks current actors. Missing evidence, custom worlds without identity, stale
  geometry or eviction falls back to full validation. Evidence is plugin-owned;
  the generic navigator does not learn walking/crouching semantics.
- Cached coordinate storage is bounded at 1,048,576 `Vec3` points (about 12 MiB),
  in addition to the existing entry limit and map/metadata overhead. Snapshot
  copies share paths through `Arc`. Eviction removes optimization, not safety.
- Public `pipeline::StepHandoff` exclusively borrows `PluginWorld` through
  preparation and consumption. `prepare` uses the existing support/actor stages,
  records only fully reached movement, and clears prior evidence on failure.
  Its read-only `view` permits steering queries but not world mutation.
- `Motor::update_prepared` consumes the handoff's world; callers cannot substitute
  a different world. Actual actor, pose, shape, revision and normalized planar
  displacement must match the proposal. Changed intent or stance falls back to
  normal validation. Jump/ascent and discrete planned actions retain their own
  sweep/traversal contracts. `Motor::update` remains the ordinary input API.
- `CharacterWalk::initial_support` carries the starting contact already computed
  by walking. Normal descending/supported motor movement uses that result rather
  than querying support before running the walker. This is geometry data, not a
  reusable movement permission; actor-blocked requests still use existing contact
  and slide handling.

The surface executor prepares the exact normalized command displacement and then
hands it to the motor. It preserves domain bounds, speed/acceleration policy and
live actor checks. Scenery-only fallback and trajectory preview remain separate
requests when their filters or paths differ. There is no cross-tick motor cache,
new worker, new crate, collision bypass or reduction in observation frequency.

New regression tests count one actor query on a prepared planner edge, exercise
moving blockers and cache eviction, reject changed world/body/path/revision, and
compare prepared versus ordinary motor positions at 30/60/144 Hz. A compile-fail
test rejects world mutation while a `StepHandoff` is borrowed. Existing tests
continue to exercise ramps, crests, stairs, jumping and other plugin providers.

## Verification and Measurements

Tests cover compatible custom contracts, downstream failure short-circuiting, no
world mutation from stages, one actor-stage query, no trajectory copy, changing
blockers/body sizes, unsupported starts, partial paths, both directions over terrain
crests, and safe recorded segments. A compile-fail example rejects world mutation
between support calculation and actor validation. Existing ramp, stair, jump,
collision, steering and worker tests remain applicable.

```sh
cargo test --release --workspace
cargo test --release -p io-locomotion movement_stage_cpu_probe -- --ignored --nocapture
```

The scoped release probe retains the original two-pass implementation as a test-only
reference. In one run of 2,000 three-metre requests with nearby scenery:

| Pass | Time/request | World queries/request |
| --- | ---: | ---: |
| Old scenery check | 147.5 us | 108 |
| Old second full-world walk | 163.3 us | 108 |
| New actor-only validation | 2.5 us | 1 |
| Old combined check | 274.4 us | 216 |
| New combined stages | 140.4 us | 109 |

These are separate scoped timing batches, so individual times need not add to the
combined measurement. They are not a full-tick profile, GPU measurement or a timing
guarantee. The deterministic query-count tests establish the removed duplicate work.

### Full Simulation Comparison (2026-09-27)

The same saved stress inputs were replayed sequentially with release binaries,
1,800 paced samples per run (about thirty real seconds), no concurrent builds.
The previous probe binary is retained in `build/stages-before/io-worker-probe`.
Inputs, binary hashes, raw samples and summaries are in
`build/stages-{before,after}/{worker,repeats}`.

For the 128-NPC timed-mover scene, medians of three runs per version:

| Metric | Before | Staged |
| --- | ---: | ---: |
| Sampled tick p95 | 35.30 ms | 19.40 ms |
| Sampled tick p99 | 37.40 ms | 21.21 ms |
| Completed ticks | 1,272 | 1,755 |
| Simulated time in thirty real seconds | 21.20 s | 29.25 s |
| Overruns | 711 | 353 |
| NPCs moving >1 metre | 128/128 | 128/128 |
| Median NPC travel distance | 32.41 m | 33.99 m |

This reduces p99 by about 43%, but does **not** keep every tick within the 16.67 ms
budget. Improved time progression changes when patrols, congestion and moving
fixtures interact, so these are equal-input workloads, not identical trajectories.
Median total travel was 4,059 m before versus 4,026 m after; the improvement is not
a claim that every individual NPC progresses more quickly.

Single-run controls: the 16-NPC scene stayed at 1,798 ticks, with p99 11.73 -> 9.58 ms.
The stationary-load 128-NPC scene improved from 1,196 -> 1,738 ticks and p99
36.95 -> 20.84 ms; total NPC travel increased from 5,002 -> 6,833 m. These controls
have only one repeat, not statistical capacity guarantees. This probe measures CPU
simulation and frame preparation, not native GPU FPS. Global moving-geometry route
invalidation remains unchanged and is not solved by this pipeline.
