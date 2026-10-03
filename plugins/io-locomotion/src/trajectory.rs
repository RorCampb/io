//! Short rolling trajectories owned by the locomotion plugin, not the route search.
use crate::surface::{Agent, MovementProfile, Navigation, Waypoint};
use crate::Error;
use io_game::stage::Stage;
use io_types::Vec3;
use io_world::WorldView;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TrajectorySettings {
    pub preview_seconds: f32,
    pub max_distance: f32,
    pub corner_distance: f32,
    pub lateral_acceleration: f32,
    pub refresh_seconds: f32,
    pub samples: usize,
}
impl Default for TrajectorySettings {
    fn default() -> Self {
        Self {
            preview_seconds: 0.8,
            max_distance: 3.,
            corner_distance: 1.2,
            lateral_acceleration: 4.,
            refresh_seconds: 0.2,
            samples: 12,
        }
    }
}
impl TrajectorySettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.preview_seconds.is_finite()
            || !(0.2..=2.).contains(&self.preview_seconds)
            || !self.max_distance.is_finite()
            || !(0.5..=6.).contains(&self.max_distance)
            || !self.corner_distance.is_finite()
            || !(0.1..=self.max_distance).contains(&self.corner_distance)
            || !self.lateral_acceleration.is_finite()
            || !(0.5..=30.).contains(&self.lateral_acceleration)
            || !self.refresh_seconds.is_finite()
            || !(0.05..=1.).contains(&self.refresh_seconds)
            || !(6..=24).contains(&self.samples)
        {
            return Err("invalid local trajectory settings");
        }
        Ok(())
    }
}
fn length(v: Vec3) -> f32 {
    v.x.hypot(v.y)
}
fn unit(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, 0.).scaled(1. / length(v).max(1e-6))
}

/// Pure geometric proposal. The executor validates it; this grants no collision permission.
#[derive(Clone, Debug)]
pub struct CurveProposal {
    pub controls: [Vec3; 4],
}
impl CurveProposal {
    pub fn point(&self, t: f32) -> Vec3 {
        let [a, b, c, d] = self.controls;
        let u = 1. - t;
        a.scaled(u * u * u)
            + b.scaled(3. * u * u * t)
            + c.scaled(3. * u * t * t)
            + d.scaled(t * t * t)
    }
}
pub struct RefinementInput {
    pub position: Vec3,
    pub velocity: Vec3,
    pub corner: Vec3,
    pub following: Vec3,
    pub settings: TrajectorySettings,
}
pub trait TrajectoryRefiner: std::fmt::Debug + Send + Sync {
    fn propose(&self, input: RefinementInput) -> Option<CurveProposal>;
}
#[derive(Debug)]
pub struct CornerRefiner;
impl TrajectoryRefiner for CornerRefiner {
    fn propose(&self, r: RefinementInput) -> Option<CurveProposal> {
        let incoming = r.corner - r.position;
        let outgoing = r.following - r.corner;
        let distance = length(incoming);
        let alignment = unit(incoming).dot(unit(outgoing));
        // Reversals, stance changes and vertical transitions keep their original contract.
        if distance < 0.3
            || length(outgoing) < 0.3
            || !(-0.5..0.995).contains(&alignment)
            || incoming.z.abs() > 0.003
            || outgoing.z.abs() > 0.003
        {
            return None;
        }
        let heading = if length(r.velocity) > 0.05 {
            unit(r.velocity)
        } else {
            unit(incoming)
        };
        if heading.dot(unit(incoming)) < 0.5 {
            return None;
        }
        let exit = r
            .settings
            .corner_distance
            .min(length(outgoing) * 0.45)
            .min(distance * 0.75);
        let end = r.corner + unit(outgoing).scaled(exit);
        Some(CurveProposal {
            controls: [
                r.position,
                r.position + heading.scaled(distance * 0.65),
                end - unit(outgoing).scaled(exit * 0.8),
                end,
            ],
        })
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct TrajectoryStats {
    pub proposals: u64,
    pub checks: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub completed: u64,
    pub anticipations: u64,
}
#[derive(Clone, Debug)]
struct Curve {
    points: Vec<Vec3>,
    checked: usize,
    cursor: usize,
    speed: f32,
}
#[derive(Clone, Debug, Default)]
enum LocalPath {
    #[default]
    Route,
    Preparing(Curve),
    Following(Curve),
    Rejected,
}
#[derive(Clone, Debug, Default)]
pub struct TrajectoryStage {
    path: LocalPath,
    clock: f32,
    preview_at: f32,
    alerted: bool,
    retry_at: f32,
    context: Option<Context>,
    pub stats: TrajectoryStats,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Context {
    actor: Option<u64>,
    profile: MovementProfile,
    next: Waypoint,
    following: Option<Waypoint>,
    settings: TrajectorySettings,
    world: Option<u64>,
}
pub struct TrajectoryRequest<'a> {
    pub world: &'a dyn WorldView,
    pub navigation: &'a mut Navigation,
    pub agent: &'a Agent,
    pub next: Waypoint,
    pub position: Vec3,
    pub velocity: Vec3,
    pub speed: f32,
    pub braking: f32,
    pub seconds: f32,
    pub settings: TrajectorySettings,
    pub refiner: &'a dyn TrajectoryRefiner,
}
#[derive(Clone, Copy, Debug)]
pub struct TrajectoryTarget {
    pub position: Vec3,
    pub speed: f32,
    pub complete_corner: bool,
    pub reconsider: bool,
}
impl TrajectoryStage {
    pub fn clear(&mut self) {
        self.path = LocalPath::Route;
        self.alerted = false;
        self.retry_at = 0.;
        self.context = None;
        // Keep sampling phase and counters across waypoint changes.
    }
    pub fn points(&self) -> &[Vec3] {
        match &self.path {
            LocalPath::Following(c) => &c.points[c.cursor..],
            _ => &[],
        }
    }
}
impl Stage<TrajectoryRequest<'_>> for TrajectoryStage {
    type Output = TrajectoryTarget;
    type Error = Error;
    fn run(&mut self, r: TrajectoryRequest<'_>) -> Result<TrajectoryTarget, Error> {
        r.settings.validate().map_err(|_| Error::InvalidInput)?;
        if !r.position.finite()
            || !r.next.position.finite()
            || !r.velocity.finite()
            || !r.speed.is_finite()
            || !(0.1..=12.).contains(&r.speed)
            || !r.braking.is_finite()
            || r.braking <= 0.
            || !r.seconds.is_finite()
            || !(0. ..=0.25).contains(&r.seconds)
            || r.seconds == 0.
        {
            return Err(Error::InvalidInput);
        }
        self.clock += r.seconds;
        if matches!(self.path, LocalPath::Rejected) && self.clock >= self.retry_at {
            self.path = LocalPath::Route;
        }
        let route = r.agent.remaining_route().ok_or(Error::InvalidInput)?;
        if route.first() != Some(&r.next) {
            return Err(Error::InvalidInput);
        }
        let following = route.get(1).copied().filter(|p| p.action == r.next.action);
        let context = Context {
            actor: r.agent.actor(),
            profile: r.agent.profile(),
            next: r.next,
            following,
            settings: r.settings,
            world: r.world.changes().map(|log| log.identity()),
        };
        if self.context != Some(context) {
            self.clear();
            self.context = Some(context);
        }
        let horizon = (length(r.velocity) * r.settings.preview_seconds
            + length(r.velocity).powi(2) / (2. * r.braking))
            .max(r.settings.corner_distance)
            .min(r.settings.max_distance);
        if matches!(self.path, LocalPath::Route) && length(r.next.position - r.position) <= horizon
        {
            if let Some(following) = following {
                if let Some(proposal) = r.refiner.propose(RefinementInput {
                    position: r.position,
                    velocity: r.velocity,
                    corner: r.next.position,
                    following: following.position,
                    settings: r.settings,
                }) {
                    self.stats.proposals += 1;
                    let valid = proposal.controls.iter().all(|p| {
                        p.finite()
                            && (*p - r.position).dot(*p - r.position)
                                <= (r.settings.max_distance * 2.).powi(2)
                    }) && length(proposal.controls[0] - r.position) < 1e-5
                        && (proposal.controls[0].z - r.position.z).abs() < 1e-5;
                    let end = proposal.controls[3];
                    let edge = following.position - r.next.position;
                    let t = (end - r.next.position).dot(edge) / edge.dot(edge).max(1e-10);
                    if !valid
                        || !(0.01..0.95).contains(&t)
                        || (end - (r.next.position + edge.scaled(t)))
                            .dot(end - (r.next.position + edge.scaled(t)))
                            > 1e-6
                    {
                        return Err(Error::InvalidInput);
                    }
                    let points: Vec<_> = (0..=r.settings.samples)
                        .map(|i| proposal.point(i as f32 / r.settings.samples as f32))
                        .collect();
                    let mut speed = r.speed;
                    for triple in points.windows(3) {
                        let a = triple[1] - triple[0];
                        let b = triple[2] - triple[1];
                        let angle = unit(a).dot(unit(b)).clamp(-1., 1.).acos();
                        if angle > 0.001 {
                            speed = speed.min(
                                (r.settings.lateral_acceleration * (length(a) + length(b)) * 0.5
                                    / angle)
                                    .sqrt(),
                            );
                        }
                    }
                    self.path = LocalPath::Preparing(Curve {
                        points,
                        checked: 0,
                        cursor: 0,
                        speed: speed.max(0.1),
                    });
                }
            }
        }
        // One short scenery/support check per invocation, never a whole-route scan.
        if let LocalPath::Preparing(curve) = &mut self.path {
            self.stats.checks += 1;
            let index = curve.checked;
            if !crate::surface::supported_segment(
                r.world,
                r.agent.profile(),
                curve.points[index],
                curve.points[index + 1],
                r.next.action,
            ) {
                self.stats.rejected += 1;
                self.path = LocalPath::Rejected;
                self.retry_at = self.clock + r.settings.refresh_seconds;
            } else {
                curve.checked += 1;
                if curve.checked + 1 == curve.points.len() {
                    self.stats.accepted += 1;
                    self.path = LocalPath::Following(curve.clone());
                }
            }
        }
        let mut result = TrajectoryTarget {
            position: r.next.position,
            speed: r.speed,
            complete_corner: false,
            reconsider: false,
        };
        if let LocalPath::Following(curve) = &mut self.path {
            // Monotonic projection prevents an updated target pulling the actor backwards.
            while curve.cursor + 2 < curve.points.len() {
                let next = curve.points[curve.cursor + 1];
                let edge = next - curve.points[curve.cursor];
                if (r.position - next).dot(edge) < 0. {
                    break;
                }
                curve.cursor += 1;
            }
            let end = *curve.points.last().unwrap();
            let exit = end - curve.points[curve.points.len() - 2];
            result.complete_corner =
                (r.position - end).dot(exit) >= 0. || length(r.position - end) < 0.08;
            let lead = (length(r.velocity) * 0.35).max(0.6);
            result.position = curve
                .points
                .iter()
                .skip(curve.cursor + 1)
                .copied()
                .find(|p| length(*p - r.position) >= lead)
                .unwrap_or_else(|| {
                    let to = following.map_or(end, |w| w.position);
                    end + unit(to - end).scaled(lead.min(length(to - end)))
                });
            result.speed = curve.speed;
            if result.complete_corner {
                self.stats.completed += 1;
            }
        }
        // Preview scenery independently of attention as physical feasibility, not NPC knowledge.
        // Stagger initial probes by Item ID to avoid a synchronized population-wide spike.
        if self.preview_at == 0. {
            self.preview_at =
                (r.agent.actor().unwrap_or(0) % 17) as f32 / 17. * r.settings.refresh_seconds;
        }
        if self.clock >= self.preview_at {
            self.preview_at = self.clock + r.settings.refresh_seconds;
            let delta = result.position - r.position;
            let distance = length(delta).min(horizon);
            let point = r.position + unit(delta).scaled(distance);
            self.stats.checks += 1;
            if !r.navigation.steering_segment_clear(
                &crate::surface::Scenery(r.world),
                r.agent,
                r.position,
                point,
                r.next.action,
            ) && !self.alerted
            {
                result.reconsider = true;
                self.alerted = true;
                self.stats.anticipations += 1;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curve_matches_entry_heading_and_exit_edge_without_a_corner_kink() {
        let proposal = CornerRefiner
            .propose(RefinementInput {
                position: Vec3::default(),
                velocity: Vec3::new(2., 0., 0.),
                corner: Vec3::new(2., 0., 0.),
                following: Vec3::new(2., 3., 0.),
                settings: Default::default(),
            })
            .unwrap();
        assert_eq!(proposal.point(0.), Vec3::default());
        assert_eq!(proposal.point(1.), proposal.controls[3]);
        let start = unit(proposal.controls[1] - proposal.controls[0]);
        let end = unit(proposal.controls[3] - proposal.controls[2]);
        assert!(start.dot(Vec3::new(1., 0., 0.)) > 0.999);
        assert!(end.dot(Vec3::new(0., 1., 0.)) > 0.999);
        for i in 1..100 {
            let p = proposal.point(i as f32 / 100.);
            assert!(p.x >= 0. && p.x <= 2.001 && p.y >= 0.);
        }
    }
    #[test]
    fn settings_bound_work_and_vertical_or_reversing_routes_fall_back() {
        for s in [
            TrajectorySettings {
                samples: 25,
                ..Default::default()
            },
            TrajectorySettings {
                preview_seconds: f32::NAN,
                ..Default::default()
            },
            TrajectorySettings {
                lateral_acceleration: 0.,
                ..Default::default()
            },
        ] {
            assert!(s.validate().is_err());
        }
        for following in [Vec3::new(-1., 0., 0.), Vec3::new(2., 3., 1.)] {
            assert!(CornerRefiner
                .propose(RefinementInput {
                    position: Vec3::default(),
                    velocity: Vec3::new(1., 0., 0.),
                    corner: Vec3::new(2., 0., 0.),
                    following,
                    settings: Default::default()
                })
                .is_none());
        }
    }
    #[test]
    fn clearing_or_cloning_a_stage_does_not_share_execution_progress() {
        let mut stage = TrajectoryStage {
            path: LocalPath::Following(Curve {
                points: vec![Vec3::default(), Vec3::new(1., 1., 0.)],
                checked: 1,
                cursor: 0,
                speed: 1.,
            }),
            ..Default::default()
        };
        let snapshot = stage.clone();
        stage.clear();
        assert!(stage.points().is_empty());
        assert_eq!(snapshot.points().len(), 2);
    }
}
