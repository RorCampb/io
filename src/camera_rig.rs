#![forbid(unsafe_code)]
#[cfg(test)]
#[path = "camera_rig_tests.rs"]
mod tests;
use io_scene::{CameraRig, CameraRigs};
use io_types::Vec3;
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub region: Option<usize>,
    pub weight: f32,
}

#[derive(Clone, Debug)]
enum Override<T> {
    Authored,
    Manual(T),
    Returning { from: T, weight: f32 },
}
impl<T: Copy> Override<T> {
    fn sample(&self) -> Option<(T, f32)> {
        match *self {
            Self::Authored => None,
            Self::Manual(v) => Some((v, 1.)),
            Self::Returning { from, weight } => Some((from, weight)),
        }
    }
    fn release(&mut self, from: T) {
        if !matches!(self, Self::Authored) {
            *self = Self::Returning { from, weight: 1. };
        }
    }
    fn advance(&mut self, alpha: f32) {
        if let Self::Returning { weight, .. } = self {
            *weight *= 1. - alpha;
            if *weight < 1e-4 {
                *self = Self::Authored;
            }
        }
    }
}
#[derive(Clone, Debug)]
pub struct RigController {
    base: CameraRig,
    orientation: Override<(f32, f32)>,
    zoom: Override<f32>,
    region: Option<usize>,
    orbit_input: bool,
    zoom_input: bool,
}
fn angle_delta(a: f32, b: f32) -> f32 {
    (a - b + 180.).rem_euclid(360.) - 180.
}
impl RigController {
    pub fn held(rig: &CameraRig) -> Self {
        let mut result = Self::new(rig);
        result.orientation = Override::Manual((rig.yaw_degrees, rig.pitch_degrees));
        result.zoom = Override::Manual(rig.zoom);
        result
    }
    pub fn new(rig: &CameraRig) -> Self {
        Self {
            base: rig.clone(),
            orientation: Override::Authored,
            zoom: Override::Authored,
            region: None,
            orbit_input: false,
            zoom_input: false,
        }
    }
    pub fn orbit(&mut self, yaw: f32, pitch: f32) {
        if yaw == 0. && pitch == 0. {
            return;
        }
        let p = self.pose();
        let yaw = self.base.yaw_degrees
            + angle_delta(
                p.yaw_degrees + yaw.rem_euclid(std::f32::consts::TAU).to_degrees(),
                self.base.yaw_degrees,
            )
            .clamp(-self.base.yaw_limit_degrees, self.base.yaw_limit_degrees);
        self.orientation =
            Override::Manual((yaw, (p.pitch_degrees + pitch.to_degrees()).clamp(2., 85.)));
        self.orbit_input = true;
    }
    pub fn zoom(&mut self, steps: f32) {
        if steps == 0. {
            return;
        }
        self.zoom = Override::Manual(
            (self.pose().zoom * (steps.clamp(-100., 100.) * 0.12).exp())
                .clamp(self.base.min_zoom, self.base.max_zoom),
        );
        self.zoom_input = true;
    }
    pub fn pose(&self) -> CameraRig {
        let mut p = self.base.clone();
        if let Some(((yaw, pitch), weight)) = self.orientation.sample() {
            p.yaw_degrees += angle_delta(yaw, p.yaw_degrees)
                .clamp(-p.yaw_limit_degrees, p.yaw_limit_degrees)
                * weight;
            p.pitch_degrees += (pitch - p.pitch_degrees) * weight;
        }
        if let Some((zoom, weight)) = self.zoom.sample() {
            p.zoom = (p.zoom.ln() + (zoom.ln() - p.zoom.ln()) * weight).exp();
        }
        p.zoom = p.zoom.clamp(p.min_zoom, p.max_zoom);
        p
    }
    pub fn update(
        &mut self,
        rigs: &CameraRigs,
        regions: &[io_world::Interior],
        point: Option<Vec3>,
        dt: f32,
    ) {
        self.update_selected(rigs, regions, point, dt, None);
    }
    pub fn update_selected(
        &mut self,
        rigs: &CameraRigs,
        regions: &[io_world::Interior],
        point: Option<Vec3>,
        dt: f32,
        selection: Option<Selection>,
    ) {
        let weight = |i: usize| {
            if let Some(s) = selection {
                return if s.region == Some(i) { s.weight } else { 0. };
            }
            point
                .and_then(|p| {
                    regions.get(i).and_then(|v| {
                        rigs.interiors
                            .get(&v.name)
                            .map(|r| v.proximity(p, r.approach))
                    })
                })
                .unwrap_or(0.)
        };
        let candidate = (0..regions.len())
            .map(|i| (i, weight(i)))
            .filter(|(_, t)| *t > 0.05)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let inside = |i: usize| {
            point.is_some_and(|p| {
                let b = regions[i].bounds;
                p.x >= b.min.x
                    && p.x <= b.max.x
                    && p.y >= b.min.y
                    && p.y <= b.max.y
                    && p.z >= b.min.z - 0.2
                    && p.z <= b.max.z
            })
        };
        // Hysteresis prevents adjoining volumes from repeatedly releasing player control.
        let selected = if let Some(s) = selection {
            s.region
        } else {
            match (self.region, candidate) {
                (Some(old), Some((new, t)))
                    if old != new
                        && weight(old) > 0.05
                        && t < weight(old) + 0.1
                        && (matches!(rigs.motion, io_scene::RigMotion::Authored)
                            || inside(old)
                            || !inside(new)) =>
                {
                    Some(old)
                }
                (_, Some((i, _))) => Some(i),
                _ => None,
            }
        };
        if selected != self.region {
            let p = self.pose();
            if matches!(rigs.motion, io_scene::RigMotion::Authored) && !self.orbit_input {
                self.orientation.release((p.yaw_degrees, p.pitch_degrees));
            }
            if matches!(rigs.motion, io_scene::RigMotion::Authored) && !self.zoom_input {
                self.zoom.release(p.zoom);
            }
            self.region = selected;
        }
        self.orbit_input = false;
        self.zoom_input = false;
        let mut goal = rigs.exterior.clone();
        if let Some(i) = selected {
            let rig = &rigs.interiors[&regions[i].name];
            let t = weight(i);
            if matches!(rigs.motion, io_scene::RigMotion::Authored) {
                goal.yaw_degrees += angle_delta(rig.yaw_degrees, goal.yaw_degrees) * t;
                goal.pitch_degrees += (rig.pitch_degrees - goal.pitch_degrees) * t;
                goal.zoom = (goal.zoom.ln() + (rig.zoom.ln() - goal.zoom.ln()) * t).exp();
                goal.yaw_limit_degrees += (rig.yaw_limit_degrees - goal.yaw_limit_degrees) * t;
                goal.min_zoom += (rig.min_zoom - goal.min_zoom) * t;
                goal.max_zoom += (rig.max_zoom - goal.max_zoom) * t;
            }
            goal.fov_degrees += (rig.fov_degrees - goal.fov_degrees) * t;
            goal.target_height += (rig.target_height - goal.target_height) * t;
            goal.response_seconds += (rig.response_seconds - goal.response_seconds) * t;
        }
        let a = -(-dt / goal.response_seconds).exp_m1();
        self.base.yaw_degrees += angle_delta(goal.yaw_degrees, self.base.yaw_degrees) * a;
        self.base.pitch_degrees += (goal.pitch_degrees - self.base.pitch_degrees) * a;
        self.base.zoom = (self.base.zoom.ln() + (goal.zoom.ln() - self.base.zoom.ln()) * a).exp();
        self.base.fov_degrees += (goal.fov_degrees - self.base.fov_degrees) * a;
        self.base.target_height += (goal.target_height - self.base.target_height) * a;
        self.base.yaw_limit_degrees += (goal.yaw_limit_degrees - self.base.yaw_limit_degrees) * a;
        self.base.min_zoom += (goal.min_zoom - self.base.min_zoom) * a;
        self.base.max_zoom += (goal.max_zoom - self.base.max_zoom) * a;
        self.base.response_seconds = goal.response_seconds;
        self.orientation.advance(a);
        self.zoom.advance(a);
    }
    pub fn region(&self) -> Option<usize> {
        self.region
    }
}
