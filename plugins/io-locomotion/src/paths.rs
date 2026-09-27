//! Shared, read-only movement feasibility used by route providers and the motor.
use crate::Error;
use io_traversal::Progress;
use io_types::Vec3;
use io_world::{
    character_segment_clear, character_support, character_supported_segment, surface_candidates,
    walk_character, CharacterBody, CharacterSweep, SupportProbe, SurfaceQuery, WorldView,
};
use serde::Deserialize;
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub step_height: f32,
    pub jump_distance: f32,
    pub max_drop: f32,
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.step_height.is_finite()
            || !(0. ..=0.5).contains(&self.step_height)
            || !self.jump_distance.is_finite()
            || !(0. ..=4.).contains(&self.jump_distance)
            || !self.max_drop.is_finite()
            || !(0. ..=2.).contains(&self.max_drop)
        {
            return Err("invalid athletics capability".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Walk,
    Step,
    Jump,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub(crate) actor: u64,
    pub(crate) body: CharacterBody,
    pub(crate) speed: f32,
    pub(crate) gravity: f32,
    pub(crate) jump_speed: f32,
    pub(crate) step_height: f32,
    pub(crate) jump_distance: f32,
    pub(crate) max_drop: f32,
}
pub(crate) fn length(v: Vec3) -> f32 {
    v.dot(v).sqrt()
}
pub(crate) fn support(w: &dyn WorldView, p: Profile, at: Vec3) -> bool {
    character_support(w, Some(p.actor), at, p.body, SupportProbe::CONTACT)
        .is_ok_and(|s| s.is_some())
}
pub(crate) fn clear(w: &dyn WorldView, p: Profile, a: Vec3, b: Vec3) -> bool {
    character_segment_clear(w, Some(p.actor), a, b, p.body)
}
#[derive(Clone, Debug)]
pub(crate) enum Path {
    Walk {
        start: Vec3,
        end: Vec3,
        duration: f32,
    },
    Step {
        points: [Vec3; 4],
        distance: f32,
        duration: f32,
    },
    Jump {
        start: Vec3,
        end: Vec3,
        velocity: Vec3,
        gravity: f32,
        duration: f32,
    },
}
impl Path {
    pub(crate) fn duration(&self) -> f32 {
        match self {
            Self::Walk { duration, .. }
            | Self::Step { duration, .. }
            | Self::Jump { duration, .. } => *duration,
        }
    }
    pub(crate) fn position(&self, time: f32) -> Vec3 {
        let t = time.min(self.duration());
        match *self {
            Self::Walk {
                start,
                end,
                duration,
            } => start + (end - start).scaled(t / duration),
            Self::Jump {
                start,
                end,
                velocity,
                gravity,
                duration,
            } => {
                if t >= duration {
                    end
                } else {
                    start + velocity.scaled(t) - Vec3::new(0., 0., 0.5 * gravity * t * t)
                }
            }
            Self::Step {
                points,
                distance,
                duration,
            } => {
                let mut left = distance * t / duration;
                for pair in points.windows(2) {
                    let d = length(pair[1] - pair[0]);
                    if d > 0. && left < d {
                        return pair[0] + (pair[1] - pair[0]).scaled(left / d);
                    }
                    left -= d;
                }
                points[3]
            }
        }
    }
}
// Steps are a swept up/across/down envelope, not a teleport or a hidden ramp.
// Intermediate floor probes prevent using this controller to bridge empty gaps.
fn step_path(w: &dyn WorldView, p: Profile, a: Vec3, b: Vec3) -> Option<Path> {
    if p.step_height <= 0. || (b.z - a.z).abs() > p.step_height + 0.001 {
        return None;
    }
    let top = a.z.max(b.z) + 0.002;
    let points = [a, Vec3::new(a.x, a.y, top), Vec3::new(b.x, b.y, top), b];
    if !points.windows(2).all(|v| clear(w, p, v[0], v[1])) {
        return None;
    }
    let horizontal = (b.x - a.x).hypot(b.y - a.y);
    let count = (horizontal / 0.1).ceil().max(1.) as usize;
    for i in 0..=count {
        let mut at = a + (b - a).scaled(i as f32 / count as f32);
        at.z = top;
        // At the riser, body clearance belongs to the swept envelope above.
        let query = SurfaceQuery::new(at.x, at.y, top - p.step_height - 0.004, top + 0.003)
            .ok()?
            .excluding(p.actor);
        if !surface_candidates(w, query)
            .iter()
            .any(|s| s.normal.z > 0.9)
        {
            return None;
        }
    }
    let distance = points.windows(2).map(|v| length(v[1] - v[0])).sum::<f32>();
    Some(Path::Step {
        points,
        distance,
        duration: (distance / p.speed).max(0.001),
    })
}
fn jump_path(w: &dyn WorldView, p: Profile, a: Vec3, b: Vec3) -> Option<Path> {
    let horizontal = (b.x - a.x).hypot(b.y - a.y);
    if p.jump_distance == 0. || horizontal > p.jump_distance || a.z - b.z > p.max_drop {
        return None;
    }
    let discriminant = p.jump_speed * p.jump_speed - 2. * p.gravity * (b.z - a.z);
    if discriminant <= 0. {
        return None;
    }
    let duration = (p.jump_speed + discriminant.sqrt()) / p.gravity;
    if duration > 2. || horizontal / duration > p.speed {
        return None;
    }
    let velocity = Vec3::new((b.x - a.x) / duration, (b.y - a.y) / duration, p.jump_speed);
    let path = Path::Jump {
        start: a,
        end: b,
        velocity,
        gravity: p.gravity,
        duration,
    };
    // Continuous body sweeps between fine ballistic samples. Subdivide at execution
    // using the same bound; reject contact, including overhead and landing approach.
    let count = (duration / 0.01).ceil() as usize;
    for i in 0..count {
        if !clear(
            w,
            p,
            path.position(duration * i as f32 / count as f32),
            path.position(duration * (i + 1) as f32 / count as f32),
        ) {
            return None;
        }
    }
    Some(path)
}
pub(crate) fn path(
    w: &dyn WorldView,
    p: Profile,
    a: Vec3,
    b: Vec3,
    action: Action,
) -> Option<Path> {
    if !support(w, p, a) || !support(w, p, b) {
        return None;
    }
    match action {
        Action::Walk => {
            character_supported_segment(w, Some(p.actor), a, b, p.body).then_some(Path::Walk {
                start: a,
                end: b,
                duration: (length(b - a) / p.speed).max(0.001),
            })
        }
        Action::Step => step_path(w, p, a, b),
        Action::Jump => jump_path(w, p, a, b),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedMotion {
    pub path: Path,
    pub profile: Profile,
    pub elapsed: f32,
}
pub(crate) struct PlannedStep {
    pub movement: CharacterSweep,
    pub progress: Progress,
    pub vertical_speed: Option<f32>,
    pub jumped: bool,
    pub stepping: bool,
}
impl PlannedMotion {
    pub fn advance(
        &mut self,
        w: &dyn WorldView,
        start: Vec3,
        dt: f32,
    ) -> Result<PlannedStep, Error> {
        let mut end = start;
        let mut progress = Progress::Running;
        let duration = self.path.duration();
        let finish = (self.elapsed + dt).min(duration);
        let count = ((finish - self.elapsed) / 0.01).ceil().max(1.) as usize;
        if length(start - self.path.position(self.elapsed)) > 0.01 {
            progress = Progress::Blocked;
        } else {
            for i in 1..=count {
                let t = self.elapsed + (finish - self.elapsed) * i as f32 / count as f32;
                let desired = self.path.position(t);
                let next = match self.path {
                    Path::Walk { .. } => {
                        let walk = walk_character(
                            w,
                            Some(self.profile.actor),
                            end,
                            Vec3::new(desired.x - end.x, desired.y - end.y, 0.),
                            self.profile.body,
                        )
                        .map_err(|_| Error::InvalidWorld)?;
                        if !walk.reached {
                            progress = Progress::Blocked;
                            break;
                        }
                        walk.position
                    }
                    _ => desired,
                };
                if !clear(w, self.profile, end, next) {
                    progress = Progress::Blocked;
                    break;
                }
                end = next;
            }
        }
        let grounded = support(w, self.profile, end);
        if progress != Progress::Blocked && finish >= duration {
            progress = if grounded {
                Progress::Complete
            } else {
                Progress::Blocked
            };
        }
        let vertical_speed = match self.path {
            Path::Jump {
                velocity, gravity, ..
            } => Some(velocity.z - gravity * finish),
            _ => None,
        };
        let jumped = matches!(self.path, Path::Jump { .. }) && self.elapsed == 0.;
        self.elapsed = finish;
        Ok(PlannedStep {
            movement: CharacterSweep {
                position: end,
                grounded,
                hit_ceiling: false,
            },
            progress,
            vertical_speed,
            jumped,
            stepping: matches!(self.path, Path::Step { .. }),
        })
    }
}
