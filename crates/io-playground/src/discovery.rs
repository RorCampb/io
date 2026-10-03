//! Supplied playground policy: bounded nearby scenery and optional character observations.
use crate::reaction::{
    MotionEstimate, ObservedMotion, ReactionRequest, ReactionSettings, ReactionStage,
};
use crate::{ObservationBinding, ObservationTrack};
use io_game::stage::Stage;
use io_perception::{pipeline::ItemNotice, VisionProfile};
use io_traversal::{NavigationPriority, NavigationRequest, NavigationStatus};
use io_types::{Bounds, Rotation, Vec3};
use io_world::{Collider, WorldView};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObstacleObservationSettings {
    pub reaction: Option<ReactionSettings>,
    pub vision: VisionProfile,
    pub notice_attention: f32,
    pub interval_seconds: f32,
    pub retention_seconds: f32,
    pub observers_per_tick: usize,
    pub tracks_per_observer: usize,
    /// Separate capacity so nearby characters cannot evict scenery observations.
    pub character_tracks_per_observer: usize,
}
impl Default for ObstacleObservationSettings {
    fn default() -> Self {
        Self {
            reaction: None,
            vision: VisionProfile {
                range: 18.,
                fov_degrees: 200.,
                gain_per_second: 6.,
                decay_per_second: 2.,
            },
            notice_attention: 0.15,
            interval_seconds: 0.1,
            retention_seconds: 2.,
            observers_per_tick: 8,
            tracks_per_observer: 6,
            character_tracks_per_observer: 0,
        }
    }
}
impl ObstacleObservationSettings {
    pub fn validate(self) -> Result<(), String> {
        if let Some(settings) = self.reaction {
            settings.validate()?;
        }
        self.vision.validate()?;
        if !self.notice_attention.is_finite()
            || !(0.01..=1.).contains(&self.notice_attention)
            || !self.interval_seconds.is_finite()
            || !(0.05..=0.5).contains(&self.interval_seconds)
            || !self.retention_seconds.is_finite()
            || !(self.interval_seconds..=30.).contains(&self.retention_seconds)
            || !(1..=32).contains(&self.observers_per_tick)
            || !(1..=16).contains(&self.tracks_per_observer)
            || self.character_tracks_per_observer > 16
        {
            return Err("invalid obstacle observation settings".into());
        }
        Ok(())
    }
}

pub struct ObstacleDiscoveryRequest<'a> {
    pub world: &'a dyn WorldView,
    pub observer: u64,
    pub settings: ObstacleObservationSettings,
    pub explicit_tracks: &'a [ObservationTrack],
}
pub struct ObstacleDiscoveryStage;
impl Stage<ObstacleDiscoveryRequest<'_>> for ObstacleDiscoveryStage {
    type Output = Vec<u64>;
    type Error = String;
    fn run(&mut self, r: ObstacleDiscoveryRequest<'_>) -> Result<Vec<u64>, String> {
        r.settings.validate()?;
        let actor = r.world.item(r.observer).ok_or("missing observer")?;
        let body = actor.character_body.ok_or("observer needs a body")?;
        let p = actor.transform.anchor;
        let mut candidates = r
            .world
            .query(p, r.settings.vision.range)
            .into_iter()
            .filter_map(|index| {
                let item = &r.world.items()[index];
                if item.id == r.observer
                    || (item.character_body.is_none() && item.collider.is_none())
                    || (item.character_body.is_some()
                        && r.settings.character_tracks_per_observer == 0)
                    || r.explicit_tracks
                        .iter()
                        .any(|t| t.observer() == r.observer && t.target() == item.id)
                {
                    return None;
                }
                let b = item.current_spatial_bounds();
                // This plugin observes obstacles at body height, not support floors or roofs.
                if b.max.z <= p.z + 0.05 || b.min.z >= p.z + body.height {
                    return None;
                }
                let near = Vec3::new(
                    p.x.clamp(b.min.x, b.max.x),
                    p.y.clamp(b.min.y, b.max.y),
                    p.z.clamp(b.min.z, b.max.z),
                );
                let d = (near - p).dot(near - p);
                (d <= r.settings.vision.range.powi(2)).then_some((
                    d,
                    item.id,
                    item.character_body.is_some(),
                ))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        candidates.dedup_by_key(|c| c.1);
        let (mut scenery, mut characters) = (0, 0);
        candidates.retain(|&(_, _, character)| {
            let (count, limit) = if character {
                (&mut characters, r.settings.character_tracks_per_observer)
            } else {
                (&mut scenery, r.settings.tracks_per_observer)
            };
            *count += 1;
            *count <= limit
        });
        Ok(candidates.into_iter().map(|(_, id, _)| id).collect())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct DiscoveryStats {
    pub character_samples: u64,
    pub character_notices: u64,
    pub motion_samples: u64,
    pub predicted_conflicts: u64,
    pub urgent_reactions: u64,
    pub predictive_replans: u64,
    pub scans: u64,
    pub samples: u64,
    pub notices: u64,
    pub reconsiderations: u64,
    pub unchanged_notices: u64,
    pub tracks: usize,
}
#[derive(Clone, Debug)]
struct Tracked {
    character: bool,
    motion: MotionEstimate,
    track: ObservationTrack,
    last_near: f64,
    noticed_geometry: Option<NoticedGeometry>,
    route_notice: Option<ItemNotice>,
}
/// Captured only after a qualified notice, never from an unseen world mutation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NoticedGeometry {
    anchor: Vec3,
    rotation: Rotation,
    collider: Option<Collider>,
    bounds: Bounds,
}
#[derive(Clone, Debug)]
struct Observer {
    actor: u64,
    sampled_at: f64,
    tracks: Vec<Tracked>,
}
#[derive(Clone, Debug)]
pub(crate) struct ObstacleObservations {
    motions: Vec<ObservedMotion>,
    reactions: std::collections::BTreeMap<u64, (f64, NavigationPriority)>,
    settings: ObstacleObservationSettings,
    observers: Vec<Observer>,
    cursor: usize,
    seconds: f64,
    pub stats: DiscoveryStats,
}
impl ObstacleObservations {
    pub fn tracks(&self) -> impl Iterator<Item = &ObservationTrack> {
        self.observers
            .iter()
            .flat_map(|o| o.tracks.iter().map(|t| &t.track))
    }
    /// Prefer attended characters for the traffic HUD, otherwise strongest scenery.
    /// Both values still refer to one target, not independent maxima.
    pub fn meters(&self) -> Vec<(u64, f32, f32)> {
        self.observers
            .iter()
            .map(|o| {
                let best = o.tracks.iter().max_by(|a, b| {
                    (a.character && a.track.attention() >= self.settings.notice_attention)
                        .cmp(
                            &(b.character && b.track.attention() >= self.settings.notice_attention),
                        )
                        .then_with(|| a.track.attention().total_cmp(&b.track.attention()))
                });
                (
                    o.actor,
                    best.map_or(0., |t| t.track.evidence()),
                    best.map_or(0., |t| t.track.attention()),
                )
            })
            .collect()
    }
    pub fn reactions(
        &mut self,
        movement: &crate::movement::TraversalService,
        world: &dyn WorldView,
    ) -> Result<Vec<NavigationRequest>, String> {
        let Some(settings) = self.settings.reaction else {
            return Ok(vec![]);
        };
        let mut conflicts =
            std::collections::BTreeMap::<u64, crate::reaction::PredictedConflict>::new();
        for observation in &self.motions {
            let Some(actor) = movement.actor(observation.observer) else {
                continue;
            };
            if !matches!(
                actor.status,
                NavigationStatus::Following
                    | NavigationStatus::Planning
                    | NavigationStatus::Blocked
            ) {
                continue;
            }
            let Some(body) = world
                .item(observation.observer)
                .and_then(|i| i.character_body)
            else {
                continue;
            };
            let route = movement.route(observation.observer);
            if !movement.has_route(observation.observer) {
                continue;
            }
            if let Some(conflict) = ReactionStage.run(ReactionRequest {
                observation: *observation,
                now: self.seconds,
                position: actor.execution.position,
                velocity: actor.execution.actual_velocity,
                body,
                route: &route,
                settings,
            })? {
                let best = conflicts.entry(observation.observer).or_insert(conflict);
                if conflict.seconds < best.seconds {
                    *best = conflict;
                }
            }
        }
        let mut requests = vec![];
        for (id, conflict) in conflicts {
            let ticket = movement.actor(id).unwrap().ticket;
            self.stats.predicted_conflicts += 1;
            requests.push(NavigationRequest::Prioritize {
                ticket,
                priority: conflict.priority,
                seconds: settings.lease_seconds,
            });
            let state = self
                .reactions
                .entry(id)
                .or_insert((f64::NEG_INFINITY, NavigationPriority::Routine));
            if self.seconds - state.0 >= f64::from(settings.cooldown_seconds)
                || conflict.priority > state.1
            {
                requests.push(NavigationRequest::Reconsider { ticket });
                self.stats.predictive_replans += 1;
                if conflict.priority == NavigationPriority::Urgent {
                    self.stats.urgent_reactions += 1;
                }
                *state = (self.seconds, conflict.priority);
            }
        }
        Ok(requests)
    }
    pub fn take_route_notices(&mut self, mut ready: impl FnMut(u64) -> bool) -> Vec<ItemNotice> {
        self.observers
            .iter_mut()
            .filter(|o| ready(o.actor))
            .flat_map(|o| o.tracks.iter_mut().filter_map(|t| t.route_notice.take()))
            .collect()
    }
    pub fn new(settings: ObstacleObservationSettings, actors: impl Iterator<Item = u64>) -> Self {
        Self {
            motions: vec![],
            reactions: Default::default(),
            settings,
            observers: actors
                .map(|actor| Observer {
                    actor,
                    sampled_at: f64::NEG_INFINITY,
                    tracks: vec![],
                })
                .collect(),
            cursor: 0,
            seconds: 0.,
            stats: Default::default(),
        }
    }
    pub fn update(
        &mut self,
        world: &dyn WorldView,
        explicit: &[ObservationTrack],
        dt: f32,
    ) -> Result<Vec<ItemNotice>, String> {
        self.settings.validate()?;
        if !dt.is_finite() || !(0. ..=1.).contains(&dt) {
            return Err("invalid observation timestep".into());
        }
        self.seconds += f64::from(dt);
        self.motions.clear();
        let mut notices = Vec::new();
        // Rotate even when an observer isn't due. No all-NPC/all-Item scan or catch-up burst.
        for _ in 0..self.settings.observers_per_tick.min(self.observers.len()) {
            let count = self.observers.len();
            let observer = &mut self.observers[self.cursor];
            self.cursor = (self.cursor + 1) % count;
            if world.item(observer.actor).is_none() {
                self.stats.tracks -= observer.tracks.len();
                observer.tracks.clear();
                continue;
            }
            let elapsed = self.seconds - observer.sampled_at;
            if elapsed < f64::from(self.settings.interval_seconds) {
                continue;
            }
            observer.sampled_at = self.seconds;
            let ids = ObstacleDiscoveryStage.run(ObstacleDiscoveryRequest {
                world,
                observer: observer.actor,
                settings: self.settings,
                explicit_tracks: explicit,
            })?;
            self.stats.scans += 1;
            self.stats.tracks -= observer.tracks.len();
            observer.tracks.retain_mut(|entry| {
                if ids.contains(&entry.track.target()) {
                    entry.last_near = self.seconds;
                }
                world.item(entry.track.target()).is_some()
                    && world
                        .item(entry.track.target())
                        .is_some_and(|i| i.character_body.is_some() == entry.character)
                    && !explicit.iter().any(|t| {
                        t.observer() == observer.actor && t.target() == entry.track.target()
                    })
                    && self.seconds - entry.last_near <= f64::from(self.settings.retention_seconds)
            });
            // Freshly discovered Items start at zero attention; discovery itself grants no knowledge.
            let mut fresh = Vec::new();
            for id in ids.iter().copied() {
                if observer.tracks.iter().any(|t| t.track.target() == id) {
                    continue;
                }
                let character = world.item(id).unwrap().character_body.is_some();
                let limit = if character {
                    self.settings.character_tracks_per_observer
                } else {
                    self.settings.tracks_per_observer
                };
                if observer
                    .tracks
                    .iter()
                    .filter(|t| t.character == character)
                    .count()
                    == limit
                {
                    let Some(index) = observer
                        .tracks
                        .iter()
                        .position(|t| t.character == character && !ids.contains(&t.track.target()))
                    else {
                        continue;
                    };
                    observer.tracks.remove(index);
                }
                observer.tracks.push(Tracked {
                    character,
                    motion: Default::default(),
                    last_near: self.seconds,
                    noticed_geometry: None,
                    route_notice: None,
                    track: ObservationTrack::new(
                        ObservationBinding {
                            observer: observer.actor,
                            target: id,
                            label: format!(
                                "{}-{id}",
                                if character { "character" } else { "obstacle" }
                            ),
                            vision: self.settings.vision,
                            notice_attention: self.settings.notice_attention,
                        },
                        world,
                    )?,
                });
                fresh.push(id);
            }
            self.stats.tracks += observer.tracks.len();
            for entry in &mut observer.tracks {
                // Never credit a newly acquired track with time before its discovery.
                // Long scheduling gaps cannot instantly fill attention on reacquisition.
                let seconds = if fresh.contains(&entry.track.target()) {
                    0.
                } else {
                    elapsed.min(0.25) as f32
                };
                entry.track.update(world, seconds)?;
                self.stats.samples += 1;
                if entry.character {
                    self.stats.character_samples += 1;
                }
                // Seeing another actor is not the scenery braking policy. Crowd
                // response belongs to behavior/steering, not automatic global replans.
                if !entry.character && self.settings.reaction.is_some() {
                    let seen = entry.track.evidence() > 0.03;
                    let bounds = seen.then(|| {
                        world
                            .item(entry.track.target())
                            .unwrap()
                            .current_spatial_bounds()
                    });
                    if let Some(velocity) = entry.motion.sample(bounds, self.seconds) {
                        if entry.track.attention() >= self.settings.notice_attention {
                            self.stats.motion_samples += 1;
                            self.motions.push(ObservedMotion {
                                observer: observer.actor,
                                target: entry.track.target(),
                                bounds: bounds.unwrap(),
                                velocity,
                                observed_at: self.seconds,
                            });
                        }
                    }
                }
                if let Some(notice) = entry.track.notice() {
                    self.stats.notices += 1;
                    if entry.character {
                        self.stats.character_notices += 1;
                        notices.push(notice);
                        continue;
                    }
                    let item = world.item(notice.target).ok_or("missing noticed Item")?;
                    let geometry = NoticedGeometry {
                        anchor: item.transform.anchor,
                        rotation: item.transform.rotation,
                        collider: item.collider,
                        bounds: notice.bounds,
                    };
                    if entry.noticed_geometry == Some(geometry) {
                        self.stats.unchanged_notices += 1;
                        continue;
                    }
                    entry.noticed_geometry = Some(geometry);
                    // Preserve the earliest unhandled old bounds when changes coalesce.
                    let mut route_notice = notice;
                    if let Some(pending) = entry.route_notice {
                        route_notice.previous_bounds =
                            pending.previous_bounds.or(Some(pending.bounds));
                    }
                    entry.route_notice = Some(route_notice);
                    notices.push(notice);
                }
            }
        }
        Ok(notices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_perception::pipeline::NoticeKind;
    use io_world::{
        BodyKind, CharacterBody, Collider, ColliderShape, Item, PhysicsBody, Space, Transform,
        World,
    };

    fn fixture() -> World {
        let actor = Item {
            id: 1,
            character_body: Some(CharacterBody {
                radius: 0.35,
                height: 1.9,
                max_slope: 0.8,
            }),
            ..Default::default()
        };
        let block = |id, p, half: Vec3| Item {
            id,
            transform: Transform::new(p, Vec3::new(1., 1., 1.), 0.).unwrap(),
            collider: Some(Collider::new(ColliderShape::Box { half_extents: half })),
            physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
            occupancy: io_world::Occupancy {
                local_bounds: io_types::Bounds {
                    min: half.scaled(-1.),
                    max: half,
                },
            },
            ..Default::default()
        };
        World::new(
            Space::new(Vec3::new(100., 100., 30.)),
            vec![
                actor,
                block(2, Vec3::new(0., -4., 1.), Vec3::new(1., 0.5, 1.)),
                block(3, Vec3::new(0., -7., 1.), Vec3::new(0.5, 0.5, 1.)),
                block(4, Vec3::new(0., 0., -0.5), Vec3::new(20., 20., 0.5)),
                block(5, Vec3::new(0., 0., 5.), Vec3::new(20., 20., 0.5)),
            ],
        )
    }

    #[test]
    fn characters_notice_each_other_without_colliders_or_scenery_replan_floods() {
        let original = fixture();
        let mut items = original.items().to_vec();
        let mut peer = items[0].clone();
        peer.id = 10;
        peer.transform.anchor = Vec3::new(2., -2., 0.);
        peer.transform.rotation = Rotation::yaw(std::f32::consts::PI).unwrap();
        items.push(peer);
        let mut hidden = items[0].clone();
        hidden.id = 11;
        hidden.transform.anchor = Vec3::new(0., -7., 0.);
        items.push(hidden);
        let world = World::new(original.space().clone(), items);
        let settings = ObstacleObservationSettings {
            reaction: Some(Default::default()),
            tracks_per_observer: 1,
            character_tracks_per_observer: 2,
            ..Default::default()
        };
        let mut observations = ObstacleObservations::new(settings, [1, 10].into_iter());
        let mut notices = vec![];
        for _ in 0..30 {
            notices.extend(observations.update(&world, &[], 0.1).unwrap());
        }
        assert!(notices.iter().any(|n| n.observer == 1 && n.target == 10));
        assert!(notices.iter().any(|n| n.observer == 10 && n.target == 1));
        assert!(
            !notices.iter().any(|n| n.observer == 1 && n.target == 11),
            "hidden character cannot be noticed"
        );
        assert!(observations.observers.iter().all(|o| o.tracks.len() <= 3));
        assert!(
            observations.observers[0]
                .tracks
                .iter()
                .any(|t| !t.character),
            "characters must not evict scenery"
        );
        assert!(observations.stats.character_notices > 0);
        assert!(observations
            .motions
            .iter()
            .all(|m| m.target != 1 && m.target != 10 && m.target != 11));
        assert!(observations
            .take_route_notices(|_| true)
            .iter()
            .all(|n| n.target != 1 && n.target != 10 && n.target != 11));
        let tracked = observations
            .tracks()
            .find(|t| t.observer() == 1 && t.target() == 10)
            .unwrap();
        assert!(tracked.attention() >= settings.notice_attention);
        assert!(observations.meters()[0].2 >= settings.notice_attention);
    }
    fn relocate(world: &mut World, id: u64, p: Vec3) {
        assert!(world.set_kinematic_target(id, p, Default::default()));
        let index = world.items().iter().position(|i| i.id == id).unwrap();
        world.simulate(&[index], 1. / 60.);
    }
    #[test]
    fn discovery_is_not_visibility_and_tracks_keep_attention_and_noticed_bounds() {
        let mut world = fixture();
        let mut observations = ObstacleObservations::new(Default::default(), [1].into_iter());
        let mut notices = vec![];
        for _ in 0..60 {
            notices.extend(observations.update(&world, &[], 1. / 60.).unwrap());
        }
        assert_eq!(
            observations.stats.tracks, 2,
            "ignore floor and overhead geometry"
        );
        assert_eq!(
            notices.len(),
            1,
            "hidden target and unchanged target emit no extra notices"
        );
        assert_eq!(notices[0].target, 2);
        assert_eq!(notices[0].kind, NoticeKind::Acquired);
        let snapshot = observations.clone();
        // Move the hidden target without exposing it: no knowledge of that change.
        relocate(&mut world, 3, Vec3::new(0., -8., 1.));
        for _ in 0..30 {
            assert!(observations
                .update(&world, &[], 1. / 60.)
                .unwrap()
                .is_empty());
        }
        relocate(&mut world, 2, Vec3::new(2., -4., 1.));
        let mut changed = vec![];
        for _ in 0..60 {
            changed.extend(observations.update(&world, &[], 1. / 60.).unwrap());
        }
        let moved = changed.iter().find(|n| n.target == 2).unwrap();
        assert_eq!(moved.kind, NoticeKind::Changed);
        assert_eq!(moved.previous_bounds.unwrap().min, notices[0].bounds.min);
        assert!(changed
            .iter()
            .any(|n| n.target == 3 && n.kind == NoticeKind::Acquired));
        assert_eq!(snapshot.stats.notices, 1, "snapshot memory is independent");
    }
    #[test]
    fn tracks_expire_without_notices_and_explicit_pairs_are_not_duplicated() {
        let mut world = fixture();
        let settings = ObstacleObservationSettings::default();
        let explicit = ObservationTrack::new(
            ObservationBinding {
                observer: 1,
                target: 2,
                label: "manual".into(),
                vision: settings.vision,
                notice_attention: settings.notice_attention,
            },
            &world,
        )
        .unwrap();
        let mut observations = ObstacleObservations::new(settings, [1].into_iter());
        observations.update(&world, &[explicit], 0.1).unwrap();
        assert_eq!(observations.stats.tracks, 1);
        assert!(world.set_pose_3d(1, Vec3::new(80., 80., 0.), Default::default()));
        for _ in 0..30 {
            assert!(observations.update(&world, &[], 0.1).unwrap().is_empty());
        }
        assert_eq!(observations.stats.tracks, 0);
    }
    #[test]
    fn schedule_is_bounded_fair_and_new_tracks_do_not_gain_backdated_attention() {
        let old = fixture();
        let mut items = old.items().to_vec();
        for id in 10..30 {
            let mut item = items[0].clone();
            item.id = id;
            items.push(item);
        }
        let world = World::new(old.space().clone(), items);
        let settings = ObstacleObservationSettings {
            observers_per_tick: 2,
            tracks_per_observer: 1,
            ..Default::default()
        };
        let mut observations = ObstacleObservations::new(settings, 10..30);
        for n in 1..=10 {
            assert!(observations.update(&world, &[], 0.1).unwrap().is_empty());
            assert_eq!(observations.stats.scans, n * 2);
            assert_eq!(observations.stats.samples, n * 2);
        }
        assert_eq!(observations.stats.tracks, 20);
        assert!(observations
            .observers
            .iter()
            .all(|o| o.tracks[0].track.attention() == 0.));
        for _ in 0..10 {
            observations.update(&world, &[], 0.1).unwrap();
        }
        assert!(observations
            .observers
            .iter()
            .all(|o| o.tracks[0].track.attention() > 0.));
    }
    #[test]
    fn configuration_rejects_unbounded_or_nonfinite_work() {
        for settings in [
            ObstacleObservationSettings {
                observers_per_tick: 0,
                ..Default::default()
            },
            ObstacleObservationSettings {
                tracks_per_observer: 17,
                ..Default::default()
            },
            ObstacleObservationSettings {
                interval_seconds: f32::NAN,
                ..Default::default()
            },
            ObstacleObservationSettings {
                notice_attention: 0.,
                ..Default::default()
            },
        ] {
            assert!(settings.validate().is_err());
        }
    }

    #[test]
    fn journal_overflow_resamples_without_replanning_unchanged_geometry() {
        let mut world = fixture();
        let mut observations = ObstacleObservations::new(Default::default(), [1].into_iter());
        for _ in 0..20 {
            observations.update(&world, &[], 0.1).unwrap();
        }
        assert_eq!(observations.stats.notices, 1);
        for n in 0..5000 {
            assert!(world.set_pose_3d(
                1,
                Vec3::new((n % 2) as f32 * 0.001, 0., 0.),
                Default::default()
            ));
        }
        assert!(observations.update(&world, &[], 0.2).unwrap().is_empty());
        assert!(observations.stats.unchanged_notices > 0);
        relocate(&mut world, 2, Vec3::new(2., -4., 1.));
        let notices = observations.update(&world, &[], 0.2).unwrap();
        assert!(notices
            .iter()
            .any(|n| n.target == 2 && n.previous_bounds.is_some()));
    }

    #[test]
    fn notices_wait_for_a_route_without_repeated_delivery() {
        let world = fixture();
        let mut observations = ObstacleObservations::new(Default::default(), [1].into_iter());
        for _ in 0..20 {
            observations.update(&world, &[], 0.1).unwrap();
            assert!(observations.take_route_notices(|_| false).is_empty());
        }
        let notices = observations.take_route_notices(|actor| actor == 1);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].target, 2);
        assert!(observations.take_route_notices(|_| true).is_empty());
        observations.update(&world, &[], 0.1).unwrap();
        assert!(observations.take_route_notices(|_| true).is_empty());
    }

    #[test]
    fn hidden_motion_never_enters_prediction_and_meters_share_one_target() {
        let mut world = fixture();
        let settings = ObstacleObservationSettings {
            reaction: Some(Default::default()),
            ..Default::default()
        };
        let mut observations = ObstacleObservations::new(settings, [1].into_iter());
        for n in 0..30 {
            relocate(&mut world, 3, Vec3::new(0., -7. - n as f32 * 0.01, 1.));
            observations.update(&world, &[], 0.1).unwrap();
            assert!(observations.motions.iter().all(|m| m.target != 3));
        }
        let meters = observations.meters();
        assert_eq!(meters.len(), 1);
        assert_eq!(meters[0].0, 1);
        assert!(meters[0].1 > 0. && meters[0].2 > 0.);
        assert!(observations.stats.motion_samples > 0);
        let snapshot = observations.clone();
        relocate(&mut world, 2, Vec3::new(0., -3.5, 1.));
        observations.update(&world, &[], 0.1).unwrap();
        assert!(observations
            .motions
            .iter()
            .any(|m| m.target == 2 && m.velocity.y > 0.));
        assert!(snapshot.motions.iter().all(|m| m.velocity.y == 0.));
    }
}
