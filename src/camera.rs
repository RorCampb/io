#![forbid(unsafe_code)]
use io_types::{Bounds, Vec3};
use io_world::Space;
use std::f32::consts::{FRAC_PI_4, TAU};

#[derive(Clone, Debug)]
pub struct Camera {
    target: Vec3,
    render_distance: f32,
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
        if !bounds.within_radius(self.target, self.render_distance) {
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
        let f = forward.scaled(-1. / (self.render_distance * 2. + 1024.));
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
