# Round-Based Game Prototype

```sh
make release
./build/release/io --scene assets/game/encounter.json
```

The optional `game` section registers the example `io-encounter` plugin with
the `io-game` host. Existing scenes do not
change. This demo uses three colored boxes and one static physics floor; no new
art pipeline or renderer-specific character class is required.

## Controls and Rules

- WASD moves the selected player along camera-relative ground axes during
  exploration or free movement. Movement follows camera rotation and diagonal
  input is normalized. Right-drag orbits; middle-drag pans; wheel zooms.
- Enter begins combat. Every round starts with three seconds of simultaneous
  movement in the demo, configurable through `game.movement_seconds`.
- NPCs approach their nearest living enemy during the movement window.
- At the deadline, voluntary movement stops. Descending initiative determines
  turns, with ascending Item ID as the deterministic tie-breaker.
- Number keys 1..4 select an ability without executing it. Left-click a visible
  enemy to attack with the selected ability during your turn. Clicks on the UI
  are intercepted. T changes the inspected target but does not attack.
  Valid attacks consume the turn; invalid clicks/attacks do not.
- T cycles living enemies. Space passes. C switches player characters during
  free movement if the scene has more than one player-controlled combatant.
- A yellow silhouette outlines the controlled player. You can also click a
  friendly player to select them during free movement.
- NPC turns resolve after a short simulation-time delay: first usable ability
  against the nearest attackable enemy, otherwise pass.
- After all living combatants act, the next movement window begins.
- Death removes a character from movement, targeting, reactions and future turns.
  Combat finishes when at most one living faction remains. Reopen to reset.

Movement commands carry their exploration/round window identity. The worker
rejects commands delivered after that window closes, including old commands
delivered during a later round. Directions are cleared at phase boundaries.
Movement is locked through both attack turns and projectile resolution; corpse
physics intentionally continues.

The demo's spark uses `"delivery": {"type":"projectile","speed":8}` (world
units per simulated second). Its world-space glow and short trail travel toward
the clicked Item ID. It tracks the target center and applies damage and death
knockback only on impact, then advances the turn. Other actions cannot skip the
flight. A dead source/target cancels it without another hit. Opportunity attacks
must use instant delivery; existing abilities default to instant delivery.

Damage numbers rise and fade above the hit location for 1.5 simulation seconds.
A bounded 256-hit presentation buffer survives latest-only snapshot publication;
the UI displays the newest 16. It is feedback, not an authoritative event bus.

Opportunity areas use distances between Item anchors. An enemy with an
opportunity ability threatens its configured range. A moving character leaving
that area triggers a hit immediately during the movement tick, limited to one
reaction per enemy per round. Reactions reset at the next round, not at the next
attack turn. Relative swept segments detect even entering and leaving an area
within one tick; exits are ordered by crossing time and stable IDs. A lethal
reaction stops the victim at the crossing before death physics takes over.
Stationary targets do not provoke just because the threatening enemy walks away.

## Ownership and Contracts

`io-game` depends only on `io-world` and `io-types`. It owns `Session<P>`, the
`GamePlugin` lifecycle, checked `PluginWorld` access, and generic `Round<R, O>`
phase transitions. It does not import or name the encounter plugin.
See [Game Plugins](game-plugins.md) for the developer contract and extension limits.

`io-encounter` is the example developer plugin. It contains:

- `GameDefinition`, `CharacterTemplate`, `AbilityDefinition`, and named
  `CombatantBinding` authoring data, with strict versioned serialization.
- Private `Encounter` runtime state and read-only combatants, `Phase` (including
  explicit `Resolving` projectile state), engagement
  pairs, reaction budgets, current turn, movement intent and last combat event.
- Owned compact `GameCommand` and typed `GameError` rule rejections.

Health remains exclusively in the Item's existing `Durability`. Bindings resolve
scene Item names to IDs at startup. Appearance and meshes remain in the existing
asset catalog, so changing a model does not change its abilities. Ability effects
are a closed enum; this milestone implements deterministic single-target enemy
damage, not a scripting language. New values for existing mechanics are data
edits; new effect mechanics still require code.

The application's `src/demo.rs` registers the plugin through
`io_encounter::register`, validating definitions and world bindings. `Game` is
an alias for `Session<Encounter>`. Its typed command handler enforces
phase, control, turn ownership, health, ability ownership, enemy faction, and
range. A template grants up to four abilities; the opportunity ability must be
in that loadout. Authoring structs are editable; accepted runtime definitions
are shared immutable data, separate from round progress.

The existing worker owns both `Game` and `World`. Game and world commands share
the same bounded queue and correlation-ID/reply rules. `Game::step` advances both
using the configured simulation timestep, never render-frame elapsed time.
Each publication includes a cloned game snapshot beside its world snapshot.
Inline mode uses the same rules without a worker. No OpenGL calls occur there.

C translates input to intents and draws owned text supplied by the Rust app
adapter. The renderer contains no round, health or damage decisions. The small
native ABI validates numeric action tags; it does not pass Rust enum layout
across FFI. In worker mode, submission success means queued, not applied.
The menu uses the existing HUD renderer and is hidden by `--no-hud`.

## Death Knockback

An optional template `death_physics` specifies mass, horizontal/upward impulse,
and explicit physical box half-extents/offset in world units. These do not scale
automatically with a replacement mesh; adjust the collider when changing sizes.
The demo gives all characters a gentle outward/upward impulse on death, applied
above the center of mass to tip the box. This uses the existing rigid-body
solver, gravity, contacts, friction, and sleeping, not an animation or teleport.

Living game-controlled characters must not also have route motion or a physics
body. On death, `World::attach_dynamic_body` atomically validates and installs
the body/collider/initial impulse. It refreshes spatial bounds and resets
index-based solver caches because the physical body list changed; existing
bodies are woken conservatively. Game movement then relinquishes that Item.
Corpses keep simulating during attack turns, after combat ends and offscreen.
They are single rigid bodies, not skeletal ragdolls.

## Deliberate Limits

This is a local prototype, not a full dungeon game or editor. Living movement
is direct planar movement with no navmesh, wall collision, or character-body
blocking; living actors can overlap. Only corpses and authored physical scenery
participate in collision. Movement poses update at simulation tick cadence.
Combat starts explicitly with Enter rather than enemy detection. The entire
configured roster shares one encounter. No party networking, inventory,
equipment ownership, area spells, smoke, status effects, spell resources,
line-of-sight rules, skeletal attack animation timing, or save/load of runtime
combat is implemented. `last_event` is inspectable presentation state, not a
reliable event history; latest-only publications can skip intermediate events.

Click picking uses frontmost projected world-space render bounds, including
scenery occlusion. It is conservative box picking, not pixel-perfect animated
mesh picking. Projectiles are homing gameplay effects drawn as depth-tested
glowing billboards, not rigid bodies: they currently do not collide with walls
or intercept unintended targets. These limitations are separate from the
authoritative clicked-target/impact/damage contract.

Reaction trajectory calculations use each tick's planned relative movement.
They are gameplay sphere crossings, not continuous rigid-body collision tests.
The editor can build on the serialized definitions and validated command
boundary later; no live editor or hot reload is included here.

## Checks

`make test` includes generic `io-game` transition/plugin contract tests,
`io-encounter` rules, dynamic death-body attachment and settling,
worker ownership/publication checks, and a native C integration test that drives
movement, the attack menu, NPC turns and the next round. `make gpu-test` checks
the game menu's pixels and upload reuse alongside the existing renderer suite.
