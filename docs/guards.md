# Two-guard traversal test

```sh
make PROFILE=release
./build/release/io --scene assets/dungeon/guards.json
```

WASD moves the player, Space jumps, Shift crouches. Two-finger drag or right drag
orbits; pinch/wheel zooms. The camera starts as an overview, not a following camera.
F2 hides the existing attention tuning panel (it edits the first observer only).

- Cyan jumper can cross the central gap. Orange walker has zero jump distance and
  must use the northern bridge. Both climb and descend real stair colliders.
- Each guard independently observes the player and patrols, chases, searches around
  its last sighting, or returns to patrol. Use the eastern walls to break sight.
- The orange bridge shutter alternates every 12 simulated seconds. It defers while
  an NPC is airborne or a character occupies its conservative destination bounds.
  It switches poses in one physics tick: this is a topology-change test fixture,
  not a polished door animation or crushing mechanic.
- Cyan/orange route lines and small waypoint marks show remaining planned connections.
  A jump connection is drawn as a straight link, not its ballistic trajectory.
- HUD shows each observer's behavior, navigation status, motor state, objective,
  ticket revision and remaining route count. Existing target-attached meters show
  each observer's attention/focus. The two observers do not share sightings.

Try walking toward the west to meet the patrols, then jump back over the gap. Hide
behind an eastern wall, reappear during a search, and wait out a search completely.
The opening/closing bridge also tests route invalidation without any player action.

## Ownership

`io-playground` composes existing perception, pursuit, traversal, and locomotion.
Its optional `barrier_cycle` is developer/example behavior using the existing
`WorldCommand::SetKinematicTarget`; no engine door type or guard controller was added.
`navigation.athletics` supplies defaults and each agent's `athletics` overrides its
capabilities. Both guards use the same provider, search and motor implementation.

The athletics provider now supports approach destinations, and search no longer
flattens remembered positions to the domain floor. Grounded objective replacement
lets an in-progress jump land before a behavior retargets it. Live collision safety
still applies; geometry invalidation can interrupt execution.

Core additions are read-only remaining-route inspection and public Item spatial
bounds. Diagnostics do not own or mutate another copy of route progress.

This remains map-aware routing, not perception-restricted map knowledge. Athletics
has no route cache, crowds/priority negotiation, crouch links, foot IK, or arbitrary
mesh navigation. Search offsets preserve remembered height but are not a general
surface-aware exploration planner. Close-range actors can block one another.

## Verification and authoring

```sh
cargo test --release guards_
python3 tools/build_guard_demo.py
```

The integration tests cover independent jump capabilities, off-screen
last-seen/reacquisition behavior, blocked-route recovery and occupied-barrier safety.
An additional background-worker course test checks both capability routes, stairs,
the jump and live collision safety. Accelerated tests explicitly select inline mode.
The generator composes existing assets; it does not author a route or action list.

## Performance diagnostic

```sh
cargo test --release guards_stair_cpu_probe -- --ignored --nocapture
```

This manual CPU-only probe separates standing idle, pushing against a riser with
NPCs removed, and the same input with NPCs active. It is not a GPU FPS assertion.
Initial M3 Pro measurements: player-only riser contact ~0.03ms/tick, but NPC-enabled
ticks spike to ~60ms. The uncached athletics provider budgets graph expansions,
not the collision/support work inside each expansion. This scene exposes a real
planning latency issue in the inline baseline. The windowed scene now selects
`navigation.planning: "background"`. Search runs outside the simulation tick;
execution and live collision checks remain authoritative on the simulation worker.
See [Navigation benchmarks](navigation-benchmarks.md) for the before/after protocol.
