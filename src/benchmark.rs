#![forbid(unsafe_code)]
use io_types::Vec3;
use io_world::{ColorMode, Item, Renderable, Space, Transform, World};

pub fn world() -> World {
    let mut items = Vec::new();
    for x in 0..200 {
        for y in 0..200 {
            items.push(Item {
                id: items.len() as u64 + 1,
                transform: Transform::new(
                    if x == 100 && y == 100 {
                        Vec3::new(4995., 4995., 3.)
                    } else {
                        Vec3::new(x as f32 * 50. + 5., y as f32 * 50. + 5., 0.)
                    },
                    if x == 100 && y == 100 {
                        Vec3::new(10., 10., 10.)
                    } else {
                        Vec3::new(3., 4., 2. + ((x + y) % 5) as f32)
                    },
                    ((x + y) % 4) as f32 * 0.35,
                )
                .unwrap(),
                renderable: Some(Renderable {
                    color_mode: if (x + y) % 11 == 0 {
                        ColorMode::Pulse
                    } else {
                        ColorMode::Tint
                    },
                    ..Renderable::default()
                }),
                simulated_ticks: 0,
                ..Item::default()
            });
        }
    }
    for x in 0..20 {
        for y in 0..20 {
            items.push(Item {
                id: items.len() as u64 + 1,
                transform: Transform::new(
                    Vec3::new(4902. + x as f32 * 10., 4902. + y as f32 * 10., 0.),
                    Vec3::new(1.5, 1.5, 3.),
                    0.,
                )
                .unwrap(),
                renderable: Some(Renderable {
                    color_mode: if (x + y) % 3 == 0 {
                        ColorMode::Pulse
                    } else {
                        ColorMode::Tint
                    },
                    ..Renderable::default()
                }),
                simulated_ticks: 0,
                ..Item::default()
            });
        }
    }
    World::new(Space::new(Vec3::new(10000., 10000., 256.)), items)
}
