use crate::*;
use io_traversal::navigation::Domain;
use io_types::Vec3;
use serde::Deserialize;
use std::ops::Deref;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub player: String,
    #[serde(flatten)]
    pub locomotion: Locomotion,
    #[serde(default)]
    pub navigation: Option<NavigationDefinition>,
    #[serde(default)]
    pub observations: Vec<ObservationDefinition>,
    #[serde(default)]
    pub obstacle_observations: Option<ObstacleObservationSettings>,
    #[serde(default)]
    pub debug_routes: bool,
    #[serde(default)]
    pub barrier_cycle: Option<crate::BarrierDefinition>,
    #[serde(default)]
    pub barrier_cycles: Vec<crate::BarrierDefinition>,
}
impl Deref for Definition {
    type Target = Locomotion;
    fn deref(&self) -> &Locomotion {
        &self.locomotion
    }
}
impl Definition {
    pub fn barriers(&self) -> impl Iterator<Item = &crate::BarrierDefinition> {
        self.barrier_cycle.iter().chain(&self.barrier_cycles)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.player.is_empty() || self.player.len() > 64 {
            return Err("invalid traversal player".into());
        }
        self.locomotion.validate()?;
        if let Some(settings) = self.obstacle_observations {
            settings.validate()?;
            if self.navigation.is_none() {
                return Err("obstacle observations require navigation actors".into());
            }
        }
        let mut barriers = std::collections::HashSet::new();
        for b in self.barriers() {
            b.validate()?;
            if !barriers.insert(&b.item) || barriers.len() > 64 {
                return Err("duplicate or excessive playground barrier cycles".into());
            }
        }
        if let Some(n) = &self.navigation {
            n.validate(&self.player)?;
        }
        if self.observations.len() > 4 {
            return Err("at most four observation tracks".into());
        }
        let mut pairs = std::collections::HashSet::new();
        for observation in &self.observations {
            observation.validate()?;
            if !pairs.insert((&observation.observer, &observation.target)) {
                return Err("duplicate observation pair".into());
            }
        }
        if let Some(navigation) = &self.navigation {
            for npc in &navigation.agents {
                if let Some(p) = &npc.pursuit {
                    if !self
                        .observations
                        .iter()
                        .any(|o| o.observer == npc.item && o.target == p.target)
                    {
                        return Err("pursuit needs a matching observer/target track".into());
                    }
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OnArrival {
    #[default]
    Stop,
    Patrol {
        points: Vec<[f32; 3]>,
    },
}
impl OnArrival {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Stop => Ok(()),
            Self::Patrol { points }
                if (2..=64).contains(&points.len())
                    && points.iter().flatten().all(|v| v.is_finite()) =>
            {
                Ok(())
            }
            Self::Patrol { .. } => Err("patrol requires 2-64 finite destinations".into()),
        }
    }
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcDefinition {
    pub item: String,
    pub goal: [f32; 3],
    pub locomotion: Option<Locomotion>,
    /// Overrides the navigation default for this actor, not a separate controller.
    pub athletics: Option<crate::athletics::Settings>,
    #[serde(default = "yes")]
    pub can_crouch: bool,
    #[serde(default)]
    pub familiar_points: Vec<[f32; 3]>,
    #[serde(default)]
    pub on_arrival: OnArrival,
    #[serde(default)]
    pub pursuit: Option<PursuitDefinition>,
    #[serde(default)]
    pub steering: io_locomotion::SteeringSettings,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationDefinition {
    pub domain: Domain,
    #[serde(default)]
    pub planning: io_traversal::PlanningMode,
    #[serde(default)]
    pub athletics: Option<crate::athletics::Settings>,
    pub expansions_per_tick: usize,
    pub agents: Vec<NpcDefinition>,
}
impl NavigationDefinition {
    pub fn validate(&self, player: &str) -> Result<(), String> {
        self.domain.validate()?;
        if let Some(settings) = self.athletics {
            settings.validate()?;
        }
        if !(1..=128).contains(&self.expansions_per_tick) {
            return Err("invalid navigation budget".into());
        }
        let mut names = std::collections::HashSet::from([player]);
        for a in &self.agents {
            if self.athletics.is_some() && a.can_crouch {
                return Err("athletics requires can_crouch=false".into());
            }
            if let Some(settings) = a.athletics {
                settings.validate()?;
                if self.athletics.is_none() {
                    return Err("actor athletics requires athletics navigation".into());
                }
            }
            if a.item.is_empty()
                || a.item.len() > 64
                || !names.insert(a.item.as_str())
                || a.goal.iter().any(|v| !v.is_finite())
                || a.familiar_points.len() > 256
                || a.familiar_points.iter().flatten().any(|v| !v.is_finite())
            {
                return Err("invalid navigation agent".into());
            }
            if let Some(settings) = &a.locomotion {
                settings.validate()?;
            }
            a.on_arrival.validate()?;
            a.steering.validate()?;
            if let Some(p) = &a.pursuit {
                p.settings.validate()?;
                if p.target.is_empty() || p.target.len() > 64 || p.target == a.item {
                    return Err("invalid pursuit target".into());
                }
            }
        }
        Ok(())
    }
}
pub struct NpcBinding {
    pub steering: io_locomotion::SteeringSettings,
    pub motor: Motor,
    pub goal: Vec3,
    pub can_crouch: bool,
    pub familiar_points: Vec<Vec3>,
    pub on_arrival: OnArrival,
    pub pursuit: Option<PursuitBinding>,
}
