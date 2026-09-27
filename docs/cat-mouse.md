# Cat and Mouse

```sh
make release
./build/release/io --scene assets/dungeon/cat-mouse.json
```

WASD moves the hero; Shift crouches; Space jumps. Orbit and zoom use the usual
trackpad/right-drag controls. The arena has a central wall, an L-shaped corner,
low cover, stacks, and perimeter walls. Tall cover breaks sight; low cover can
hide more of your body when crouching. Character collision is still authoritative.

The runner is slightly slower than your normal jog. His status appears in the
HUD, along with a tag count. The gold meter above you is his attention; cyan is
instantaneous directional focus, not your awareness of him.

## Live Vision Controls

The bottom-left panel edits the first observation track (runner watching hero).
Drag **FOV** from 5 to 360 degrees of total horizontal width. Sensitivity falls
smoothly from direct front to zero at half that angle on each side; it is not
uniform inside the cone. This test starts at 240 degrees (edges at +/-120), so
sideways targets contribute evidence instead of falling outside the old +/-60
boundary. A 360-degree setting still tapers to zero directly behind the observer.

Drag **Notice Threshold** from 1 to 100 percent to change the attention required
to enter/re-enter pursuit. The separate `minimum_focus` gate remains 3 percent.
Changing a threshold does not cancel an existing chase or erase attention.
If sustained focus stays below the threshold, waiting alone will not trigger
pursuit: attention approaches focus rather than accumulating without limit.

**F2** hides/shows the panel; **Reset to Scene Settings** restores its initial
values. Applied values come back from the simulation worker; PENDING indicates
an edit awaiting application. Changes are runtime-only, not saved to JSON.
For persistent values edit `observations[].vision.fov_degrees` and
`navigation.agents[].pursuit.settings.notice_attention` in the scene.
The passive attention scene also exposes FOV, with the pursuit slider disabled.
The panel requires the HUD and is not shown in editor, benchmark or capture mode.

These are `io-playground::Input::TuneAttention` commands with validated actor-pair
settings, transported through the existing worker queue. C only renders widgets;
it does not own perception or pursuit state. Steering/pathfinding are unchanged.

## Behavior

- Patrol: follows authored destinations until both attention and focus exceed
  configured thresholds. Glimpses need not trigger pursuit.
- Chase: updates a remembered position only from an actual visual contact.
  Requests approach destinations near that position instead of trying to occupy
  the same space as your body. Up to 16 candidates surround the remembered target;
  endpoints must have support, body clearance and a clear final grid connector.
  Failed routes advance to another candidate rather than keeping an impossible
  stopping point inside cover. A chosen point stays stable while moving around
  obstacles; target movement or scenery changes refresh the candidates. Exhausted
  candidates can be retried at the configured repath interval to recover from
  transient blockers. This is a bounded search, not a guarantee that every target
  has a reachable approach. New routes are rate-limited, and in-progress path
  searches are allowed to finish rather than being restarted every tick.
- Search: after losing sight for the configured grace period, visits the last
  seen position and nearby offsets. Reacquires you if sufficient sight returns;
  otherwise times out and resumes patrol. This is a simple local sweep, not
  strategic reasoning about where you probably hid.
- Tagged: requires current visual contact and proximity, increments the counter,
  and pauses for three seconds so you can escape. No damage, respawn, or forced
  player movement is involved.

## Contracts

This policy lives in `io-playground/src/pursuit.rs`, the example game plugin, not
in the renderer, physics solver, perception service, or navigator. The policy
receives a typed sighting (position, focus, attention), its own pose and navigation
status. It does not receive a WorldView or a hidden target pose. Perception alone
creates visible contacts; the navigator still uses scenery and live colliders
for safe routes, independently of visual knowledge. Actors continue off-camera.

`io-playground` implements the existing `io-game::GamePlugin`. It submits typed
navigation requests to `io-traversal::Movement` and consumes authoritative feedback
directly. `io-traversal` contains no chase, search, tagging, patrol, or visual
observation policy. The host's existing `traversal` JSON section is retained for
compatibility, but its game definition is now owned by `io-playground`.
See [Movement Plugins](movement-plugins.md) for the developer contract.

Enable it with optional `pursuit` on a traversal navigation agent:

```json
"pursuit": {
  "target": "hero",
  "settings": {
    "notice_attention": 0.18,
    "minimum_focus": 0.03,
    "lost_sight_seconds": 0.75,
    "search_seconds": 12,
    "search_radius": 3,
    "repath_seconds": 0.5,
    "tag_distance": 1.25,
    "grace_seconds": 3
  }
}
```

The scene must also supply an observation track from that agent to its target.
Missing tracks, unknown actors and invalid settings fail validation. Tag distance
must exceed the sum of character radii plus clearance. Omitting `pursuit` keeps
existing stop/patrol behavior. The patrol route and movement speed remain ordinary
navigation/locomotion settings. Phase transitions produce typed plugin events.

This uses the existing level-floor navigator; there is no jump planning,
hearing, door operation, shared alert system, or general action planner. Search
points can be obstructed; failed points are skipped within a finite search window.
Destinations are bounded to the authored navigation domain. The original attention
demo remains a passive perception exercise.

Regenerate with `python3 tools/build_cat_mouse_demo.py` after generating the
attention scene. Test with `make test` and `make PROFILE=release gpu-test`.
