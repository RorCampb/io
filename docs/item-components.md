# Item Components

This refactor separates existing runtime state. It does not add physics, a new
scheduler, or a new scene format. `Item` remains the identity of a placed world
object, not a model or a character superclass.

## Current Shape

```text
Item
  id
  transform: Transform
  occupancy: Occupancy
  renderable: Option<Renderable>
  durability: Option<Durability>
  depletion_response: Option<DepletionResponse>
  motion: Option<PathMotion>
  animation: Option<AnimationState>
  simulated_ticks
```

`Transform` owns position, yaw, scale, and world-maintained interpolation history.
`Occupancy` owns the local gameplay/spatial bounds. Both remain required for placed
items. Occupancy is not a physics collider.

`Renderable` owns appearance ID, visual-state domain, render bounds, tint, and an
explicit color mode. The old `character` boolean is gone. Legacy diagnostic color
pulsing is represented as `ColorMode::Pulse`, not a classification of the object.
The model and skeleton remain shared in the asset catalog.

`Durability` owns current/maximum hit points with validated construction and
private counters. `PathMotion` and `AnimationState` are the existing checked
movement and playback components, retained without a cosmetic rename.

`Item::default()` has no renderer, health, animation, movement, or response. Its
zero ID is an assembly placeholder and must be replaced before world insertion.

## Composition in Rust

A non-rendered moving object needs no mesh or health:

```rust
use io_types::Vec3;
use io_world::{Item, PathMotion, Space, World};

let mover = Item {
    id: 42,
    motion: PathMotion::new(
        vec![Vec3::default(), Vec3::new(20., 0., 0.)],
        2.,
        0.,
    ),
    ..Item::default()
};
let mut world = World::try_new(
    Space::try_new(Vec3::new(100., 100., 100.))?,
    vec![mover],
)?;
world.simulate(&[0], 1.);
// Item 42 is now two units along its route; no model was sampled or drawn.
```

For damageability, explicitly attach `Durability::new(current, maximum)?`.
Damage on an item without that component is a no-op. Reaching zero does not
implicitly stop movement, freeze animation, or remove the object.

Attach a `DepletionResponse` to request those consequences. Its fields are
`stop_motion`, optional `visual_state`, and an exhaustive `DepletionAnimation`:
`Keep`, `Freeze`, or `PlayOnce(AnimationState)`. A visual-only response can break
an appearance without stopping its movement. Responses are consumed once when
durability first reaches zero, including zero at world construction. There is no
healing/revival or automatic response rearming in this API yet.

## Enforced Contracts

- `World::try_new` validates every assembly before exposing or indexing it.
- Transforms must be finite with positive scale; occupancy and render bounds must
  be valid. The spatial envelope covers both occupancy and rendering, so a small
  mesh cannot hide a larger gameplay extent from queries.
- Model animation requires a `Renderable`; no-render movement does not.
- Durability requires a positive maximum and current within 0..maximum.
- Depletion responses require durability. A visual-state response requires a
  renderable with that state. Replacement animation must be an unstarted,
  positive-speed one-shot on a renderable.
- Accepted items are owned by World and exposed through shared references.
  Pose/state/damage methods maintain validation, spatial indexes, and revisions.
  There is no unrestricted mutable item getter or live component replacement API.
- World simulation selects actual motion/animation/time-varying visual state,
  not a generic character flag. Projection skips absent renderables deliberately;
  unresolved references on present renderables still fail the frame.

The world remains independent of the mesh catalog. The scene adapter resolves
appearance IDs, state domains, and clip names; those numeric references are not
compile-time proofs of membership in a particular catalog.

## Compatibility and Scope

The flat version-1 scene schema is unchanged. `src/demo.rs` now explicitly builds
components from it. Existing scene health defaults to 100 and still produces a
durability component. Its initial maximum is `max(100, health)`, since the old
format has no maximum field. Legacy stop/freeze/death behavior is assembled into
a depletion response by that adapter, not hardcoded into every runtime item.

Existing asset packages, LODs, effects, and animation-event ordering remain in
place. C receives the same frame/model/instance layouts. The legacy
`IoItemState.health` field reads durability's current value, or zero when absent;
that old snapshot field cannot distinguish absence from depletion. Rust callers
can inspect the optional component directly.

Components currently use concrete optional fields within Item. This is not an
archetype ECS, dynamic trait-object registry, or a plugin loader. New assembly
combinations are possible in Rust; component-based JSON/templates and validated
live attachment/removal remain separate future changes. Physics, colliders,
dialogue, AI, and independent off-screen scheduling were deliberately not added.
