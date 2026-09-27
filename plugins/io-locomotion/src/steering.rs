use crate::Error;
use io_types::Vec3;
use serde::Deserialize;

/// NPC path-following response; independent of game policy and player input.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SteeringSettings {
    /// Route shortcut horizon, in meters.
    pub look_ahead: f32,
    /// Maximum velocity change in meters per second squared.
    pub acceleration: f32,
    pub braking: f32,
    /// Near-target speed is capped at remaining distance divided by this many seconds.
    pub arrival_response: f32,
}
impl Default for SteeringSettings {
    fn default() -> Self {
        Self {
            look_ahead: 3.,
            acceleration: 9.,
            braking: 14.,
            arrival_response: 0.25,
        }
    }
}
impl SteeringSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.look_ahead.is_finite()
            || !(0.2..=6.).contains(&self.look_ahead)
            || !self.acceleration.is_finite()
            || !(0.5..=80.).contains(&self.acceleration)
            || !self.braking.is_finite()
            || !(0.5..=80.).contains(&self.braking)
            || !self.arrival_response.is_finite()
            || !(0.05..=1.).contains(&self.arrival_response)
        {
            return Err("invalid steering settings");
        }
        Ok(())
    }
}

/// Read-only input: steering has no world mutation or hidden target access.
#[derive(Clone, Copy, Debug)]
pub struct SteeringInput {
    pub actor: u64,
    pub position: Vec3,
    pub target: Vec3,
    pub current_velocity: Vec3,
    pub max_speed: f32,
    pub seconds: f32,
}

/// A finite planar velocity. The controller also enforces the actor's speed and clearance.
#[derive(Clone, Copy, Debug)]
pub struct SteeringOutput(Vec3);
impl SteeringOutput {
    pub fn new(velocity: Vec3) -> Result<Self, Error> {
        if !velocity.finite() || velocity.z != 0. {
            return Err(Error::InvalidInput);
        }
        Ok(Self(velocity))
    }
    pub fn velocity(self) -> Vec3 {
        self.0
    }
}

/// Pure movement response, shared across immutable simulation publications.
/// Implementations should depend only on their configuration and the supplied input;
/// temporal motion state is supplied as actual feedback, not hidden mutable plugin state.
pub trait Steering: std::fmt::Debug + Send + Sync {
    fn steer(&self, input: SteeringInput) -> Result<SteeringOutput, Error>;
}

#[derive(Clone, Debug)]
pub struct AccelerationSteering {
    acceleration: f32,
    braking: f32,
    arrival_response: f32,
}
impl AccelerationSteering {
    pub fn new(settings: SteeringSettings) -> Result<Self, &'static str> {
        settings.validate()?;
        Ok(Self {
            acceleration: settings.acceleration,
            braking: settings.braking,
            arrival_response: settings.arrival_response,
        })
    }
    fn velocity(&self, current: Vec3, delta: Vec3, speed: f32, dt: f32) -> Vec3 {
        let delta = Vec3::new(delta.x, delta.y, 0.);
        let distance = delta.x.hypot(delta.y);
        if distance < 0.000001 {
            return Vec3::default();
        }
        let current = Vec3::new(current.x, current.y, 0.);
        // Finish sub-millimeter travel through the motor rather than converge
        // forever just outside a support boundary. Respect speed and braking.
        let finish = delta.scaled(1. / dt);
        let change = finish - current;
        if distance <= 0.001
            && distance / dt <= speed
            && change.x.hypot(change.y) <= self.braking * dt
        {
            return finish;
        }
        // Leave a small braking margin. Slow near the target instead of overshooting it.
        let desired_speed = speed
            .min((self.braking * distance).sqrt())
            .min(distance / self.arrival_response)
            .min(distance / dt);
        let desired = delta.scaled(desired_speed / distance);
        let change = desired - current;
        let rate = if current.dot(desired) < 0. || desired.dot(desired) < current.dot(current) {
            self.braking
        } else {
            self.acceleration
        };
        let velocity =
            current + change.scaled((rate * dt / change.x.hypot(change.y).max(0.0001)).min(1.));
        let magnitude = velocity.x.hypot(velocity.y);
        velocity.scaled((speed.min(distance / dt) / magnitude.max(0.0001)).min(1.))
    }
}
impl Steering for AccelerationSteering {
    fn steer(&self, input: SteeringInput) -> Result<SteeringOutput, Error> {
        if !input.position.finite()
            || !input.target.finite()
            || !input.current_velocity.finite()
            || !input.max_speed.is_finite()
            || !(0.1..=12.).contains(&input.max_speed)
            || !input.seconds.is_finite()
            || !(0. ..=0.25).contains(&input.seconds)
            || input.seconds == 0.
        {
            return Err(Error::InvalidInput);
        }
        SteeringOutput::new(self.velocity(
            input.current_velocity,
            input.target - input.position,
            input.max_speed,
            input.seconds,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn velocity_turns_continuously_and_brakes_for_reversed_goal() {
        let settings = SteeringSettings::default();
        let steering = AccelerationSteering::new(settings).unwrap();
        for hz in [30, 60, 144] {
            let dt = 1. / hz as f32;
            let mut velocity = Vec3::new(2.7, 0., 0.);
            for _ in 0..hz {
                let next = steering.velocity(velocity, Vec3::new(0., 10., 0.), 2.7, dt);
                assert!(
                    (next - velocity).dot(next - velocity).sqrt() <= settings.braking * dt + 1e-5
                );
                assert!(next.dot(next).sqrt() <= 2.70001);
                velocity = next;
            }
            assert!(velocity.y > 2.69 && velocity.x.abs() < 0.01);
            let mut drift = 0.;
            for _ in 0..hz {
                velocity = steering.velocity(velocity, Vec3::new(0., -10., 0.), 2.7, dt);
                drift += velocity.y.max(0.) * dt;
            }
            assert!(drift < 0.3, "old-goal drift at {hz} Hz: {drift}");
            assert!(velocity.y < -2.69);
        }
    }
    #[test]
    fn settings_reject_nonfinite_and_out_of_bounds() {
        for settings in [
            SteeringSettings {
                look_ahead: f32::NAN,
                ..Default::default()
            },
            SteeringSettings {
                acceleration: 0.,
                ..Default::default()
            },
            SteeringSettings {
                braking: f32::INFINITY,
                ..Default::default()
            },
        ] {
            assert!(settings.validate().is_err());
        }
    }
}
