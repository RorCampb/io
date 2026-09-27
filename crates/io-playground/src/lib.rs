#![forbid(unsafe_code)]
//! Example game plugin. Patrol, perception interpretation and cat-and-mouse rules live here.
pub use io_locomotion::athletics;
mod barrier;
mod config;
mod discovery;
pub use barrier::BarrierDefinition;
pub use discovery::{
    DiscoveryStats, ObstacleDiscoveryRequest, ObstacleDiscoveryStage, ObstacleObservationSettings,
};
mod movement;
mod observation;
mod pursuit;
#[cfg(test)]
mod tests;
pub use config::*;
use io_game::{GamePlugin, PluginInfo, PluginWorld, Tick};
use io_locomotion::{
    ActorBinding, MovementEvent, NavigationGoal, NavigationRequest, NavigationStatus as Status,
    NavigationTicket,
};
pub use io_locomotion::{Command, Error, Locomotion, Motion, MotionEvent, Motor};
use io_types::{Envelope, MessageId, Vec3};
use io_world::WorldView;
use movement::TraversalService;
pub use observation::{
    ObservationBinding, ObservationDefinition, ObservationTrack, RouteAttentionRequest,
    RouteAttentionStage,
};
pub use pursuit::{PursuitBinding, PursuitDefinition, PursuitPhase, PursuitSettings};
use std::collections::BTreeMap;

pub type Game = io_game::Session<Traversal>;

#[derive(Clone, Debug)]
pub struct NpcDiagnostic {
    pub actor: u64,
    pub goal: Option<NavigationGoal>,
    pub feedback: io_locomotion::ActorFeedback,
    pub route: Vec<Vec3>,
}

/// Playground controls, not movement-engine commands. Settings are runtime-only.
#[derive(Clone, Copy, Debug)]
pub enum Input {
    Motor(Command),
    TuneAttention(AttentionSettings),
}
impl From<Command> for Input {
    fn from(command: Command) -> Self {
        Self::Motor(command)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct AttentionSettings {
    pub observer: u64,
    pub target: u64,
    pub fov_degrees: f32,
    pub notice_attention: f32,
}
impl AttentionSettings {
    pub fn validate(self) -> Result<(), Error> {
        if !self.fov_degrees.is_finite()
            || !(5. ..=360.).contains(&self.fov_degrees)
            || !self.notice_attention.is_finite()
            || !(0.01..=1.).contains(&self.notice_attention)
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Observation(io_perception::pipeline::ItemNotice),
    Motion { actor: u64, event: MotionEvent },
    Movement(MovementEvent),
    Pursuit { actor: u64, phase: PursuitPhase },
}

#[derive(Clone, Debug)]
struct Behavior {
    actor: u64,
    ticket: NavigationTicket,
    patrol_goal: Vec3,
    on_arrival: OnArrival,
    next_destination: usize,
    pursuit: Option<pursuit::Pursuit>,
    requested: Option<NavigationGoal>,
    deferred_goal: Option<Option<NavigationGoal>>,
    request_age: f32,
    approach_distance: f32,
}

#[derive(Clone, Debug)]
pub struct Traversal {
    player: Motor,
    movement: Option<TraversalService>,
    behaviors: Vec<Behavior>,
    observations: Vec<ObservationTrack>,
    obstacle_observations: Option<discovery::ObstacleObservations>,
    expected_observations: usize,
    next_message: u64,
    domain: Option<io_traversal::navigation::Domain>,
    debug_routes: bool,
    barriers: Vec<barrier::Barrier>,
    expected_barriers: Vec<BarrierDefinition>,
}

fn submit(
    movement: &mut TraversalService,
    next: &mut u64,
    request: NavigationRequest,
) -> Result<NavigationTicket, Error> {
    *next = next.checked_add(1).ok_or(Error::InvalidInput)?;
    movement
        .request(Envelope::new(MessageId(*next), request))
        .payload
        .map_err(|_| Error::InvalidInput)
}
impl Traversal {
    pub fn new(
        definition: Definition,
        player: u64,
        clips: BTreeMap<Motion, (usize, f64)>,
        world: &dyn WorldView,
    ) -> Result<Self, String> {
        Self::with_npcs(definition, player, clips, vec![], world)
    }
    pub fn with_npcs(
        definition: Definition,
        player: u64,
        clips: BTreeMap<Motion, (usize, f64)>,
        bindings: Vec<NpcBinding>,
        world: &dyn WorldView,
    ) -> Result<Self, String> {
        definition.validate()?;
        if definition.navigation.as_ref().map_or(0, |n| n.agents.len()) != bindings.len() {
            return Err("unresolved navigation agents".into());
        }
        if definition.navigation.as_ref().is_some_and(|n| {
            n.agents
                .iter()
                .zip(&bindings)
                .any(|(s, b)| s.pursuit.is_some() != b.pursuit.is_some())
        }) {
            return Err("unresolved pursuit bindings".into());
        }
        let mut actors = Vec::new();
        let mut behaviors = Vec::new();
        for b in bindings {
            let actor = b.motor.actor();
            if actor == player {
                return Err("duplicate movement authority".into());
            }
            b.on_arrival.validate()?;
            let approach_distance = match &b.pursuit {
                Some(p) => {
                    let a = world
                        .item(actor)
                        .and_then(|i| i.character_body)
                        .ok_or("invalid pursuit observer")?;
                    let t = world
                        .item(p.target)
                        .and_then(|i| i.character_body)
                        .ok_or("pursuit needs a character target")?;
                    if p.target == actor || p.settings.tag_distance <= a.radius + t.radius + 0.1 {
                        return Err(
                            "tag distance must exceed both character radii plus clearance".into(),
                        );
                    }
                    (p.settings.tag_distance * 0.8).max(a.radius + t.radius + 0.05)
                }
                None => 0.,
            };
            behaviors.push(Behavior {
                actor,
                ticket: NavigationTicket { actor, revision: 0 },
                patrol_goal: b.goal,
                on_arrival: b.on_arrival,
                next_destination: 0,
                pursuit: b.pursuit.map(pursuit::Pursuit::new).transpose()?,
                requested: None,
                deferred_goal: None,
                request_age: 0.,
                approach_distance,
            });
            actors.push(ActorBinding {
                steering: b.steering,
                motor: b.motor,
                can_crouch: b.can_crouch,
                familiar_points: b.familiar_points,
            });
        }
        let mut movement = match &definition.navigation {
            Some(n) => Some(TraversalService::new(
                n.domain,
                n.expansions_per_tick,
                actors,
                n.athletics,
                &n.agents.iter().map(|a| a.athletics).collect::<Vec<_>>(),
                n.planning,
                world,
            )?),
            None => None,
        };
        let mut next_message = 0;
        if let Some(m) = &mut movement {
            for b in &mut behaviors {
                let goal = NavigationGoal::Position(b.patrol_goal);
                b.ticket = submit(
                    m,
                    &mut next_message,
                    NavigationRequest::Start {
                        actor: b.actor,
                        goal,
                    },
                )
                .map_err(|e| format!("initial movement: {e:?}"))?;
                b.requested = Some(goal);
            }
        }
        let expected_barriers = definition.barriers().cloned().collect();
        let obstacle_observations = definition.obstacle_observations.map(|settings| {
            discovery::ObstacleObservations::new(settings, behaviors.iter().map(|b| b.actor))
        });
        Ok(Self {
            player: Motor::new(definition.locomotion, player, clips, world)?,
            movement,
            behaviors,
            observations: vec![],
            obstacle_observations,
            expected_observations: definition.observations.len(),
            next_message,
            domain: definition.navigation.map(|n| n.domain),
            debug_routes: definition.debug_routes,
            barriers: vec![],
            expected_barriers,
        })
    }
    pub fn with_observations(
        mut self,
        bindings: Vec<ObservationBinding>,
        world: &dyn WorldView,
    ) -> Result<Self, String> {
        if bindings.len() != self.expected_observations || !self.observations.is_empty() {
            return Err("unresolved observation tracks".into());
        }
        let mut pairs = std::collections::HashSet::new();
        for binding in bindings {
            if !pairs.insert((binding.observer, binding.target)) {
                return Err("duplicate observation pair".into());
            }
            self.observations
                .push(ObservationTrack::new(binding, world)?);
        }
        Ok(self)
    }
    pub fn with_barrier(self, item: u64, world: &dyn WorldView) -> Result<Self, String> {
        self.with_barriers(&[item], world)
    }
    pub fn with_barriers(mut self, items: &[u64], world: &dyn WorldView) -> Result<Self, String> {
        if items.len() != self.expected_barriers.len()
            || !self.barriers.is_empty()
            || items.iter().collect::<std::collections::HashSet<_>>().len() != items.len()
        {
            return Err("unresolved or duplicate barrier bindings".into());
        }
        self.barriers = self
            .expected_barriers
            .iter()
            .zip(items)
            .map(|(d, &id)| barrier::Barrier::new(d, id, world))
            .collect::<Result<_, _>>()?;
        Ok(self)
    }
    pub fn observations(&self) -> &[ObservationTrack] {
        &self.observations
    }
    pub fn discovery_stats(&self) -> Option<DiscoveryStats> {
        self.obstacle_observations.as_ref().map(|o| o.stats)
    }
    /// Snapshot-owned presentation data; observations and movement remain authoritative.
    pub fn diagnostics(&self) -> Vec<NpcDiagnostic> {
        let Some(m) = &self.movement else {
            return vec![];
        };
        self.behaviors
            .iter()
            .filter_map(|b| {
                Some(NpcDiagnostic {
                    actor: b.actor,
                    goal: b.requested,
                    feedback: m.actor(b.actor)?,
                    route: m.route(b.actor),
                })
            })
            .collect()
    }
    pub fn debug_routes(&self) -> bool {
        self.debug_routes
    }
    pub fn attention_settings(&self, index: usize) -> Option<(AttentionSettings, bool)> {
        let track = self.observations.get(index)?;
        let pursuit = self
            .behaviors
            .iter()
            .find(|b| b.actor == track.observer())
            .and_then(|b| b.pursuit.as_ref())
            .filter(|p| p.binding.target == track.target());
        Some((
            AttentionSettings {
                observer: track.observer(),
                target: track.target(),
                fov_degrees: track.vision().fov_degrees,
                notice_attention: pursuit.map_or(track.notice_attention(), |p| {
                    p.binding.settings.notice_attention
                }),
            },
            pursuit.is_some(),
        ))
    }
    fn tune_attention(&mut self, settings: AttentionSettings) -> Result<(), Error> {
        settings.validate()?;
        let track = self
            .observations
            .iter_mut()
            .find(|t| t.observer() == settings.observer && t.target() == settings.target)
            .ok_or(Error::InvalidInput)?;
        track.set_fov(settings.fov_degrees);
        track.set_notice_attention(settings.notice_attention);
        if let Some(p) = self
            .behaviors
            .iter_mut()
            .find(|b| b.actor == settings.observer)
            .and_then(|b| b.pursuit.as_mut())
            .filter(|p| p.binding.target == settings.target)
        {
            p.binding.settings.notice_attention = settings.notice_attention;
        }
        Ok(())
    }
    pub fn player(&self) -> u64 {
        self.player.actor()
    }
    pub fn motion(&self) -> Motion {
        self.player.motion()
    }
    pub fn crouched(&self) -> bool {
        self.player.crouched()
    }
    pub fn grounded(&self) -> bool {
        self.player.grounded()
    }
    pub fn vertical_speed(&self) -> f32 {
        self.player.vertical_speed()
    }
    pub fn navigation_status(&self) -> Vec<(u64, Status, usize)> {
        self.movement.as_ref().map_or_else(Vec::new, |m| {
            m.actors()
                .map(|f| (f.ticket.actor, f.status, f.known_cells))
                .collect()
        })
    }
    pub fn navigation_stats(&self) -> Option<io_traversal::navigation::Stats> {
        self.movement.as_ref().map(TraversalService::stats)
    }
    pub fn planning_stats(&self) -> Option<io_traversal::PlanningStats> {
        self.movement.as_ref().map(TraversalService::planning_stats)
    }
    pub fn athletics_feedback(&self) -> Option<Vec<io_locomotion::MotorFeedback>> {
        match self.movement.as_ref()? {
            TraversalService::Athletics(m) => Some(m.actors().map(|f| f.execution).collect()),
            TraversalService::Surface(_) => None,
        }
    }
    pub fn pursuit_status(&self) -> Vec<(u64, PursuitPhase, u32)> {
        self.behaviors
            .iter()
            .filter_map(|b| b.pursuit.as_ref().map(|p| (b.actor, p.phase(), p.tags())))
            .collect()
    }
}
impl GamePlugin for Traversal {
    type Command = Input;
    type Event = Event;
    type Error = Error;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "playground",
            version: 1,
        }
    }
    fn validate(&self, world: &dyn WorldView) -> Result<(), Error> {
        if self.expected_barriers.len() != self.barriers.len() {
            return Err(Error::InvalidWorld);
        }
        if self.observations.len() != self.expected_observations {
            return Err(Error::InvalidWorld);
        }
        self.player.validate(world)?;
        if let Some(m) = &self.movement {
            m.validate(world)?;
        }
        for b in &self.behaviors {
            if let Some(p) = &b.pursuit {
                if !self
                    .observations
                    .iter()
                    .any(|o| o.observer() == b.actor && o.target() == p.binding.target)
                {
                    return Err(Error::InvalidWorld);
                }
            }
        }
        Ok(())
    }
    fn active_items(&self) -> Vec<u64> {
        std::iter::once(self.player())
            .chain(self.behaviors.iter().map(|b| b.actor))
            .chain(self.barriers.iter().map(|b| b.item()))
            .chain(
                self.observations
                    .iter()
                    .flat_map(|o| [o.observer(), o.target()]),
            )
            .collect()
    }
    fn command(&mut self, _: &mut PluginWorld<Event>, command: Input) -> Result<(), Error> {
        match command {
            Input::Motor(command) => self.player.command(command),
            Input::TuneAttention(settings) => self.tune_attention(settings),
        }
    }
    fn before_step(&mut self, world: &mut PluginWorld<Event>, tick: Tick) -> Result<(), Error> {
        for barrier in &mut self.barriers {
            let safe = self
                .movement
                .as_ref()
                .is_none_or(|m| m.actors().all(|a| a.execution.grounded));
            barrier.update(world, tick.seconds(), safe)?;
        }
        if let Some(movement) = &mut self.movement {
            movement.before_step(world, tick.seconds())?;
        }
        Ok(())
    }
    fn update(&mut self, world: &mut PluginWorld<Event>, tick: Tick) -> Result<(), Error> {
        let dt = tick.seconds();
        for event in self.player.update(world, dt)? {
            world.emit(Event::Motion {
                actor: self.player(),
                event,
            });
        }
        for track in &mut self.observations {
            track.update(world, dt).map_err(|_| Error::InvalidWorld)?;
            if let Some(notice) = track.notice() {
                world.emit(Event::Observation(notice));
            }
        }
        let dynamic_notices = match &mut self.obstacle_observations {
            Some(observations) => observations
                .update(world, &self.observations, dt)
                .map_err(|_| Error::InvalidWorld)?,
            None => vec![],
        };
        for &notice in &dynamic_notices {
            world.emit(Event::Observation(notice));
        }
        if let (Some(movement), Some(domain)) = (&mut self.movement, self.domain) {
            // Initial search already reads geometry. Hold discoveries until there is
            // a route/result to judge instead of scheduling another full search blindly.
            let dynamic_notices = self
                .obstacle_observations
                .as_mut()
                .map_or_else(Vec::new, |o| {
                    o.take_route_notices(|actor| {
                        movement.actor(actor).is_some_and(|f| {
                            f.status != Status::Planning || movement.has_route(actor)
                        })
                    })
                });
            // Correlate authoritative notices directly by Item ID. Presentation
            // events are not read back, and unobserved journal changes do not enter
            // this reaction policy. Live collision checks remain unconditional.
            let mut reconsidered = std::collections::BTreeSet::new();
            for notice in self
                .observations
                .iter()
                .filter_map(ObservationTrack::notice)
                .chain(dynamic_notices.iter().copied())
            {
                let Some(item) = world.item(notice.target) else {
                    continue;
                };
                if item.collider.is_none() || item.character_body.is_some() {
                    continue;
                }
                let Some(feedback) = movement.actor(notice.observer) else {
                    continue;
                };
                let Some(body) = world.item(notice.observer).and_then(|i| i.character_body) else {
                    continue;
                };
                let route = movement.route(notice.observer);
                use io_game::stage::Stage;
                if let Some(request) = RouteAttentionStage
                    .run(RouteAttentionRequest {
                        notice,
                        ticket: feedback.ticket,
                        status: feedback.status,
                        position: feedback.execution.position,
                        route: &route,
                        body,
                    })
                    .map_err(|_| Error::InvalidWorld)?
                {
                    if !reconsidered.insert(notice.observer) {
                        continue;
                    }
                    submit(movement, &mut self.next_message, request)?;
                    if dynamic_notices
                        .iter()
                        .any(|n| n.observer == notice.observer && n.target == notice.target)
                    {
                        if let Some(observations) = &mut self.obstacle_observations {
                            observations.stats.reconsiderations += 1;
                        }
                    }
                }
            }
            for b in &mut self.behaviors {
                b.request_age += dt;
                let feedback = movement.actor(b.actor).ok_or(Error::InvalidWorld)?;
                let (intent, changed, interval) = match &mut b.pursuit {
                    Some(p) => {
                        let contact = self
                            .observations
                            .iter()
                            .find(|o| o.observer() == b.actor && o.target() == p.binding.target)
                            .and_then(ObservationTrack::contact);
                        let before = p.phase();
                        let intent =
                            p.decide(dt, feedback.execution.position, contact, feedback.status);
                        let changed = before != p.phase();
                        if changed {
                            world.emit(Event::Pursuit {
                                actor: b.actor,
                                phase: p.phase(),
                            });
                        }
                        (intent, changed, p.binding.settings.repath_seconds)
                    }
                    None => (pursuit::Intent::Patrol, false, 0.),
                };
                let desired = match intent {
                    pursuit::Intent::Patrol if changed => {
                        Some(Some(NavigationGoal::Position(b.patrol_goal)))
                    }
                    pursuit::Intent::Patrol if feedback.status == Status::Arrived => {
                        match &b.on_arrival {
                            OnArrival::Stop => None,
                            OnArrival::Patrol { points } => {
                                let p = points[b.next_destination];
                                b.next_destination = (b.next_destination + 1) % points.len();
                                Some(Some(NavigationGoal::Position(Vec3::new(p[0], p[1], p[2]))))
                            }
                        }
                    }
                    pursuit::Intent::Patrol => None,
                    pursuit::Intent::Hold => Some(None),
                    pursuit::Intent::Approach(position) => {
                        let goal = NavigationGoal::Approach {
                            position,
                            distance: b.approach_distance,
                            repath_seconds: interval,
                        };
                        let shifted = match b.requested {
                            Some(NavigationGoal::Approach { position: old, .. }) => {
                                (position - old).dot(position - old)
                                    > (domain.cell_size * 0.5).powi(2)
                            }
                            _ => true,
                        };
                        (changed
                            || (shifted
                                && b.request_age >= interval
                                && feedback.status != Status::Planning))
                            .then_some(Some(goal))
                    }
                    pursuit::Intent::SearchAt(mut p) => {
                        p.x =
                            p.x.clamp(domain.origin[0], domain.origin[0] + domain.size[0]);
                        p.y =
                            p.y.clamp(domain.origin[1], domain.origin[1] + domain.size[1]);
                        Some(Some(NavigationGoal::Position(p)))
                    }
                };
                // Finish a committed airborne action before replacing its route.
                // Safety/collision invalidation remains the executor's responsibility.
                if let Some(goal) =
                    desired.filter(|g| changed || *g != b.requested || b.deferred_goal.is_some())
                {
                    b.deferred_goal = Some(goal);
                }
                if !feedback.execution.grounded {
                    continue;
                }
                if let Some(goal) = b.deferred_goal.take() {
                    let request = match goal {
                        Some(goal) => NavigationRequest::Replace {
                            ticket: b.ticket,
                            goal,
                        },
                        None => NavigationRequest::Cancel { ticket: b.ticket },
                    };
                    b.ticket = submit(movement, &mut self.next_message, request)?;
                    b.requested = goal;
                    b.request_age = 0.;
                }
            }
            let frame = movement.update(world, dt)?;
            // Optional presentation copies. Rules use the authoritative feedback above, never history.
            for event in frame.events {
                world.emit(Event::Movement(event));
            }
        }
        Ok(())
    }
}
