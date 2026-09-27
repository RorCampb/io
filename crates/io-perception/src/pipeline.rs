//! Item change relevance -> current visual evidence -> existing attention integration.
//! Cached evidence never grants knowledge of a hidden Item's changed state.
use crate::{observe, Awareness, Observation, VisionProfile, VisualContact};
use io_game::stage::Stage;
use io_types::{Bounds, Vec3};
use io_world::{ChangeSource, WorldView};

#[derive(Clone, Debug, Default)]
pub struct ObservationCache {
    key: Option<(u64, u64, VisionProfile)>,
    cursor: Option<u64>,
    observation: Observation,
    visible_position: Option<Vec3>,
    target_revision: u64,
    noticed_revision: Option<u64>,
    noticed_bounds: Option<Bounds>,
    uncertain: bool,
    world_identity: Option<u64>,
    checked_revision: Option<u64>,
}

pub struct ObservationRequest<'a> {
    pub world: &'a dyn WorldView,
    pub observer: u64,
    pub target: u64,
    pub vision: VisionProfile,
    pub seconds: f32,
    /// Reaction policy, not a filter on physical safety or future visibility checks.
    pub minimum_attention: f32,
    pub cache: &'a mut ObservationCache,
    pub awareness: &'a mut Awareness,
}

pub struct RelevantObservation<'a> {
    request: ObservationRequest<'a>,
    cursor: Option<u64>,
    target_revision: u64,
    resample: bool,
    uncertain: bool,
}
pub struct SampledObservation<'a> {
    relevant: RelevantObservation<'a>,
    observation: Observation,
    visible_position: Option<Vec3>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Acquired,
    Changed,
    Resynchronized,
}

#[derive(Clone, Copy, Debug)]
pub struct ItemNotice {
    pub observer: u64,
    pub target: u64,
    pub kind: NoticeKind,
    pub position: Vec3,
    pub attention: f32,
    /// Last attention-qualified bounds, never an unseen intermediate pose.
    pub previous_bounds: Option<Bounds>,
    pub bounds: Bounds,
}
pub struct ObservationResult {
    pub observation: Observation,
    pub contact: Option<VisualContact>,
    /// Direct result for authoritative plugin decisions, not an event-history lookup.
    pub notice: Option<ItemNotice>,
    pub resampled: bool,
}

pub struct RelevanceStage;
pub struct EvidenceStage;
pub struct AttentionStage;

fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x
        && a.max.x >= b.min.x
        && a.min.y <= b.max.y
        && a.max.y >= b.min.y
        && a.min.z <= b.max.z
        && a.max.z >= b.min.z
}
impl<'a> Stage<ObservationRequest<'a>> for RelevanceStage {
    type Output = RelevantObservation<'a>;
    type Error = String;
    fn run(&mut self, request: ObservationRequest<'a>) -> Result<Self::Output, String> {
        request.vision.validate()?;
        if request
            .cache
            .key
            .is_some_and(|(a, b, _)| (a, b) != (request.observer, request.target))
        {
            return Err("observation cache belongs to another observer/target pair".into());
        }
        if !request.seconds.is_finite()
            || !(0. ..=1.).contains(&request.seconds)
            || !request.minimum_attention.is_finite()
            || !(0. ..=1.).contains(&request.minimum_attention)
            || request.observer == request.target
        {
            return Err("invalid observation request".into());
        }
        let a = request
            .world
            .item(request.observer)
            .ok_or("unknown observer")?
            .current_spatial_bounds();
        let b = request
            .world
            .item(request.target)
            .ok_or("unknown target")?
            .current_spatial_bounds();
        let corridor = Bounds {
            min: Vec3::new(
                a.min.x.min(b.min.x),
                a.min.y.min(b.min.y),
                a.min.z.min(b.min.z),
            ),
            max: Vec3::new(
                a.max.x.max(b.max.x),
                a.max.y.max(b.max.y),
                a.max.z.max(b.max.z),
            ),
        };
        let log = request.world.changes();
        if log
            .zip(request.cache.world_identity)
            .is_some_and(|(log, id)| log.identity() != id)
        {
            return Err("observation cache belongs to another world".into());
        }
        let cursor = log.map(|l| l.cursor());
        let changes = log.and_then(|l| request.cache.cursor.and_then(|c| l.since(c)));
        let mut resample =
            request.cache.key != Some((request.observer, request.target, request.vision));
        let mut target_revision = request.cache.target_revision;
        let mut uncertain = request.cache.uncertain;
        match changes {
            Some(changes) => {
                for change in changes {
                    match change.source {
                        ChangeSource::Item(id) if id == request.target => {
                            target_revision = change.sequence;
                            resample = true;
                        }
                        ChangeSource::Item(id) if id == request.observer => resample = true,
                        ChangeSource::Terrain => resample = true,
                        ChangeSource::Item(_) => {
                            resample |= change.occlusion
                                && change
                                    .before
                                    .into_iter()
                                    .chain(change.after)
                                    .any(|bounds| overlaps(bounds, corridor));
                        }
                    }
                }
            }
            None => {
                resample = true;
                // A gap cannot be interpreted as either unchanged or a known change.
                uncertain |= request.cache.key.is_some()
                    && (log.is_some()
                        || request.cache.checked_revision != Some(request.world.revision()));
                target_revision = cursor.unwrap_or(target_revision);
            }
        }
        Ok(RelevantObservation {
            request,
            cursor,
            target_revision,
            resample,
            uncertain,
        })
    }
}
impl<'a> Stage<RelevantObservation<'a>> for EvidenceStage {
    type Output = SampledObservation<'a>;
    type Error = String;
    fn run(&mut self, relevant: RelevantObservation<'a>) -> Result<Self::Output, String> {
        let r = &relevant.request;
        let (observation, visible_position) = if relevant.resample {
            let observation = observe(r.world, r.observer, r.target, r.vision)?;
            let position = (observation.evidence > 0.)
                .then(|| r.world.item(r.target).unwrap().transform.anchor);
            (observation, position)
        } else {
            (r.cache.observation, r.cache.visible_position)
        };
        Ok(SampledObservation {
            relevant,
            observation,
            visible_position,
        })
    }
}
impl Stage<SampledObservation<'_>> for AttentionStage {
    type Output = ObservationResult;
    type Error = String;
    fn run(&mut self, input: SampledObservation<'_>) -> Result<Self::Output, String> {
        let RelevantObservation {
            request: r,
            cursor,
            target_revision,
            resample,
            uncertain,
        } = input.relevant;
        r.awareness
            .advance(input.observation.evidence, r.vision, r.seconds)?;
        let attention = r.awareness.value();
        let contact = input.visible_position.map(|position| VisualContact {
            position,
            focus: input.observation.evidence,
            attention,
        });
        let notice = contact
            .filter(|_| attention >= r.minimum_attention)
            .filter(|_| uncertain || r.cache.noticed_revision != Some(target_revision))
            .map(|contact| ItemNotice {
                observer: r.observer,
                target: r.target,
                position: contact.position,
                attention,
                previous_bounds: r.cache.noticed_bounds,
                bounds: r.world.item(r.target).unwrap().current_spatial_bounds(),
                kind: match r.cache.noticed_revision {
                    None => NoticeKind::Acquired,
                    Some(_) if uncertain => NoticeKind::Resynchronized,
                    Some(_) => NoticeKind::Changed,
                },
            });
        if let Some(notice) = notice {
            r.cache.noticed_revision = Some(target_revision);
            r.cache.noticed_bounds = Some(notice.bounds);
        }
        r.cache.key = Some((r.observer, r.target, r.vision));
        r.cache.world_identity = r.world.changes().map(|l| l.identity());
        r.cache.checked_revision = Some(r.world.revision());
        r.cache.cursor = cursor;
        r.cache.target_revision = target_revision;
        r.cache.uncertain = uncertain && notice.is_none();
        r.cache.observation = input.observation;
        r.cache.visible_position = input.visible_position;
        Ok(ObservationResult {
            observation: input.observation,
            contact,
            notice,
            resampled: resample,
        })
    }
}
