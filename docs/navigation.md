# Character Navigation

This scene uses the optional `io-locomotion` plugin, not a humanoid feature in core. The generic
`io_traversal::navigation::RouteProvider` and `io_traversal::TraversalExecutor` contracts do
not require surface grids or character components. See
[Movement Plugins](movement-plugins.md) to supply a different implementation.
The names and geometric limits below describe `io_locomotion::surface`.

```sh
make release
./build/release/io --scene assets/dungeon/navigation.json
```

The scout is a second Mixamo character, initially beside/behind the player. It
plans around the player and pillars, crouches through the low entrance, and
stands up in the chamber at `[50, 37, 0]`. Follow it with WASD and Shift; camera
controls are unchanged. The HUD shows each NPC's planning/following/arrival status,
discovered cells and shared-cache hits. Restart the scene to repeat.

`entry.json` remains the unchanged camera/locomotion regression scene. Regenerate
this separate example after intentional dungeon edits with
`python3 tools/build_navigation_demo.py`.

## Ownership

- `io-world` owns physical geometry, spatial queries, character body dimensions,
  collision sweeps, and portal membership. `navigation_revision` changes when
  physical scenery moves or terrain is replaced, not when characters walk or
  animate. Snapshots carry that revision.
- Body configuration (`CharacterBody`) is distinct from runtime support state
  (`grounded`). See [Bodies and Surfaces](world-surfaces.md) for migration and the
  remaining traversal-extension roadmap.
- `io-traversal::navigation` owns generic incremental graph search.
  The optional `io_locomotion::surface` provider owns the shared surface-clearance cache.
  Each `Agent` holds its goal, typed status, route/search, and personal `Knowledge`.
  Geometry is queried from the world, never copied into each character.
- `io_locomotion::Motor` executes movement, crouching, gravity, jumping,
  turning and animation for both humans and NPCs. `io-traversal::Movement` owns
  budgeted route execution, checked requests and per-tick feedback. The example
  `io-playground::Traversal` game plugin
  supplies player input or navigation intentions; it does not teleport NPCs along
  paths. `io-game::Session` advances controlled actors even when none are visible.
- The host resolves model/clip names for each actor. C/OpenGL receives the usual
  transforms and animation state; no AI-specific renderer or FFI API was added.

Interior labels do not make geometry traversable. Clearance comes from real
colliders and support tests. Accepted motor movement uses the existing portal
crossing logic to change interior membership.

## Data Contract

Add `navigation` under a scene's `traversal` configuration:

```json
{
  "domain": {
    "origin": [40, 30, 0],
    "size": [20, 36],
    "cell_size": 0.5
  },
  "expansions_per_tick": 12,
  "agents": [{
    "item": "scout",
    "goal": [50, 37, 0],
    "can_crouch": true,
    "familiar_points": [[51, 59, 0], [51, 58, 0]]
  }]
}
```

Domain, goal and familiar points are **absolute world coordinates**. The domain
defines an XY sampling region, not a fixed floor or an NPC's familiar-area boundary.
`origin[2]` is retained for scene compatibility but no longer constrains elevation.
Smaller cell size adds horizontal detail; XY columns are limited to 65,536.
The background queue admits 32 jobs at once, not 32 total NPCs. Multiple supporting surfaces can
occupy one column. Nodes are discovered lazily, not baked as a volume of voxels.
Goals and familiar points remain actual supported feet anchors, not arbitrary
positions above a surface or automatic requests to jump between levels.

Each NPC can supply a complete `locomotion` object with the same speed, height,
gravity, turning and clip fields as player locomotion. Omission inherits those
settings, but clip names are still resolved against the NPC's own model. Body
radius and maximum slope come from that item's `character_body` component. Different heights/radii are
numbers, not different Rust actor types. Navigation currently generates only
`Walk` and `Crouch` actions; the motor also supports player-controlled jumps.

The default arrival policy stops at the destination. An optional
`on_arrival: {"type":"patrol", "points":[...]}` repeats 2-64 authored world-space
destinations through the same navigator. See the attention exercise for an example.

Shared directed connections are keyed by starting column and physical surface,
destination column, exact radius/standing/crouching dimensions and maximum slope.
A standalone `io_locomotion::surface::MovementProfile` has a `max_slope` field (legacy
serialized profiles default to 0.8); motor bindings always copy the actual body limit.
Movement speeds affect route cost, not geometric cache compatibility. A shorter
actor can walk where a taller actor must crouch; a non-crouching actor cannot
reuse a crouching route as if it were walkable.

Familiar points prefill knowledge incrementally. Subsequent planning discovers
additional surface nodes through the same queries, regardless of whether they were
familiar. `Agent::set_goal` retains that knowledge. Knowledge here means geometry
examined by navigation, **not** NPC eyesight or a sensory/behavior simulation.
The legacy `known_cells` metric now counts surface nodes: two stacked floors at
the same XY coordinate are two entries.

## Budgets and Changes

The horizontal lattice considers eight directions with cardinal/diagonal costs of
1000/1414 and an octile-distance heuristic. Each expansion follows support from its
current layer to neighboring columns. Bounded height probes query nearby physical
geometry through the world spatial index; no whole-map vertical scan is required.
The returned supporting source and body-adjusted height identify the next node.
Every connection checks body clearance, including
continuous swept-box tests against oriented box colliders and terrain triangles.
Support placements are checked at bounded intervals through the same
`walk_character` implementation used by the motor. `RouteRefinement` prepares
shortcuts over up to 32 upcoming nodes within a configurable horizon (default 3m),
then validates merged collinear runs to remove unnecessary intermediate stops.
It preserves walk/crouch and discrete-action boundaries. This work runs once per
new route, on the planning worker in background mode, not every following tick.
`RouteFollower` consumes the resulting existing waypoint contract; it can skip
exact collinear continuation arithmetically without a new geometry trace.
AccelerationSteering changes requested
velocity gradually and slows near its current target; the live movement segment
is checked as surface-following motion before motor execution, not as a flat
segment at the old height. Tight waypoint corners are retained until the next
connector is clear from the actor's actual position. These checks are separately counted by
`Stats::steering_queries`, not charged as A* expansions. Snapshot preparation
has its own `Stats::refinement_queries`. See [Movement stages](movement-stages.md#route-preparation-pipeline).

Each navigation agent may configure:

```json
"steering": {"look_ahead": 3, "acceleration": 9, "braking": 14, "arrival_response": 0.25}
```

Look-ahead is in meters, acceleration/braking in meters per second squared, and
arrival response in seconds. These are mechanics, not enemy policy. Developers
can replace the response through the public `Steering` trait; see
[Movement Plugins](movement-plugins.md). This is not spline navigation or crowd
avoidance. Collision, cancellation and unavailable routes can stop immediately.

The plugin assigns one shared expansion budget round-robin across NPCs; this is
not a per-NPC multiplier. Following a route does not run A* again every tick.
Cache entries are bounded at 262,144. Published game clones share immutable route
arrays and copy-on-write search/cache data. The expansion budget bounds frontier
work, not an exact millisecond deadline; collider density still affects cost.
Search/knowledge storage grows with discovered surface nodes, not just XY columns.
The column limit is not a total layered-node memory cap.

Scenery revision changes invalidate the cache and discovered cells. Cache invalidation
still covers the whole navigation domain, not individual tiles. Accepted surface routes
are live-checked, so this does not cancel them or their background replacement jobs.
Searches performed inline against changing inputs still restart; background searches
use immutable snapshots and validate a current connector when handing off a continuous
route. Discrete-action providers remain revision-bound by default. A navigation
service and its agents belong to one world lifetime; recreate them when loading
another world (the scene/plugin adapter does this).

Approach requests keep the target's height. `Navigation::offset_destination`
follows local support from a valid feet reference on the desired layer to propose
approach-ring destinations (at most 20m offsets), then checks endpoint availability.
This does not prove a full route from the requesting actor. The current helper
requires that reference to support the requesting body's dimensions; it is not an
arbitrary airborne-target projection. Candidate discovery and endpoint checks are
bounded separately from A* expansions.

Other characters are checked separately during planning and never baked into
the shared scenery cache. Actual movement always uses live collisions. If a
character blocks immediate walking, the NPC yields while retaining its route. Route
refinement tests snapshot scenery; immediate steering and motor execution test live actors too.
Occupied goal connectors can be planned, with transient checks deferred within a
body/grid-sized approach region; the actor still stops before contact. Other actor
checks along search edges remain enabled. Persistent collision/support failure can
block and retry with a capped 1-8 second backoff. `Blocked` means temporary traffic; `Unreachable`
means no route in this domain for this profile. If necessary a second budgeted
scenery-only search distinguishes those outcomes. This is not crowd steering or
deadlock-free multi-agent planning. Developers issue ticket-checked `Reconsider`
when their observation/attention policy warrants re-evaluating the same objective;
see [Attention](attention.md#attention-to-route-decisions). Geometry changes alone do
not wake every unreachable walking objective. A permanent actor obstruction still
needs a game/local-avoidance policy; this change does not supply traffic priorities.

## Current Limits

This milestone plans walking/crouching across level floors, tilted box ramps,
heightfield hills and connected stacked levels, with walls, pillars, doorways and
low ceilings. Stacked surfaces remain disconnected unless a supported route joins
them; sharing XY coordinates does not create a vertical shortcut. It checks body
clearance and support along every sampled edge, not just destination cells.
It does not generate jump trajectories, stair-climbing, ladders, moving-platform
rides or developer-defined action links. Unsupported/out-of-domain goals report
unreachable, not a fake straight-line route. This is a surface graph, not arbitrary
flying/swimming navigation. Narrow routes can still be missed at coarse cell sizes.
Upright-box character geometry (rather than a capsule), conservative sphere-obstacle
envelopes and grid resolution can reject routes a more advanced controller could accept.
Sphere colliders are obstacles, not accepted supporting surfaces in this milestone.

Future jump links must validate takeoff, the complete swept trajectory, and
landing against the motor's speed/gravity. Interior/portal metadata can later
provide hierarchical links; an interior's bounding box is not a walkable floor.

## Verification

```sh
cargo test -p io-traversal -p io-locomotion -p io-playground
cargo test navigation_
make test
make PROFILE=release gpu-test
```

Tests cover numeric body clearance, crouch-only routes, shared cache reuse with
separate personal knowledge, bounded planning, scenery invalidation, unsupported
levels, missing floors, strict config/asset bindings, and actual off-camera
dungeon traversal through both portals. Motor integration tests also traverse tilted
ramps in both directions, terrain crests, and a ramp-connected upper deck at
30/60/144Hz; disconnected stacked layers are rejected. The pre-existing ignored overhead-camera
regression remains unrelated and unresolved.
