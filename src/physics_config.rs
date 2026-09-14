#![forbid(unsafe_code)]
use io_types::Vec3;
use io_world::{BodyKind, Collider, ColliderShape, PhysicsBody, PhysicsSettings, SleepSettings};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolverConfig {
    #[default]
    Pgs,
    Tgs,
}

fn vec(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}
fn gravity() -> [f32; 3] {
    [0., 0., -9.81]
}
fn count_if_present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    u32::deserialize(deserializer).map(Some)
}
fn one() -> f32 {
    1.
}
fn damping() -> f32 {
    0.05
}
fn friction() -> f32 {
    0.6
}
fn membership() -> u32 {
    1
}
fn filter() -> u32 {
    u32::MAX
}
fn cell_size() -> f32 {
    2.
}
fn enabled() -> bool {
    true
}
fn linear_threshold() -> f32 {
    0.12
}
fn angular_threshold() -> f32 {
    0.15
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SleepConfig {
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default = "linear_threshold")]
    pub linear_threshold: f32,
    #[serde(default = "angular_threshold")]
    pub angular_threshold: f32,
    #[serde(default = "one")]
    pub idle_seconds: f32,
}
impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linear_threshold: linear_threshold(),
            angular_threshold: angular_threshold(),
            idle_seconds: 1.,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsConfig {
    #[serde(default)]
    pub solver: SolverConfig,
    #[serde(default = "enabled")]
    pub warm_start: bool,
    #[serde(default = "cell_size")]
    pub cell_size: f32,
    #[serde(default)]
    pub sleep: SleepConfig,
    #[serde(default = "gravity")]
    pub gravity: [f32; 3],
    #[serde(default, deserialize_with = "count_if_present")]
    pub substeps: Option<u32>,
    #[serde(default, deserialize_with = "count_if_present")]
    pub iterations: Option<u32>,
}
impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            solver: SolverConfig::Pgs,
            cell_size: cell_size(),
            sleep: SleepConfig::default(),
            warm_start: true,
            gravity: gravity(),
            substeps: None,
            iterations: None,
        }
    }
}
impl PhysicsConfig {
    pub fn resolve(&self) -> Result<PhysicsSettings, String> {
        let defaults = match self.solver {
            SolverConfig::Pgs => PhysicsSettings::default(),
            SolverConfig::Tgs => PhysicsSettings::tgs(),
        };
        let value = PhysicsSettings {
            solver: defaults.solver,
            warm_start: self.warm_start,
            cell_size: self.cell_size,
            sleep: SleepSettings {
                enabled: self.sleep.enabled,
                linear_threshold: self.sleep.linear_threshold,
                angular_threshold: self.sleep.angular_threshold,
                idle_seconds: self.sleep.idle_seconds,
            },
            gravity: vec(self.gravity),
            substeps: self.substeps.unwrap_or(defaults.substeps),
            iterations: self.iterations.unwrap_or(defaults.iterations),
        };
        value.validate()?;
        Ok(value)
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BodyConfig {
    Static {},
    Kinematic {},
    Dynamic {
        mass: f32,
        #[serde(default)]
        velocity: [f32; 3],
        #[serde(default)]
        angular_velocity: [f32; 3],
        #[serde(default = "one")]
        gravity_scale: f32,
        #[serde(default)]
        linear_damping: f32,
        #[serde(default = "damping")]
        angular_damping: f32,
    },
}
impl BodyConfig {
    pub fn resolve(&self) -> Result<PhysicsBody, String> {
        let body = match self {
            Self::Static {} => PhysicsBody::new(BodyKind::Static),
            Self::Kinematic {} => PhysicsBody::new(BodyKind::Kinematic),
            Self::Dynamic {
                mass,
                velocity,
                angular_velocity,
                gravity_scale,
                linear_damping,
                angular_damping,
            } => {
                let mut body = PhysicsBody::new(BodyKind::Dynamic);
                body.mass = *mass;
                body.velocity = vec(*velocity);
                body.angular_velocity = vec(*angular_velocity);
                body.gravity_scale = *gravity_scale;
                body.linear_damping = *linear_damping;
                body.angular_damping = *angular_damping;
                body
            }
        };
        body.validate()?;
        Ok(body)
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeConfig {
    Box { half_extents: [f32; 3] },
    Sphere { radius: f32 },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColliderConfig {
    pub shape: ShapeConfig,
    #[serde(default)]
    pub offset: [f32; 3],
    #[serde(default = "friction")]
    pub friction: f32,
    #[serde(default)]
    pub restitution: f32,
    #[serde(default = "membership")]
    pub memberships: u32,
    #[serde(default = "filter")]
    pub filter: u32,
}
impl ColliderConfig {
    pub fn resolve(&self) -> Result<Collider, String> {
        let value = Collider {
            shape: match self.shape {
                ShapeConfig::Box { half_extents } => ColliderShape::Box {
                    half_extents: vec(half_extents),
                },
                ShapeConfig::Sphere { radius } => ColliderShape::Sphere { radius },
            },
            offset: vec(self.offset),
            friction: self.friction,
            restitution: self.restitution,
            memberships: self.memberships,
            filter: self.filter,
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn solver_presets_are_explicit_and_overrides_are_validated() {
        let pgs: PhysicsConfig = serde_json::from_value(json!({})).unwrap();
        assert_eq!(pgs.resolve().unwrap(), PhysicsSettings::default());
        let tgs: PhysicsConfig = serde_json::from_value(json!({"solver": "tgs"})).unwrap();
        assert_eq!(tgs.resolve().unwrap(), PhysicsSettings::tgs());
        let override_config: PhysicsConfig = serde_json::from_value(json!({
            "solver": "tgs", "substeps": 16, "iterations": 3, "warm_start": false
        }))
        .unwrap();
        let settings = override_config.resolve().unwrap();
        assert_eq!(settings.solver, io_world::SolverMode::Tgs);
        assert_eq!((settings.substeps, settings.iterations), (16, 3));
        assert!(!settings.warm_start);
        for value in [
            json!({"solver": "automatic"}),
            json!({"solver": null}),
            json!({"iterations": null}),
            json!({"substeps": null}),
            json!({"substeps": -1}),
        ] {
            assert!(serde_json::from_value::<PhysicsConfig>(value).is_err());
        }
        for value in [
            json!({"solver": "tgs", "substeps": 0}),
            json!({"solver": "tgs", "substeps": 33}),
            json!({"solver": "tgs", "iterations": 65}),
        ] {
            assert!(serde_json::from_value::<PhysicsConfig>(value)
                .unwrap()
                .resolve()
                .is_err());
        }
    }

    #[test]
    fn tagged_physics_contracts_reject_unknown_fields_and_invalid_values() {
        for value in [
            json!({"type": "dynamic"}),
            json!({"type": "dynamic", "mass": 1, "force": 5}),
            json!({"type": "static", "mass": 1}),
            json!({"type": "character"}),
        ] {
            assert!(serde_json::from_value::<BodyConfig>(value).is_err());
        }
        let body: BodyConfig =
            serde_json::from_value(json!({"type": "dynamic", "mass": 0})).unwrap();
        assert!(body.resolve().is_err());
        for shape in [
            json!({"type": "capsule", "radius": 1}),
            json!({"type": "sphere", "radius": 1, "half_extents": [1,1,1]}),
        ] {
            assert!(serde_json::from_value::<ShapeConfig>(shape).is_err());
        }
        let collider: ColliderConfig =
            serde_json::from_value(json!({"shape": {"type": "sphere", "radius": -1}})).unwrap();
        assert!(collider.resolve().is_err());
        let settings: PhysicsConfig = serde_json::from_value(json!({"iterations": 0})).unwrap();
        assert!(settings.resolve().is_err());
        for value in [
            json!({"cell_size": 0}),
            json!({"sleep": {"idle_seconds": -1}}),
            json!({"sleep": {"linear_threshold": 20}}),
        ] {
            let settings: PhysicsConfig = serde_json::from_value(value).unwrap();
            assert!(settings.resolve().is_err());
        }
        assert!(
            serde_json::from_value::<PhysicsConfig>(json!({"sleep": {"force_sleep": true}}))
                .is_err()
        );
        let settings: PhysicsConfig = serde_json::from_value(json!({"warm_start": false})).unwrap();
        assert!(!settings.resolve().unwrap().warm_start);
        assert!(PhysicsConfig::default().resolve().unwrap().warm_start);
        assert!(serde_json::from_value::<PhysicsConfig>(json!({"warm_start": "yes"})).is_err());
    }

    #[test]
    fn scene_assembly_enforces_body_dependencies_and_pose_authority() {
        let base = json!({
            "version": 1, "dimensions": [100,100,100], "origin": [0,0,0],
            "camera": {"target": [0,0,0], "zoom": 1},
            "assets": [{"name": "box", "builtin": "box"}],
            "items": [{"name": "body", "asset": "box", "position": [0,0,10],
                "physics_body": {"type": "dynamic", "mass": 1},
                "collider": {"shape": {"type": "box", "half_extents": [0.5,0.5,0.5]}}}]
        });
        let assemble = |value| {
            let config = serde_json::from_value(value).unwrap();
            let library =
                crate::model::ModelLibrary::from_config(config, std::path::Path::new(".")).unwrap();
            crate::demo::world(&library)
        };
        assert!(assemble(base.clone()).is_ok());
        let mut missing = base.clone();
        missing["items"][0]
            .as_object_mut()
            .unwrap()
            .remove("collider");
        assert!(assemble(missing).is_err());
        let mut route = base.clone();
        route["items"][0]["motion"] = json!({"route": [[0,0,10],[5,0,10]], "speed": 1});
        assert!(assemble(route.clone()).is_err());
        route["items"][0]["physics_body"] = json!({"type": "kinematic"});
        assert!(assemble(route).is_ok());
        let mut orientation = base;
        orientation["items"][0]["rotation_xyzw"] = json!([0, 0, 0, 1]);
        orientation["items"][0]["yaw_degrees"] = json!(30);
        assert!(assemble(orientation).is_err());
    }
}
