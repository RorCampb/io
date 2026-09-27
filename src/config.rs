#![forbid(unsafe_code)]
pub use io_assets::contract::{AppearanceConfig, AssetConfig, LodConfig, PackageImport};
pub use io_scene::InteriorDefinition as InteriorConfig;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneConfig {
    #[serde(default)]
    pub interiors: Vec<InteriorConfig>,
    #[serde(default)]
    pub portals: Vec<io_scene::PortalDefinition>,
    pub traversal: Option<io_playground::Definition>,
    pub exploration: Option<io_village::Definition>,
    pub terrain: Option<TerrainConfig>,
    pub game: Option<io_encounter::GameDefinition>,
    #[serde(default)]
    pub simulation: crate::timing::SimulationTiming,
    #[serde(default)]
    pub physics: crate::physics_config::PhysicsConfig,
    pub version: u32,
    pub dimensions: [f32; 3],
    pub origin: [f32; 3],
    pub camera: CameraConfig,
    #[serde(default)]
    pub assets: Vec<AssetConfig>,
    #[serde(default)]
    pub packages: Vec<PackageImport>,
    #[serde(default)]
    pub appearances: Vec<AppearanceConfig>,
    pub items: Vec<ItemConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraConfig {
    pub rigs: Option<io_scene::CameraRigs>,
    pub orbit: Option<crate::camera_boom::OrbitCamera>,
    pub interior: Option<crate::camera::InteriorCamera>,
    #[serde(default)]
    pub projection: crate::camera::Projection,
    #[serde(default)]
    pub coverage: crate::camera::RenderCoverage,
    pub target: [f32; 3],
    pub zoom: f32,
    pub follow: Option<String>,
    #[serde(default = "default_render_distance")]
    pub render_distance: f32,
}
fn default_render_distance() -> f32 {
    120.
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemConfig {
    // Load older scenes, but keep one canonical component name in code and new assets.
    #[serde(alias = "grounded")]
    pub character_body: Option<CharacterBodyConfig>,
    pub physics_body: Option<crate::physics_config::BodyConfig>,
    pub collider: Option<crate::physics_config::ColliderConfig>,
    pub rotation_xyzw: Option<[f32; 4]>,
    pub name: String,
    pub asset: Option<String>,
    pub appearance: Option<String>,
    pub visual_state: Option<String>,
    pub on_death_visual_state: Option<String>,
    pub position: [f32; 3],
    #[serde(default)]
    pub yaw_degrees: f32,
    #[serde(default = "ones")]
    pub scale: [f32; 3],
    #[serde(default = "ones")]
    pub tint: [f32; 3],
    pub animation: Option<AnimationConfig>,
    pub motion: Option<MotionConfig>,
    #[serde(default = "default_health")]
    pub health: u32,
    pub on_death: Option<DeathAnimationConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeathAnimationConfig {
    pub clip: String,
    #[serde(default = "one")]
    pub speed: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationConfig {
    pub clip: String,
    #[serde(default = "one")]
    pub speed: f32,
    #[serde(default)]
    pub events: Vec<AnimationEventConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationEventConfig {
    pub name: String,
    pub at: f64,
    pub target: String,
    pub effect: EffectConfig,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectConfig {
    Damage { amount: u32 },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionConfig {
    pub route: Vec<[f32; 3]>,
    pub speed: f32,
    #[serde(default)]
    pub start_distance: f64,
}
fn one() -> f32 {
    1.
}
fn default_health() -> u32 {
    100
}
fn ones() -> [f32; 3] {
    [1.; 3]
}
impl SceneConfig {
    pub fn read(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let scene: Self =
            serde_json::from_slice(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        scene.validate()?;
        Ok(scene)
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(rigs) = &self.camera.rigs {
            rigs.validate(
                &self
                    .interiors
                    .iter()
                    .map(|v| v.name.clone())
                    .collect::<Vec<_>>(),
            )?;
            if self.camera.interior.is_some() {
                return Err("Authored rigs cannot be combined with interior guidance".into());
            }
        }
        if let Some(orbit) = self.camera.orbit {
            if !orbit.valid()
                || matches!(
                    self.camera.projection,
                    crate::camera::Projection::Orthographic {}
                )
            {
                return Err(
                    "orbit camera needs valid distance limits and a perspective lens".into(),
                );
            }
        }
        io_scene::resolve_layout(&self.interiors, &self.portals, self.origin)?;
        if self.camera.interior.is_some_and(|v| !v.valid()) {
            return Err("invalid interior camera".into());
        }
        if let Some(interior) = self.camera.interior {
            match self.camera.projection {
                crate::camera::Projection::ZoomPerspective { end_zoom, .. }
                    if interior.zoom >= end_zoom => {}
                _ => return Err("interior camera needs full perspective at its guided zoom".into()),
            }
        }
        if [
            self.game.is_some(),
            self.exploration.is_some(),
            self.traversal.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
            > 1
        {
            return Err("choose one game, exploration or traversal plugin".into());
        }
        if let Some(traversal) = &self.traversal {
            traversal.validate()?;
        }
        if let Some(exploration) = &self.exploration {
            exploration.validate()?;
        }
        if let Some(terrain) = &self.terrain {
            terrain.resolve()?;
        }
        if let Some(game) = &self.game {
            game.validate()?;
        }
        self.physics.resolve()?;
        if self.version != 1 {
            return Err("unsupported scene version".into());
        }
        if self
            .dimensions
            .iter()
            .any(|v| !v.is_finite() || *v <= 0. || *v > 1_000_000.)
            || self
                .origin
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || self
                .camera
                .target
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || !self.camera.zoom.is_finite()
            || !(0.025..=16.).contains(&self.camera.zoom)
            || !self.camera.render_distance.is_finite()
            || !(8.0..=20000.0).contains(&self.camera.render_distance)
            || !self.camera.coverage.valid()
            || !self.camera.projection.valid()
        {
            return Err("invalid world or camera dimensions".into());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainConfig {
    pub origin: [f32; 2],
    pub spacing: f32,
    pub width: usize,
    pub depth: usize,
    pub heights: Vec<f32>,
}
impl TerrainConfig {
    pub fn resolve(&self) -> Result<io_world::HeightField, String> {
        io_world::HeightField::new(
            self.origin,
            self.spacing,
            self.width,
            self.depth,
            self.heights.clone(),
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterBodyConfig {
    pub radius: f32,
    pub height: f32,
    pub max_slope: f32,
}
impl CharacterBodyConfig {
    pub fn resolve(&self) -> Result<io_world::CharacterBody, String> {
        let m = io_world::CharacterBody {
            radius: self.radius,
            height: self.height,
            max_slope: self.max_slope,
        };
        m.validate()?;
        Ok(m)
    }
}

#[cfg(test)]
mod character_body_tests {
    use super::*;
    const ITEM: &str = r#"{"name":"hero","position":[0,0,0],"character_body":{"radius":0.35,"height":1.9,"max_slope":0.8}}"#;
    #[test]
    fn canonical_and_legacy_names_resolve_the_same_body() {
        let canonical: ItemConfig = serde_json::from_str(ITEM).unwrap();
        let legacy: ItemConfig =
            serde_json::from_str(&ITEM.replace("character_body", "grounded")).unwrap();
        assert_eq!(
            canonical.character_body.unwrap().resolve().unwrap(),
            legacy.character_body.unwrap().resolve().unwrap()
        );
    }
    #[test]
    fn aliases_cannot_override_each_other_and_body_contract_is_checked() {
        let duplicate = ITEM.replace(
            "\"character_body\":",
            "\"grounded\":null,\"character_body\":",
        );
        assert!(serde_json::from_str::<ItemConfig>(&duplicate).is_err());
        assert!(
            serde_json::from_str::<ItemConfig>(&ITEM.replace("radius", "is_grounded")).is_err()
        );
        let invalid: ItemConfig = serde_json::from_str(&ITEM.replace("0.35", "-0.35")).unwrap();
        assert!(invalid.character_body.unwrap().resolve().is_err());
    }
}
