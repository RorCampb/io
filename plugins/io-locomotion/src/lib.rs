#![forbid(unsafe_code)]
//! Optional upright-body locomotion plugin. Core traversal never depends on this crate.
mod animation;
pub mod athletics;
mod executor;
mod motor;
mod paths;
pub mod pipeline;
#[cfg(test)]
mod pipeline_tests;
mod steering;
pub mod surface;
#[cfg(test)]
mod surface_tests;
pub use executor::*;
pub use io_traversal::{
    Error, NavigationError, NavigationGoal, NavigationRequest, NavigationStatus, NavigationTicket,
};
