#![forbid(unsafe_code)]
use io_types::{Bounds, Vec3};
use io_world::Space;
use std::f32::consts::{FRAC_PI_4, TAU};

/// Viewport coverage covers a world-space height band without expanding simulation.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderCoverage {
    #[default]
    Radius,
    Viewport {
        min_z: f32,
        max_z: f32,
    },
}
impl RenderCoverage {
    pub fn valid(self) -> bool {
        match self {
            Self::Radius => true,
            Self::Viewport { min_z, max_z } => {
                min_z.is_finite()
                    && max_z.is_finite()
                    && min_z.abs() <= 100_000.
                    && max_z.abs() <= 100_000.
                    && min_z <= max_z
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Camera {
    target: Vec3,
    render_distance: f32,
    coverage: RenderCoverage,
    zoom: f32,
    yaw: f32,
    pitch: f32,
    width: i32,
    height: i32,
}

impl Camera {
    pub fn target(&self) -> Vec3 {
        self.target
    }
    pub fn render_distance(&self) -> f32 {
        self.render_distance
    }
    pub fn set_coverage(&mut self, coverage: RenderCoverage) -> bool {
        if !coverage.valid() {
            return false;
        }
        self.coverage = coverage;
        true
    }
    pub fn render_radius(&self) -> f32 {
        match self.coverage {
            RenderCoverage::Radius => self.render_distance,
            RenderCoverage::Viewport { min_z, max_z } => {
                let (hw, hh) = self.half_view();
                let dz = (min_z - self.target.z)
                    .abs()
                    .max((max_z - self.target.z).abs());
                let (sp, cp) = self.pitch.sin_cos();
                // Screen-up = ground-up * sin(pitch) + height * cos(pitch).
                let ground_half = (hh + dz * cp) / sp;
                self.render_distance.max(hw.hypot(ground_half).hypot(dz))
            }
        }
    }
    pub fn set_target(&mut self, target: Vec3) -> bool {
        if !target.finite() {
            return false;
        }
        self.target = target;
        true
    }
    pub fn set_zoom(&mut self, zoom: f32) -> bool {
        if !zoom.is_finite() || !(0.025..=16.).contains(&zoom) {
            return false;
        }
        self.zoom = zoom;
        true
    }
    pub fn copy_viewport(&mut self, other: &Self) {
        self.width = other.width;
        self.height = other.height;
    }
    pub fn new(target: Vec3) -> Self {
        assert!(target.finite(), "camera target must be finite");
        Self {
            target,
            render_distance: 120.,
            coverage: RenderCoverage::Radius,
            zoom: 1.,
            yaw: FRAC_PI_4,
            pitch: (1.0_f32 / 3.0_f32.sqrt()).asin(),
            width: 1280,
            height: 800,
        }
    }
    pub fn orbit(&mut self, yaw: f32, pitch: f32) -> bool {
        if !yaw.is_finite() || !pitch.is_finite() {
            return false;
        }
        self.yaw = (self.yaw + yaw.rem_euclid(TAU)).rem_euclid(TAU);
        self.pitch = (self.pitch + pitch).clamp(5.0_f32.to_radians(), 85.0_f32.to_radians());
        true
    }
    pub fn zoom_by(&mut self, steps: f32) -> bool {
        if !steps.is_finite() {
            return false;
        }
        self.zoom = (self.zoom * (steps.clamp(-100., 100.) * 0.12).exp()).clamp(0.025, 16.);
        true
    }
    pub fn set_distance(&mut self, distance: f32) -> bool {
        if !distance.is_finite() || distance <= 0. {
            return false;
        }
        self.render_distance = distance.clamp(8., 20000.);
        true
    }
    pub fn set_viewport(&mut self, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 || (w == self.width && h == self.height) {
            return false;
        }
        self.width = w;
        self.height = h;
        true
    }
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (s, c) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        (
            Vec3::new(c, -s, 0.),
            Vec3::new(-s * sp, -c * sp, cp),
            Vec3::new(s * cp, c * cp, sp),
        )
    }
    pub fn pixels_per_unit(&self) -> f32 {
        16. * self.zoom
    }
    /// Unit-speed ground motion aligned with screen right/up, independent of zoom.
    pub fn ground_direction(&self, x: f32, y: f32) -> Option<Vec3> {
        if !x.is_finite() || !y.is_finite() || x.abs() > 1. || y.abs() > 1. {
            return None;
        }
        let (right, _, _) = self.basis();
        let up = Vec3::new(-self.yaw.sin(), -self.yaw.cos(), 0.);
        let direction = right.scaled(x) + up.scaled(y);
        Some(direction.scaled(1. / (x * x + y * y).sqrt().max(1.)))
    }
    pub fn project(&self, point: Vec3) -> Option<(f32, f32)> {
        if !point.finite() {
            return None;
        }
        let (right, up, _) = self.basis();
        let delta = point - self.target;
        Some((
            self.width as f32 * 0.5 + delta.dot(right) * self.pixels_per_unit(),
            self.height as f32 * 0.5 - delta.dot(up) * self.pixels_per_unit(),
        ))
    }
    /// Orthographic ray in logical window pixels. Increasing depth points away from the camera.
    pub fn pick_depth(&self, x: f32, y: f32, bounds: Bounds) -> Option<f32> {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.
            || y < 0.
            || x >= self.width as f32
            || y >= self.height as f32
            || !self.sees(bounds)
        {
            return None;
        }
        let (right, up, forward) = self.basis();
        let depth = self.render_radius() * 2. + 1024.;
        let origin = self.target
            + right.scaled((x - self.width as f32 * 0.5) / self.pixels_per_unit())
            + up.scaled((self.height as f32 * 0.5 - y) / self.pixels_per_unit())
            + forward.scaled(depth);
        let direction = forward.scaled(-1.);
        let mut near = 0_f32;
        let mut far = depth * 2.;
        for (o, d, min, max) in [
            (origin.x, direction.x, bounds.min.x, bounds.max.x),
            (origin.y, direction.y, bounds.min.y, bounds.max.y),
            (origin.z, direction.z, bounds.min.z, bounds.max.z),
        ] {
            if d.abs() < 1e-8 {
                if o < min || o > max {
                    return None;
                }
            } else {
                let a = (min - o) / d;
                let b = (max - o) / d;
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            if near > far {
                return None;
            }
        }
        Some(near)
    }
    /// Orthographic projection of a conservative bounding sphere in logical
    /// window pixels. Stable across animation, item yaw, and camera translation.
    pub fn projected_diameter(&self, bounds: Bounds, scale: Vec3) -> f32 {
        let extent = bounds.extent();
        let scaled = Vec3::new(extent.x * scale.x, extent.y * scale.y, extent.z * scale.z);
        2. * scaled.dot(scaled).sqrt() * self.pixels_per_unit()
    }
    pub fn half_view(&self) -> (f32, f32) {
        (
            self.width as f32 / (2. * self.pixels_per_unit()),
            self.height as f32 / (2. * self.pixels_per_unit()),
        )
    }
    pub fn pan(&mut self, dx: f32, dy: f32, space: &Space) -> bool {
        if !dx.is_finite() || !dy.is_finite() {
            return false;
        }
        let (right, _, _) = self.basis();
        let ground_up = Vec3::new(-self.yaw.sin(), -self.yaw.cos(), 0.);
        let delta = right.scaled(-dx / self.pixels_per_unit())
            + ground_up.scaled(dy / (self.pixels_per_unit() * self.pitch.sin()));
        let target = self.target + delta;
        if !target.finite() {
            return false;
        }
        self.set_target(space.clamp_target(target))
    }
    pub fn sees(&self, bounds: Bounds) -> bool {
        if !bounds.within_radius(self.target, self.render_radius()) {
            return false;
        }
        let (right, up, _) = self.basis();
        let delta = bounds.center() - self.target;
        let (hw, hh) = self.half_view();
        delta.dot(right).abs() <= hw + bounds.projected_radius(right)
            && delta.dot(up).abs() <= hh + bounds.projected_radius(up)
    }
    pub fn clip_from_world(&self) -> [f32; 16] {
        let (right, up, forward) = self.basis();
        let (hw, hh) = self.half_view();
        let r = right.scaled(1. / hw);
        let u = up.scaled(1. / hh);
        // Nearer points along the camera-facing axis get smaller OpenGL depth.
        let f = forward.scaled(-1. / (self.render_radius() * 2. + 1024.));
        [
            r.x,
            u.x,
            f.x,
            0.,
            r.y,
            u.y,
            f.y,
            0.,
            r.z,
            u.z,
            f.z,
            0.,
            -r.dot(self.target),
            -u.dot(self.target),
            -f.dot(self.target),
            1.,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewport_coverage_contains_height_band_corners_at_any_orbit_and_zoom() {
        let mut c = Camera::new(Vec3::new(-280., -180., 25.));
        c.set_distance(180.);
        assert!(!c.set_coverage(RenderCoverage::Viewport {
            min_z: 10.,
            max_z: 0.
        }));
        assert!(!c.set_coverage(RenderCoverage::Viewport {
            min_z: f32::NAN,
            max_z: 0.
        }));
        assert!(serde_json::from_str::<RenderCoverage>(
            r#"{"type":"viewport","min_z":0,"max_z":100,"extra":1}"#
        )
        .is_err());
        assert!(c.set_coverage(RenderCoverage::Viewport {
            min_z: -10.,
            max_z: 100.
        }));
        for pitch in [5_f32, 35., 85.] {
            c.pitch = pitch.to_radians();
            for zoom in [0.025, 0.1, 0.65, 16.] {
                c.set_zoom(zoom);
                for (w, h) in [(1280, 800), (800, 1280), (2400, 600)] {
                    c.set_viewport(w, h);
                    c.orbit(0.7, 0.);
                    let (right, _, _) = c.basis();
                    let ground_up = Vec3::new(-c.yaw.sin(), -c.yaw.cos(), 0.);
                    let (hw, hh) = c.half_view();
                    for z in [-10., 100.] {
                        for x in [-hw, hw] {
                            for y in [-hh, hh] {
                                let dz = z - c.target.z;
                                let p = c.target
                                    + right.scaled(x)
                                    + ground_up.scaled((y - dz * c.pitch.cos()) / c.pitch.sin())
                                    + Vec3::new(0., 0., dz);
                                let bounds = Bounds {
                                    min: p - Vec3::new(0.1, 0.1, 0.1),
                                    max: p + Vec3::new(0.1, 0.1, 0.1),
                                };
                                assert!(c.sees(bounds), "missing corner at {pitch}/{zoom}/{w}/{h}");
                            }
                        }
                    }
                    assert_eq!(c.render_distance(), 180., "simulation radius must not grow");
                }
            }
        }
    }
    #[test]
    fn movement_and_picking_follow_orbit_zoom_and_logical_viewport() {
        let mut camera = Camera::new(Vec3::new(3., 4., 0.));
        for yaw in [0., 0.7, 1.2, 3.] {
            camera.orbit(yaw, 0.1);
            let (right, up, _) = camera.basis();
            let w = camera.ground_direction(0., 1.).unwrap();
            let d = camera.ground_direction(1., 0.).unwrap();
            assert!(w.dot(up) > 0. && w.dot(right).abs() < 1e-5);
            assert!(d.dot(right) > 0.99 && d.dot(up).abs() < 1e-5);
            let diagonal = camera.ground_direction(1., 1.).unwrap();
            assert!((diagonal.dot(diagonal) - 1.).abs() < 1e-5);
            for (width, height, zoom) in [(1280, 800, 2.), (640, 400, 4.)] {
                camera.set_viewport(width, height);
                camera.set_zoom(zoom);
                let bounds = Bounds {
                    min: Vec3::new(3., 4., 0.),
                    max: Vec3::new(4., 5., 2.),
                };
                let (x, y) = camera.project(bounds.center()).unwrap();
                assert!(camera.pick_depth(x, y, bounds).is_some());
                assert!(camera.pick_depth(-1., y, bounds).is_none());
                assert!(camera.pick_depth(f32::NAN, y, bounds).is_none());
            }
        }
        assert!(camera.ground_direction(f32::NAN, 0.).is_none());
    }
    #[test]
    fn camera_matrix_centers_target_and_zoom_scales_view() {
        let mut c = Camera::new(Vec3::new(5000., 5000., 0.));
        c.orbit(0.7, 0.2);
        let m = c.clip_from_world();
        for row in 0..3 {
            let v = m[row] * c.target.x
                + m[4 + row] * c.target.y
                + m[8 + row] * c.target.z
                + m[12 + row];
            assert!(v.abs() < 0.001);
        }
        let before = c.half_view().0;
        c.zoom_by(2.0_f32.ln() / 0.12);
        assert!((c.half_view().0 - before * 0.5).abs() < 0.001);
        assert!(!c.orbit(f32::NAN, 0.));
        assert!(!c.zoom_by(f32::INFINITY));
        assert!(!c.set_distance(-1.));
    }
    #[test]
    fn overflowing_pan_preserves_camera_state() {
        let mut camera = Camera::new(Vec3::new(10., 10., 0.));
        camera.set_zoom(0.025);
        let target = camera.target();
        let space = Space::new(Vec3::new(100., 100., 100.));
        assert!(!camera.pan(f32::MAX, f32::MAX, &space));
        assert_eq!(camera.target(), target);
        assert!(camera.clip_from_world().iter().all(|v| v.is_finite()));
    }
    #[test]
    fn bounds_overlapping_view_or_distance_are_not_lost() {
        let c = Camera::new(Vec3::new(0., 0., 0.));
        let (right, _, _) = c.basis();
        let p = right.scaled(c.half_view().0 + 1.);
        let b = Bounds {
            min: p - Vec3::new(4., 4., 4.),
            max: p + Vec3::new(4., 4., 4.),
        };
        assert!(c.sees(b));
        let far = Bounds {
            min: Vec3::new(500., 500., 0.),
            max: Vec3::new(501., 501., 1.),
        };
        assert!(!c.sees(far));
    }
}
