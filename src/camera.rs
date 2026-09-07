use crate::space::Vec2;
use std::f32::consts::{FRAC_PI_4, TAU};

#[derive(Clone, Debug)]
pub struct Camera {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    width: u32,
    height: u32,
    axis_a: Vec2,
    axis_b: Vec2,
    axis_c: Vec2,
}

impl Camera {
    pub fn new() -> Self {
        let mut camera = Self {
            yaw: FRAC_PI_4,
            pitch: (1.0_f32 / 3.0_f32.sqrt()).asin(),
            zoom: 1.0,
            width: 1280,
            height: 800,
            axis_a: Vec2::new(0.0, 0.0),
            axis_b: Vec2::new(0.0, 0.0),
            axis_c: Vec2::new(0.0, 0.0),
        };
        camera.update_axes();
        camera
    }

    pub fn orbit(&mut self, yaw_delta: f32, pitch_delta: f32) -> bool {
        if !yaw_delta.is_finite() || !pitch_delta.is_finite() {
            return false;
        }
        self.yaw = (self.yaw + yaw_delta.rem_euclid(TAU)).rem_euclid(TAU);
        self.pitch = (self.pitch + pitch_delta).clamp(5.0_f32.to_radians(), 85.0_f32.to_radians());
        self.update_axes();
        true
    }

    pub fn zoom_by(&mut self, steps: f32) -> bool {
        if !steps.is_finite() {
            return false;
        }
        self.zoom = (self.zoom * (steps.clamp(-100.0, 100.0) * 0.12).exp()).clamp(0.1, 10.0);
        self.update_axes();
        true
    }

    pub fn set_viewport(&mut self, width: i32, height: i32) -> bool {
        if width <= 0 || height <= 0 || (self.width == width as u32 && self.height == height as u32)
        {
            return false;
        }
        self.width = width as u32;
        self.height = height as u32;
        self.update_axes();
        true
    }

    pub fn reset(&mut self) {
        let (width, height) = (self.width, self.height);
        *self = Self::new();
        self.width = width;
        self.height = height;
        self.update_axes();
    }

    pub fn project(&self, a: f32, b: f32, c: f32) -> Vec2 {
        // Orbit and zoom around the normalized volume's center, not an axis endpoint.
        Vec2::new(self.width as f32 * 0.5, self.height as f32 * 0.5)
            .add(self.axis_a.scaled(a - 0.5))
            .add(self.axis_b.scaled(b - 0.5))
            .add(self.axis_c.scaled(c - 0.5))
    }

    fn update_axes(&mut self) {
        let fit = (self.width as f32 / 1280.0).min(self.height as f32 / 800.0);
        let scale = 400.0 * fit * self.zoom;
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        // Orthographic basis: A/B span the horizontal plane; C points upward.
        self.axis_a = Vec2::new(cos_yaw, sin_yaw * sin_pitch).scaled(scale);
        self.axis_b = Vec2::new(-sin_yaw, cos_yaw * sin_pitch).scaled(scale);
        self.axis_c = Vec2::new(0.0, -cos_pitch).scaled(scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: Vec2, expected: Vec2) {
        assert!(
            (actual.x - expected.x).abs() < 0.001,
            "{actual:?} != {expected:?}"
        );
        assert!(
            (actual.y - expected.y).abs() < 0.001,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn orbit_and_zoom_keep_volume_center_fixed() {
        let mut camera = Camera::new();
        camera.orbit(1.2, 0.3);
        camera.zoom_by(4.0);
        near(camera.project(0.5, 0.5, 0.5), Vec2::new(640.0, 400.0));
        camera.set_viewport(800, 600);
        near(camera.project(0.5, 0.5, 0.5), Vec2::new(400.0, 300.0));
    }

    #[test]
    fn zoom_scales_offsets_and_is_reversible() {
        let mut camera = Camera::new();
        let before = camera.project(0.2, 0.7, 0.9);
        camera.zoom_by(2.0_f32.ln() / 0.12);
        near(
            camera.project(0.2, 0.7, 0.9),
            Vec2::new(2.0 * before.x - 640.0, 2.0 * before.y - 400.0),
        );
        camera.zoom_by(-2.0_f32.ln() / 0.12);
        near(camera.project(0.2, 0.7, 0.9), before);
    }

    #[test]
    fn orbit_reprojects_three_dimensions_and_wraps() {
        let mut camera = Camera::new();
        let before = camera.project(1.0, 0.5, 0.5);
        camera.orbit(TAU, 0.0);
        near(camera.project(1.0, 0.5, 0.5), before);
        camera.orbit(std::f32::consts::PI, 0.0);
        near(
            camera.project(1.0, 0.5, 0.5),
            Vec2::new(1280.0 - before.x, 800.0 - before.y),
        );
        let bottom = camera.project(0.5, 0.5, 0.0);
        let top = camera.project(0.5, 0.5, 1.0);
        assert_eq!(bottom.x, top.x);
        assert!(top.y < bottom.y);
    }

    #[test]
    fn projection_preserves_grid_intersections() {
        let mut camera = Camera::new();
        camera.orbit(-0.8, 0.2);
        camera.zoom_by(3.0);
        let start = camera.project(0.0, 0.25, 0.75);
        let end = camera.project(1.0, 0.25, 0.75);
        near(camera.project(0.5, 0.25, 0.75), start.add(end).scaled(0.5));
    }

    #[test]
    fn limits_and_invalid_input_preserve_a_usable_camera() {
        let mut camera = Camera::new();
        let before = camera.project(0.0, 0.0, 0.0);
        assert!(!camera.orbit(f32::NAN, 0.0));
        assert!(!camera.zoom_by(f32::INFINITY));
        assert!(!camera.set_viewport(0, 0));
        near(camera.project(0.0, 0.0, 0.0), before);
        camera.orbit(f32::MAX, f32::MAX);
        camera.zoom_by(f32::MAX);
        assert_eq!(camera.zoom, 10.0);
        assert!(camera.pitch < std::f32::consts::FRAC_PI_2);
        camera.zoom_by(-f32::MAX);
        assert_eq!(camera.zoom, 0.1);
        assert!(camera.project(1.0, 0.0, 1.0).x.is_finite());
        camera.set_viewport(900, 700);
        camera.reset();
        assert_eq!(camera.zoom, 1.0);
        near(camera.project(0.5, 0.5, 0.5), Vec2::new(450.0, 350.0));
    }
}
