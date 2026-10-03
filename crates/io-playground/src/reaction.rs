//! Observation-derived prediction policy. No hidden world poses or geometry queries.
use io_game::stage::Stage;
use io_traversal::NavigationPriority;
use io_types::{Bounds, Vec3};
use io_world::CharacterBody;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReactionSettings {
    /// This policy predicts translating obstacles, not static support geometry.
    pub minimum_obstacle_speed: f32,
    pub horizon_seconds: f32,
    pub brake_seconds: f32,
    pub cooldown_seconds: f32,
    pub lease_seconds: f32,
    pub margin: f32,
}
impl Default for ReactionSettings {
    fn default() -> Self {
        Self {
            minimum_obstacle_speed: 0.25,
            horizon_seconds: 2.,
            brake_seconds: 0.6,
            cooldown_seconds: 0.5,
            lease_seconds: 0.35,
            margin: 0.25,
        }
    }
}
impl ReactionSettings {
    pub fn validate(self) -> Result<(), String> {
        if ![
            self.minimum_obstacle_speed,
            self.horizon_seconds,
            self.brake_seconds,
            self.cooldown_seconds,
            self.lease_seconds,
            self.margin,
        ]
        .iter()
        .all(|v| v.is_finite())
            || !(0.25..=4.).contains(&self.horizon_seconds)
            || !(0.05..=10.).contains(&self.minimum_obstacle_speed)
            || !(0.1..=self.horizon_seconds).contains(&self.brake_seconds)
            || !(0.1..=2.).contains(&self.cooldown_seconds)
            || !(0.05..=2.).contains(&self.lease_seconds)
            || !(0. ..=1.).contains(&self.margin)
        {
            return Err("invalid observation reaction settings".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ObservedMotion {
    pub observer: u64,
    pub target: u64,
    pub bounds: Bounds,
    pub velocity: Vec3,
    pub observed_at: f64,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct MotionEstimate {
    previous: Option<(Vec3, f64)>,
    velocity: Option<Vec3>,
}
impl MotionEstimate {
    pub fn sample(&mut self, bounds: Option<Bounds>, now: f64) -> Option<Vec3> {
        let Some(bounds) = bounds else {
            *self = Self::default();
            return None;
        };
        let center = (bounds.min + bounds.max).scaled(0.5);
        let previous = self.previous.replace((center, now));
        let (old, time) = previous?;
        let dt = (now - time) as f32;
        if !(0.001..=0.5).contains(&dt) {
            self.velocity = None;
            return None;
        }
        let raw = (center - old).scaled(1. / dt);
        if !raw.finite() || raw.dot(raw) > 10000. {
            self.velocity = None;
            return None;
        }
        let alpha = 1. - (-dt / 0.1).exp();
        let velocity = self.velocity.map_or(raw, |v| v + (raw - v).scaled(alpha));
        self.velocity = Some(velocity);
        Some(velocity)
    }
}

pub struct ReactionRequest<'a> {
    pub observation: ObservedMotion,
    pub now: f64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub body: CharacterBody,
    pub route: &'a [Vec3],
    pub settings: ReactionSettings,
}
#[derive(Clone, Copy, Debug)]
pub struct PredictedConflict {
    pub target: u64,
    pub seconds: f32,
    pub priority: NavigationPriority,
}
pub struct ReactionStage;
impl Stage<ReactionRequest<'_>> for ReactionStage {
    type Output = Option<PredictedConflict>;
    type Error = String;
    fn run(&mut self, r: ReactionRequest<'_>) -> Result<Self::Output, String> {
        r.settings.validate()?;
        if !r.now.is_finite()
            || !r.observation.observed_at.is_finite()
            || !r.position.finite()
            || !r.velocity.finite()
            || !r.observation.velocity.finite()
            || !r.observation.bounds.valid()
            || r.body.validate().is_err()
            || r.route.iter().any(|p| !p.finite())
        {
            return Err("invalid motion observation".into());
        }
        let age = (r.now - r.observation.observed_at) as f32;
        if !(0. ..=0.5).contains(&age) {
            return Ok(None);
        }
        let velocity = r.observation.velocity;
        if velocity.dot(velocity) < r.settings.minimum_obstacle_speed.powi(2) {
            return Ok(None);
        }
        let offset = velocity.scaled(age);
        let radius = r.body.radius + r.settings.margin;
        let min = r.observation.bounds.min + offset - Vec3::new(radius, radius, r.body.height);
        let max = r.observation.bounds.max + offset + Vec3::new(radius, radius, 0.);
        let speed = r.velocity.x.hypot(r.velocity.y);
        let mut from = r.position;
        let mut time = 0.;
        // Bounded piecewise route prediction. Stopped actors remain stationary;
        // an obstacle can still move into them. No world query or route search.
        for index in 0..=r.route.len().min(8) {
            let to = if index < r.route.len().min(8) {
                r.route[index]
            } else {
                from
            };
            let delta = to - from;
            let distance = delta.dot(delta).sqrt();
            let remaining = r.settings.horizon_seconds - time;
            if remaining <= 0. {
                break;
            }
            if distance <= 0.001 && index < r.route.len().min(8) {
                continue;
            }
            let duration = if speed > 0.05 && distance > 0.001 {
                (distance / speed).min(remaining)
            } else {
                remaining
            };
            let motion = if speed > 0.05 && distance > 0.001 {
                delta.scaled(speed / distance)
            } else {
                Vec3::default()
            };
            if let Some(hit) = contact_time(
                from - velocity.scaled(time),
                motion - velocity,
                min,
                max,
                duration,
            ) {
                let seconds = time + hit;
                return Ok(Some(PredictedConflict {
                    target: r.observation.target,
                    seconds,
                    priority: if seconds <= r.settings.brake_seconds {
                        NavigationPriority::Urgent
                    } else {
                        NavigationPriority::Elevated
                    },
                }));
            }
            from = from + motion.scaled(duration);
            time += duration;
        }
        Ok(None)
    }
}

fn contact_time(p: Vec3, v: Vec3, min: Vec3, max: Vec3, seconds: f32) -> Option<f32> {
    let (mut enter, mut exit) = (0_f32, seconds);
    for (p, v, lo, hi) in [
        (p.x, v.x, min.x, max.x),
        (p.y, v.y, min.y, max.y),
        (p.z, v.z, min.z, max.z),
    ] {
        if v.abs() < 1e-6 {
            if p < lo || p > hi {
                return None;
            }
        } else {
            let (a, b) = ((lo - p) / v, (hi - p) / v);
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
            if enter > exit {
                return None;
            }
        }
    }
    Some(enter)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds(p: Vec3) -> Bounds {
        Bounds {
            min: p - Vec3::new(0.5, 0.5, 0.),
            max: p + Vec3::new(0.5, 0.5, 2.),
        }
    }
    fn predict(p: Vec3, v: Vec3, now: f64) -> Option<PredictedConflict> {
        ReactionStage
            .run(ReactionRequest {
                observation: ObservedMotion {
                    observer: 1,
                    target: 2,
                    bounds: bounds(p),
                    velocity: v,
                    observed_at: 0.,
                },
                now,
                position: Vec3::default(),
                velocity: Vec3::new(2., 0., 0.),
                body: CharacterBody {
                    radius: 0.35,
                    height: 1.9,
                    max_slope: 0.8,
                },
                route: &[Vec3::new(10., 0., 0.)],
                settings: Default::default(),
            })
            .unwrap()
    }
    #[test]
    fn prediction_accounts_for_speed_crossing_and_misses_not_just_distance() {
        assert!(
            predict(Vec3::new(2., 0., 0.), Vec3::default(), 0.).is_none(),
            "static geometry stays with the surface/clearance policy, not swept bounds prediction"
        );
        let slow = predict(Vec3::new(5., 0., 0.), Vec3::new(-1., 0., 0.), 0.).unwrap();
        let fast = predict(Vec3::new(5., 0., 0.), Vec3::new(-6., 0., 0.), 0.).unwrap();
        assert!(fast.seconds < slow.seconds);
        assert_eq!(slow.priority, NavigationPriority::Elevated);
        assert_eq!(fast.priority, NavigationPriority::Urgent);
        assert!(predict(Vec3::new(2., 3., 0.), Vec3::new(0., -3., 0.), 0.).is_some());
        assert!(predict(Vec3::new(2., 3., 0.), Vec3::new(0., 3., 0.), 0.).is_none());
        assert!(predict(Vec3::new(5., 0., 0.), Vec3::new(3., 0., 0.), 0.).is_none());
        assert!(predict(Vec3::new(2., 0., 4.), Vec3::new(-3., 0., 0.), 0.).is_none());
        assert!(predict(Vec3::new(5., 0., 0.), Vec3::new(-6., 0., 0.), 1.).is_none());
    }
    #[test]
    fn motion_uses_observed_time_and_does_not_bridge_hidden_gaps_or_teleports() {
        let mut estimate = MotionEstimate::default();
        assert!(estimate.sample(Some(bounds(Vec3::default())), 0.).is_none());
        let v = estimate
            .sample(Some(bounds(Vec3::new(0.2, 0., 0.))), 0.1)
            .unwrap();
        assert!((v.x - 2.).abs() < 1e-5);
        assert!(estimate.sample(None, 0.2).is_none());
        assert!(estimate
            .sample(Some(bounds(Vec3::new(10., 0., 0.))), 0.3)
            .is_none());
        assert!(estimate
            .sample(Some(bounds(Vec3::new(100., 0., 0.))), 0.4)
            .is_none());
        assert!(estimate
            .sample(Some(bounds(Vec3::new(100., 0., 0.))), 2.)
            .is_none());
    }
    #[test]
    fn prediction_is_time_consistent_at_multiple_tick_rates() {
        for hz in [30, 60, 144] {
            let mut first = None;
            for tick in 0..hz * 3 {
                let time = tick as f32 / hz as f32;
                let p = Vec3::new(14. - 6. * time, 0., 0.);
                if predict(p, Vec3::new(-6., 0., 0.), 0.)
                    .is_some_and(|p| p.priority == NavigationPriority::Urgent)
                {
                    first = Some(time);
                    break;
                }
            }
            let time = first.unwrap();
            assert!((1.34..1.39).contains(&time), "hz={hz}: {time}");
        }
    }
}
