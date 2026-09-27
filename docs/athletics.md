# Athletics Course

```sh
make release
./build/release/io --scene assets/dungeon/athletics.json
```

The blue NPC alternates between two destination goals. It discovers a route over
actual stair colliders, across a gap and around tall obstacles, then returns.
There is no authored list of jump/step coordinates or invisible staircase ramp.
Use the normal two-finger/right-drag orbit and zoom controls. The separate player
starts on the viewing platform; the initial camera shows the course.

The scene opts into the supplied `io-locomotion` plugin's athletics provider:

```json
"athletics": { "step_height": 0.3, "jump_distance": 3, "max_drop": 0.6 }
```

These are limits, not promises that every destination inside that distance is
reachable. Body clearance, speed, jump velocity, gravity, takeoff and landing
support must also permit a connection. Lowering jump_distance to zero makes this
course unreachable. NPCs in this mode must explicitly set can_crouch=false.
Pursuit and per-agent athletics overrides are demonstrated by the
[two-guard test](guards.md).

## Execution

Both existing surface navigation and athletics call the same plugin `Motor`.
The athletics executor only starts actions and reports motor progress. Shared
read-only path calculations validate and sample:

- Walking with the world's supported movement query.
- Steps with a swept up/across/down envelope and supporting-surface probes.
- Jumps with a constant-gravity arc, swept body clearance and supported landing.

The motor rechecks physical clearance during movement. Cancellation clears the
planned action, not gravity: an airborne actor can still land. Animation is
handled by the same separate AnimationDriver used for player locomotion.
This is kinematic upright-body locomotion, not rigid-body impulses or foot IK.

## Limits

This provider samples a bounded horizontal lattice and up to two support
candidates per sampled column. Jump candidates use cardinal directions and a
bounded range. This is not unrestricted climbing, arbitrary mesh navigation or
crowd avoidance. It currently recomputes connections rather than caching them;
the legacy shared HUD cache counter may consequently remain zero.

```sh
cargo test -p io-locomotion
cargo test --release athletics_npc
cargo test -p io-traversal --test item_plugin
```

The complete course test checks stairs, airborne gap crossing, obstacle detours,
both travel directions and absence of body overlap. Motor tests compare direct
input and planned jumps at 30/60/144 Hz, plus cancellation and invalid landings/
overhead clearance. The core-only lift test requires no locomotion plugin.
