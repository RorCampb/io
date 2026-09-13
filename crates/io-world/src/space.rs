#![forbid(unsafe_code)]
use io_types::Vec3;

#[derive(Clone, Copy, Debug)]
pub enum Axis {
    A,
    B,
    C,
}

#[derive(Clone, Debug)]
pub struct Space {
    dimensions: Vec3,
    subdivisions: [u32; 3],
}

impl Space {
    pub fn new(dimensions: Vec3) -> Self {
        Self::try_new(dimensions).expect("invalid world dimensions")
    }
    pub fn try_new(dimensions: Vec3) -> Result<Self, String> {
        if !dimensions.finite()
            || [dimensions.x, dimensions.y, dimensions.z]
                .iter()
                .any(|v| *v <= 0. || *v > 1_000_000.)
        {
            return Err("world dimensions must be finite and within (0, 1000000]".into());
        }
        Ok(Self {
            dimensions,
            subdivisions: [1; 3],
        })
    }
    pub fn dimensions(&self) -> Vec3 {
        self.dimensions
    }
    pub fn subdivisions(&self) -> [u32; 3] {
        self.subdivisions
    }
    pub fn resize_subdivisions(&mut self, axis: Axis, delta: i32) -> bool {
        let i = match axis {
            Axis::A => 0,
            Axis::B => 1,
            Axis::C => 2,
        };
        let old = self.subdivisions[i];
        self.subdivisions[i] = (i64::from(old) + i64::from(delta)).clamp(1, 1024) as u32;
        old != self.subdivisions[i]
    }
    pub fn clamp_target(&self, point: Vec3) -> Vec3 {
        Vec3::new(
            point.x.clamp(0., self.dimensions.x),
            point.y.clamp(0., self.dimensions.y),
            point.z.clamp(0., self.dimensions.z),
        )
    }
}
