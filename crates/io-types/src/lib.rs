#![forbid(unsafe_code)]
//! Shared geometry types. This crate has no world, asset, or renderer dependencies.

use std::ops::{Add, Sub};
mod message;
pub use message::{Envelope, MessageId};

/// Stable visual definition, resolved outside the world crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppearanceId(pub u32);

impl Default for AppearanceId {
    fn default() -> Self {
        Self(1)
    }
}

/// Index within an appearance; zero is its default state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VisualStateId(pub u32);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn scaled(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn dot(self, b: Self) -> f32 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }
    pub fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    pub fn cross(self, b: Self) -> Self {
        Self::new(
            self.y * b.z - self.z * b.y,
            self.z * b.x - self.x * b.z,
            self.x * b.y - self.y * b.x,
        )
    }
}

/// Unit quaternion. Checked construction prevents invalid orientation state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation([f32; 4]);
impl Default for Rotation {
    fn default() -> Self {
        Self([0., 0., 0., 1.])
    }
}
impl Rotation {
    pub fn from_xyzw(value: [f32; 4]) -> Result<Self, String> {
        let q = glam::Quat::from_array(value);
        let norm = q.length_squared();
        if !q.is_finite() || !norm.is_finite() || norm < 1e-12 {
            return Err("rotation must be a finite nonzero quaternion".into());
        }
        Ok(Self(q.normalize().to_array()))
    }
    pub fn yaw(radians: f32) -> Result<Self, String> {
        if !radians.is_finite() {
            return Err("yaw must be finite".into());
        }
        Self::from_xyzw(glam::Quat::from_rotation_z(radians).to_array())
    }
    pub fn xyzw(self) -> [f32; 4] {
        self.0
    }
    pub fn integrate(self, angular_velocity: Vec3, dt: f32) -> Result<Self, String> {
        let step = angular_velocity.scaled(dt);
        if !step.finite() {
            return Err("nonfinite angular step".into());
        }
        Self::from_xyzw(
            (glam::Quat::from_scaled_axis(glam::Vec3::new(step.x, step.y, step.z))
                * glam::Quat::from_array(self.0))
            .to_array(),
        )
    }
    pub fn angular_velocity_to(self, other: Self, dt: f32) -> Vec3 {
        let mut delta =
            glam::Quat::from_array(other.0) * glam::Quat::from_array(self.0).conjugate();
        if delta.w < 0. {
            delta = -delta;
        }
        let axis = delta.to_scaled_axis() / dt;
        Vec3::new(axis.x, axis.y, axis.z)
    }
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let p = glam::Quat::from_array(self.0) * glam::Vec3::new(v.x, v.y, v.z);
        Vec3::new(p.x, p.y, p.z)
    }
    pub fn inverse_rotate(self, v: Vec3) -> Vec3 {
        Self(glam::Quat::from_array(self.0).conjugate().to_array()).rotate(v)
    }
    pub fn interpolate(self, other: Self, alpha: f32) -> Self {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0., 1.)
        } else {
            1.
        };
        Self(
            glam::Quat::from_array(self.0)
                .slerp(glam::Quat::from_array(other.0), alpha)
                .normalize()
                .to_array(),
        )
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub fn valid(self) -> bool {
        self.min.finite()
            && self.max.finite()
            && self.extent().finite()
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
            && self.min.z <= self.max.z
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Vec3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Vec3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }
    pub fn center(self) -> Vec3 {
        self.min.scaled(0.5) + self.max.scaled(0.5)
    }
    pub fn extent(self) -> Vec3 {
        self.max.scaled(0.5) - self.min.scaled(0.5)
    }
    pub fn within_radius(self, point: Vec3, radius: f32) -> bool {
        if !self.valid() || !point.finite() || !radius.is_finite() || radius < 0. {
            return false;
        }
        let nearest = Vec3::new(
            point.x.clamp(self.min.x, self.max.x),
            point.y.clamp(self.min.y, self.max.y),
            point.z.clamp(self.min.z, self.max.z),
        );
        let d = nearest - point;
        d.dot(d) <= radius * radius
    }
    pub fn projected_radius(self, axis: Vec3) -> f32 {
        let e = self.extent();
        e.x * axis.x.abs() + e.y * axis.y.abs() + e.z * axis.z.abs()
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    #[test]
    fn quaternion_construction_rejects_invalid_and_normalizes_valid_input() {
        for value in [[0.; 4], [f32::NAN, 0., 0., 1.], [f32::MAX; 4]] {
            assert!(Rotation::from_xyzw(value).is_err());
        }
        assert_eq!(
            Rotation::from_xyzw([0., 0., 0., 2.]).unwrap(),
            Rotation::default()
        );
        let q = Rotation::yaw(1.).unwrap();
        let v = Vec3::new(1., 2., 3.);
        let error = q.inverse_rotate(q.rotate(v)) - v;
        assert!(error.dot(error) < 1e-10);
    }
    #[test]
    fn interpolation_uses_short_arc_and_handles_equivalent_quaternions() {
        let a = Rotation::yaw(170_f32.to_radians()).unwrap();
        let b = Rotation::yaw(-170_f32.to_radians()).unwrap();
        let mid = a.interpolate(b, 0.5).rotate(Vec3::new(1., 0., 0.));
        assert!(mid.x < -0.999 && mid.y.abs() < 0.001);
        let q = Rotation::from_xyzw(a.xyzw().map(|v| -v)).unwrap();
        let error =
            a.interpolate(q, 0.5).rotate(Vec3::new(1., 0., 0.)) - a.rotate(Vec3::new(1., 0., 0.));
        assert!(error.dot(error) < 1e-10);
    }
}
