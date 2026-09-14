# Game Plugin Contract

## Boundaries

`io-game` is a reusable framework, not this particular combat game's rules.
`io-encounter` is the first developer-style plugin. The root `io` application
chooses it in `src/demo.rs` and provides its input/HUD adapter. Neither
`io-game`, `io-world`, nor `io-assets` depends on `io-encounter`.

```text
Application: assets + Item assembly + plugin registration + input/HUD adapter
    |
    +-- io-encounter (example plugin: combat definitions and rules)
    |       |
    |       +-- io-game (Session, Round, typed lifecycle and world access)
    |               |
    +---------------+-- io-world (Items, components, physics, spatial state)
    +-- io-assets (Blender/glTF asset contracts)
    +-- C/OpenGL renderer (presentation)
```

An Item has one identity and one set of world-owned components. A model is its
appearance, not a gameplay class. Plugin bindings refer to the existing Item IDs.
They do not make a separate character world or copy health/physics state.

## Implementing a Plugin

Implement `io_game::GamePlugin` in a separate Rust crate. It defines:

- `Command`: your owned input enum, not another case in an engine enum.
- `Event`: your owned output enum for bounded presentation/history.
- `Error`: your typed rejections, including conversions from framework errors.
- `info`: a stable plugin ID and API contract version (currently 1).
- `validate`: check the world bindings and component requirements without mutation.
- `active_items`: IDs that must simulate even outside camera-selected regions.
- `command`: handle one typed request through checked world operations.
- `before_step`: optional physics targets/forces before world integration.
- `update`: inspect integrated world state and advance game rules after integration.

Construct the plugin with its own validated authoring data, then register it:

```rust,ignore
let plugin = MyGame::new(my_config, &world)?;
let mut game = io_game::Session::register(plugin, &world)?;
game.command(&mut world, MyCommand::Use { actor, target })?;
game.step(&mut world, &visible_simulation_indices, fixed_dt)?;
```

`Session` validates registration, the timestep and active references. It calls
`before_step`, advances the world **once**, then calls `update`. The application
owns scheduling and the fixed timestep; no plugin receives a second world
simulation entry point. The native worker uses the same host as inline mode.

`PluginWorld` implements read-only `WorldView`. It exposes existing typed
`WorldCommand` operations (damage, impulse, gravity scale, kinematic target,
visual state, axis resize), checked non-physical placement, dynamic-body
attachment, and typed event emission. It exposes neither `&mut Item` nor
`&mut World`. Placement rejects physics- or route-controlled Items; it is NOT
a collision-aware movement controller.

World operations validate individually and are visible immediately to the next
operation in the callback. Callbacks are not whole-world transactions: plugins
must validate an action before applying multiple effects. A failed callback does
not roll back earlier operations, and an error after integration does not undo
that simulation step. The host stops that call and returns the typed error.

These are trusted, statically linked Rust plugins, not a hostile-mod sandbox.
Plugins must keep snapshot state clone-independent (no shared mutable gameplay
state behind `Arc<Mutex<_>>`), use deterministic simulation inputs, and avoid
blocking I/O in callbacks. `Session` exposes the plugin read-only, not through
`DerefMut`. Cloned sessions retain independent encounter state and event history.

## Round and Continuous Modes

`Round<Resolution, Outcome>` owns:

- Exploration, an optional frozen Ready confirmation phase, and a configurable
  timed movement window. Ready exposes no movement token; `begin_movement`
  explicitly starts the countdown. The encounter plugin's `confirm_round_start`
  setting opts into confirmation before each round (default false).
- Checked transitions into an explicitly ordered turn roster.
- The active turn, eligibility-based skipping, and the next round.
- An opaque in-flight resolution payload, completion, and movement locking.
- A plugin-defined outcome and return to exploration without replacing the world.

Only the framework changes its private phase. Plugins supply the roster order,
eligibility predicate, resolution payload and outcome. There are no sword,
faction, damage or NPC branches in `Round`.

The encounter plugin supplies initiative ordering, living-character eligibility,
projectile resolution, and winning factions. It retains its existing opportunity
reaction and death-body rules. `Round` rejects duplicate/zero actor IDs, empty
orders at the turn boundary, invalid durations and out-of-phase transitions.
Round IDs do not restart when that Round returns to exploration. Application
input must still be stamped for the active mode/window and cleared on mode changes.

A continuous-world plugin can use `Session` without `Round`. The independent
`crates/io-game/tests/plugin_contract.rs` Workshop example exercises exactly
that: its own input/event enums act on a model-less physical Item, with no health,
factions or encounter dependency. An exploration game can own a Round and enter
or leave it using the same world. A richer named `FreeWorld` framework (quests,
streaming, mode routing) is not implemented by this refactor.

## Assets and Components

Artists deliver Blender/glTF models through the existing `io.asset-package`
contract. Scene authoring assigns appearances and existing Item components,
including physics bodies and explicit collider dimensions. A plugin binds those
Items and decides what they mean: a character, lever, barrel or objective.

Plugin-specific state such as spell cooldowns or quest progress belongs in typed
plugin structs keyed by Item ID. Existing world-owned state such as durability
and velocity should not be duplicated there. The Item component set is currently
closed Rust structs, not an arbitrary runtime component registry.

- New models, colors, placements and existing ability parameters are data-only.
- New game rules live in plugin code, with application input/UI wiring as needed.
- A new general world capability or component still requires `io-world` work.
- A new graphics capability still requires renderer work; a plugin is not a shader system.

The encounter rejects living rigid-body physics ownership. Optional `Grounded`
components now resolve movement against shared terrain and conservative obstacle
bounds; scenes without that component retain the original direct movement.
See [Village Exploration](villages.md) for the new exploration plugin and exact
controller/terrain limitations. Plugins can also use kinematic targets or dynamic impulses.

## Scope and Verification

One authoritative plugin is registered per `Session`; a plugin can compose its
own rule modules. This is not yet a multiple-addon dependency registry, hot reload,
runtime DLL/WASM loading, runtime save format, or a universal UI plugin system.
Adding a compiled plugin requires rebuilding and choosing it at the application
composition root. The existing native input/HUD adapter remains encounter-specific.

Events retain the newest 256 entries with monotonic sequence numbers and a drop
counter. They are snapshot-friendly feedback, not a reliable transport: consumers
must detect gaps. Critical commands still use the worker's bounded correlated
request/reply queue. Synchronous event handlers cannot recursively invoke each other.

Tests cover a second plugin, pre/post physics ordering, exactly-once/offscreen
simulation, component checks, snapshot independence, invalid registration/ticks,
bounded events, phase validation, skipped turns, fresh round IDs, and exhaustion.
The migrated encounter suite checks existing combat behavior through `Session`.
