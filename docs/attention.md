# Attention Exercise

```sh
make release
./build/release/io --scene assets/dungeon/attention.json
```

The patrolling **runner is the observer**, and your controllable **hero is the
target**. The runner walks around a barrier using the navigator and the same
character motor as the hero. Leave the controls alone to watch awareness rise
and fade as the runner turns, or move with WASD to test cover and rear approaches.
Hold Shift to crouch.
Two-finger/right-drag orbit and pinch/wheel zoom adjust the camera, not NPC sight.

Two small, read-only slider-style gauges follow the tracked hero above your head:

- Gold: **RUNNER ATTENTION**, the runner's time-smoothed visual awareness of you.
- Cyan: **FOCUS**, the current angle-, distance-, and visibility-weighted score.
  Strongest directly ahead; fades toward zero at the peripheral edge.

The name on the gold meter identifies whose awareness it measures; the character
under the meter is the target. Your facing does not control the runner's eyesight.
Behind the runner, focus should be zero and existing awareness should fade,
not rise. His motor turns gradually, so sight follows his body rather than snapping
instantly to his next movement direction.

The labels stay attached through movement, camera orbit and zoom, using the same
interpolated character position as rendering. They are debug overlays: they remain
visible through scenery so you can inspect decay when the target is hidden from
the observer. They do not imply the observer knows the target's hidden position.
They show measurements; they are not draggable configuration controls.

## Mechanics and Ownership

For combining visual facts with radio or other sources, see the engine's
[shared observation intake](observation-intake.md). It retains explicit provenance
and does not inject reported information into visual attention.

`io-world::sight_sample_clear` reuses the existing collider/terrain sight test,
excluding the observer and target themselves. `io-perception` samples nine
positions on an upright character-body proxy, or current bounds for other Items. A
sample must be inside the observer's horizontal view cone, in range, and unobstructed.
Evidence is weighted by view angle and distance. Sample exposure is an
approximation, not exact mesh silhouette area, and can change in discrete steps.
Other characters do not currently occlude the sight rays.

`fov_degrees` accepts 5 through 360 degrees and is the full horizontal angle:
120 means 60 degrees on either side
of the body's local -Y forward direction. Vertical displacement affects 3D range
and occlusion, but not peripheral weighting. This avoids a nearby character's legs
artificially lowering detection just because they are below eye level. There is
no separate vertical field of view or head pitch yet; this is an upright-body
vision model, not a complete eye/head rig.

For each visible sample, the directional weight uses a dot product with the
model's world-space forward vector and a smoothstep falloff:

```text
c = clamp((dot(forward, direction) - cos(half_fov)) / (1 - cos(half_fov)), 0, 1)
directional_weight = c*c*(3 - 2*c)
focus = sum(directional_weight * (1 - distance/range)) / 9
```

Blocked and out-of-cone samples contribute zero. The angular weight is 1 at
direct front and 0 at the edge, with no abrupt slope at either end. For the
passive demo's 60-degree half-angle, its values at 0/30/45/55/60 degrees are roughly
100/82/37/6/0 percent, before visibility and distance attenuation. Raw unweighted
sample exposure remains available to Rust but is not the cyan display anymore.

The bottom-left live tuning panel (F2 to hide/show) adjusts FOV without resetting
awareness. The cat-and-mouse scene starts at a wider 240 degrees and also exposes
its pursuit attention threshold. See [Live Vision Controls](cat-mouse.md#live-vision-controls).

The per-observer/target awareness value follows that focus level:

```text
rate = gain if focus > A else decay
dA/dt = rate * (focus - A)
A_next = focus + (A - focus) * exp(-rate * dt)
```

It uses the exact exponential solution for evidence held constant over each
simulation update, including a defined zero-rate case. Brief glimpses contribute
little, sustained exposure builds awareness toward the current focus level, and
occlusion causes gradual decay. Constant 10% peripheral focus approaches 10%
awareness, not a much higher alert level. Awareness retained from an earlier
frontal view may temporarily exceed current focus, then decays toward it.
This replaces the previous saturating accumulator, where weak focus could
eventually produce disproportionately high awareness. A zero gain freezes rises;
a zero decay freezes falls. Rates are in inverse seconds, not percentages per tick.
Tests compare constant-evidence integration at 30, 60 and 144 Hz. Changing the sample
frequency can still change which brief moving glimpses are observed.

The `io-playground` example plugin composes this with locomotion and navigation. It keeps the
observer and target active off-camera, while the renderer only receives generic
projected gauges. There is no aggression threshold, automatic head turning,
communication, target selection, or behavior planner in this exercise. The gold
"attention" gauge demonstrates accumulated awareness, not a full attention policy.

## Configuration

Observation pairs live under the scene's `traversal.observations`:

```json
{
  "observer": "runner",
  "target": "hero",
  "notice_attention": 0.2,
  "vision": {
    "range": 24,
    "fov_degrees": 120,
    "gain_per_second": 2.4,
    "decay_per_second": 0.35
  }
}
```

Each pair has independent memory. The example supports four pairs, with labels
stacked when several observers track the same target. The observer still needs a
character body (eye height/facing); the target may be any valid Item. Unknown names, duplicate pairs, self-observation and
invalid numerical settings are rejected.

The observer's existing navigation entry uses an optional arrival policy:

```json
"on_arrival": {
  "type": "patrol",
  "points": [[55, 60, 0], [45, 60, 0], [45, 50, 0], [55, 50, 0]]
}
```

After reaching its initial goal it repeatedly requests those destinations. The
default remains `{"type":"stop"}`, preserving the original navigation demo.
This is a simple authored routine, not a tactical planner. The motor still checks
collisions and navigation still handles unreachable/temporarily blocked routes.

## Item Change Pipeline

The existing `ObservationTrack` now composes the existing `io-game::Stage` contract:

```rust
use io_game::stage::Stage;
use io_perception::pipeline::{RelevanceStage, EvidenceStage, AttentionStage};

let mut pipeline = RelevanceStage.then(EvidenceStage).then(AttentionStage);
// ObservationRequest borrows WorldView, an ObservationCache and the existing Awareness.
let result = pipeline.run(request)?;
// Use result.notice directly in a developer behavior stage; it does not issue routes.
```

- `RelevanceStage` matches changed Item IDs against this observer/target pair. Sight
  obstruction changes intersecting the pair's conservative bounds also trigger checks,
  including an occluder's old location. Terrain replacement triggers resampling.
- `EvidenceStage` refreshes visual evidence only when those inputs (or FOV settings)
  changed. A notification is not proof of perception. Non-character targets use
  current geometry bounds, not swept/interpolated render history; this is not mesh vision.
- `AttentionStage` advances the existing `Awareness` every tick, even when evidence
  is reused. A stationary visible Item can become noticed without moving again.
  `notice_attention` defaults to 0.2 and accepts 0..1. No threshold gates collision checks.

`ObservationResult` supplies evidence, current contact, whether rays were resampled,
and an optional `ItemNotice { observer, target, kind, position, attention,
previous_bounds, bounds }`. The previous bounds are the last *noticed* bounds,
not an unseen intermediate pose. Notices
are Acquired, Changed or Resynchronized. Hidden changes do not expose positions or
produce notices; pending changes can be noticed when visible and attentive later.
This is a generic signal, not an interpretation such as "door opened". Developers
decide what that means and whether to replace a navigation objective. No automatic
replan is issued by perception.

`ObservationTrack::notice()` exposes the latest result directly for plugin logic.
`Event::Observation` additionally publishes it in the existing bounded presentation
history; authoritative decisions must not depend on that potentially lossy history.
Existing pursuit keeps consuming visual contact and its own settings, not a new motor.

`io-world::ChangeLog` records pose/rotation, character height, visual state, damage,
dynamic-body attachment, actual physics displacement and terrain replacement. A
requested kinematic destination is not a displacement until physics applies it.
Animation clock advances and force/settings changes are not visual changes by
themselves in this proxy model. This is not a complete gameplay event log or
developer component registry. Private plugin state still needs developer events.

The journal retains 4,096 records, uses copy-on-write sharing with immutable world
snapshots, and has per-world identity plus monotonic sequence cursors. Consumers
behind retained history explicitly resample; they must not infer no change. Custom
WorldView adapters without a journal also resample conservatively. Caches are
bound to one world and observer/target pair; snapshot clones retain that lineage.

### Scope

This integrates **existing, configured observation relationships**. It does not yet
discover new target relationships automatically, build a map-wide observer subscription
index, or share acquired knowledge between NPCs. A target already registered but out
of sight can enter view through target/observer movement or a moving occluder, even
with zero attention. The demo still caps configured pairs at four; the stage API does not.

### Attention To Route Decisions

The playground now consumes notices directly through `RouteAttentionStage` before
updating navigation. It selects the observer's existing objective by Item ID and
ticket. For a physical non-character target, its current or last-noticed bounds
must overlap the remaining body-sized route corridor. Blocked/unreachable objectives
can reconsider a newly noticed opening even when no route exists. Unrelated observers,
idle/finished objectives and unrelated corridors are not notified. This conservative
policy belongs to the example game plugin, not perception or engine object semantics.

```rust
// A developer's own behavior stage can choose this same checked request.
let reply = movement.request(Envelope::new(message_id,
    NavigationRequest::Reconsider { ticket }));
```

`Reconsider` preserves the objective ticket, rejects stale tickets, and coalesces
notifications without cancelling work already in flight. Background execution keeps
following a safe accepted route; inline reconsideration searches a separate candidate
within its existing expansion budget. A notice received during background work queues
at most one further reconsideration, so an older result cannot silently consume it.
Replacement routes must connect from the actor's current position before installation.

`SurfaceRoutes` opts into `RouteValidity::LiveChecked`: unrelated world revisions
no longer cancel continuous routes or their background jobs. Scenery caches still
invalidate for correctness. Live support and collision checks can stop movement and
request a repair even for a wall the NPC has not seen. This is safety, not a broadcast
of new knowledge. Unreachable objectives stay dormant until reconsidered or retargeted.
The default provider contract remains `RevisionBound`; discrete athletics/cargo actions
retain conservative geometry-revision and takeoff validation. This is **not** a general
knowledge-restricted planner or a per-edge dependency cache implementation.

Occupied destination handling is a separate fix: the supplied walking provider can
approach an occupied goal, then yield locally with its route retained. It does not
ignore physical actors, teleport through them, or claim general crowd deadlock freedom.

### Dynamic Obstacle Discovery (Playground Plugin)

The 128-NPC stress scene now opts into discovery instead of declaring every
observer/target pair. In the scene's `traversal` configuration:

```json
"obstacle_observations": {
  "vision": {
    "range": 18,
    "fov_degrees": 200,
    "gain_per_second": 6,
    "decay_per_second": 2
  },
  "notice_attention": 0.15,
  "interval_seconds": 0.1,
  "retention_seconds": 2,
  "observers_per_tick": 8,
  "tracks_per_observer": 6,
  "character_tracks_per_observer": 6
}
```

Omit this setting to retain explicit-only observation behavior. It applies to
registered navigation NPCs as observers, with the player eligible as a target.
`character_tracks_per_observer` defaults to zero for older scenes and accepts
0..16; its capacity is separate from the 1..16 scenery slots. Crowds cannot evict
all scenery tracks. Characters need no separate collider to be observable.
Existing explicit pursuit tracks
keep their independent settings and per-tick sampling, and take precedence over
discovery for the same pair. Automatic tracks share compact world-scaled meters
above their observer, not one screen-sized label per pair. Attended characters
take precedence for that HUD pair; evidence and attention refer to the same target.

Ownership and flow:

1. `io-playground::ObstacleDiscoveryStage` queries nearby Item bounds. The supplied
   policy selects nearest collidable non-character Items and optionally nearest
   characters using independent quotas, overlapping body height,
   ignoring floors below the feet and roofs overhead. It uses stable Item IDs and
   distance/ID ordering, not asset names, districts, or preselected obstacles.
2. Plugin-owned `ObstacleObservations` rotates through observers, maintains bounded
   tracks and their individual attention/cache state, and runs the existing
   relevance/evidence/attention stages. Discovery alone grants no awareness:
   visibility, facing, range and attention thresholds still apply.
3. A qualified notice compares current observed geometry with that pair's last
   noticed geometry. Journal overflow forces fresh evidence, but reacquiring
   unchanged geometry does not request another route. This comparison occurs only
   after a notice; hidden mutations do not update remembered geometry.
4. The existing `RouteAttentionStage` correlates notices with remaining routes and
   submits the existing ticketed `Reconsider` request. The coordinator coalesces
   work and retains safe routes. Live collision safety remains unconditional.

Character tracks publish the same `ItemNotice` observation facts and maintain
their own memory, but do not enter scenery invalidation or the moving-obstacle
emergency-braking policy. The example leaves patrol objectives unchanged. Plugins
can read `Traversal::discovered_observations()` directly to supply their own
social/combat responses; `observers_noticing(target)` counts current visual evidence
above the attention threshold. This is not a chase command or a global visibility
flag. The HUD uses that query for the player-seen count. Offscreen NPCs still
participate in the same bounded observer schedule.

When an initial search has no route yet, each retained track holds its latest
qualified routing notice until there is a route or search result to evaluate.
It does not blindly request a second search on first sight of every nearby wall.
Pending changes coalesce, preserving the earliest unhandled old bounds, and the
plugin submits at most one reconsideration per observer per tick. These notices
expire with their tracks; they are not an unbounded reliable event log.

There is no new engine crate, navigation algorithm, or renderer contract. Developers
can replace this plugin's candidate/reaction policy using `WorldView` queries,
`ObservationTrack`, pipeline stages and `NavigationRequest`; the engine does not
define what a door, enemy, or interesting object means.

The observer budget caps serviced observers per tick, and the track cap bounds
memory and samples. Discovery queries themselves still scale with local spatial
density; this is not a hard CPU-time guarantee. Sampling occurs no faster than the
interval and can be slower under a large population/budget ratio. New tracks start
at zero; sampling gaps credit at most 0.25 seconds, avoiding an instant attention
jump after a stall. Out-of-range/unselected tracks retain memory for up to the
retention interval, checked on service, but can be evicted earlier for nearer
candidates. Expired tracks reacquire from zero. Brief events between samples can
be missed. This policy is not full environmental memory or knowledge-limited search.

`io-worker-probe` reports `discovery` counters: scans, samples, qualified notices,
unchanged notices suppressed for routing, reconsideration submissions, and current
tracks. A reconsideration count does not imply a completed or accepted replacement.
Tests cover occlusion, retained noticed bounds, expiry, explicit-pair precedence,
budget fairness, snapshot isolation, journal overflow, an automatically discovered
opening, and a detour requested more than two meters before obstacle contact.

### Existing Pipeline Verification

Regression tests cover unchanged evidence with continuing attention integration,
irrelevant Item changes, hidden/reacquired targets, ordinary physical targets,
occluder movement, current rather than interpolated bounds, snapshot isolation,
world/pair identity, invalid requests, journal overflow and rejected/no-op mutations.
The existing attention/pursuit native scenes use this pipeline without asset changes.
End-to-end route tests additionally cover an opening below/above the notice threshold,
the same objective ticket reaching its destination, unrelated geometry churn during
background planning, safe execution during repeated reconsideration, stale requests,
unobserved wall collision, and approaching/waiting/resuming at an occupied goal.
The opening test also exposed an idle-facing bug: the locomotion motor initialized
yaw to zero rather than the Item's configured orientation. It now preserves initial
facing, so attention's forward direction is not silently reset on the first update.

A release overhead check replayed the same 128-NPC stationary-load scene for thirty
seconds, once per binary, sequentially without concurrent builds. Before/after raw
data and input/binary hashes are in `build/observation-overhead-{before,after}`.
Tick p95 was 19.61/19.52ms, p99 20.55/20.81ms, completed ticks 1749/1747 and moving
actor-time 62.55/62.17%. This limited check found no large overhead regression; it
is not a statistically established speedup or an NPC-throughput improvement.
The scene has no observation pairs, so it measures journal/publication overhead,
not perception scalability. Planning stalls and patrol congestion remain visible.

Regenerate the scene with `python3 tools/build_attention_demo.py`. It uses existing
assets and leaves the dungeon and navigation scenes unchanged.

## Verification

```sh
make test
make PROFILE=release gpu-test all
```

The new GPU test exercises object tracking, orbit projection, zero/partial focus,
bounded awareness, fading awareness, and actual gauge pixels. It writes
`build/camera-attention-visible.ppm`, `camera-attention-covered.ppm` and
`camera-attention-return.ppm`. Headless tests also verify off-camera simulation,
independent observer memory, rear blindness across multiple orientations,
close-range visibility, strict contracts, and time integration.
