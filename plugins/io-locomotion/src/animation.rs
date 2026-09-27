//! Maps locomotion feedback to authored clips; never changes an Item's physical pose.
use crate::{Error, Locomotion};
use io_game::PluginWorld;
use io_types::RootMotion;
use io_world::{AnimationState, WorldView};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    Idle,
    Walk,
    CrouchIdle,
    CrouchWalk,
    Jump,
    Fall,
    Land,
}
impl Motion {
    pub const ALL: [Self; 7] = [
        Self::Idle,
        Self::Walk,
        Self::CrouchIdle,
        Self::CrouchWalk,
        Self::Jump,
        Self::Fall,
        Self::Land,
    ];
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipDefinition {
    pub clip: String,
    pub speed: f32,
    pub looping: bool,
    pub fixed_root_height: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationDriver {
    clips: Arc<BTreeMap<Motion, (usize, f64)>>,
    settings: Arc<Locomotion>,
    visual: Motion,
}
impl AnimationDriver {
    pub fn new(
        settings: Arc<Locomotion>,
        clips: BTreeMap<Motion, (usize, f64)>,
    ) -> Result<Self, String> {
        if clips.len() != Motion::ALL.len()
            || Motion::ALL
                .iter()
                .any(|m| clips.get(m).is_none_or(|(_, d)| !d.is_finite() || *d <= 0.))
        {
            return Err("unresolved traversal clips".into());
        }
        Ok(Self {
            settings,
            clips: Arc::new(clips),
            visual: Motion::Idle,
        })
    }
    pub fn motion(&self) -> Motion {
        self.visual
    }
    fn state(&self, m: Motion) -> AnimationState {
        let (clip, duration) = self.clips[&m];
        let spec = &self.settings.clips[&m];
        let mut state = if spec.looping {
            AnimationState::looping(clip)
        } else {
            AnimationState::once(clip, duration).expect("validated clip")
        };
        state.set_speed(spec.speed).expect("validated speed");
        state.set_root_motion(if spec.fixed_root_height {
            RootMotion::InPlaceFixedHeight
        } else {
            RootMotion::InPlace
        });
        state
    }
    pub fn update<E>(
        &mut self,
        w: &mut PluginWorld<E>,
        actor: u64,
        next: Motion,
    ) -> Result<(), Error> {
        let old = w
            .item(actor)
            .and_then(|i| i.animation.as_ref())
            .ok_or(Error::InvalidWorld)?;
        if (next != self.visual || old.root_motion() == RootMotion::Authored)
            && !old.transitioning()
        {
            let state = old
                .transition_to(self.state(next), self.settings.blend_seconds)
                .map_err(|_| Error::InvalidWorld)?;
            if !w.set_animation(actor, state) {
                return Err(Error::InvalidWorld);
            }
            self.visual = next;
        }
        Ok(())
    }
}
