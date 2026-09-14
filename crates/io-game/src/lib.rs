#![forbid(unsafe_code)]
//! Reusable gameplay lifecycle and round framework. Concrete game rules live in plugins.
mod plugin;
mod round;
pub use plugin::*;
pub use round::*;

#[cfg(test)]
mod tests;
