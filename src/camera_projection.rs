#![forbid(unsafe_code)]
use io_types::{Bounds, Vec3};

/// One homogeneous view shared by rasterization, selection, culling and LOD.
/// In camera coordinates w = 1 - convergence * z, so the target plane keeps
/// its scale while parallel rays continuously converge toward a finite eye.
pub(crate) struct CameraProjection {
    pub target: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    pub half_width: f32,
    pub half_height: f32,
    pub width: f32,
    pub height: f32,
    pub convergence: f32,
    pub front: f32,
    pub back: f32,
    pub radius: f32,
}

impl CameraProjection {
    fn weight(&self, z: f32) -> f32 {
        1. - self.convergence * z
    }
    fn depth_coefficients(&self) -> (f32, f32) {
        let (front, back, k) = (
            f64::from(self.front),
            f64::from(self.back),
            f64::from(self.convergence),
        );
        let span = front - back;
        // Evaluate directly, rather than subtracting nearly equal far-depth terms.
        let a = -(2. - k * (front + back)) / span;
        let b = (front + back - 2. * k * front * back) / span;
        (a as f32, b as f32)
    }
    pub fn matrix(&self) -> [f32; 16] {
        let r = self.right.scaled(1. / self.half_width);
        let u = self.up.scaled(1. / self.half_height);
        let (a, b) = self.depth_coefficients();
        let f = self.forward.scaled(a);
        let w = self.forward.scaled(-self.convergence);
        [
            r.x,
            u.x,
            f.x,
            w.x,
            r.y,
            u.y,
            f.y,
            w.y,
            r.z,
            u.z,
            f.z,
            w.z,
            -r.dot(self.target),
            -u.dot(self.target),
            b - f.dot(self.target),
            1. - w.dot(self.target),
        ]
    }
    pub fn project(&self, point: Vec3) -> Option<(f32, f32)> {
        if !point.finite() {
            return None;
        }
        let d = point - self.target;
        let z = d.dot(self.forward);
        let w = self.weight(z);
        if z < self.back || z > self.front || w <= 0. {
            return None;
        }
        let x = self.width * 0.5 * (1. + d.dot(self.right) / (self.half_width * w));
        let y = self.height * 0.5 * (1. - d.dot(self.up) / (self.half_height * w));
        (x.is_finite() && y.is_finite()).then_some((x, y))
    }
    fn point_on_ray(&self, x: f32, y: f32, z: f32) -> Vec3 {
        self.target
            + self.forward.scaled(z)
            + (self.right.scaled(x) + self.up.scaled(y)).scaled(self.weight(z))
    }
    pub fn pick_depth(&self, x: f32, y: f32, bounds: Bounds) -> Option<f32> {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.
            || y < 0.
            || x >= self.width
            || y >= self.height
            || !self.sees(bounds)
        {
            return None;
        }
        let x = (2. * x / self.width - 1.) * self.half_width;
        let y = (1. - 2. * y / self.height) * self.half_height;
        let origin = self.point_on_ray(x, y, self.front);
        let direction = self.point_on_ray(x, y, self.back) - origin;
        let mut near = 0_f32;
        let mut far = 1_f32;
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
        Some(near * direction.dot(direction).sqrt())
    }
    pub fn sees(&self, bounds: Bounds) -> bool {
        if !bounds.within_radius(self.target, self.radius) {
            return false;
        }
        let d = bounds.center() - self.target;
        let w_axis = self.forward.scaled(-self.convergence);
        let x_axis = self.right.scaled(1. / self.half_width);
        let y_axis = self.up.scaled(1. / self.half_height);
        // Test support points against the same six planes used by GL clipping.
        [
            (w_axis + x_axis, 1.),
            (w_axis - x_axis, 1.),
            (w_axis + y_axis, 1.),
            (w_axis - y_axis, 1.),
            (self.forward, -self.back),
            (self.forward.scaled(-1.), self.front),
        ]
        .iter()
        .all(|&(axis, offset)| d.dot(axis) + offset + bounds.projected_radius(axis) >= -1e-4)
    }
    pub fn projected_diameter(&self, bounds: Bounds, scale: Vec3, center: Vec3) -> f32 {
        let e = bounds.extent();
        let e = Vec3::new(e.x * scale.x, e.y * scale.y, e.z * scale.z);
        let radius = e.dot(e).sqrt();
        let d = center - self.target;
        let w = self.weight(d.dot(self.forward));
        let nearest_w = w - self.convergence * radius;
        if nearest_w <= 0. {
            return f32::MAX;
        }
        // Bound projected size even off-axis; a near-plane crossing stays high LOD.
        let off_axis = d.dot(self.right).abs().max(d.dot(self.up).abs());
        let stretch = 1. + self.convergence * off_axis / w;
        (2. * radius * (self.height / (2. * self.half_height)) * stretch / nearest_w).min(f32::MAX)
    }
    pub fn cover_height_band(&mut self, min_z: f32, max_z: f32) {
        let mut corners = [Vec3::default(); 8];
        for (i, corner) in corners.iter_mut().enumerate() {
            *corner = self.point_on_ray(
                if i & 1 == 0 {
                    -self.half_width
                } else {
                    self.half_width
                },
                if i & 2 == 0 {
                    -self.half_height
                } else {
                    self.half_height
                },
                if i & 4 == 0 { self.back } else { self.front },
            );
        }
        let mut radius_squared = self.radius * self.radius;
        let mut include = |p: Vec3| {
            let d = p - self.target;
            radius_squared = radius_squared.max(d.dot(d));
        };
        // Clipping a convex frustum by a horizontal slab leaves original vertices
        // and intersections of its twelve edges with the two slab planes.
        for (i, &a) in corners.iter().enumerate() {
            if (min_z..=max_z).contains(&a.z) {
                include(a);
            }
            for bit in [1, 2, 4] {
                if i & bit != 0 {
                    continue;
                }
                let b = corners[i | bit];
                if (b.z - a.z).abs() < 1e-8 {
                    continue;
                }
                for z in [min_z, max_z] {
                    let t = (z - a.z) / (b.z - a.z);
                    if (0. ..=1.).contains(&t) {
                        include(a + (b - a).scaled(t));
                    }
                }
            }
        }
        self.radius = radius_squared.sqrt() * 1.0001;
    }
}
