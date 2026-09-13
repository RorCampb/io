#![forbid(unsafe_code)]
use crate::camera::Camera;
use crate::model::ModelLibrary;
use io_types::{AppearanceId, Vec3, VisualStateId};
use io_world::{Playback, World};
use std::collections::HashMap;

#[derive(Clone, Copy)]
struct LodChoice {
    appearance: AppearanceId,
    state: VisualStateId,
    index: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub item_id: u64,
    pub model_id: u32,
    pub transform: [f32; 16],
    pub color: [f32; 3],
    pub joint_offset: u32,
}
#[derive(Default)]
pub struct Frame {
    pub instances: Vec<Instance>,
    pub grid: Vec<Vec3>,
    pub clip_from_world: [f32; 16],
    pub candidate_count: usize,
    pub serial: u64,
    pub joint_matrices: Vec<[f32; 16]>,
    lod_choices: HashMap<u64, LodChoice>,
    next_lod_choices: HashMap<u64, LodChoice>,
}
impl Frame {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &mut self,
        world: &World,
        camera: &Camera,
        serial: u64,
        library: &ModelLibrary,
        alpha: f32,
        active: &[usize],
        grid: bool,
    ) -> Result<(), String> {
        if !alpha.is_finite() {
            return Err("frame interpolation must be finite".into());
        }
        self.instances.clear();
        self.joint_matrices.clear();
        self.grid.clear();
        self.next_lod_choices.clear();
        self.clip_from_world = camera.clip_from_world();
        let candidates = world.query(camera.target(), camera.render_distance());
        self.candidate_count = candidates.len();
        for id in candidates {
            let item = &world.items()[id];
            let Some(renderable) = &item.renderable else {
                continue;
            };
            if camera.sees(item.visibility_bounds()) {
                let appearance = library
                    .appearance(renderable.appearance_id)
                    .ok_or_else(|| format!("item {} has an unresolved appearance", item.id))?;
                let group = appearance
                    .states
                    .get(renderable.visual_state.0 as usize)
                    .ok_or_else(|| format!("item {} has an unresolved visual state", item.id))?;
                let index =
                    if group.variants.len() == 1 && group.variants[0].min_screen_pixels == 0. {
                        0
                    } else {
                        let previous = self
                            .lod_choices
                            .get(&item.id)
                            .filter(|c| {
                                c.appearance == renderable.appearance_id
                                    && c.state == renderable.visual_state
                            })
                            .map(|c| c.index);
                        let pixels = camera
                            .projected_diameter(appearance.render_bounds, item.transform.size);
                        let index = group.select(pixels, previous, appearance.hysteresis);
                        self.next_lod_choices.insert(
                            item.id,
                            LodChoice {
                                appearance: renderable.appearance_id,
                                state: renderable.visual_state,
                                index,
                            },
                        );
                        index
                    };
                let Some(variant) = group.variants.get(index) else {
                    continue;
                };
                let blend = if active.binary_search(&id).is_ok() {
                    alpha.clamp(0., 1.)
                } else {
                    1.
                };
                let joint_offset = u32::try_from(self.joint_matrices.len())
                    .map_err(|_| "frame joint offset overflow")?;
                joint_offset
                    .checked_add(
                        u32::try_from(variant.joint_mapping.len())
                            .map_err(|_| "variant joint count overflow")?,
                    )
                    .ok_or("frame joint count overflow")?;
                if !variant.joint_mapping.is_empty() {
                    let model = library
                        .mesh(appearance.base_mesh)
                        .ok_or("unresolved animation source")?;
                    let palette = if let Some(animation) = &item.animation {
                        let duration = f64::from(
                            model
                                .clips()
                                .get(animation.clip())
                                .ok_or("unresolved animation clip")?
                                .duration(),
                        );
                        let time = animation.sample_time(blend, duration);
                        match animation.playback() {
                            Playback::Loop => model.palette(Some(animation.clip()), time),
                            Playback::Once { .. } => {
                                model.palette_clamped(Some(animation.clip()), time)
                            }
                        }
                    } else {
                        model.palette(None, 0.)
                    };
                    if variant.mesh_id == appearance.base_mesh {
                        self.joint_matrices.extend(palette);
                    } else {
                        self.joint_matrices
                            .extend(variant.joint_mapping.iter().map(|&i| palette[i]));
                    }
                }
                self.instances.push(Instance {
                    item_id: item.id,
                    model_id: variant.mesh_id,
                    transform: item.render_transform(blend),
                    color: renderable.color(item.simulated_ticks),
                    joint_offset,
                });
            }
        }
        std::mem::swap(&mut self.lod_choices, &mut self.next_lod_choices);
        self.instances
            .sort_unstable_by_key(|i| (i.model_id, i.item_id));
        if grid {
            self.build_grid(world, camera);
        }
        self.serial = serial;
        Ok(())
    }
    fn build_grid(&mut self, world: &World, camera: &Camera) {
        let r2 = camera.render_distance() * camera.render_distance()
            - camera.target().z * camera.target().z;
        if r2 <= 0. {
            return;
        }
        let radius = r2.sqrt();
        let space = world.space();
        let subs = space.subdivisions();
        // Debug grid detail adapts to screen density; world units and item occupancy never change.
        for (axis, subdivision) in subs.iter().enumerate().take(2) {
            let mut step = 1. / *subdivision as f32;
            let minimum = (12. / camera.pixels_per_unit()).max(radius / 100.);
            while step < minimum {
                step *= 2.;
            }
            let (center, other, limit, other_limit) = if axis == 0 {
                (
                    camera.target().x,
                    camera.target().y,
                    space.dimensions().x,
                    space.dimensions().y,
                )
            } else {
                (
                    camera.target().y,
                    camera.target().x,
                    space.dimensions().y,
                    space.dimensions().x,
                )
            };
            let first = ((center - radius).max(0.) / step).ceil() as i32;
            let last = ((center + radius).min(limit) / step).floor() as i32;
            for i in first..=last {
                let value = i as f32 * step;
                let length = (r2 - (value - center).powi(2)).max(0.).sqrt();
                let lo = (other - length).max(0.);
                let hi = (other + length).min(other_limit);
                if lo >= hi {
                    continue;
                }
                if axis == 0 {
                    self.grid
                        .extend([Vec3::new(value, lo, 0.), Vec3::new(value, hi, 0.)]);
                } else {
                    self.grid
                        .extend([Vec3::new(lo, value, 0.), Vec3::new(hi, value, 0.)]);
                }
            }
        }
    }
}
