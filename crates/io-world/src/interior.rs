//! Authored inhabitable regions, independent of meshes and camera preferences.
use io_types::{Bounds, Vec3};
#[derive(Clone, Debug)]
pub struct Interior {
    pub name: String,
    pub bounds: Bounds,
    pub ceiling: Option<f32>,
    /// Horizontal inward direction through the entrance.
    pub entry_direction: Vec3,
}
impl Interior {
    pub fn validate(&self) -> Result<(), String> {
        let d = self.entry_direction;
        if self.name.is_empty()
            || self.name.len() > 64
            || !self.bounds.valid()
            || self.bounds.extent().x <= 0.
            || self.bounds.extent().y <= 0.
            || self.bounds.extent().z <= 0.
            || !d.finite()
            || d.z.abs() > 1e-5
            || (d.dot(d) - 1.).abs() > 1e-4
            || self
                .ceiling
                .is_some_and(|z| !z.is_finite() || z <= self.bounds.min.z || z > self.bounds.max.z)
        {
            return Err("invalid interior volume".into());
        }
        Ok(())
    }
    pub fn proximity(&self, p: Vec3, margin: f32) -> f32 {
        if !p.finite()
            || !margin.is_finite()
            || margin <= 0.
            || p.z < self.bounds.min.z - 0.2
            || p.z > self.bounds.max.z
        {
            return 0.;
        }
        let x = p.x.clamp(self.bounds.min.x, self.bounds.max.x);
        let y = p.y.clamp(self.bounds.min.y, self.bounds.max.y);
        let t = (1. - (p.x - x).hypot(p.y - y) / margin).clamp(0., 1.);
        t * t * (3. - 2. * t)
    }
}
