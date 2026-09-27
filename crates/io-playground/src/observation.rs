use io_game::stage::Stage;
use io_perception::pipeline::{
    AttentionStage, EvidenceStage, ItemNotice, ObservationCache, ObservationRequest, RelevanceStage,
};
use io_perception::{observe, Awareness, Observation, VisionProfile};
use io_traversal::{NavigationRequest, NavigationStatus, NavigationTicket};
use io_types::{Bounds, Vec3};
use io_world::CharacterBody;
use io_world::WorldView;
use serde::Deserialize;

/// Supplied game policy, not engine knowledge: a noticed obstacle can invalidate
/// an intended corridor. Other games can replace this stage with their own rules.
pub struct RouteAttentionStage;
pub struct RouteAttentionRequest<'a> {
    pub notice: ItemNotice,
    pub ticket: NavigationTicket,
    pub status: NavigationStatus,
    pub position: Vec3,
    pub route: &'a [Vec3],
    pub body: CharacterBody,
}
impl Stage<RouteAttentionRequest<'_>> for RouteAttentionStage {
    type Output = Option<NavigationRequest>;
    type Error = String;
    fn run(&mut self, input: RouteAttentionRequest<'_>) -> Result<Self::Output, String> {
        if input.notice.observer != input.ticket.actor
            || input.body.validate().is_err()
            || !input.position.finite()
            || input.route.iter().any(|p| !p.finite())
            || !input.notice.bounds.valid()
            || input.notice.previous_bounds.is_some_and(|b| !b.valid())
        {
            return Err("invalid route attention input or mismatched observer".into());
        }
        let affected = match input.status {
            NavigationStatus::Idle | NavigationStatus::Arrived | NavigationStatus::Failed => false,
            // A newly noticed opening can unblock a search with no accepted route.
            NavigationStatus::Blocked | NavigationStatus::Unreachable => true,
            NavigationStatus::Planning if input.route.is_empty() => true,
            NavigationStatus::Planning | NavigationStatus::Following => {
                let mut from = input.position;
                input.route.iter().any(|&to| {
                    let r = input.body.radius;
                    let corridor = Bounds {
                        min: Vec3::new(
                            from.x.min(to.x) - r,
                            from.y.min(to.y) - r,
                            from.z.min(to.z) - 0.01,
                        ),
                        max: Vec3::new(
                            from.x.max(to.x) + r,
                            from.y.max(to.y) + r,
                            from.z.max(to.z) + input.body.height,
                        ),
                    };
                    from = to;
                    input
                        .notice
                        .previous_bounds
                        .into_iter()
                        .chain(Some(input.notice.bounds))
                        .any(|b| {
                            b.min.x <= corridor.max.x
                                && b.max.x >= corridor.min.x
                                && b.min.y <= corridor.max.y
                                && b.max.y >= corridor.min.y
                                && b.min.z <= corridor.max.z
                                && b.max.z >= corridor.min.z
                        })
                })
            }
        };
        Ok(affected.then_some(NavigationRequest::Reconsider {
            ticket: input.ticket,
        }))
    }
}
fn default_notice_attention() -> f32 {
    0.2
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationDefinition {
    pub observer: String,
    pub target: String,
    pub vision: VisionProfile,
    #[serde(default = "default_notice_attention")]
    pub notice_attention: f32,
}
impl ObservationDefinition {
    pub fn validate(&self) -> Result<(), String> {
        self.vision.validate()?;
        if !self.notice_attention.is_finite() || !(0. ..=1.).contains(&self.notice_attention) {
            return Err("invalid observation notice_attention".into());
        }
        if self.observer == self.target
            || [&self.observer, &self.target]
                .iter()
                .any(|name| name.is_empty() || name.len() > 64)
        {
            return Err("invalid observation pair".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct ObservationBinding {
    pub observer: u64,
    pub target: u64,
    pub label: String,
    pub vision: VisionProfile,
    pub notice_attention: f32,
}
#[derive(Clone, Debug)]
pub struct ObservationTrack {
    binding: ObservationBinding,
    memory: Awareness,
    observation: Observation,
    visible_position: Option<io_types::Vec3>,
    cache: ObservationCache,
    notice: Option<ItemNotice>,
    resampled: bool,
}
impl ObservationTrack {
    pub fn new(binding: ObservationBinding, world: &dyn WorldView) -> Result<Self, String> {
        if !binding.notice_attention.is_finite() || !(0. ..=1.).contains(&binding.notice_attention)
        {
            return Err("invalid observation notice_attention".into());
        }
        let observation = observe(world, binding.observer, binding.target, binding.vision)?;
        let visible_position = (observation.evidence > 0.)
            .then(|| world.item(binding.target).unwrap().transform.anchor);
        Ok(Self {
            binding,
            memory: Awareness::default(),
            observation,
            visible_position,
            cache: ObservationCache::default(),
            notice: None,
            resampled: true,
        })
    }
    pub fn observer(&self) -> u64 {
        self.binding.observer
    }
    pub fn vision(&self) -> VisionProfile {
        self.binding.vision
    }
    pub(crate) fn set_fov(&mut self, degrees: f32) {
        self.binding.vision.fov_degrees = degrees;
    }
    pub(crate) fn set_notice_attention(&mut self, value: f32) {
        self.binding.notice_attention = value;
    }
    /// Authoritative result of the latest update, available directly to behavior.
    pub fn notice(&self) -> Option<ItemNotice> {
        self.notice
    }
    pub fn resampled(&self) -> bool {
        self.resampled
    }
    pub fn target(&self) -> u64 {
        self.binding.target
    }
    pub fn label(&self) -> &str {
        &self.binding.label
    }
    pub fn attention(&self) -> f32 {
        self.memory.value()
    }
    pub fn notice_attention(&self) -> f32 {
        self.binding.notice_attention
    }
    pub fn exposure(&self) -> f32 {
        self.observation.exposure
    }
    pub fn evidence(&self) -> f32 {
        self.observation.evidence
    }
    pub fn contact(&self) -> Option<io_perception::VisualContact> {
        self.visible_position
            .map(|position| io_perception::VisualContact {
                position,
                focus: self.evidence(),
                attention: self.attention(),
            })
    }
    pub fn update(&mut self, world: &dyn WorldView, dt: f32) -> Result<(), String> {
        let result = RelevanceStage
            .then(EvidenceStage)
            .then(AttentionStage)
            .run(ObservationRequest {
                world,
                observer: self.observer(),
                target: self.target(),
                vision: self.binding.vision,
                seconds: dt,
                minimum_attention: self.binding.notice_attention,
                cache: &mut self.cache,
                awareness: &mut self.memory,
            })?;
        self.observation = result.observation;
        self.visible_position = result.contact.map(|c| c.position);
        self.notice = result.notice;
        self.resampled = result.resampled;
        Ok(())
    }
}
