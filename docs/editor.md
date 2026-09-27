# Native World Editor: Camera Rigs

```sh
make release
./build/release/io --editor --scene assets/dungeon/entry.json
```

This is an editor mode of the same native SDL/OpenGL application. No browser,
second renderer or IPC process is involved. Minimum window size is 1100x700.

The dungeon currently enables `camera.orbit`: free player-controlled camera with
collision. Play preserves that setting, and Save does not remove it. Room-rig
previews and camera-path guides below show the retained alternative rig system,
not the active free-orbit camera. Remove `camera.orbit` in JSON to try those rigs.
Two-finger trackpad movement orbits and pinch zooms; existing mouse controls remain.

## Workflow

1. Select Exterior, Low Passage or Entry Chamber in the left panel.
2. Frame Volume shows the activation bounds with yellow, through-wall guides.
3. Click a numeric value to type; Enter applies and Escape cancels. The minus/plus
   buttons apply small increments. Invalid edits leave the document unchanged.
4. Preview Rig looks from that rig toward the region's center and authored target
   height. It does not teleport the player. Inspector changes update this preview.
5. Left-drag orbits, right/middle-drag pans, and scrolling zooms in edit mode.
   Use Current View stores the angles and zoom, not the panned target position.
   Capture refuses zoom outside the selected rig's interval; adjust limits first.
6. Play or F5 starts traversal from the original spawn with the current draft.
   WASD moves, Space jumps, Shift crouches, arrows/right-drag orbit, scroll zooms.
   Stop/F5 restores the original simulation while retaining authored edits.
7. Save or Cmd/Ctrl-S explicitly writes the scene. Undo/Redo buttons or Cmd/Ctrl-Z
   and Cmd/Ctrl-Shift-Z traverse up to 64 edit operations.

Duplicate Volume creates an independently named copy of a selected interior and
its rig. Move/resize the copy in the inspector. Volume coordinates are scene-local
(the scene origin is added for rendering and runtime activation).

## Camera Path Guides

Select an interior and click **Show Camera Path**. The editor frames the preview:
yellow is the activation volume, cyan is a sample player route, and orange points
and lines show the camera trajectory. Five wire view-cone markers show direction
and FOV. Orbit/zoom the editor to inspect them; Hide Camera Path returns to bounds.

The sampler runs the same rig controller used in gameplay, along a straight route
through the volume's center and entry direction, including the approach on both
sides. It uses traversal crouch speed for a low volume, walk speed otherwise, or
2 m/s for a scene without traversal. It assumes level ground and no manual camera
input. It does not solve navigation or collisions, and these are not editable rail
control points. Nearby rigs participate, so the selected passage can show the
following room transition too. Samples are cached until relevant settings change.

Orthographic views have no finite camera eye. Their guide markers, and perspective
eyes farther than 80 m, use a capped 80 m proxy so the overlay remains finite.
The cones illustrate the configured perspective lens; they are not exact hybrid
frusta during the orthographic blend. Gameplay projection is not capped this way.

## Camera Contract

`camera.rigs` contains an `exterior` profile and an `interiors` map keyed by the
existing interior name. Optional `motion` selects the behavior; omitted means
`{"type":"authored"}` for compatibility. Each profile requires:

| Field | Meaning |
| --- | --- |
| yaw_degrees, pitch_degrees | Authored orbit direction and elevation |
| zoom | Base framing scale; higher means closer |
| fov_degrees | Lens angle, 20..100 degrees; defaults to 45 for older rig files |
| target_height | Meters above the followed character's feet |
| yaw_limit_degrees | Allowed manual yaw offset on either side |
| min_zoom, max_zoom | Manual zoom interval, including the base zoom |
| response_seconds | Exponential transition time constant |
| approach | Distance outside the volume where its influence begins |

In authored mode, runtime blends toward the strongest influential region using its proximity curve,
then smooths angle, log-zoom, FOV, limits and target height. Region selection has
hysteresis to avoid bouncing at adjoining boundaries. Without an incumbent, equal
influence selects the later region in the scene.

Manual orbit and zoom take ownership of their respective channels and hold their
absolute framing within that space, rather than being pulled back every tick.
Entering a different active region or returning outside smoothly releases those
overrides to the new authored view. Input on that transition frame wins. A full
180-degree yaw allowance permits continuous complete rotations; smaller limits
remain available for deliberately restricted rigs. Pitch remains within 2..85.

Authored mode allows bounded zoom and rotation in every space, including while
crawling. Zooming far out restores the orthographic view. The exterior lens is 45
degrees, passage 50, and chamber 68. FOV uses the rig's response time; widening the
lens reveals more of the room without automatically dollying closer to cancel
the expansion. FOV has no visual effect at the fully orthographic endpoint and
gradually takes effect during the perspective blend. Picking, culling, LOD and
rendering use the same resulting projection.

There are no pillar sweeps or per-obstacle zoom adjustments in this mode.
`camera.rigs` cannot be combined with legacy `camera.interior` or `camera.orbit`.
Placement and framing are authored responsibilities: an unsuitable rig can still
see through or be occluded by a wall. Approach affects all sides of a region,
not just its doorway; this is not portal-aware path planning.

### Geometry Orbit Prototype

The dungeon now opts into this mode:

```json
"motion": {
  "type": "interior_envelope",
  "clearance": 0.12,
  "close_distance": 0.6,
  "max_retreat": 8.0,
  "max_rise": 3.0,
  "max_fov_degrees": 95.0
}
```

Distances are meters. In this mode Exterior owns default yaw/pitch/zoom and global
control limits. Manual intent survives region transitions. Interior yaw, pitch,
zoom and their limits are marked GLOBAL and disabled in the inspector; select
Exterior to edit them. Interior capture is disabled because its framing is
derived, not a stored pose. Target height, baseline FOV, response, approach and
volume bounds still affect that interior and its preview.

Scroll-out follows a smooth piecewise path: retreat for the first 60% of log-zoom,
rise through 85%, then widen FOV. Scroll-in reverses it. Retreat comes from
ray/plane intersections at the chosen yaw, not a fixed room camera angle.
Clearance includes a near-plane allowance, and rays through the connected volume
union limit poses near an adjoining ceiling or wall. Available space expands
smoothly; newly tight limits clamp immediately. This does not query individual
mesh obstacles. See [dungeon scope](dungeon.md#interior-camera-scope) for limitations.

Preview and trajectory guides use the same solver as Play. Inside, zoom-out stays
bounded perspective; leaving restores the requested hybrid exterior view.
The five motion parameters above currently require JSON edits. Authored mode is
still available for direct per-room pose design.

## Ownership and Persistence

- `io-world`: runtime interiors, portals and spatial membership in world snapshots.
- `io-scene`: serializable camera, interior and portal contracts, resolved against
  the world's geometry validation. Both editor and runtime use these definitions.
- `io-editor`: graphics-independent draft, validation, history, and persistence.
- Root App/camera adapters: apply editor commands and evaluate runtime rigs.
- `csrc/editor_ui.c`: native controls/input focus; existing HUD shaders draw them.

Editing pauses simulation. Playtest uses the inline simulator so stop/reset are
deterministic; ordinary game launches retain their simulation worker. Editing is
disabled during Play, except Save and Stop. UI events are consumed before game
input. The C interface transfers a bounded inspector snapshot and command tags;
document/history logic stays in safe Rust.

Saving patches only `camera.rigs`, `interiors` and `portals`, removing incompatible legacy
camera guidance. Assets, items, physics, plugins and other camera fields are
preserved. It compares the source bytes to detect external edits and refuses a
conflicting save. A same-directory temporary file is flushed and atomically
renamed; quitting with unsaved changes prompts Save/Discard/Cancel.

This milestone edits **camera activation volumes, not physical walls or roofs**.
Changing bounds does not move meshes or colliders. Full item/component inspectors,
asset import, drag gizmos, region renaming/deletion, and gameplay/physics panels
are not implemented yet. Existing interiors can be duplicated; new blank scenes
currently need their first interior in scene JSON.

## Entrance Editing

Entries prefixed **P:** are portals. Select one to inspect its rectangular opening
and direction line. Edit center, horizontal unit normal, width, height, and the
connected space indices (0 = Exterior, 1.. = interiors in list order). The JSON
stores names, not these UI indices. Invalid connections, off-boundary openings
and out-of-room dimensions are rejected without changing the draft.

Select an interior and use **Add Entrance** to seed an opening on its configured
entry face. The adjacent space is detected from the authored volumes. This does
not carve a mesh or remove a collider. Edit/commit, undo/redo, save/reopen and Play
all use the same portal contract. Moving a room boundary cannot silently leave an
invalid attached portal; such a single-field edit is rejected. Coordinated layout
changes and doorway relocation across faces can be authored as a validated JSON
edit; multi-object drag editing and portal deletion/renaming are not yet provided.

Play initializes world membership from the draft layout. Trajectory guides also
advance through the draft's portals rather than selecting rooms by camera proximity.
Native portal editor capture: `build/camera-editor-portal.ppm`.

## Verification

`make test` covers rig validation, rejected edits, grouped history, capture,
duplication, save/reload, external-edit protection, and runtime transitions.
`make PROFILE=release gpu-test` includes a native editor suite exercising clicks,
numeric keyboard focus, undo/redo, paused gameplay rejection, simulation reset,
volume overlays and rig preview. Captures are written to
`build/camera-editor-{volume,rig,trajectory}.ppm`. The native dungeon suite checks
full passage rotation and bounded perspective zoom-out while crouched. Rust
regressions cover holding manual framing, releasing it in a new region, lens
expansion without dollying, time-based FOV interpolation, and finite guide samples.
