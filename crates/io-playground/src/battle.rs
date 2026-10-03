//! Supplied squad tactics. Navigation and visual perception retain their existing authorities.
use crate::{movement::TraversalService, ObservationTrack};
use io_game::channel::{ChannelId, ItemChannel};
use io_game::stage::Stage;
use io_locomotion::{Error, NavigationGoal, NavigationStatus};
use io_perception::intake::{
    ObservationMemory, ObservationRecord, ObservationSource, ProducerId, Remember,
};
use io_types::{MessageId, Vec3};
use io_world::{character_fits, character_support, sight_segment_clear, SupportProbe, WorldView};
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BattleRole {
    Melee,
    Ranged,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleMember {
    pub item: String,
    pub role: BattleRole,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BattleDefinition {
    pub members: Vec<BattleMember>,
    pub preparation_seconds: f32,
    pub memory_seconds: f32,
    pub decision_seconds: f32,
    pub report_seconds: f32,
    pub melee_distance: f32,
    pub ranged_distance: f32,
}
impl Default for BattleDefinition {
    fn default() -> Self {
        Self {
            members: vec![],
            preparation_seconds: 1.2,
            memory_seconds: 12.,
            decision_seconds: 0.75,
            report_seconds: 0.6,
            melee_distance: 1.7,
            ranged_distance: 8.,
        }
    }
}
impl BattleDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=16).contains(&self.members.len())
            || self.members.iter().any(|m| m.item.is_empty())
            || self
                .members
                .iter()
                .map(|m| &m.item)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.members.len()
            || !self.preparation_seconds.is_finite()
            || !(0.1..=10.).contains(&self.preparation_seconds)
            || !self.memory_seconds.is_finite()
            || !(3. ..=60.).contains(&self.memory_seconds)
            || self.memory_seconds <= self.preparation_seconds
            || !self.decision_seconds.is_finite()
            || !(0.2..=3.).contains(&self.decision_seconds)
            || !self.report_seconds.is_finite()
            || !(0.1..=2.).contains(&self.report_seconds)
            || !self.melee_distance.is_finite()
            || !(1.2..=4.).contains(&self.melee_distance)
            || !self.ranged_distance.is_finite()
            || !(4. ..=20.).contains(&self.ranged_distance)
        {
            return Err("invalid battle settings".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattlePhase {
    Idle,
    Preparing,
    Pursuing,
    Searching,
    Engaged,
}

#[derive(Clone, Copy, Debug)]
enum Knowledge {
    Position(Vec3),
    Claim(Option<Vec3>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fact {
    Position,
    Intent,
}
#[derive(Clone, Copy, Debug)]
enum Radio {
    Sighting {
        target: u64,
        position: Vec3,
        observed_at: f64,
    },
    Claim {
        position: Option<Vec3>,
        observed_at: f64,
        until: f64,
    },
}

#[derive(Clone, Debug)]
struct Member {
    actor: u64,
    label: String,
    role: BattleRole,
    phase: BattlePhase,
    observations: ObservationMemory<Fact, Knowledge>,
    preparing_until: f64,
    next_decision: f64,
    next_report: f64,
    goal: Option<NavigationGoal>,
    home: Vec3,
}
impl Member {
    /// This plugin prefers freshest information, breaking equal-time ties in favor of direct sight.
    /// The shared memory preserves other sources rather than making that trust decision for us.
    fn target(&self, target: u64) -> Option<(Vec3, f64, ObservationSource)> {
        self.observations
            .records()
            .filter_map(|(key, record)| match record.payload() {
                Knowledge::Position(position)
                    if *key == Fact::Position && record.subject() == target =>
                {
                    Some((*position, record.observed_at(), record.source()))
                }
                _ => None,
            })
            .max_by(|a, b| {
                a.1.total_cmp(&b.1).then_with(|| {
                    matches!(a.2, ObservationSource::Direct { .. })
                        .cmp(&matches!(b.2, ObservationSource::Direct { .. }))
                })
            })
    }
}
#[derive(Clone, Debug)]
pub struct BattleDiagnostic {
    pub actor: u64,
    pub label: String,
    pub role: BattleRole,
    pub phase: BattlePhase,
    pub source: Option<ObservationSource>,
    pub last_seen: Option<Vec3>,
    pub age: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct Battle {
    settings: BattleDefinition,
    target: u64,
    members: Vec<Member>,
    channel: ItemChannel<Radio>,
    seconds: f64,
    tick: u64,
    log: VecDeque<String>,
    reports: u64,
}
impl Battle {
    pub fn new(
        settings: BattleDefinition,
        target: u64,
        bindings: &[(String, u64, Vec3)],
    ) -> Result<Self, String> {
        settings.validate()?;
        let mut channel = ItemChannel::new(
            ChannelId(1),
            settings.members.len() * 4,
            settings.members.len(),
        )
        .unwrap();
        let mut members = Vec::new();
        for m in &settings.members {
            let (_, actor, home) = bindings
                .iter()
                .find(|(name, _, _)| *name == m.item)
                .ok_or("unresolved battle member")?;
            let actor = *actor;
            if !channel.join(actor).map_err(|e| format!("{e:?}"))? {
                return Err("duplicate battle actor".into());
            }
            members.push(Member {
                actor,
                label: m.item.clone(),
                role: m.role,
                phase: BattlePhase::Idle,
                observations: ObservationMemory::new(
                    actor,
                    settings.members.len() * 3,
                    settings.members.len() + 1,
                )
                .unwrap(),
                preparing_until: 0.,
                next_decision: 0.,
                next_report: 0.,
                goal: None,
                home: *home,
            });
        }
        Ok(Self {
            settings,
            target,
            members,
            channel,
            seconds: 0.,
            tick: 0,
            log: VecDeque::new(),
            reports: 0,
        })
    }
    pub fn diagnostics(&self) -> Vec<BattleDiagnostic> {
        self.members
            .iter()
            .map(|m| BattleDiagnostic {
                actor: m.actor,
                label: m.label.clone(),
                role: m.role,
                phase: m.phase,
                source: m.target(self.target).map(|(_, _, source)| source),
                last_seen: m.target(self.target).map(|(position, _, _)| position),
                age: m
                    .target(self.target)
                    .map_or(0., |(_, at, _)| (self.seconds - at) as f32),
            })
            .collect()
    }
    pub fn reports(&self) -> u64 {
        self.reports
    }
    pub fn controls(&self, actor: u64) -> bool {
        self.members.iter().any(|m| m.actor == actor)
    }
    pub fn log(&self) -> impl DoubleEndedIterator<Item = &String> {
        self.log.iter()
    }

    fn receive(&mut self) -> Result<(), Error> {
        for m in &mut self.members {
            m.observations
                .advance(self.seconds)
                .map_err(|_| Error::InvalidWorld)?;
            for report in self
                .channel
                .poll(m.actor, 64)
                .map_err(|_| Error::InvalidWorld)?
            {
                let sender = report.payload.sender;
                if sender == m.actor {
                    continue;
                }
                match report.payload.payload {
                    Radio::Sighting {
                        target,
                        position,
                        observed_at,
                    } if target == self.target => {
                        let record = ObservationRecord::reported(
                            m.actor,
                            target,
                            &report,
                            (observed_at, self.seconds),
                            Knowledge::Position(position),
                        )
                        .map_err(|_| Error::InvalidWorld)?;
                        m.observations
                            .run(Remember {
                                key: Fact::Position,
                                record,
                                expires_at: observed_at + f64::from(self.settings.memory_seconds),
                            })
                            .map_err(|_| Error::InvalidWorld)?;
                    }
                    Radio::Sighting { .. } => {}
                    Radio::Claim {
                        position,
                        observed_at,
                        until,
                    } => {
                        let record = ObservationRecord::reported(
                            m.actor,
                            sender,
                            &report,
                            (observed_at, self.seconds),
                            Knowledge::Claim(position),
                        )
                        .map_err(|_| Error::InvalidWorld)?;
                        m.observations
                            .run(Remember {
                                key: Fact::Intent,
                                record,
                                expires_at: until,
                            })
                            .map_err(|_| Error::InvalidWorld)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn update(
        &mut self,
        world: &dyn WorldView,
        tracks: &[ObservationTrack],
        movement: &TraversalService,
        dt: f32,
    ) -> Result<BTreeMap<u64, Option<NavigationGoal>>, Error> {
        self.seconds += f64::from(dt);
        self.tick = self.tick.checked_add(1).ok_or(Error::InvalidInput)?;
        self.receive()?;
        let mut outgoing = Vec::new();
        let mut decisions = BTreeMap::new();
        let settings = &self.settings;
        for (index, m) in self.members.iter_mut().enumerate() {
            let feedback = movement.actor(m.actor).ok_or(Error::InvalidWorld)?;
            let position = feedback.execution.position;
            let track = tracks
                .iter()
                .find(|t| t.observer() == m.actor && t.target() == self.target)
                .ok_or(Error::InvalidWorld)?;
            let direct = track
                .contact()
                .filter(|s| s.focus >= 0.03 && s.attention >= track.notice_attention());
            if let Some(s) = direct {
                let record = ObservationRecord::direct(
                    m.actor,
                    self.target,
                    ProducerId(1),
                    MessageId(self.tick),
                    (self.seconds, self.seconds),
                    Knowledge::Position(s.position),
                )
                .map_err(|_| Error::InvalidWorld)?;
                m.observations
                    .run(Remember {
                        key: Fact::Position,
                        record,
                        expires_at: self.seconds + f64::from(settings.memory_seconds),
                    })
                    .map_err(|_| Error::InvalidWorld)?;
                if self.seconds >= m.next_report {
                    outgoing.push((
                        m.actor,
                        Radio::Sighting {
                            target: self.target,
                            position: s.position,
                            observed_at: self.seconds,
                        },
                    ));
                    m.next_report = self.seconds + f64::from(settings.report_seconds);
                }
            }
            let sighting = m.target(self.target);
            if sighting.is_none() && m.phase != BattlePhase::Idle {
                m.goal = None;
                m.phase = BattlePhase::Idle;
                decisions.insert(m.actor, Some(NavigationGoal::Position(m.home)));
                outgoing.push((
                    m.actor,
                    Radio::Claim {
                        position: None,
                        observed_at: self.seconds,
                        until: self.seconds + 3.,
                    },
                ));
            }
            let Some((seen_position, seen_at, _)) = sighting else {
                continue;
            };
            if m.phase == BattlePhase::Idle {
                m.phase = BattlePhase::Preparing;
                m.preparing_until = self.seconds + f64::from(settings.preparation_seconds);
                m.next_decision = m.preparing_until;
                decisions.insert(m.actor, None);
            }
            if self.seconds < m.preparing_until {
                continue;
            }
            let stale = self.seconds - seen_at > 1.5;
            if stale {
                m.phase = BattlePhase::Searching;
            } else if m.phase != BattlePhase::Engaged || direct.is_none() {
                m.phase = BattlePhase::Pursuing;
            }
            if self.seconds < m.next_decision {
                continue;
            }
            m.next_decision = self.seconds + f64::from(settings.decision_seconds);
            let blocked = matches!(
                feedback.status,
                NavigationStatus::Blocked
                    | NavigationStatus::Unreachable
                    | NavigationStatus::Failed
            );
            let goal = choose_goal(world, m, index, seen_position, settings, blocked);
            if direct.is_some()
                && distance(position, seen_position)
                    <= match m.role {
                        BattleRole::Melee => settings.melee_distance + 0.4,
                        BattleRole::Ranged => settings.ranged_distance + 2.,
                    }
                && m.goal
                    .is_some_and(|g| distance(position, goal_position(g)) < 0.8)
            {
                m.phase = BattlePhase::Engaged;
            }
            // Keep a useful route while planning. Meaningful displacement or failure permits a replacement.
            let changed = m
                .goal
                .is_none_or(|old| distance(goal_position(old), goal_position(goal)) > 0.8);
            if changed || blocked {
                m.goal = Some(goal);
                decisions.insert(m.actor, Some(goal));
            }
            outgoing.push((
                m.actor,
                Radio::Claim {
                    position: m.goal.map(goal_position),
                    observed_at: self.seconds,
                    until: self.seconds + 3.,
                },
            ));
        }
        // All readers consume first; reports become observations on the next update.
        for (sender, payload) in outgoing {
            self.channel
                .send(sender, self.tick, payload)
                .map_err(|_| Error::InvalidWorld)?;
            self.reports += 1;
            if matches!(payload, Radio::Sighting { .. }) {
                if self.log.len() == 8 {
                    self.log.pop_front();
                }
                self.log
                    .push_back(format!("RADIO {sender}: TARGET LAST SEEN"));
            }
        }
        Ok(decisions)
    }
}

fn distance(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.dot(d).sqrt()
}
fn goal_position(goal: NavigationGoal) -> Vec3 {
    match goal {
        NavigationGoal::Position(p) | NavigationGoal::Approach { position: p, .. } => p,
    }
}

fn choose_goal(
    world: &dyn WorldView,
    m: &Member,
    index: usize,
    target: Vec3,
    settings: &BattleDefinition,
    blocked: bool,
) -> NavigationGoal {
    let shape = world
        .item(m.actor)
        .and_then(|i| i.character_body)
        .expect("validated battle body");
    let reserved = |p| {
        m.observations.records().any(|(key, r)| *key == Fact::Intent && r.subject() < m.actor
            && matches!(r.payload(), Knowledge::Claim(Some(position)) if distance(*position, p) < 1.2))
    };
    let fits = |p| {
        character_fits(world, m.actor, p, shape)
            && character_support(world, Some(m.actor), p, shape, SupportProbe::CONTACT)
                .is_ok_and(|s| s.is_some())
    };
    let eye = Vec3::new(0., 0., shape.height * 0.8);
    let radius = match m.role {
        BattleRole::Melee => settings.melee_distance,
        BattleRole::Ranged => settings.ranged_distance,
    };
    for offset in 0..8 {
        let angle = ((index * 2 + offset) % 8) as f32 * std::f32::consts::FRAC_PI_4;
        let p = target + Vec3::new(angle.cos() * radius, angle.sin() * radius, 0.);
        if !reserved(p)
            && fits(p)
            && sight_segment_clear(world, p + eye, target + eye).unwrap_or(false)
            && (!blocked
                || m.goal
                    .is_none_or(|old| distance(goal_position(old), p) > 1.))
        {
            return NavigationGoal::Position(p);
        }
    }
    NavigationGoal::Approach {
        position: target,
        distance: radius,
        repath_seconds: settings.decision_seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn squad() -> Battle {
        Battle::new(
            BattleDefinition {
                members: vec![
                    BattleMember {
                        item: "a".into(),
                        role: BattleRole::Melee,
                    },
                    BattleMember {
                        item: "b".into(),
                        role: BattleRole::Ranged,
                    },
                ],
                ..Default::default()
            },
            99,
            &[
                ("a".into(), 10, Vec3::default()),
                ("b".into(), 20, Vec3::default()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn channel_sightings_enter_shared_memory_with_original_age_and_expire_without_new_messages() {
        let mut b = squad();
        b.channel
            .send(
                10,
                100,
                Radio::Sighting {
                    target: 99,
                    position: Vec3::new(1., 2., 0.),
                    observed_at: 1.,
                },
            )
            .unwrap();
        b.seconds = 2.;
        b.receive().unwrap();
        assert!(
            b.members[0].target(99).is_none(),
            "sender does not gain a second reported observation"
        );
        let (position, at, origin) = b.members[1].target(99).unwrap();
        assert_eq!(position, Vec3::new(1., 2., 0.));
        assert_eq!(at, 1.);
        assert_eq!(
            origin,
            ObservationSource::Reported {
                sender: 10,
                channel: ChannelId(1)
            }
        );
        assert_eq!(b.diagnostics()[1].age, 1.);
        assert_eq!(
            b.members[1]
                .observations
                .records()
                .next()
                .unwrap()
                .1
                .received_at(),
            2.
        );
        b.seconds = 13.;
        b.receive().unwrap();
        assert!(b.members[1].target(99).is_none());
        assert_eq!(b.channel.buffered(), 0);
    }

    #[test]
    fn plugin_prefers_fresh_direct_sight_but_retains_reported_observation_separately() {
        let mut b = squad();
        let direct = ObservationRecord::direct(
            20,
            99,
            ProducerId(1),
            MessageId(1),
            (2., 2.),
            Knowledge::Position(Vec3::new(2., 0., 0.)),
        )
        .unwrap();
        b.members[1]
            .observations
            .run(Remember {
                key: Fact::Position,
                record: direct,
                expires_at: 14.,
            })
            .unwrap();
        b.channel
            .send(
                10,
                100,
                Radio::Sighting {
                    target: 99,
                    position: Vec3::new(1., 0., 0.),
                    observed_at: 1.,
                },
            )
            .unwrap();
        b.seconds = 3.;
        b.receive().unwrap();
        assert_eq!(b.members[1].observations.len(), 2);
        assert!(matches!(
            b.members[1].target(99).unwrap().2,
            ObservationSource::Direct { .. }
        ));
        b.channel
            .send(
                10,
                101,
                Radio::Sighting {
                    target: 99,
                    position: Vec3::new(4., 0., 0.),
                    observed_at: 4.,
                },
            )
            .unwrap();
        b.seconds = 4.;
        b.receive().unwrap();
        assert!(matches!(
            b.members[1].target(99).unwrap().2,
            ObservationSource::Reported { .. }
        ));
        assert_eq!(b.members[1].observations.len(), 2);
    }

    #[test]
    fn radio_claims_and_releases_use_the_same_intake_without_becoming_sightings() {
        let mut b = squad();
        b.channel
            .send(
                10,
                1,
                Radio::Claim {
                    position: Some(Vec3::new(1., 0., 0.)),
                    observed_at: 1.,
                    until: 4.,
                },
            )
            .unwrap();
        b.seconds = 1.;
        b.receive().unwrap();
        assert!(b.members[1].target(99).is_none());
        assert!(matches!(
            b.members[1]
                .observations
                .records()
                .next()
                .unwrap()
                .1
                .payload(),
            Knowledge::Claim(Some(_))
        ));
        b.channel
            .send(
                10,
                2,
                Radio::Claim {
                    position: None,
                    observed_at: 2.,
                    until: 5.,
                },
            )
            .unwrap();
        b.seconds = 2.;
        b.receive().unwrap();
        assert_eq!(b.members[1].observations.len(), 1);
        assert!(matches!(
            b.members[1]
                .observations
                .records()
                .next()
                .unwrap()
                .1
                .payload(),
            Knowledge::Claim(None)
        ));
    }
}
