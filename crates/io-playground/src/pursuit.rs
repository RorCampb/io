//! Example game policy. Decisions receive sightings, never the hidden target's world pose.
use io_traversal::NavigationStatus as Status;
use io_types::Vec3;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PursuitDefinition {
    pub target: String,
    pub settings: PursuitSettings,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PursuitSettings {
    pub notice_attention: f32,
    pub minimum_focus: f32,
    pub lost_sight_seconds: f32,
    pub search_seconds: f32,
    pub search_radius: f32,
    pub repath_seconds: f32,
    pub tag_distance: f32,
    pub grace_seconds: f32,
}
impl PursuitSettings {
    pub fn validate(self) -> Result<(), String> {
        for (value, min, max) in [
            (self.notice_attention, 0.01, 1.),
            (self.minimum_focus, 0.001, 1.),
            (self.lost_sight_seconds, 0.1, 10.),
            (self.search_seconds, 1., 120.),
            (self.search_radius, 0.5, 20.),
            (self.repath_seconds, 0.1, 5.),
            (self.tag_distance, 0.5, 5.),
            (self.grace_seconds, 1., 30.),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err("invalid pursuit settings".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PursuitBinding {
    pub target: u64,
    pub settings: PursuitSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PursuitPhase {
    Patrol,
    Chase,
    Search,
    Tagged,
}

use io_perception::VisualContact as Sighting;

#[derive(Clone, Debug)]
enum State {
    Patrol,
    Chase {
        last_seen: Vec3,
        unseen: f32,
    },
    Search {
        last_seen: Vec3,
        elapsed: f32,
        leg: usize,
    },
    Tagged {
        remaining: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Intent {
    Patrol,
    Approach(Vec3),
    SearchAt(Vec3),
    Hold,
}

#[derive(Clone, Debug)]
pub(crate) struct Pursuit {
    pub binding: PursuitBinding,
    state: State,
    tags: u32,
}
impl Pursuit {
    pub fn new(binding: PursuitBinding) -> Result<Self, String> {
        binding.settings.validate()?;
        Ok(Self {
            binding,
            state: State::Patrol,
            tags: 0,
        })
    }
    pub fn phase(&self) -> PursuitPhase {
        match self.state {
            State::Patrol => PursuitPhase::Patrol,
            State::Chase { .. } => PursuitPhase::Chase,
            State::Search { .. } => PursuitPhase::Search,
            State::Tagged { .. } => PursuitPhase::Tagged,
        }
    }
    pub fn tags(&self) -> u32 {
        self.tags
    }

    pub fn decide(
        &mut self,
        dt: f32,
        position: Vec3,
        sight: Option<Sighting>,
        navigation: Status,
    ) -> Intent {
        let settings = self.binding.settings;
        let sight = sight.filter(|s| s.focus >= settings.minimum_focus);
        match &mut self.state {
            State::Tagged { remaining } => {
                *remaining -= dt;
                if *remaining <= 0. {
                    self.state = State::Patrol;
                    return Intent::Patrol;
                }
                return Intent::Hold;
            }
            State::Patrol | State::Search { .. } => {
                if let Some(s) = sight.filter(|s| s.attention >= settings.notice_attention) {
                    self.state = State::Chase {
                        last_seen: s.position,
                        unseen: 0.,
                    };
                }
            }
            State::Chase { .. } => {}
        }
        match &mut self.state {
            State::Patrol => Intent::Patrol,
            State::Chase { last_seen, unseen } => {
                if let Some(s) = sight {
                    *last_seen = s.position;
                    *unseen = 0.;
                    let delta = s.position - position;
                    if delta.dot(delta) <= settings.tag_distance * settings.tag_distance {
                        self.tags = self.tags.saturating_add(1);
                        self.state = State::Tagged {
                            remaining: settings.grace_seconds,
                        };
                        return Intent::Hold;
                    }
                } else {
                    *unseen += dt;
                }
                let last = *last_seen;
                if *unseen >= settings.lost_sight_seconds {
                    self.state = State::Search {
                        last_seen: last,
                        elapsed: 0.,
                        leg: 0,
                    };
                    Intent::SearchAt(last)
                } else {
                    Intent::Approach(last)
                }
            }
            State::Search {
                last_seen,
                elapsed,
                leg,
            } => {
                *elapsed += dt;
                if *elapsed >= settings.search_seconds {
                    self.state = State::Patrol;
                    return Intent::Patrol;
                }
                if matches!(
                    navigation,
                    Status::Arrived | Status::Unreachable | Status::Blocked
                ) {
                    *leg += 1;
                }
                // A small deterministic sweep, centered only on the last genuine sighting.
                let offsets = [(0., 0.), (-1., 0.), (0., 1.), (1., 0.), (0., -1.)];
                let (x, y) = offsets[*leg % offsets.len()];
                Intent::SearchAt(*last_seen + Vec3::new(x, y, 0.).scaled(settings.search_radius))
            }
            State::Tagged { .. } => Intent::Hold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hunter() -> Pursuit {
        Pursuit::new(PursuitBinding {
            target: 2,
            settings: PursuitSettings {
                notice_attention: 0.2,
                minimum_focus: 0.03,
                lost_sight_seconds: 0.5,
                search_seconds: 3.,
                search_radius: 2.,
                repath_seconds: 0.5,
                tag_distance: 1.2,
                grace_seconds: 2.,
            },
        })
        .unwrap()
    }
    fn sight(position: Vec3) -> Option<Sighting> {
        Some(Sighting {
            position,
            focus: 0.8,
            attention: 0.7,
        })
    }
    #[test]
    fn lowering_notice_threshold_changes_decision_without_changing_evidence() {
        let mut h = hunter();
        let sight = Some(Sighting {
            position: Vec3::new(5., 0., 0.),
            focus: 0.15,
            attention: 0.15,
        });
        h.decide(0.1, Vec3::default(), sight, Status::Idle);
        assert_eq!(h.phase(), PursuitPhase::Patrol);
        h.binding.settings.notice_attention = 0.1;
        h.decide(0.1, Vec3::default(), sight, Status::Idle);
        assert_eq!(h.phase(), PursuitPhase::Chase);
    }
    #[test]
    fn requires_evidence_and_attention_then_searches_only_last_seen_position() {
        let mut h = hunter();
        let position = Vec3::default();
        let target = Vec3::new(5., 4., 0.);
        h.decide(
            0.1,
            position,
            Some(Sighting {
                position: target,
                focus: 0.01,
                attention: 0.9,
            }),
            Status::Following,
        );
        assert_eq!(h.phase(), PursuitPhase::Patrol);
        h.decide(
            0.1,
            position,
            Some(Sighting {
                position: target,
                focus: 0.9,
                attention: 0.1,
            }),
            Status::Following,
        );
        assert_eq!(h.phase(), PursuitPhase::Patrol);
        assert!(
            matches!(h.decide(0.1, position, sight(target), Status::Following), Intent::Approach(p) if p == target)
        );
        for _ in 0..6 {
            h.decide(0.1, position, None, Status::Following);
        }
        assert_eq!(h.phase(), PursuitPhase::Search);
        assert!(
            matches!(h.decide(0.1, position, None, Status::Following), Intent::SearchAt(p) if p == target)
        );
        for _ in 0..40 {
            h.decide(0.1, position, None, Status::Following);
        }
        assert_eq!(h.phase(), PursuitPhase::Patrol);
    }
    #[test]
    fn reacquires_and_tags_only_visible_targets_with_escape_grace() {
        let mut h = hunter();
        let position = Vec3::default();
        h.decide(
            0.1,
            position,
            sight(Vec3::new(4., 0., 0.)),
            Status::Following,
        );
        for _ in 0..6 {
            h.decide(0.1, position, None, Status::Following);
        }
        h.decide(
            0.1,
            position,
            sight(Vec3::new(0.8, 0., 0.)),
            Status::Following,
        );
        assert_eq!(h.phase(), PursuitPhase::Tagged);
        assert_eq!(h.tags(), 1);
        for _ in 0..10 {
            h.decide(
                0.1,
                position,
                sight(Vec3::new(0.8, 0., 0.)),
                Status::Following,
            );
        }
        assert_eq!(h.tags(), 1);
        for _ in 0..12 {
            h.decide(0.1, position, None, Status::Following);
        }
        assert_eq!(h.phase(), PursuitPhase::Patrol);
    }
    #[test]
    fn invalid_settings_are_rejected() {
        let mut binding = hunter().binding;
        binding.settings.repath_seconds = 0.;
        assert!(Pursuit::new(binding.clone()).is_err());
        binding.settings.repath_seconds = f32::NAN;
        assert!(Pursuit::new(binding).is_err());
    }
}
