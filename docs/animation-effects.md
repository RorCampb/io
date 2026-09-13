# Animation Events and Effects

Items retain state. Trait implementations change that state. Animation players
hold optional event bindings; neither C nor the asset importer knows what damage
means. Health now belongs to an optional `Durability` component. The flat scene
adapter retains its existing 100-health default; a runtime Item has no durability
unless explicitly assembled with it. See [Item Components](item-components.md).

## Configuration

An item's animation can bind named, clip-relative events to named scene items:

```json
"animation": {
  "clip": "Run",
  "speed": 1,
  "events": [
    {
      "name": "step",
      "at": 0.5,
      "target": "practice-target",
      "effect": {"type": "damage", "amount": 10}
    }
  ]
}
```

This fragment comes from `assets/street-kit/effects-demo.json`, a small standalone
fixture using the existing runner. It deliberately uses a Run event to prove the
connection; it is not an authored sword attack. The target starts with `health: 40`
and loses 10 health at 0.5 seconds into each loop. The integration test checks
40 -> 30 -> 20 without a camera or renderer.

Run `make`, then `./build/io --scene assets/street-kit/effects-demo.json` to see
the fixture. Damage changes retained health only: there is no health UI, hit flash,
destruction, collision/range check, or automatic removal at zero health. Native
callers can inspect health using the existing `io_app_item_state` API.
The normal street demo is unchanged.

Event names must be nonempty and unique within a player's track. `at` is in
seconds relative to the clip start, strictly greater than zero and no greater
than the clip duration. An event at the duration fires at the end of each loop.
Events are configured in JSON, not imported from Blender markers. Target names
resolve to stable item IDs, including targets listed later in the file. Missing
targets, unknown effect types, misspelled fields, and out-of-range times fail loading.

## Execution

1. The fixed simulation tick advances each selected item's movement and animation clock.
2. Crossed event times in `(previous_time, time]` append effect requests to a queue.
3. Requests are ordered by time within the tick, source item ID, and event order.
4. After all item updates finish, each effect mutates the world through its public API.
5. Rendering samples poses separately and never fires gameplay events.

The event scan handles loop boundaries, multiple loops in a tick, and playback
speed. Paused players do not emit events. Duplicate simulation selections do not
emit duplicates. Requests at the same timestamp execute sequentially in the
documented order; this is not a simultaneous combat-resolution system.

An inactive target can receive damage from an active source. An inactive source
still pauses with the current camera-based simulation policy; this feature does
not add background simulation. Damage saturates at zero. Missing runtime targets,
targets without durability, and damage that changes nothing are no-ops. Successful
damage advances the world revision. At zero, an optional depletion response may
change visuals, animation, or motion; only a changed spatial envelope updates the
index. Without a response, movement and animation continue unchanged.

## Extending Behavior

`crates/io-world/src/effect.rs` defines `Effect::apply(target, world)` and `Damage`.
`EffectKind` provides explicit dispatch for supported content, while
`src/config.rs` defines the JSON schema and `src/demo.rs` resolves configuration
into runtime effects. The trait does not itself make new Rust implementations
discoverable from JSON.

Reusing damage requires only configuration changes. A genuinely new effect needs
its implementation, an explicit configuration/dispatch mapping, and tests; it
does not require editing the renderer or giving every Item a new behavior method.
Effects needing additional state will also need the corresponding component.

`AnimationState` exposes read-only playback access plus checked `set_speed`,
`seek`, and `set_events` methods. Speed is finite in 0..64; event tracks attach only
to looping playback. One-shot death playback is supported and holds its final
pose; death configuration requires positive speed. These checks prevent mutation
from bypassing the player's invariants after creation.

Attack input, general switching or blending clips, dynamic target selection,
and collision validation are later layers. When adding animation
switching, replace the clip and its event track together. Gameplay-critical
timelines must continue even when visible bone-pose evaluation is skipped.
