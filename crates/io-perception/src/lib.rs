#![forbid(unsafe_code)]
//! Geometry produces evidence; an independent, time-integrated memory retains it.
//! No hostility, target selection, movement or combat policy lives here.
use io_types::Vec3;
use io_world::{sight_sample_clear, WorldView};
use serde::Deserialize;
pub mod intake;
pub mod pipeline;

/// A current visual contact, not knowledge of a hidden target's live position.
#[derive(Clone, Copy, Debug)]
pub struct VisualContact {
    pub position: Vec3,
    pub focus: f32,
    pub attention: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VisionProfile {
    pub range: f32,
    /// Full horizontal field of view around the upright body's forward direction.
    pub fov_degrees: f32,
    /// Exponential approach rate when current visual evidence exceeds awareness.
    pub gain_per_second: f32,
    /// Exponential approach rate when visual evidence drops below awareness.
    pub decay_per_second: f32,
}
impl VisionProfile {
    pub fn validate(self) -> Result<(), &'static str> {
        if ![
            self.range,
            self.fov_degrees,
            self.gain_per_second,
            self.decay_per_second,
        ]
        .iter()
        .all(|v| v.is_finite())
            || !(0.1..=1000.).contains(&self.range)
            || !(5. ..=360.).contains(&self.fov_degrees)
            || !(0. ..=100.).contains(&self.gain_per_second)
            || !(0. ..=100.).contains(&self.decay_per_second)
        {
            return Err("invalid vision profile");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Observation {
    /// Fraction of body samples visible inside this observer's view and range.
    pub exposure: f32,
    /// Instantaneous focus: exposure weighted by angle and distance, not a probability.
    pub evidence: f32,
}

fn directional_focus(alignment: f32, edge: f32) -> f32 {
    let centered = ((alignment - edge) / (1. - edge)).clamp(0., 1.);
    // Smoothstep has zero slope both at direct front and at the peripheral edge.
    centered * centered * (3. - 2. * centered)
}

pub fn observe(
    world: &dyn WorldView,
    observer: u64,
    target: u64,
    profile: VisionProfile,
) -> Result<Observation, String> {
    profile.validate()?;
    if observer == target {
        return Err("cannot observe self".into());
    }
    let source = world.item(observer).ok_or("unknown observer")?;
    let target = world.item(target).ok_or("unknown observation target")?;
    let source_shape = source
        .character_body
        .ok_or("observer needs a character body")?;
    let eye = source.transform.anchor + Vec3::new(0., 0., source_shape.height * 0.85);
    let forward = source.transform.rotation.rotate(Vec3::new(0., -1., 0.));
    let forward = Vec3::new(forward.x, forward.y, 0.);
    let forward_length = forward.dot(forward).sqrt();
    if forward_length <= 1e-5 {
        return Err("observer needs a horizontal facing direction".into());
    }
    let forward = forward.scaled(1. / forward_length);
    let bounds = target.current_spatial_bounds();
    let edge = (profile.fov_degrees.to_radians() * 0.5).cos();
    let mut observation = Observation::default();
    // A fixed nine-sample body proxy is cheap and deterministic, not exact visible mesh area.
    for height in [0.2, 0.55, 0.9] {
        for offset in [-1., 0., 1.] {
            let point = match target.character_body {
                Some(shape) => {
                    target.transform.anchor
                        + Vec3::new(0., 0., shape.height * height)
                        + target.transform.rotation.rotate(Vec3::new(
                            shape.radius * 0.85 * offset,
                            0.,
                            0.,
                        ))
                }
                None => {
                    // Bounds proxy for non-character Items. Not mesh-exact visibility.
                    let center = bounds.center();
                    let extent = (bounds.max - bounds.min).scaled(0.425);
                    center
                        + Vec3::new(
                            extent.x * offset,
                            extent.y * offset,
                            (bounds.max.z - bounds.min.z) * (height - 0.5),
                        )
                }
            };
            let delta = point - eye;
            let distance = delta.dot(delta).sqrt();
            if distance <= 1e-5 || distance >= profile.range {
                continue;
            }
            // Height still affects range and occlusion, not peripheral sensitivity.
            // A spherical cone incorrectly penalizes a nearby character's lower body.
            let horizontal = Vec3::new(delta.x, delta.y, 0.);
            let horizontal_distance = horizontal.dot(horizontal).sqrt();
            if horizontal_distance <= 1e-5 {
                continue;
            }
            let alignment = forward.dot(horizontal.scaled(1. / horizontal_distance));
            if alignment <= edge {
                continue;
            }
            if sight_sample_clear(world, observer, target.id, eye, point)? {
                let angle = directional_focus(alignment, edge);
                observation.exposure += 1. / 9.;
                observation.evidence += angle * (1. - distance / profile.range) / 9.;
            }
        }
    }
    observation.exposure = observation.exposure.clamp(0., 1.);
    observation.evidence = observation.evidence.clamp(0., 1.);
    Ok(observation)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Awareness {
    value: f64,
}
impl Awareness {
    pub fn value(self) -> f32 {
        self.value as f32
    }
    /// Exact integration for evidence held constant during this simulation interval.
    /// Invalid input leaves memory untouched. Elapsed time is seconds, never render frames.
    pub fn advance(
        &mut self,
        evidence: f32,
        profile: VisionProfile,
        seconds: f32,
    ) -> Result<(), &'static str> {
        profile.validate()?;
        if !evidence.is_finite()
            || !(0. ..=1.).contains(&evidence)
            || !seconds.is_finite()
            || !(0. ..=1.).contains(&seconds)
        {
            return Err("invalid awareness update");
        }
        let target = f64::from(evidence);
        let rate = if target > self.value {
            profile.gain_per_second
        } else {
            profile.decay_per_second
        };
        // Evidence determines the level, not just how quickly awareness saturates.
        // Constant weak peripheral focus must never grow into strong awareness.
        let blend = -(-f64::from(rate) * f64::from(seconds)).exp_m1();
        self.value = (self.value + (target - self.value) * blend).clamp(0., 1.);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
