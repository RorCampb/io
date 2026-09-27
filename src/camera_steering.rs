//! Local obstacle avoidance; user orbit intent remains separate from the resolved view.
use io_types::Vec3;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Avoidance {
    pub yaw_range_degrees: f32,
    pub pitch_range_degrees: f32,
    pub turn_speed_degrees: f32,
}
impl Avoidance {
    pub fn valid(self) -> bool {
        [
            self.yaw_range_degrees,
            self.pitch_range_degrees,
            self.turn_speed_degrees,
        ]
        .iter()
        .all(|v| v.is_finite())
            && (15. ..=90.).contains(&self.yaw_range_degrees)
            && (5. ..=60.).contains(&self.pitch_range_degrees)
            && (15. ..=360.).contains(&self.turn_speed_degrees)
    }
}
pub fn angles(direction: Vec3) -> (f32, f32) {
    (
        direction.x.atan2(direction.y),
        direction.z.clamp(-1., 1.).asin(),
    )
}
pub fn direction(yaw: f32, pitch: f32) -> Vec3 {
    let (s, c) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    Vec3::new(s * cp, c * cp, sp)
}
fn difference(a: f32, b: f32) -> f32 {
    (a - b + PI).rem_euclid(TAU) - PI
}

#[derive(Clone, Debug)]
pub struct Steering {
    settings: Avoidance,
    pub direction: Option<Vec3>,
    goal: Option<Vec3>,
    manual_seconds: f32,
}
impl Steering {
    pub fn new(settings: Avoidance) -> Self {
        Self {
            settings,
            direction: None,
            goal: None,
            manual_seconds: 0.,
        }
    }
    pub fn manual_orbit(&mut self) {
        self.manual_seconds = 0.3;
        self.goal = None;
    }
    pub fn manual_active(&self) -> bool {
        self.manual_seconds > 0.
    }
    /// `usable` returns the collision-free radius fraction in 0..1. Search is
    /// bounded to 37 directions and only runs when the preferred ray is obstructed.
    pub fn update(
        &mut self,
        desired: Vec3,
        dt: f32,
        response: f32,
        usable: impl Fn(Vec3) -> f32,
    ) -> Vec3 {
        if self.manual_seconds > 0. {
            self.manual_seconds = (self.manual_seconds - dt).max(0.);
            self.direction = Some(desired);
            return desired;
        }
        if dt == 0. {
            return *self.direction.get_or_insert(desired);
        }
        let (yaw, pitch) = angles(desired);
        let yr = self.settings.yaw_range_degrees.to_radians();
        let pr = self.settings.pitch_range_degrees.to_radians();
        let score = |d: Vec3| {
            let (y, p) = angles(d);
            usable(d) - 0.3 * (difference(y, yaw) / yr).hypot((p - pitch) / pr)
        };
        let preferred = usable(desired);
        let mut best = desired;
        let mut best_score = preferred;
        if preferred < 0.95 {
            if let Some(old) = self.goal {
                let (y, p) = angles(old);
                if difference(y, yaw).abs() <= yr + 1e-5 && (p - pitch).abs() <= pr + 1e-5 {
                    let old_score = score(old) + 0.04; // Hysteresis avoids left/right chatter.
                    if old_score > best_score {
                        best = old;
                        best_score = old_score;
                    }
                }
            }
            for yi in -4..=4 {
                for shift in [0., -pr * 0.5, -pr, pr] {
                    let d = direction(
                        yaw + yr * yi as f32 / 4.,
                        (pitch + shift).clamp(2_f32.to_radians(), 85_f32.to_radians()),
                    );
                    let candidate = score(d);
                    if candidate > best_score + 0.001 {
                        best = d;
                        best_score = candidate;
                    }
                }
            }
        }
        self.goal = Some(best);
        let old = self.direction.unwrap_or(desired);
        let (old_yaw, old_pitch) = angles(old);
        let (new_yaw, new_pitch) = angles(best);
        let dy = difference(new_yaw, old_yaw);
        let dp = new_pitch - old_pitch;
        let angle = dy.hypot(dp);
        let alpha = (-(-dt / response).exp_m1())
            .min(self.settings.turn_speed_degrees.to_radians() * dt / angle.max(1e-6));
        let next = direction(old_yaw + dy * alpha, old_pitch + dp * alpha);
        self.direction = Some(next);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn steering() -> Steering {
        Steering::new(Avoidance {
            yaw_range_degrees: 60.,
            pitch_range_degrees: 40.,
            turn_speed_degrees: 120.,
        })
    }
    #[test]
    fn finds_room_lowers_camera_and_returns_to_requested_view() {
        let mut s = steering();
        let desired = direction(45_f32.to_radians(), 35_f32.to_radians());
        let usable = |d: Vec3| {
            let (y, p) = angles(d);
            if y.abs() < 0.05 && p < 0.1 {
                1.
            } else {
                0.1
            }
        };
        let mut last = desired;
        for _ in 0..120 {
            let next = s.update(desired, 1. / 60., 0.18, usable);
            assert!((next - last).dot(next - last).sqrt() < 0.04);
            last = next;
        }
        assert!(usable(last) > 0.9);
        for _ in 0..120 {
            last = s.update(desired, 1. / 60., 0.18, |_| 1.);
        }
        assert!(last.dot(desired) > 0.9999);
    }
    #[test]
    fn manual_rotation_takes_priority_and_schema_is_checked() {
        let mut s = steering();
        let desired = direction(0.5, 0.2);
        s.manual_orbit();
        for _ in 0..10 {
            assert_eq!(s.update(desired, 1. / 60., 0.18, |_| 0.1), desired);
        }
        assert!(!Avoidance {
            turn_speed_degrees: f32::NAN,
            ..s.settings
        }
        .valid());
        assert!(!Avoidance {
            yaw_range_degrees: 180.,
            ..s.settings
        }
        .valid());
    }
}
