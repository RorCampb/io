# Traversal Stress District

This is a workload for the existing `io-playground` plugin, `io-traversal`
coordinator/background worker and optional `io-locomotion` surface provider. No new
crate, route type or motor was added. NPCs receive patrol
destinations; they must discover their routes from real collider/support geometry.
The current `stress-128.json` also enables the playground's bounded obstacle
discovery and attention-driven route reconsideration. It is not a pursuit demo
or a general crowd-avoidance system.

## Run

### Dense Crowd With Character Awareness

```sh
python3 tools/build_traversal_stress.py --npcs 384 --extra-obstacles 12 --debug-routes
make release
./build/release/io --scene assets/dungeon/stress-384.json
```

This variant triples the previous population: 24 NPCs per district, 384 total,
1,897 Items and 1,064 colliders. It adds 192 collidable barricades/crates without
changing the patrol destinations. Spawns are checked against body clearance and
support, not merely distinct coordinates. The original 128-NPC scene remains.

Both freshly generated stress scenes enable six scenery tracks **plus** six
character tracks per observer. The latter can include the player or any nearby
NPC, selected by distance, not a global all-pairs subscription or privileged
player tracking. The 384 scene services 24 observers per tick, preserving the
same nominal 16-tick sweep as the eight-observer 128 scene. FOV, occlusion,
attention gain/decay and retention still apply; discovery alone is not notice.

NPCs keep their patrol behavior. Noticing a character publishes observation
facts and retains attention; it does not mean chase/attack. Character motion
does not enter the moving-scenery braking/replanning policy, which would make
crowds stop or continually replan around one another. Live body collisions remain.
The HUD reports how many NPCs currently notice the player. E/A meters prefer an
attended character target over scenery, and keep both values tied to that target.
The meter buffer now holds 512 pairs, enough for every NPC here. Coarse route
traces retain their separate 256-actor debug budget.

### Original Size

```sh
make release
./build/release/io --scene assets/dungeon/stress-128.json
```

- 400 x 400 metres, sixteen neighborhoods, 96 simple buildings, crates, cover walls,
  lamps and sixteen raised crossings with physical ramps.
- 128 NPCs, 1,449 total Items and 872 colliders. The gold character is the player.
- Simulation target: 144 Hz, not a guarantee of achieved SIM Hz or display refresh.
- Automatic observation: eight observers serviced per tick, up to six nearby
  scenery and six character tracks each. See [discovery configuration](attention.md#dynamic-obstacle-discovery-playground-plugin).
- Eight orange kinematic loads move ten metres over three seconds, resting twelve
  seconds between movements. They affect actual collision and navigation revisions.
- WASD moves; Space jumps; Shift crouches. Existing two-finger orbit/pinch zoom and
  arrow/right-drag camera controls remain available.
- HUD shows planning/following/arrived/blocked totals, outstanding jobs, results and
  deferred admissions. The existing FPS/SIM overlay still measures rendering and
  simulation separately. `LAST PLAN` is job wall latency, not per-tick CPU cost.

`stress-128.json` now enables `traversal.debug_routes`. Cyan/orange lines show the
accepted remaining routes, starting at each NPC; crosses mark waypoints. The two
colors separate alternating actors, not success/failure or planning state. Guides
draw through scenery for debugging. A pending replacement is not shown until
accepted; an NPC with no accepted route has no line. The overlay supports up to
256 actors and 128 waypoints per actor, rather than the previous two-actor limit.
Set `debug_routes` to `false` for clean viewing/performance comparisons. Regenerate
with `--debug-routes` to keep the overlay enabled; the generator defaults it off.

Agents now opt into `steering.trajectory` for rolling corner refinement. Yellow
traces show local sampled trajectories overlaid on the coarse cyan/orange route.
See [trajectory contracts and settings](movement-stages.md#rolling-local-trajectories).
Remove that agent's `trajectory` setting for a waypoint-only comparison. The older
saved scene variants remain unchanged; newly generated ones enable trajectories.

Use `stress-16.json`, `stress-64.json` or `stress-256.json` to change population.
`stress-overview.json` shows the entire district with 128 NPCs.
`stress-static.json` keeps those eight loads stationary; `stress-churn.json` moves
them continuously. These two control cases share the same 128-NPC layout and goals.
The 256-NPC scene has 1,577 Items. These are authored workloads, not engine limits.
Those older saved scenes retain their existing 60 Hz/explicit-only configuration;
do not treat them as identical controls for today's 144 Hz observation-enabled scene.

```sh
python3 tools/build_traversal_stress.py --npcs 128 --long-routes \
  --output assets/dungeon/stress-long.json
```

The generator supports 1..384 nonoverlapping spawns for this layout. `--long-routes`
sends some agents between neighborhoods instead of only around their local block.
`--moving timed|continuous|static` and `--overview` select the other controls.
Package paths are rebased if output is written outside `assets/dungeon`.
Newly generated scenes enable obstacle discovery and default to 144 Hz;
`--tick-hz 60` selects the historical simulation target. For a discovery-off
control, omit `traversal.obstacle_observations` from a copied scene.

## Measure Progress, Not Just FPS

Build before measuring; do not run compilation or another scene alongside a run.

```sh
cargo build --release -p io --bin io-worker-probe
python3 tools/benchmark_navigation.py build/my-stress-results \
  --scenes stress-16 stress-64 stress-128 stress-256 stress-static stress-churn \
  --frames 1800 --repeats 3
```

This samples for approximately thirty real seconds per run. The output directory
must not already exist. It saves exact inputs, executable/input hashes, raw tick
samples, end states and a summary. It measures paced simulation plus CPU render
packet preparation, **not GPU FPS**. Unpaced native benchmarks can finish before
background routes are ready, so they are not equivalent gameplay-load comparisons.

Read simulation p95/p99 and overruns alongside `npcs_moved`, per-actor distance,
first-motion time, route latency, accepted/rejected results, pending jobs and mover
distance. `npcs_moved` counts actors travelling over one metre. Distances are sampled
path lengths and can underestimate travel if snapshots are skipped. A fast tick
with all NPCs waiting is not a successful navigation result.

### Initial Baseline (2026-09-27)

One thirty-second run per case, release build, sequentially on this machine
(macOS arm64). These are exploratory samples, not repeatability bounds or capacity
guarantees. Raw results and exact inputs are in `build/stress-district-baseline`.

| Case | NPCs moved >1m | Sampled tick p99 | Completed ticks | Overruns |
| --- | ---: | ---: | ---: | ---: |
| 16, timed movers | 16/16 | 11.71 ms | 1,798 | 1 |
| 64, timed movers | 64/64 | 19.28 ms | 1,778 | 302 |
| 128, timed movers | 128/128 | 36.47 ms | 1,284 | 700 |
| 256, timed movers | 239/256 | 70.53 ms | 1,105 | 517 |
| 128, stationary loads | 128/128 | 37.22 ms | 1,198 | 874 |
| 128, continuous movers | 0/128 | 3.25 ms | 1,798 | 0 |

The intended rate is 60 Hz, about 1,800 ticks over thirty seconds. Lower tick
counts mean the simulation fell behind; they are not rendered frame counts.
CPU render preparation p95 stayed below 0.9 ms in these runs, excluding GPU work.
The 128-NPC timed case had a median sampled travel distance of 32.2 metres.
Some timed fixtures paused for actor clearance; only two moved in that run.
The continuous case moved all eight fixtures about 99 metres each, but accepted
zero routes and cancelled 422 jobs. Its low tick time is a failure to make progress,
not evidence of better capacity. The stationary case also exceeded the tick budget,
so moving-geometry invalidation is not the only remaining scaling issue.

## Current Constraints

### Inactivity Diagnosis (2026-09-27)

Read-only probe instrumentation was added after stationary NPCs were mistaken for
intentional patrol idle time. There is no patrol dwell timer. The proposed wandering
policy was removed before measurement; scene files, movement speeds, collision checks,
planner budgets and simulation timing were not changed.

Two sequential 60-second release runs per existing scene, with no concurrent builds:
raw reports, input copies and binary hashes are in `build/traffic-diagnosis`.

| Scene / run | Moving actor-time | Stationary >5s at end | Cancelled jobs | Tick p99 |
| --- | ---: | ---: | ---: | ---: |
| Timed loads / 1 | 20.4% | 128/128 | 448 | 19.61 ms |
| Timed loads / 2 | 20.6% | 120/128 | 452 | 19.87 ms |
| Stationary loads / 1 | 64.7% | 32/128 | 0 | 20.66 ms |
| Stationary loads / 2 | 61.9% | 44/128 | 0 | 20.70 ms |

All 128 NPCs travelled over one metre in **every** run. That older progress metric
alone concealed the subsequent inactivity. Timed runs ended with all 128 reported
as Planning and 32 outstanding jobs. About 99% of their stationary actor-time was
labelled Planning, not Blocked. World navigation revisions changed 1,155/1,268 times;
the stationary controls changed only eight times during initialization. Lower late-run
tick costs in the timed scene reflect fewer moving actors, not higher capacity.

There are two demonstrated problems:

1. Global invalidation: `RouteProvider::revision` defaults to the world's navigation
   revision. `RouteCoordinator::prepare` cancels execution, clears the route and requests
   planning when that revision changes. Independent moving loads repeatedly invalidate
   actors in unrelated blocks. Searches and queued admissions compete to recover before
   the next change. This is repeated replanning/starvation, not all actors physically
   blocking each other.
2. Circular destination occupancy: in stationary run 1, 32 persistently stopped actors
   formed eight four-actor loops. Example Item IDs: 1346 -> 1362 -> 1378 -> 1394 -> 1346.
   Each destination was occupied by the next stopped actor (within 0.015m of its anchor,
   bodies have 0.35m radius). `SurfaceRoutes::endpoints` checks `finish` before route
   search; `finish` rejects a destination connector occupied by another actor. The
   actors therefore never start clearing the loop. The coordinator retries the same
   objective with backoff and playground patrol advances only on Arrived. This is an
   actual circular wait, not evidence that every stationary actor is deadlocked.

Those were the pre-fix findings. The attention/routing follow-up below implements
continuous route retention and occupied-destination approach/wait behavior, without
collision bypasses or randomized replacement goals.

The probe now reports `activity.seconds_by_status`, longest/current stationary spans,
per-snapshot moving counts, stationary counts by status, navigation revisions and
planner counters, plus final goals/remaining routes. Motion means sampled displacement
of at least 0.05m per simulated second; Following alone is not motion. Intervals are
weighted by simulation ticks and attributed to the ending snapshot's status. Skipped
snapshots can hide brief movement/state transitions, so these are estimates, not
solver-level causal traces. Planning includes admission, search and retry waiting.
There is no renderer change and these measurements are not GPU FPS.

Reproduce with the existing benchmark command, `--scenes stress-128 stress-static
--frames 3600 --repeats 2`, choosing a new output directory. Future harness summaries
include `moving_fraction` and stationary durations; the original reports above retain
all underlying samples, including activity fields.

The later [movement-stage comparison](movement-stages.md#full-simulation-comparison-2026-09-27)
records the improvement from eliminating repeated support work. The initial baseline
above is retained as historical data, not the current implementation's performance.

The worker queue stays bounded at 32 outstanding jobs while Items and registered
NPCs may exceed that count. Full-queue admission is deferred before snapshot copying.
The existing round-robin admission still adds population-dependent startup latency.

The surface provider still invalidates its scenery cache against a global geometry
revision, but accepted live-checked routes and immutable background jobs are no longer
cancelled by unrelated revisions. Region/edge-scoped cache invalidation remains future
work; loads remain real colliders, not render-only decoration.

Actors still use live collision safety and local stop/resume yielding, not cooperative
crowd reservations, priority negotiation or anticipatory speed matching.
Congestion and blocked routes are observable outcomes. This workload tests walking
and supported slopes; the existing athletics/guards tests cover jumps and stairs.

Ramps extend slightly below the floor: the current feet-center support query can
reject a flush ramp entrance when the body's leading edge touches the incline
before its center reaches it. This is a known geometry/support edge case, not a
general support fix. A regression test walks both slopes across the raised crossing.

`barrier_cycles` is playground fixture configuration, not an engine moving-platform
component. Singular `barrier_cycle` remains compatible. Up to 64 uniquely bound
kinematic fixtures can be configured. `travel_seconds: 0` retains instantaneous
shutters; positive durations use swept actor bounds and pause before hitting actors.
Fixtures do not crush, push or transport actors, require clear authored lanes, and
retain the playground's pause-while-an-NPC-is-airborne safety rule. They remain
active off-camera through the plugin's existing active-Item contract.

## Attention/Routing Follow-Up (2026-09-27)

The existing observation stages now feed a plugin-owned route-attention stage and
ticket-checked `Reconsider` request; see [Attention](attention.md#attention-to-route-decisions).
Surface routes retain safe execution across unrelated geometry revisions; continuous
background completions validate a connector from the current pose. Occupied destinations
no longer reject the entire trip, and immediate actor blockage retains the route.
Discrete-action providers remain revision-bound. No scene population, geometry, goals,
fixture schedule or collision behavior was disabled to obtain these results.

Sequential, paced 60-second runs replayed identical saved inputs from
`build/traffic-diagnosis`. One fresh baseline per scene is in
`build/attention-routing-before`; final-build runs are in `build/attention-routing-final`.
An earlier post-change run is retained in `build/attention-routing-after`. Each directory
contains raw samples, scene copies and executable/input hashes. No builds or other engine
instances ran during measurements. These are limited runs, not statistical confidence bounds.

| Measurement | Moving loads before / final | Stationary loads before / final |
| --- | --- | --- |
| Moving actor-time | 20.59% / 75.26% | 62.07% / 82.14% |
| NPCs ending stationary >5s | 122 / 0 | 44 / 0 |
| Cancelled planning jobs | 499 / 0 | 0 / 0 |
| Total NPC distance | 4,495m / 16,232m | 13,874m / 17,304m |
| Simulation tick p95 | 18.34ms / 20.40ms | 18.83ms / 21.35ms |
| Completed ticks | 3,557 / 3,406 | 3,545 / 3,324 |

This is improved useful movement, **not** a tick-time or GPU-FPS speedup. More actors
are actually executing, so simulation cost rises and the 16.67ms budget for 60Hz is
still exceeded. Initial admission/search latency remains roughly nine seconds for
the slowest starters; zero long-stationary actors at the end does not mean no waiting
ever occurred. Congestion, crowd priorities and anticipatory pacing remain future work.

These stress scenes have no observation pairs. They verify the removal of blanket
route cancellation and occupied-goal waits, not perception scalability. The direct
attention-to-route regression is `cargo test --release -p io-playground observed_opening`:
an NPC cannot reach its goal, a gate moves, and no new plan is submitted below the
attention threshold. Lowering the threshold makes the visible opening noticed and the
NPC reaches its goal with the same objective ticket. Separate tests verify unseen walls
remain collidable and four mutually occupied patrol destinations do not deadlock.
