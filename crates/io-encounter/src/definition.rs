use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Editable authoring data, deliberately separate from live combat state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameDefinition {
    pub version: u32,
    pub movement_seconds: f64,
    #[serde(default)]
    pub confirm_round_start: bool,
    pub abilities: BTreeMap<String, AbilityDefinition>,
    pub templates: BTreeMap<String, CharacterTemplate>,
    pub combatants: Vec<CombatantBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityDefinition {
    pub range: f32,
    pub effect: AbilityEffect,
    #[serde(default)]
    pub delivery: AbilityDelivery,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AbilityDelivery {
    Instant {},
    Projectile { speed: f32 },
}
impl Default for AbilityDelivery {
    fn default() -> Self {
        Self::Instant {}
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AbilityEffect {
    Damage { amount: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Player,
    Npc,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterTemplate {
    pub death_physics: Option<DeathPhysics>,
    pub control: Control,
    pub faction: u32,
    pub initiative: i32,
    pub movement_speed: f32,
    pub abilities: Vec<String>,
    pub opportunity_ability: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeathPhysics {
    pub mass: f32,
    pub knockback_impulse: f32,
    pub lift_impulse: f32,
    pub half_extents: [f32; 3],
    pub offset: [f32; 3],
}
impl DeathPhysics {
    pub(crate) fn body(&self) -> io_world::PhysicsBody {
        let mut body = io_world::PhysicsBody::new(io_world::BodyKind::Dynamic);
        body.mass = self.mass;
        body.linear_damping = 0.4;
        body.angular_damping = 0.8;
        body
    }
    pub(crate) fn collider(&self) -> io_world::Collider {
        let mut collider = io_world::Collider::new(io_world::ColliderShape::Box {
            half_extents: io_types::Vec3::new(
                self.half_extents[0],
                self.half_extents[1],
                self.half_extents[2],
            ),
        });
        collider.offset = io_types::Vec3::new(self.offset[0], self.offset[1], self.offset[2]);
        collider
    }
    fn validate(&self) -> Result<(), String> {
        self.body().validate()?;
        self.collider().validate()?;
        let [x, y, z] = self.half_extents;
        let angular_bound =
            1.5 * z * self.knockback_impulse / (self.mass * (x.min(y).powi(2) + z * z));
        if !(0.1..=100.).contains(&self.mass)
            || [self.knockback_impulse, self.lift_impulse]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=20.).contains(v))
            || self.half_extents.iter().any(|v| !(0.1..=10.).contains(v))
            || self.offset.iter().any(|v| !v.is_finite() || v.abs() > 10.)
            || !angular_bound.is_finite()
            || angular_bound > 900.
        {
            return Err(
                "death physics needs mass 0.1..100, impulses 0..20, half extents 0.1..10".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CombatantBinding {
    pub item: String,
    pub template: String,
}

impl GameDefinition {
    pub fn validate(&self) -> Result<(), String> {
        fn name(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 24
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        }
        if self.version != 1
            || !self.movement_seconds.is_finite()
            || !(0.1..=60.).contains(&self.movement_seconds)
        {
            return Err("game requires version 1 and movement_seconds within 0.1..60".into());
        }
        if self.combatants.is_empty()
            || self.combatants.len() > 256
            || self.abilities.is_empty()
            || self.abilities.len() > 256
            || self.templates.is_empty()
            || self.templates.len() > 256
        {
            return Err("game requires 1..256 combatants, abilities and templates".into());
        }
        for (key, ability) in &self.abilities {
            if let AbilityDelivery::Projectile { speed } = ability.delivery {
                if !speed.is_finite() || !(1. ..=100.).contains(&speed) {
                    return Err(format!("invalid projectile speed: {key}"));
                }
            }
            if !name(key) || !ability.range.is_finite() || !(0.01..=1000.).contains(&ability.range)
            {
                return Err(format!("invalid ability: {key}"));
            }
            match ability.effect {
                AbilityEffect::Damage { amount: 0 } => return Err(format!("zero damage: {key}")),
                AbilityEffect::Damage { .. } => {}
            }
        }
        for (key, template) in &self.templates {
            if let Some(death) = &template.death_physics {
                death.validate()?;
            }
            if !name(key)
                || !template.movement_speed.is_finite()
                || !(0.01..=100.).contains(&template.movement_speed)
                || template.abilities.is_empty()
                || template.abilities.len() > 4
            {
                return Err(format!("invalid character template: {key}"));
            }
            let mut seen = std::collections::BTreeSet::new();
            for ability in &template.abilities {
                if !self.abilities.contains_key(ability) || !seen.insert(ability) {
                    return Err(format!("unknown/duplicate ability in {key}: {ability}"));
                }
            }
            if template
                .opportunity_ability
                .as_ref()
                .is_some_and(|a| !seen.contains(a))
            {
                return Err(format!("opportunity ability must be in {key}'s loadout"));
            }
            if template
                .opportunity_ability
                .as_ref()
                .is_some_and(|a| !matches!(self.abilities[a].delivery, AbilityDelivery::Instant {}))
            {
                return Err(format!(
                    "opportunity ability must have instant delivery: {key}"
                ));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for actor in &self.combatants {
            if !name(&actor.item)
                || !seen.insert(&actor.item)
                || !self.templates.contains_key(&actor.template)
            {
                return Err(format!("invalid combatant binding: {}", actor.item));
            }
        }
        Ok(())
    }
}
