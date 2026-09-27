# Dungeon Entry Prototype

```sh
make release
./build/release/io --scene assets/dungeon/entry.json
```

The entrance is ahead of the player between the pillars. This is a traversal
prototype, separate from the encounter and village demos, not a combat level.

- WASD: camera-relative jogging at 3.4 m/s, including while orbiting.
- Space: jump when standing on a supporting surface. No midair repeat jump.
- Hold either Shift: crouch and move slowly; release to stand when space permits.
- Two-finger trackpad movement: orbit horizontally and vertically. Pinch: zoom.
  Lift and replace fingers to switch between orbit and pinch; finger-spacing
  drift during orbit does not change zoom.
- Arrow keys or right drag also orbit. Mouse wheel or +/- also zoom.
- Tab room anchoring is disabled for this free-orbit scene.
- Escape: exit.

Walk toward the lintel: standing is blocked. Hold Shift to enter the low passage.
Releasing Shift there retains the crouched collider until the chamber has enough
headroom. The courtyard plinth is a jump/landing test. You choose the camera angle
for entering the passage: lower its elevation and aim along the opening.

## Contracts

`io-traversal` is an `io-game` plugin. Its typed movement, crouch and jump commands
cross the existing worker envelope. It owns locomotion intent and animation-state
selection, not meshes or OpenGL. The world retains the character's authoritative
transform, character body dimensions and animation component, including off camera.

The shared `io-world` character sweep uses conservative upright bounds, spatial
queries and subdivided movement against collider bounds. It supports sliding,
vertical landing/head contact and checked stance changes. This is a kinematic
controller, not a rigid-body character: it does not push dynamic bodies, resolve
exact capsule/triangle contacts, step automatically or provide foot IK.

`assets/dungeon/entry.json` supplies the asset package, seven clip bindings,
movement/jump/gravity values, body dimensions and blend duration. Existing
Mixamo files are reused, not duplicated. `tools/build_dungeon.py` regenerates the
scene using the normal item/collider contract; it is not engine code.

Animation state distinguishes idle, walking, crouched idle/walk, jump, fall and
landing. Actual displacement and support select states. Short local-TRS fades
hold the outgoing pose while the incoming clip advances; further switches wait
for that fade to finish. Planar root travel is removed; airborne clips can also
remove vertical root travel so they do not fight the controller. Crouch retains
the authored lowered pose. The sideways sneak clip is a placeholder for a proper
forward crouch and resting crouch pose. Landing currently has a short fixed
recovery window, not a complete authored animation state graph.

## Free-Orbit Camera

World-owned `interiors` declare inhabitable bounds, optional ceiling height and
inward entry direction separately from visible meshes. They are immutable in
published snapshots. Physical walls/roofs still require authored colliders;
metadata alone does not create geometry or collision.

The dungeon enables `camera.orbit`, with no avoidance and zero lookahead. You
control yaw, pitch and requested distance (0.4 to 35 meters). The lens stays at
45 degrees. There are no automatic room angles, FOV changes or ceiling-following
paths. Pitch can cross horizontal; floor collision prevents moving below ground.
The optional `target_height_fraction` is 0.7 here: the upper abdomen, 1.33 m
standing and 0.805 m crouched. It tracks body dimensions without animation bob.

The desired eye is `target + orbit_direction * requested_distance`. A conservative
sphere sweep along that line stops the eye before the nearest physical collider
or terrain. It excludes the followed character. Walls, roofs and pillars are all
ordinary blockers. The collision radius also covers the near-plane corners.
Manual orbit also sweeps the eye along small arc segments. Wall contacts redirect
the unused motion along the actual surface at the same travel speed, subject to
corners, line of sight and the maximum requested distance. The resulting path
is not a circle, so actual eye distance can change without changing requested
zoom. Floors and ceilings retain the old stop behavior; overhead handling is
deferred. Following a moving character
can still retract below the requested minimum distance; it never overwrites
your requested zoom. Clearing the obstruction eases back to the requested distance.
The target follows the character and lowers with its crouched body, not room entry.
Normal locomotion uses `running_in_place` at 0.85 playback speed; crouch/jump
remain separate bindings in the existing traversal contract.

This is radial collision, not navigation or a camera path planner. Box corners are
conservatively expanded. Decorative meshes need colliders to block the camera.
An already-overlapping target has no guaranteed clear camera position; there is
no automatic first-person or roof-cutaway fallback. Very tight angles can place
the camera close to, or inside, the character mesh.

The [native editor](editor.md) can still author interior/portal metadata:

```sh
./build/release/io --editor --scene assets/dungeon/entry.json
```

`camera.orbit` takes runtime precedence over retained `camera.rigs`. The old room
rigs and trajectory guides remain available as inactive authoring previews for
comparison; they do not control Play. Saving preserves this choice. Distance,
clearance and lens settings are currently edited in JSON.

See [space and portal contracts](portals.md) for ownership, authoring and limitations.

Remove `camera.orbit` to return to the retained room-rig experiment, including
Tab anchoring. See [collision-bounded orbit](camera.md#collision-bounded-orbit).

## Verification

`make test` covers stand/crouch clearance, jump/landing, prevention of air jumps,
off-camera simulation, animation root policies/crossfades, and worker input.
`make PROFILE=release gpu-test` includes a scripted native dungeon traversal:
standing blocks at the entrance, crouching passes, premature release stays
crouched, and the character stands in the chamber. It verifies rendered character
pixels and writes `build/camera-dungeon-{passage,chamber}.ppm`.
The native test also renders 72 orbit/zoom frames inside the passage and checks
full rotation and collision-limited perspective zoom while crouched, saving
`build/camera-dungeon-passage-wide.ppm`. Rust tests cover all six collision axes,
fixed FOV/pitch through room entry, blocked zoom intent, exterior recovery,
viewport resizing, horizontal views, and shared projection/picking. Native tests
also verify room-independent follow; capture: `build/camera-dungeon-free-orbit.ppm`.
C input tests cover coalesced two-finger orbit/pinch, extra fingers, focus loss,
and mouse-wheel fallback. Physical MacBook gestures require a hands-on check.
