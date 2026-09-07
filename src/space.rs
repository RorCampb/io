use crate::camera::Camera;

#[derive(Clone, Copy, Debug)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn scaled(self, scalar: f32) -> Self {
        Self {
            x: self.x * scalar,
            y: self.y * scalar,
        }
    }

    pub fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Space {
    pub camera: Camera,
    pub units_a: u32,
    pub units_b: u32,
    pub units_c: u32,
}

impl Space {
    pub fn new(units_a: u32, units_b: u32, units_c: u32) -> Self {
        Self {
            camera: Camera::new(),
            units_a,
            units_b,
            units_c,
        }
    }

    pub fn project(&self, a: f32, b: f32, c: f32) -> Vec2 {
        let unit_a = a / self.units_a as f32;
        let unit_b = b / self.units_b as f32;
        let unit_c = c / self.units_c as f32;

        self.camera.project(unit_a, unit_b, unit_c)
    }

    pub fn floor_lines(&self) -> Vec<[Vec2; 2]> {
        let mut lines = Vec::new();

        for a in 0..=self.units_a {
            lines.push([
                self.project(a as f32, 0.0, 0.0),
                self.project(a as f32, self.units_b as f32, 0.0),
            ]);
        }

        for b in 0..=self.units_b {
            lines.push([
                self.project(0.0, b as f32, 0.0),
                self.project(self.units_a as f32, b as f32, 0.0),
            ]);
        }

        lines
    }

    pub fn axis_a_plane_lines(&self) -> Vec<[Vec2; 2]> {
        let mut lines = Vec::new();

        for a in 0..=self.units_a {
            lines.push([
                self.project(a as f32, 0.0, 0.0),
                self.project(a as f32, 0.0, self.units_c as f32),
            ]);
        }

        for c in 0..=self.units_c {
            lines.push([
                self.project(0.0, 0.0, c as f32),
                self.project(self.units_a as f32, 0.0, c as f32),
            ]);
        }

        lines
    }

    pub fn axis_b_plane_lines(&self) -> Vec<[Vec2; 2]> {
        let mut lines = Vec::new();

        for b in 0..=self.units_b {
            lines.push([
                self.project(0.0, b as f32, 0.0),
                self.project(0.0, b as f32, self.units_c as f32),
            ]);
        }

        for c in 0..=self.units_c {
            lines.push([
                self.project(0.0, 0.0, c as f32),
                self.project(0.0, self.units_b as f32, c as f32),
            ]);
        }

        lines
    }

    pub fn outline_lines(&self) -> Vec<[Vec2; 2]> {
        vec![
            [
                self.project(0.0, 0.0, 0.0),
                self.project(self.units_a as f32, 0.0, 0.0),
            ],
            [
                self.project(0.0, 0.0, 0.0),
                self.project(0.0, self.units_b as f32, 0.0),
            ],
            [
                self.project(self.units_a as f32, 0.0, 0.0),
                self.project(self.units_a as f32, self.units_b as f32, 0.0),
            ],
            [
                self.project(0.0, self.units_b as f32, 0.0),
                self.project(self.units_a as f32, self.units_b as f32, 0.0),
            ],
            [
                self.project(0.0, 0.0, 0.0),
                self.project(0.0, 0.0, self.units_c as f32),
            ],
            [
                self.project(self.units_a as f32, 0.0, 0.0),
                self.project(self.units_a as f32, 0.0, self.units_c as f32),
            ],
            [
                self.project(0.0, self.units_b as f32, 0.0),
                self.project(0.0, self.units_b as f32, self.units_c as f32),
            ],
            [
                self.project(0.0, 0.0, self.units_c as f32),
                self.project(self.units_a as f32, 0.0, self.units_c as f32),
            ],
            [
                self.project(0.0, 0.0, self.units_c as f32),
                self.project(0.0, self.units_b as f32, self.units_c as f32),
            ],
        ]
    }
}
