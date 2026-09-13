#![forbid(unsafe_code)]
//! Retained items, world coordinates, subdivisions, and spatial queries.
//! Model IDs are opaque references resolved by the application.

#[cfg(test)]
mod component_tests;
mod components;
mod effect;
#[cfg(test)]
mod effect_tests;
mod item;
mod motion;
mod space;
mod spatial;
mod world;

pub use components::{
    ColorMode, DepletionAnimation, DepletionResponse, Durability, Occupancy, Renderable, Transform,
};
pub use effect::{AnimationEvent, AnimationEvents, Damage, Effect, EffectKind};
pub use item::Item;
pub use motion::{AnimationState, PathMotion, Playback};
pub use space::{Axis, Space};
pub use world::World;
