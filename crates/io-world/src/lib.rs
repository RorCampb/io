#![forbid(unsafe_code)]
//! Retained items, world coordinates, subdivisions, and spatial queries.
//! Model IDs are opaque references resolved by the application.
mod character;
mod support;
#[cfg(test)]
mod support_tests;
pub use support::{
    character_support, character_supported_segment, trace_character_walk, walk_character,
    CharacterSupport, CharacterWalk, SupportProbe,
};
mod geometry;
mod interior;
mod portal;
pub use character::{
    character_colliders_clear, character_fits, character_path_clear_of_actors,
    character_segment_clear, character_space_fits, sweep_character, CharacterSweep,
};
pub use interior::Interior;
pub use portal::{
    advance_location, locate_space, validate_layout, InteriorId, Portal, SpaceLocation,
};

mod command;
#[cfg(test)]
mod component_tests;
mod components;
mod effect;
#[cfg(test)]
mod effect_tests;
mod item;
mod terrain;
#[cfg(test)]
mod terrain_tests;
pub use terrain::{terrain_destination, HeightField, TerrainSample};
mod surface;
pub use surface::{surface_candidates, SurfaceHit, SurfaceQuery, SurfaceQueryError, SurfaceSource};
#[cfg(test)]
mod surface_tests;
mod visibility;
pub use visibility::{line_of_sight, sight_sample_clear, sight_segment_clear};
mod shape_cast;
pub use shape_cast::{cast_sphere, cast_sphere_hit, SphereCastHit};
mod changes;
mod motion;
mod physics;
#[cfg(test)]
mod physics_tests;
mod snapshot;
mod space;
mod spatial;
mod world;
pub use changes::{ChangeLog, ChangeSource, WorldChange};

pub use command::{CommandError, CommandOutcome, WorldCommand};
pub use components::{
    CharacterBody, ColorMode, DepletionAnimation, DepletionResponse, Durability, Occupancy,
    Renderable, Transform,
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
