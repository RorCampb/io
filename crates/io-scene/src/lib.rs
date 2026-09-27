#![forbid(unsafe_code)]
#[cfg(test)]
mod tests;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod spaces;
pub use spaces::{resolve_layout, InteriorDefinition, PortalDefinition, SpaceReference};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraRig {
    #[serde(default = "default_fov")]
    pub fov_degrees: f32,
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    pub zoom: f32,
    pub target_height: f32,
    pub yaw_limit_degrees: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub response_seconds: f32,
    pub approach: f32,
}
fn default_fov() -> f32 {
    45.
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            fov_degrees: default_fov(),
            yaw_degrees: 45.,
            pitch_degrees: 35.26439,
            zoom: 3.,
            target_height: 1.,
            yaw_limit_degrees: 180.,
            min_zoom: 0.5,
            max_zoom: 16.,
            response_seconds: 0.25,
            approach: 3.,
        }
    }
}
impl CameraRig {
    pub fn validate(&self) -> Result<(), String> {
        let values = [
            self.fov_degrees,
            self.yaw_degrees,
            self.pitch_degrees,
            self.zoom,
            self.target_height,
            self.yaw_limit_degrees,
            self.min_zoom,
            self.max_zoom,
            self.response_seconds,
            self.approach,
        ];
        if values.iter().any(|v| !v.is_finite())
            || !(20. ..=100.).contains(&self.fov_degrees)
            || !(-180. ..=180.).contains(&self.yaw_degrees)
            || !(2. ..=85.).contains(&self.pitch_degrees)
            || !(0. ..=20.).contains(&self.target_height)
            || !(0. ..=180.).contains(&self.yaw_limit_degrees)
            || !(0.025..=16.).contains(&self.min_zoom)
            || !(self.min_zoom..=16.).contains(&self.max_zoom)
            || !(self.min_zoom..=self.max_zoom).contains(&self.zoom)
            || !(0.02..=3.).contains(&self.response_seconds)
            || !(0.1..=30.).contains(&self.approach)
        {
            return Err(
                "Invalid rig: check zoom limits, pitch and positive transition values".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraRigs {
    #[serde(default)]
    pub motion: RigMotion,
    pub exterior: CameraRig,
    #[serde(default)]
    pub interiors: BTreeMap<String, CameraRig>,
}
impl CameraRigs {
    pub fn validate(&self, names: &[String]) -> Result<(), String> {
        self.motion.validate()?;
        self.exterior.validate()?;
        for (name, rig) in &self.interiors {
            if !names.contains(name) {
                return Err(format!("Rig references unknown interior: {name}"));
            }
            rig.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RigMotion {
    #[default]
    Authored,
    InteriorEnvelope {
        clearance: f32,
        close_distance: f32,
        max_retreat: f32,
        max_rise: f32,
        max_fov_degrees: f32,
    },
}
impl RigMotion {
    pub fn validate(self) -> Result<(), String> {
        match self {
            Self::Authored => Ok(()),
            Self::InteriorEnvelope {
                clearance,
                close_distance,
                max_retreat,
                max_rise,
                max_fov_degrees,
            } if [
                clearance,
                close_distance,
                max_retreat,
                max_rise,
                max_fov_degrees,
            ]
            .iter()
            .all(|v| v.is_finite())
                && (0.02..=0.5).contains(&clearance)
                && (0.1..=2.).contains(&close_distance)
                && (close_distance..=30.).contains(&max_retreat)
                && (0.1..=10.).contains(&max_rise)
                && (45. ..=100.).contains(&max_fov_degrees) =>
            {
                Ok(())
            }
            _ => Err("Invalid interior envelope limits".into()),
        }
    }
}
