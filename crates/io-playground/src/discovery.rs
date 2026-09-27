//! Supplied playground policy: discover nearby scenery, not every Item in the world.
use crate::{ObservationBinding, ObservationTrack};
use io_game::stage::Stage;
use io_perception::{pipeline::ItemNotice, VisionProfile};
use io_types::{Bounds, Rotation, Vec3};
use io_world::{Collider, WorldView};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObstacleObservationSettings {
    pub vision: VisionProfile,
    pub notice_attention: f32,
    pub interval_seconds: f32,
    pub retention_seconds: f32,
    pub observers_per_tick: usize,
    pub tracks_per_observer: usize,
}
impl Default for ObstacleObservationSettings {
    fn default() -> Self {
        Self {
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
        }
    }
}
impl ObstacleObservationSettings {
    pub fn validate(self) -> Result<(), String> {
        self.vision.validate()?;
        if !self.notice_attention.is_finite()
            || !(0.01..=1.).contains(&self.notice_attention)
            || !self.interval_seconds.is_finite()
            || !(0.05..=0.5).contains(&self.interval_seconds)
            || !self.retention_seconds.is_finite()
            || !(self.interval_seconds..=30.).contains(&self.retention_seconds)
            || !(1..=32).contains(&self.observers_per_tick)
            || !(1..=16).contains(&self.tracks_per_observer)
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
                    || item.collider.is_none()
                    || item.character_body.is_some()
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
                (d <= r.settings.vision.range.powi(2)).then_some((d, item.id))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        candidates.dedup_by_key(|c| c.1);
        candidates.truncate(r.settings.tracks_per_observer);
        Ok(candidates.into_iter().map(|(_, id)| id).collect())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct DiscoveryStats {
    pub scans: u64,
    pub samples: u64,
    pub notices: u64,
    pub reconsiderations: u64,
    pub unchanged_notices: u64,
    pub tracks: usize,
}
#[derive(Clone, Debug)]
struct Tracked {
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
    settings: ObstacleObservationSettings,
    observers: Vec<Observer>,
    cursor: usize,
    seconds: f64,
    pub stats: DiscoveryStats,
}
impl ObstacleObservations {
    pub fn take_route_notices(&mut self, mut ready: impl FnMut(u64) -> bool) -> Vec<ItemNotice> {
        self.observers
            .iter_mut()
            .filter(|o| ready(o.actor))
            .flat_map(|o| o.tracks.iter_mut().filter_map(|t| t.route_notice.take()))
            .collect()
    }
    pub fn new(settings: ObstacleObservationSettings, actors: impl Iterator<Item = u64>) -> Self {
        Self {
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
                if observer.tracks.len() == self.settings.tracks_per_observer {
                    let Some(index) = observer
                        .tracks
                        .iter()
                        .position(|t| !ids.contains(&t.track.target()))
                    else {
                        continue;
                    };
                    observer.tracks.remove(index);
                }
                observer.tracks.push(Tracked {
                    last_near: self.seconds,
                    noticed_geometry: None,
                    route_notice: None,
                    track: ObservationTrack::new(
                        ObservationBinding {
                            observer: observer.actor,
                            target: id,
                            label: format!("obstacle-{id}"),
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
                if let Some(notice) = entry.track.notice() {
                    self.stats.notices += 1;
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
}
