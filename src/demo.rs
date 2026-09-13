#![forbid(unsafe_code)]
use crate::config::EffectConfig;
use crate::model::ModelLibrary;
use io_types::{AppearanceId, Vec3};
use io_world::{
    AnimationEvent, AnimationEvents, AnimationState, ColorMode, Damage, DepletionAnimation,
    DepletionResponse, Durability, EffectKind, Item, Occupancy, PathMotion, Renderable, Space,
    Transform, World,
};
use std::collections::HashMap;

fn vec(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}
pub fn world(library: &ModelLibrary) -> Result<World, String> {
    let config = &library.config;
    let origin = vec(config.origin);
    let mut names = HashMap::new();
    for (index, spec) in config.items.iter().enumerate() {
        if names.insert(spec.name.as_str(), index as u64 + 1).is_some() {
            return Err(format!("duplicate item: {}", spec.name));
        }
    }
    let mut items = Vec::new();
    for spec in &config.items {
        let appearance_id = match (&spec.asset, &spec.appearance) {
            (Some(asset), None) => AppearanceId(
                *library
                    .names
                    .get(asset)
                    .ok_or_else(|| format!("unknown asset: {asset}"))?,
            ),
            (None, Some(name)) => *library
                .appearance_names
                .get(name)
                .ok_or_else(|| format!("unknown appearance: {name}"))?,
            _ => {
                return Err(format!(
                    "item {} needs exactly one asset or appearance",
                    spec.name
                ))
            }
        };
        let appearance = library.appearance(appearance_id).unwrap();
        let model = library.mesh(appearance.base_mesh).unwrap();
        let state_id = |name: &str| {
            appearance
                .state_names
                .get(name)
                .copied()
                .ok_or_else(|| format!("item {}: unknown visual state: {name}", spec.name))
        };
        if !spec.yaw_degrees.is_finite()
            || spec.position.iter().any(|v| !v.is_finite())
            || spec.scale.iter().any(|v| !v.is_finite() || *v <= 0.)
            || spec.tint.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(format!("invalid transform/color: {}", spec.name));
        }
        let mut item = Item {
            id: items.len() as u64 + 1,
            transform: Transform::new(
                origin + vec(spec.position),
                vec(spec.scale),
                spec.yaw_degrees.to_radians(),
            )?,
            occupancy: Occupancy {
                local_bounds: appearance.occupancy_bounds,
            },
            renderable: Some(Renderable {
                appearance_id,
                visual_state: state_id(spec.visual_state.as_deref().unwrap_or("default"))?,
                visual_state_count: appearance.states.len() as u32,
                local_bounds: appearance.render_bounds,
                tint: spec.tint,
                color_mode: ColorMode::Tint,
            }),
            // Flat version-1 scenes keep their existing health/death defaults.
            // These are adapter policy, not intrinsic Item capabilities.
            durability: Some(Durability::new(spec.health, spec.health.max(100))?),
            depletion_response: Some(DepletionResponse {
                stop_motion: true,
                visual_state: spec
                    .on_death_visual_state
                    .as_deref()
                    .map(state_id)
                    .transpose()?,
                animation: DepletionAnimation::Freeze,
            }),
            ..Item::default()
        };
        if let Some(motion) = &spec.motion {
            let route = motion.route.iter().map(|p| origin + vec(*p)).collect();
            let path = PathMotion::new(route, motion.speed, motion.start_distance)
                .ok_or_else(|| format!("invalid route: {}", spec.name))?;
            let (anchor, yaw) = path.pose();
            item.transform.anchor = anchor;
            item.transform.rotation = io_types::Rotation::yaw(yaw)?;
            item.motion = Some(path);
        }
        if let Some(animation) = &spec.animation {
            let clip = model
                .clips()
                .iter()
                .position(|c| c.name() == animation.clip)
                .ok_or_else(|| format!("unknown clip: {}", animation.clip))?;
            if !animation.speed.is_finite() || animation.speed < 0. {
                return Err("invalid animation speed".into());
            }
            let mut state = AnimationState::looping(clip);
            state.set_speed(animation.speed)?;
            if !animation.events.is_empty() {
                let events = animation
                    .events
                    .iter()
                    .map(|event| {
                        let target = *names
                            .get(event.target.as_str())
                            .ok_or_else(|| format!("unknown effect target: {}", event.target))?;
                        let effect = match event.effect {
                            EffectConfig::Damage { amount } => {
                                EffectKind::Damage(Damage { amount })
                            }
                        };
                        Ok(AnimationEvent {
                            name: event.name.clone(),
                            at: event.at,
                            target,
                            effect,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                state.set_events(
                    AnimationEvents::new(f64::from(model.clips()[clip].duration()), events)
                        .map_err(|e| format!("item {}: {e}", spec.name))?,
                )?;
            }
            item.animation = Some(state);
        }
        if let Some(death) = &spec.on_death {
            let clip = model
                .clips()
                .iter()
                .position(|c| c.name() == death.clip)
                .ok_or_else(|| format!("unknown death clip: {}", death.clip))?;
            if !death.speed.is_finite() || death.speed <= 0. {
                return Err("death animation speed must be positive".into());
            }
            let mut state = AnimationState::once(clip, f64::from(model.clips()[clip].duration()))?;
            state.set_speed(death.speed)?;
            item.depletion_response.as_mut().unwrap().animation =
                DepletionAnimation::PlayOnce(state);
        }
        if item.animation.is_none() && item.motion.is_some() {
            item.renderable.as_mut().unwrap().color_mode = ColorMode::Pulse;
        }
        items.push(item);
    }
    if let Some(follow) = &config.camera.follow {
        if !names.contains_key(follow.as_str()) {
            return Err(format!("unknown camera follow target: {follow}"));
        }
    }
    World::try_new(Space::try_new(vec(config.dimensions))?, items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn library() -> ModelLibrary {
        ModelLibrary::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/street-kit/effects-demo.json"),
        )
        .unwrap()
    }

    #[test]
    fn scene_drives_damage_and_collapse_without_rendering() {
        let library = library();
        let mut world = world(&library).unwrap();
        assert_eq!(
            world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            40
        );
        world.simulate(&[0], 0.5);
        assert_eq!(
            world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            30
        );
        world.simulate(&[0], 0.25);
        assert_eq!(
            world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            30
        );
        world.simulate(&[0], 0.75);
        assert_eq!(
            world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            20
        );
        world.simulate(&[0], 2.);
        let dead = world.item(1).unwrap();
        assert_eq!(dead.durability.as_ref().unwrap().current(), 0);
        let animation = dead.animation.as_ref().unwrap();
        assert_eq!(
            library
                .mesh(
                    library
                        .appearance(dead.renderable.as_ref().unwrap().appearance_id)
                        .unwrap()
                        .base_mesh
                )
                .unwrap()
                .clips()[animation.clip()]
            .name(),
            "Collapse"
        );
        assert_eq!(animation.time(), 0.);
        assert!(animation.events().is_none());
        world.simulate(&[0], 2.);
        assert_eq!(
            world.item(1).unwrap().animation.as_ref().unwrap().time(),
            1.5
        );
        world.simulate(&[0], 10.);
        assert_eq!(
            world.item(1).unwrap().animation.as_ref().unwrap().time(),
            1.5
        );
    }

    #[test]
    fn scene_resolves_forward_effect_targets() {
        let mut library = library();
        library.config.items[0].animation.as_mut().unwrap().events[0].target =
            "practice-target".into();
        let mut world = world(&library).unwrap();
        world.simulate(&[0], 0.5);
        assert_eq!(
            world
                .item(2)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            30
        );
        assert_eq!(
            world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            40
        );
    }

    #[test]
    fn invalid_event_references_and_times_are_rejected() {
        let mut library = library();
        library.config.items[0].animation.as_mut().unwrap().events[0].target = "missing".into();
        assert!(world(&library)
            .err()
            .unwrap()
            .contains("unknown effect target"));
        let event = &mut library.config.items[0].animation.as_mut().unwrap().events[0];
        event.target = "practice-target".into();
        event.at = 500.;
        assert!(world(&library).err().unwrap().contains("clip duration"));
        library.config.items[0].animation.as_mut().unwrap().clip = "missing".into();
        assert!(world(&library).err().unwrap().contains("unknown clip"));
    }

    #[test]
    fn unsupported_effects_are_not_silently_ignored() {
        let text = include_str!("../assets/street-kit/effects-demo.json");
        assert!(serde_json::from_str::<crate::config::SceneConfig>(
            &text.replace("\"damage\"", "\"explode\"")
        )
        .is_err());
        assert!(serde_json::from_str::<crate::config::SceneConfig>(
            &text.replace("\"amount\": 10", "\"ammount\": 10")
        )
        .is_err());
    }
}
