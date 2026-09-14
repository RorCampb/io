#![forbid(unsafe_code)]
pub use io_assets::contract::{AppearanceConfig, AssetConfig, LodConfig, PackageImport};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneConfig {
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
    pub grounded: Option<GroundedConfig>,
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
        if self.game.is_some() && self.exploration.is_some() {
            return Err("choose game or exploration plugin".into());
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
pub struct GroundedConfig {
    pub radius: f32,
    pub height: f32,
    pub max_slope: f32,
}
impl GroundedConfig {
    pub fn resolve(&self) -> Result<io_world::Grounded, String> {
        let m = io_world::Grounded {
            radius: self.radius,
            height: self.height,
            max_slope: self.max_slope,
        };
        m.validate()?;
        Ok(m)
    }
}
