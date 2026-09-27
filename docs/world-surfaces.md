# Bodies, Terrain and Supporting Surfaces

## Current Names

- `Item.character_body: Option<CharacterBody>` describes an upright character's
  radius, height and terrain slope limit. It exists while supported or airborne.
- `CharacterBody::bounds_at` supplies the conservative body bounds used by
  character collision and spatial indexing. It does not mark any object walkable.
- `CharacterSweep.grounded` and motor feedback `grounded` are runtime booleans:
  the character is currently supported rather than airborne.
- `HeightField` represents terrain as one height per horizontal coordinate. It
  is useful for hills, but cannot alone represent a bridge above a tunnel.
- A supporting surface is the physical surface beneath a character. It may come
  from terrain or an Item's collision geometry; no `ground` tag is required.

The old `ground.rs` mixed these responsibilities. Body data now lives in
`components.rs`, height-field terrain and its movement helper in `terrain.rs`,
visibility in `visibility.rs`, and character collision/motor queries in
`character.rs`. The public world exports remain the entry points.

## Scene Migration

New scenes and generators use:

```json
"character_body": {"radius": 0.35, "height": 1.9, "max_slope": 0.8}
```

`max_slope` is rise/run, not degrees. The loader accepts the old `grounded` key
as an alias, but rejects a scene containing both keys for an Item. Rust callers
must migrate `Grounded` to `CharacterBody` and `item.grounded` to
`item.character_body`. The terrain-specific helper `ground_destination` is now
`terrain_destination`. Runtime `grounded()` and feedback booleans are unchanged.

The rename itself was not a 3D navigation upgrade. `terrain_destination` follows the
optional height field (or retains the actor's height if absent). The navigator now
discovers multiple support layers and sloped walking/crouching routes separately.
`max_slope` is now enforced by shared character support/walking queries and the
io_locomotion::Motor as well as the legacy terrain-following helper.

## Shared Surface Queries

`io-world::surface_candidates(&dyn WorldView, SurfaceQuery)` now reports physical
surfaces at a horizontal coordinate within an explicit vertical interval. Each
`SurfaceHit` contains a position, an upward outward unit normal, and a typed source:
`SurfaceSource::Terrain` or `SurfaceSource::Collider(item_id)`.

```rust
use io_world::{surface_candidates, SurfaceQuery};

let query = SurfaceQuery::new(x, y, minimum_z, maximum_z)?.excluding(actor_id);
let candidates = surface_candidates(world, query);
```

The constructor rejects nonfinite coordinates, coordinates outside +/-1,000,000,
and vertical spans outside `(0, 1000]` world units. Query fields are private so invalid
queries cannot be constructed directly. Bounds are inclusive. Results are sorted
highest first, with source identity breaking equal-height ties. A bridge and the
terrain below it are separate candidates, not one overwritten height. Queries
starting inside a solid do not invent a top surface at their starting position.

Box colliders retain their orientation and offsets; spheres retain their curved
normals. Physical sizes do not inherit visual scale. `HeightField::sample` returns
the exact triangle height/normal used by the existing terrain triangulation;
`height` uses the same interpolation. Render meshes, occupancy bounds, character
bodies and interior volumes do not implicitly become collision surfaces. Results
are live geometry facts, not an NPC's perceived knowledge or permission to walk.

This is deliberately a **point surface query**, not a traversability query. It
does not prove body/head clearance, footprint support, slope eligibility, step or
jump reach, or connectivity. In particular, a surface below a low ceiling still
appears in the results. The character-aware layer below performs additional checks
before accepting a placement. Candidates alone remain insufficient to move an actor.

The internal `geometry` module supplies shared box/sphere intersections for these
queries, camera sphere casts, visibility and continuous character-box clearance.
Boundary policy is explicit: sight/camera casts count touching, while character
clearance allows touching and tests penetration. World storage/results remain
`f32`; intersection intermediates retain higher precision to avoid shifting a
surface when reconstructing it from a long cast. No crate or dependency was added.

Queries use the existing spatial index and work identically against immutable
snapshots. The source identifies geometry in that WorldView, not an eternal contact
or route-validity token. Consumers caching results must re-query after relevant
geometry changes (currently `navigation_revision` is the coarse invalidation signal).

## Character-Aware Support and Movement

The following are public `io-world` functions, shared by the existing planner and
`io_locomotion::Motor`, not a separate crate or a new game mode:

- `character_support(world, excluded, reference, body, probe)` finds a usable feet
  placement plus its `SurfaceHit`. `SupportProbe::CONTACT` checks within 3mm of the
  current anchor. `SupportProbe::new(above, below)` allows bounded placement searches
  up to 4 units each way; this is not permission to teleport or traverse that distance.
- `character_segment_clear` checks the translated upright body against oriented
  box colliders, characters, and terrain triangles, including initial/end clearance.
  This query does not require support and can therefore check jumps or falls.
- `walk_character` follows continuous slopes with bounded support placements and
  continuous collision checks between them. It returns a position, optional support
  and `reached`; it does not mutate the world. Blocking leaves the last safe placement.
  Failed connectors across slope creases are adaptively subdivided (at most 64 probes
  and 16 levels per base step); exhausting that budget stops safely. This prevents
  a straight chord through a hill crest from either tunnelling or blocking ordinary travel.
- `character_supported_segment` uses that same walking calculation, then verifies
  the requested 3D endpoint was reached. The planner uses it rather than inferring
  support from a failed downward clearance check.

CharacterBody remains the source of radius, height and maximum slope. Support
requires slope eligibility and head/body clearance, not a `ground` tag. The feet
center must lie over a candidate surface. For tilted boxes a body sweep determines
the feet-anchor height, so the uphill body corner does not penetrate the ramp.
For terrain, clearance uses the highest triangle point over the whole square
footprint, including peaks inside it, rather than only its center/corners. At finite
terrain boundaries, collider-supported travel remains possible outside terrain coverage.

The motor tries supported walking while grounded and descending/stationary. Jump
impulses leave support; gravity, collision sliding and landing still run through the
motor. Players can walk off ledges and fall; navigation rejects unsupported edges.
Walls, ceilings, and other actors cannot manufacture jump support. A small contact
tolerance handles floating-point precision, not a general stair-climbing ability.

The shared geometry module handles box translation with separating-axis overlap
intervals, with a fast path for axis-aligned boxes. Terrain triangles use the same
interval solver. The rigid-body solver reuses the oriented-box geometry primitive;
its contact generation/solver behavior is unchanged. Rendering and collider sizes
remain independent. Sphere obstacles retain conservative boxes and are not accepted
as supporting surfaces yet. Character bodies are upright boxes, not capsules.

No automatic stairs, ledge climbing, moving-platform attachment, arbitrary collision
meshes, or free-flight path search is implied. The old `terrain_destination` helper retains
its existing village behavior; it is not a substitute for the new motor queries.

Tests include actual motor travel, jumping and landing at 30/60/144Hz:

```sh
cargo test -p io-world support_tests
cargo test -p io-locomotion --test support_motion
```

## Dynamic Navigation Roadmap

Surface enumeration, character-aware movement and layered walking search are
implemented. See [Navigation](navigation.md) for budgets, cache identity and tests.
No new crate or geometry database was introduced: the existing world queries and
motor support calculation supply each connection's height and surface identity.
Remaining route/action and observation contracts are listed below.

1. **Character-aware support and movement: implemented for the current primitives.**
   Shared support, body clearance, slope following, and edge validation now back
   both motor execution and planner checks. Additional abilities need explicit
   traversal contracts rather than bypassing these safety checks.
2. **Planner-independent route execution: implemented.** `Movement<D>` binds
   Items to a typed `TraversalExecutor`; routes carry world positions and the
   provider's action type. Progress, cancellation, explicit completion, failure,
   checked tickets and direct feedback do not require a humanoid controller.
   The current grid is an optional provider, not the definition of space.
3. **Layered surface planning and typed plugin providers: implemented.**
   Built-in walking now derives connectivity from support/clearance queries rather
   than `origin.z`, keeping stacked layers distinct and following slopes. Plugin
   connections supply eligibility, costs and typed actions for their executor. The
   engine has no door/rock/tree enum; developers own that meaning and their typed
   state associated with Item IDs. Existing static plugin composition is sufficient
   to prove this; runtime library loading is not required.
   See [Movement Plugins](movement-plugins.md) for the non-humanoid physics test.
   The optional locomotion plugin now supplies a bounded stair/jump course; see
   [Athletics](athletics.md). Arbitrary interactive-object actions remain developer-owned.
4. **Observation and understanding updates.** Plugins define observable changes
   and character knowledge. Routes track the assumptions they depend on. Observed
   changes can invalidate those assumptions; unobserved changes do not grant
   omniscience, but live execution safety still prevents invalid movement.
   Essential updates must not depend on the lossy presentation event history.

Acceptance scenes should include a ramp, a bridge with a route below it, and two
stacked rooms. Then add one entirely developer-defined interactive object: one
character can operate it, another must detour. Changing it during travel must
produce observation-driven replanning or a checked execution failure. No new
object-specific engine fields, enums or match branches should be required.
