#![forbid(unsafe_code)]
use crate::config::{AppearanceConfig, LodConfig};
use io_assets::Model;
use io_types::{Bounds, VisualStateId};
use std::collections::HashMap;

pub struct RenderVariant {
    pub mesh_id: u32,
    pub min_screen_pixels: f32,
    pub joint_mapping: Vec<usize>,
}

pub struct LodGroup {
    pub variants: Vec<RenderVariant>,
}

impl LodGroup {
    /// The index after the last variant means omitted. A zero final threshold
    /// keeps a representation at every size. History belongs to each camera.
    pub fn select(&self, pixels: f32, previous: Option<usize>, margin: f32) -> usize {
        let Some(mut index) = previous.filter(|&i| i <= self.variants.len()) else {
            return self
                .variants
                .partition_point(|v| v.min_screen_pixels > pixels);
        };
        while index > 0 && pixels >= self.variants[index - 1].min_screen_pixels * (1. + margin) {
            index -= 1;
        }
        while index < self.variants.len()
            && pixels < self.variants[index].min_screen_pixels * (1. - margin)
        {
            index += 1;
        }
        index
    }
}

pub struct Appearance {
    pub base_mesh: u32,
    pub occupancy_bounds: Bounds,
    pub render_bounds: Bounds,
    pub states: Vec<LodGroup>,
    pub state_names: HashMap<String, VisualStateId>,
    pub hysteresis: f32,
}

impl Appearance {
    pub fn single(mesh_id: u32, model: &Model) -> Self {
        Self {
            base_mesh: mesh_id,
            occupancy_bounds: model.bounds(),
            render_bounds: model.bounds(),
            states: vec![LodGroup {
                variants: vec![RenderVariant {
                    mesh_id,
                    min_screen_pixels: 0.,
                    joint_mapping: (0..model.joint_count()).collect(),
                }],
            }],
            state_names: HashMap::from([("default".into(), VisualStateId::default())]),
            hysteresis: 0.1,
        }
    }

    pub fn resolve(
        spec: &AppearanceConfig,
        names: &HashMap<String, u32>,
        models: &[Model],
    ) -> Result<Self, String> {
        let base = *names
            .get(&spec.base_asset)
            .ok_or_else(|| format!("unknown base asset: {}", spec.base_asset))?;
        if !spec.hysteresis.is_finite() || !(0.0..0.5).contains(&spec.hysteresis) {
            return Err("hysteresis must be between 0 (inclusive) and 0.5 (exclusive)".into());
        }
        let source = &models[base as usize - 1];
        let mut appearance = Self::single(base, source);
        if let Some(bounds) = &spec.occupancy_bounds {
            appearance.occupancy_bounds = bounds.resolve()?;
        }
        appearance.hysteresis = spec.hysteresis;
        if !spec.lods.is_empty() {
            appearance.states[0] = appearance.resolve_group(&spec.lods, names, models)?;
        }
        for state in &spec.states {
            if state.name.is_empty() || appearance.state_names.contains_key(&state.name) {
                return Err(format!("empty or duplicate visual state: {}", state.name));
            }
            let group = appearance.resolve_group(&state.lods, names, models)?;
            let id = VisualStateId(
                u32::try_from(appearance.states.len()).map_err(|_| "too many visual states")?,
            );
            appearance.state_names.insert(state.name.clone(), id);
            appearance.states.push(group);
        }
        Ok(appearance)
    }

    fn resolve_group(
        &mut self,
        specs: &[LodConfig],
        names: &HashMap<String, u32>,
        models: &[Model],
    ) -> Result<LodGroup, String> {
        if specs.is_empty() {
            return Err("a visual state needs at least one LOD".into());
        }
        let mut previous = f32::INFINITY;
        let mut variants = Vec::new();
        let source = &models[self.base_mesh as usize - 1];
        for spec in specs {
            let threshold = spec.min_screen_pixels;
            if !threshold.is_finite() || threshold < 0. || threshold >= previous {
                return Err(
                    "LOD thresholds must be finite, nonnegative, and strictly decreasing".into(),
                );
            }
            previous = threshold;
            let mesh_id = *names
                .get(&spec.asset)
                .ok_or_else(|| format!("unknown LOD asset: {}", spec.asset))?;
            let model = &models[mesh_id as usize - 1];
            let joint_mapping = if mesh_id == self.base_mesh {
                (0..model.joint_count()).collect()
            } else if model.joint_count() == 0 {
                Vec::new()
            } else {
                model
                    .palette_mapping_from(source)
                    .map_err(|e| format!("asset {}: {e}", spec.asset))?
            };
            self.render_bounds = self.render_bounds.union(if model.joint_count() == 0 {
                model.bounds()
            } else {
                model.bounds_with_source(source, &joint_mapping)?
            });
            variants.push(RenderVariant {
                mesh_id,
                min_screen_pixels: threshold,
                joint_mapping,
            });
        }
        Ok(LodGroup { variants })
    }
}
