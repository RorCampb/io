#![forbid(unsafe_code)]
//! Retained items, world coordinates, subdivisions, and spatial queries.
//! Model IDs are opaque references resolved by the application.

mod command;
#[cfg(test)]
mod component_tests;
mod components;
mod effect;
#[cfg(test)]
mod effect_tests;
mod ground;
#[cfg(test)]
mod ground_tests;
mod item;
pub use ground::{ground_destination, line_of_sight, Grounded, HeightField};
mod motion;
mod physics;
#[cfg(test)]
mod physics_tests;
mod snapshot;
mod space;
mod spatial;
mod world;

pub use command::{CommandError, CommandOutcome, WorldCommand};
pub use components::{
    ColorMode, DepletionAnimation, DepletionResponse, Durability, Occupancy, Renderable, Transform,
};
pub use effect::{AnimationEvent, AnimationEvents, Damage, Effect, EffectKind};
pub use item::Item;
pub use motion::{AnimationState, PathMotion, Playback};
pub use physics::{
    BodyKind, Collider, ColliderShape, ContactDiagnostics, ContactEvent, ContactPassStats,
    PhysicsBody, PhysicsOverlapAudit, PhysicsSettings, PhysicsStats, SleepSettings, SolverMode,
    CORRECTION_SPEED_BOUNDS, CORRECTION_SPEED_THRESHOLD,
};
pub use snapshot::{WorldSnapshot, WorldView};
pub use space::{Axis, Space};
pub use world::World;
