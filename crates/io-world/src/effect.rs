use crate::World;

/// Gameplay behavior; implementations must not depend on cameras or mesh poses.
pub trait Effect {
    /// Returns whether the target's retained state changed.
    fn apply(&self, target: u64, world: &mut World) -> bool;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Damage {
    pub amount: u32,
}

impl Effect for Damage {
    fn apply(&self, target: u64, world: &mut World) -> bool {
        world.damage(target, self.amount)
    }
}

/// Serializable content is resolved into this explicit set of supported effects.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectKind {
    Damage(Damage),
}

impl Effect for EffectKind {
    fn apply(&self, target: u64, world: &mut World) -> bool {
        match self {
            Self::Damage(effect) => effect.apply(target, world),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvent {
    pub name: String,
    /// Seconds after clip start, in (0, duration]. Duration fires at the loop end.
    pub at: f64,
    pub target: u64,
    pub effect: EffectKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvents {
    duration: f64,
    events: Vec<AnimationEvent>,
}

pub(crate) struct PendingEffect {
    pub offset: f64,
    pub source: u64,
    pub event_index: usize,
    pub target: u64,
    pub effect: EffectKind,
}

impl AnimationEvents {
    pub fn new(duration: f64, mut events: Vec<AnimationEvent>) -> Result<Self, String> {
        if !duration.is_finite() || duration <= 0. {
            return Err("animation events need a positive clip duration".into());
        }
        let mut names = std::collections::HashSet::new();
        for event in &events {
            if event.name.is_empty() || !names.insert(&event.name) {
                return Err("animation event names must be nonempty and unique".into());
            }
            if !event.at.is_finite() || event.at <= 0. || event.at > duration {
                return Err(format!(
                    "event {} must be within (0, clip duration]",
                    event.name
                ));
            }
        }
        events.sort_by(|a, b| a.at.total_cmp(&b.at));
        Ok(Self { duration, events })
    }

    pub(crate) fn enqueue(
        &self,
        source: u64,
        start: f64,
        end: f64,
        speed: f64,
        queue: &mut Vec<PendingEffect>,
    ) {
        if !start.is_finite()
            || !end.is_finite()
            || !speed.is_finite()
            || speed <= 0.
            || end <= start
        {
            return;
        }
        for (event_index, event) in self.events.iter().enumerate() {
            // Half-open intervals prevent firing twice at an exact tick boundary.
            let first = ((start - event.at) / self.duration).floor() + 1.;
            let last = ((end - event.at) / self.duration).floor();
            if last < first.max(0.) {
                continue;
            }
            for cycle in first.max(0.) as u64..=last as u64 {
                let time = event.at + cycle as f64 * self.duration;
                queue.push(PendingEffect {
                    offset: (time - start) / speed,
                    source,
                    event_index,
                    target: event.target,
                    effect: event.effect.clone(),
                });
            }
        }
    }
}
