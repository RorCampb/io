# Spaces and Entrances

`io-world` owns `Interior`, `Portal`, `SpaceLocation` and world membership.
No new character positions, worker loops or rendering objects are introduced.
`io-scene` provides shared serialized definitions and resolves names and scene-local
coordinates into validated world geometry. `io-editor` edits those same contracts.
Camera code consumes world membership; it does not decide whether a character
has entered a room.

## Scene Contract

The optional top-level `portals` array describes vertical rectangular openings:

```json
{
  "name": "Courtyard Entrance",
  "from": {"type": "exterior"},
  "to": {"type": "interior", "name": "Low Passage"},
  "center": [50, 49.8, 0.75],
  "normal": [0, -1, 0],
  "width": 3,
  "height": 1.5
}
```

The horizontal unit normal points from `from` toward `to`; crossings work both
ways. Center is scene-local and receives the scene origin once. Width runs across
the opening; height runs along world Z. Endpoints must differ, referenced interiors
must exist, and the whole opening must lie on their boundaries with the correct
orientation. World topology changes are validated before application.

Interiors remain AABBs with optional ceiling metadata. Names resolve to typed IDs
within a layout; replacing a layout reinitializes membership. Ambiguous spawn
containment selects the smallest containing interior, then scene order. Ceiling
planes are excluded from spawn containment so a roof is not the room below.

## Movement and Views

Accepted `World::set_pose[_3d]` movement and physics transform updates process
portal crossings in segment order. A fast straight move can cross multiple rooms.
Animation alone and camera movement do not change membership. Spawn, explicit
`World::teleport_pose`, and layout replacement classify containment instead.
Snapshots carry portals and membership alongside the existing item state;
game plugins access them through `WorldView`. Membership is retained off camera.

Only the crossing point of the item's anchor is tested against the opening.
Existing character/body collision remains responsible for whether the body fits.
The membership calculation uses accepted endpoint segments, not reconstructed
curved paths through all collision substeps. Portals do not stop movement, cut
walls, open doors, navigate stairs, or implement occlusion culling.

For portal scenes, exterior camera anticipation requires being on the correct side
and within the opening's width/height and approach distance. It does not grant
membership or allow Tab anchoring before crossing. Room-anchored camera state is
local to each camera and returns to Follow on a room change. Scenes without portals
retain legacy camera proximity behavior and containment-based membership.

Closed-door availability, runtime portal enable/disable, horizontal hatches,
sloping volumes, and automatic navigation/pathfinding are outside this milestone.
The camera still uses authored volume geometry, not individual prop collisions.
