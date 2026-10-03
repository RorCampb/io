#![forbid(unsafe_code)]
//! Reusable gameplay lifecycle and round framework. Concrete game rules live in plugins.
pub mod channel;
mod plugin;
mod round;
pub mod stage;
pub use plugin::*;
pub use round::*;

#[cfg(test)]
mod tests;
