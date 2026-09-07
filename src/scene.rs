use crate::space::{Space, Vec2};
use crate::{IoLine, IoPoint};

#[derive(Clone, Copy, Debug)]
pub struct GridPoint {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct GridSize {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum ItemKind {
    Character,
    Decor,
    Scene,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub kind: ItemKind,
    pub anchor: GridPoint,
    pub footprint: GridSize,
    pub color: [f32; 3],
}

impl Item {
    pub fn screen_anchor(&self, space: &Space) -> Vec2 {
        space.project(self.anchor.a, self.anchor.b, self.anchor.c)
    }
}

#[derive(Debug)]
pub struct SceneState {
    pub space: Space,
    pub items: Vec<Item>,
    line_cache: Vec<IoLine>,
    point_cache: Vec<IoPoint>,
    dirty: bool,
}

impl SceneState {
    pub fn demo() -> Self {
        let items = vec![
            Item {
                id: 1,
                name: "spawn".to_string(),
                kind: ItemKind::Scene,
                anchor: GridPoint {
                    a: 2.0,
                    b: 2.0,
                    c: 0.0,
                },
                footprint: GridSize {
                    a: 1.0,
                    b: 1.0,
                    c: 0.0,
                },
                color: [1.0, 0.96, 0.72],
            },
            Item {
                id: 2,
                name: "decor".to_string(),
                kind: ItemKind::Decor,
                anchor: GridPoint {
                    a: 7.0,
                    b: 3.0,
                    c: 0.0,
                },
                footprint: GridSize {
                    a: 2.0,
                    b: 1.0,
                    c: 2.0,
                },
                color: [0.95, 0.82, 0.35],
            },
            Item {
                id: 3,
                name: "model".to_string(),
                kind: ItemKind::Character,
                anchor: GridPoint {
                    a: 5.0,
                    b: 6.0,
                    c: 2.0,
                },
                footprint: GridSize {
                    a: 1.0,
                    b: 1.0,
                    c: 3.0,
                },
                color: [1.0, 0.90, 0.50],
            },
        ];

        let mut scene = Self {
            space: Space::new(12, 12, 8),
            items,
            line_cache: Vec::new(),
            point_cache: Vec::new(),
            dirty: true,
        };
        scene.refresh_render_cache();
        scene
    }

    pub fn resize_units_a(&mut self, delta: i32) {
        self.space.units_a = apply_unit_delta(self.space.units_a, delta);
        self.clamp_items();
        self.dirty = true;
    }

    pub fn resize_units_b(&mut self, delta: i32) {
        self.space.units_b = apply_unit_delta(self.space.units_b, delta);
        self.clamp_items();
        self.dirty = true;
    }

    pub fn resize_units_c(&mut self, delta: i32) {
        self.space.units_c = apply_unit_delta(self.space.units_c, delta);
        self.clamp_items();
        self.dirty = true;
    }

    pub fn orbit_camera(&mut self, yaw_delta: f32, pitch_delta: f32) {
        self.dirty |= self.space.camera.orbit(yaw_delta, pitch_delta);
    }

    pub fn zoom_camera(&mut self, steps: f32) {
        self.dirty |= self.space.camera.zoom_by(steps);
    }

    pub fn set_viewport(&mut self, width: i32, height: i32) {
        self.dirty |= self.space.camera.set_viewport(width, height);
    }

    pub fn reset_camera(&mut self) {
        self.space.camera.reset();
        self.dirty = true;
    }

    pub fn lines(&mut self) -> &[IoLine] {
        self.refresh_render_cache();
        &self.line_cache
    }

    pub fn points(&mut self) -> &[IoPoint] {
        self.refresh_render_cache();
        &self.point_cache
    }

    fn refresh_render_cache(&mut self) {
        if !self.dirty {
            return;
        }

        self.line_cache.clear();
        self.point_cache.clear();

        self.extend_lines(self.space.floor_lines(), [0.84, 0.70, 0.12]);
        self.extend_lines(self.space.axis_a_plane_lines(), [0.84, 0.70, 0.12]);
        self.extend_lines(self.space.axis_b_plane_lines(), [0.84, 0.70, 0.12]);
        self.extend_lines(self.space.outline_lines(), [1.0, 0.94, 0.42]);

        for item in &self.items {
            let point = item.screen_anchor(&self.space);
            self.point_cache.push(IoPoint {
                x: point.x,
                y: point.y,
                r: item.color[0],
                g: item.color[1],
                b: item.color[2],
            });
        }

        self.dirty = false;
    }

    fn extend_lines(&mut self, lines: Vec<[Vec2; 2]>, color: [f32; 3]) {
        for [start, end] in lines {
            self.line_cache.push(IoLine {
                x1: start.x,
                y1: start.y,
                x2: end.x,
                y2: end.y,
                r: color[0],
                g: color[1],
                b: color[2],
            });
        }
    }

    fn clamp_items(&mut self) {
        for item in &mut self.items {
            item.anchor.a = item.anchor.a.clamp(0.0, self.space.units_a as f32);
            item.anchor.b = item.anchor.b.clamp(0.0, self.space.units_b as f32);
            item.anchor.c = item.anchor.c.clamp(0.0, self.space.units_c as f32);
        }
    }
}

fn apply_unit_delta(units: u32, delta: i32) -> u32 {
    if delta >= 0 {
        units.saturating_add(delta as u32).max(1)
    } else {
        units.saturating_sub(delta.unsigned_abs()).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_changes_refresh_both_buffers_without_editing_world_data() {
        let mut scene = SceneState::demo();
        let items = scene.items.clone();
        let first_line = scene.lines()[0];
        let first_point = scene.points()[0];
        scene.orbit_camera(0.7, 0.15);
        scene.zoom_camera(2.0);
        scene.set_viewport(1000, 700);
        let line = scene.lines()[0];
        let point = scene.points()[0];
        assert!((line.x1 - first_line.x1).abs() > 1.0);
        assert!((point.x - first_point.x).abs() > 1.0);
        let expected = scene.items[0].screen_anchor(&scene.space);
        assert_eq!((point.x, point.y), (expected.x, expected.y));
        assert_eq!(
            (
                scene.space.units_a,
                scene.space.units_b,
                scene.space.units_c
            ),
            (12, 12, 8)
        );
        for (before, after) in items.iter().zip(&scene.items) {
            assert_eq!(
                (before.anchor.a, before.anchor.b, before.anchor.c),
                (after.anchor.a, after.anchor.b, after.anchor.c)
            );
            assert_eq!(
                (before.footprint.a, before.footprint.b, before.footprint.c),
                (after.footprint.a, after.footprint.b, after.footprint.c)
            );
        }
        scene.reset_camera();
        scene.set_viewport(1280, 800);
        let restored = scene.points()[0];
        assert_eq!((restored.x, restored.y), (first_point.x, first_point.y));
        assert!(!scene.dirty);
        scene.set_viewport(1280, 800);
        assert!(!scene.dirty, "unchanged viewport should reuse the cache");
    }
}
