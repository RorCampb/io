#![forbid(unsafe_code)]
//! Example game plugin: factions, damage, projectiles, NPCs and opportunity attacks.
pub use io_game::MovementWindow;
use io_game::{
    FrameworkError, GamePlugin, PluginInfo, PluginWorld, Round, RoundPhase, Tick, TurnProgress,
};
pub type Game = io_game::Session<Encounter>;
type Context<'a> = PluginWorld<'a, CombatEvent>;
mod definition;
pub use definition::*;
use io_types::Vec3;
use io_world::{WorldCommand, WorldView};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityId(pub u32);

pub type Phase = RoundPhase<Projectile, u32>;

#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub source: u64,
    pub target: u64,
    pub ability: AbilityId,
    pub position: Vec3,
    pub previous_position: Vec3,
    pub speed: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum GameCommand {
    StartCombat,
    Move {
        actor: u64,
        window: MovementWindow,
        x: f32,
        y: f32,
    },
    Attack {
        actor: u64,
        target: u64,
        ability: AbilityId,
    },
    EndTurn {
        actor: u64,
    },
}

#[derive(Clone, Debug)]
pub struct DamageNumber {
    pub target: u64,
    pub amount: u32,
    pub position: Vec3,
    pub created_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameError {
    Disabled,
    WrongPhase,
    UnknownActor,
    NotPlayer,
    NotYourTurn,
    Dead,
    InvalidValue,
    UnknownAbility,
    FriendlyTarget,
    OutOfRange,
    InvalidWorld,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CombatEvent {
    RoundStarted(u32),
    Hit {
        source: u64,
        target: u64,
        damage: u32,
        opportunity: bool,
    },
    Passed(u64),
}

#[derive(Clone, Debug)]
pub struct Combatant {
    item: u64,
    name: String,
    template: CharacterTemplate,
    abilities: Vec<AbilityId>,
    reaction: Option<AbilityId>,
    reaction_spent: bool,
    direction: Vec3,
}

impl Combatant {
    pub fn item(&self) -> u64 {
        self.item
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn control(&self) -> Control {
        self.template.control
    }
    pub fn faction(&self) -> u32 {
        self.template.faction
    }
    pub fn movement_speed(&self) -> f32 {
        self.template.movement_speed
    }
    pub fn abilities(&self) -> &[AbilityId] {
        &self.abilities
    }
    pub fn reaction_available(&self) -> bool {
        self.reaction.is_some() && !self.reaction_spent
    }
}

/// No mutable Item copies or duplicate health. Cloning gives an isolated read snapshot.
#[derive(Clone, Debug)]
pub struct Encounter {
    definition: Option<Arc<GameDefinition>>,
    abilities: Arc<[(String, AbilityDefinition)]>,
    actors: Vec<Combatant>,
    round: Round<Projectile, u32>,
    engagements: BTreeSet<(u64, u64)>,
    npc_wait: f64,
    last_event: Option<CombatEvent>,
    death_sources: BTreeMap<u64, Vec3>,
    time: f64,
    damage_numbers: VecDeque<DamageNumber>,
}

impl Default for Encounter {
    fn default() -> Self {
        Self {
            definition: None,
            abilities: Arc::from([]),
            actors: vec![],
            round: Round::new(3.).expect("valid default duration"),
            engagements: BTreeSet::new(),
            npc_wait: 0.,
            last_event: None,
            death_sources: BTreeMap::new(),
            time: 0.,
            damage_numbers: VecDeque::new(),
        }
    }
}

impl Encounter {
    pub fn new_at_round(
        definition: GameDefinition,
        world: &dyn WorldView,
        names: &BTreeMap<String, u64>,
        first_round: u32,
    ) -> Result<Self, String> {
        let duration = definition.movement_seconds;
        let confirmation = definition.confirm_round_start;
        let mut encounter = Self::new(definition, world, names)?;
        encounter.round =
            Round::starting_at(duration, first_round).map_err(|_| "invalid first round")?;
        encounter
            .round
            .set_confirmation(confirmation)
            .map_err(|_| "invalid round setup")?;
        Ok(encounter)
    }
    pub fn new(
        definition: GameDefinition,
        world: &dyn WorldView,
        names: &BTreeMap<String, u64>,
    ) -> Result<Self, String> {
        definition.validate()?;
        let abilities: Arc<[(String, AbilityDefinition)]> = definition
            .abilities
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<Vec<_>>()
            .into();
        let resolve =
            |name: &str| AbilityId(abilities.iter().position(|(n, _)| n == name).unwrap() as u32);
        let mut actors = Vec::new();
        let mut ids = BTreeSet::new();
        for binding in &definition.combatants {
            let id = *names
                .get(&binding.item)
                .ok_or_else(|| format!("unknown game item: {}", binding.item))?;
            let item = world
                .item(id)
                .ok_or_else(|| format!("missing game item: {id}"))?;
            if !ids.insert(id)
                || item.durability.is_none()
                || item.motion.is_some()
                || item.physics_body.is_some()
            {
                return Err(format!("game item {} needs durability and exclusive game movement (no route/physics body)", binding.item));
            }
            let template = definition.templates[&binding.template].clone();
            actors.push(Combatant {
                item: id,
                name: binding.item.clone(),
                abilities: template.abilities.iter().map(|a| resolve(a)).collect(),
                reaction: template.opportunity_ability.as_ref().map(|a| resolve(a)),
                template,
                reaction_spent: false,
                direction: Vec3::default(),
            });
        }
        actors.sort_by_key(|a| a.item);
        let mut round =
            Round::new(definition.movement_seconds).map_err(|_| "invalid round duration")?;
        round
            .set_confirmation(definition.confirm_round_start)
            .map_err(|_| "invalid round setup")?;
        let mut game = Self {
            round,
            definition: Some(Arc::new(definition)),
            abilities,
            actors,
            ..Self::default()
        };
        game.validate_world(world)
            .map_err(|_| "game actor world bindings are invalid".to_owned())?;
        game.refresh_engagements(world);
        Ok(game)
    }
    pub fn enabled(&self) -> bool {
        self.definition.is_some()
    }
    pub fn phase(&self) -> &Phase {
        self.round.phase()
    }
    pub fn movement_window(&self) -> Option<MovementWindow> {
        self.round.movement_window()
    }
    pub fn time(&self) -> f64 {
        self.time
    }
    pub fn damage_numbers(&self) -> &VecDeque<DamageNumber> {
        &self.damage_numbers
    }
    pub fn actors(&self) -> &[Combatant] {
        &self.actors
    }
    pub fn actor(&self, id: u64) -> Option<&Combatant> {
        self.actors.iter().find(|a| a.item == id)
    }
    pub fn ability(&self, id: AbilityId) -> Option<(&str, &AbilityDefinition)> {
        self.abilities
            .get(id.0 as usize)
            .map(|(name, value)| (name.as_str(), value))
    }
    pub fn engagements(&self) -> &BTreeSet<(u64, u64)> {
        &self.engagements
    }
    pub fn last_event(&self) -> Option<&CombatEvent> {
        self.last_event.as_ref()
    }
    pub fn active_actor(&self) -> Option<u64> {
        self.round.active_actor()
    }
    pub fn alive(world: &dyn WorldView, id: u64) -> bool {
        world
            .item(id)
            .and_then(|i| i.durability.as_ref())
            .is_some_and(|d| d.current() > 0)
    }
    fn actor_index(&self, id: u64) -> Result<usize, GameError> {
        self.actors
            .iter()
            .position(|a| a.item == id)
            .ok_or(GameError::UnknownActor)
    }
    fn handle(&mut self, world: &mut Context<'_>, command: GameCommand) -> Result<(), GameError> {
        if !self.enabled() {
            return Err(GameError::Disabled);
        }
        self.validate_world(world)?;
        match command {
            GameCommand::StartCombat => {
                if matches!(self.phase(), Phase::Ready { .. }) {
                    let round = self
                        .round
                        .begin_movement()
                        .map_err(|_| GameError::WrongPhase)?;
                    self.on_round_started(world, round);
                    return Ok(());
                }
                if self.phase() != &Phase::Exploration {
                    return Err(GameError::WrongPhase);
                }
                if !self.finish_if_over(world) {
                    let round = self.round.start().expect("initial exploration checked");
                    if matches!(self.phase(), Phase::Movement { .. }) {
                        self.on_round_started(world, round);
                    }
                }
            }
            GameCommand::Move {
                actor,
                window,
                x,
                y,
            } => {
                let i = self.actor_index(actor)?;
                if self.actors[i].control() != Control::Player {
                    return Err(GameError::NotPlayer);
                }
                if !x.is_finite() || !y.is_finite() || x.abs() > 1. || y.abs() > 1. {
                    return Err(GameError::InvalidValue);
                }
                if x == 0. && y == 0. {
                    self.actors[i].direction = Vec3::default();
                    return Ok(());
                }
                if !Self::alive(world, actor) {
                    return Err(GameError::Dead);
                }
                if self.movement_window() != Some(window) {
                    return Err(GameError::WrongPhase);
                }
                match self.phase() {
                    Phase::Exploration | Phase::Movement { .. } => {}
                    Phase::Ready { .. }
                    | Phase::Turns { .. }
                    | Phase::Resolving { .. }
                    | Phase::Finished { .. } => return Err(GameError::WrongPhase),
                }
                let length = (x * x + y * y).sqrt().max(1.);
                self.actors[i].direction = Vec3::new(x / length, y / length, 0.);
            }
            GameCommand::Attack {
                actor,
                target,
                ability,
            } => {
                self.check_player_turn(world, actor)?;
                self.check_attack(world, actor, target, ability)?;
                self.attack(world, actor, target, ability);
            }
            GameCommand::EndTurn { actor } => {
                self.check_player_turn(world, actor)?;
                self.record(world, CombatEvent::Passed(actor));
                self.next_turn(world);
            }
        }
        self.attach_deaths(world)?;
        self.refresh_engagements(world);
        Ok(())
    }
    fn check_player_turn(&self, world: &dyn WorldView, actor: u64) -> Result<(), GameError> {
        if !matches!(self.phase(), Phase::Turns { .. }) {
            return Err(GameError::WrongPhase);
        }
        let i = self.actor_index(actor)?;
        if self.actors[i].control() != Control::Player {
            return Err(GameError::NotPlayer);
        }
        if !Self::alive(world, actor) {
            return Err(GameError::Dead);
        }
        if self.active_actor() != Some(actor) {
            return Err(GameError::NotYourTurn);
        }
        Ok(())
    }
    pub fn check_attack(
        &self,
        world: &dyn WorldView,
        actor: u64,
        target: u64,
        ability: AbilityId,
    ) -> Result<(), GameError> {
        let source = self.actor(actor).ok_or(GameError::UnknownActor)?;
        let target_actor = self.actor(target).ok_or(GameError::UnknownActor)?;
        if !source.abilities.contains(&ability) {
            return Err(GameError::UnknownAbility);
        }
        let (_, definition) = self.ability(ability).ok_or(GameError::UnknownAbility)?;
        if !Self::alive(world, actor) || !Self::alive(world, target) {
            return Err(GameError::Dead);
        }
        if source.faction() == target_actor.faction() {
            return Err(GameError::FriendlyTarget);
        }
        if distance(world, actor, target) > definition.range {
            return Err(GameError::OutOfRange);
        }
        Ok(())
    }
    fn attack(&mut self, world: &mut Context<'_>, source: u64, target: u64, ability: AbilityId) {
        match self.abilities[ability.0 as usize].1.delivery {
            AbilityDelivery::Instant {} => {
                self.hit(world, source, target, ability, false);
                self.next_turn(world);
            }
            AbilityDelivery::Projectile { speed } => {
                let position = world.item(source).unwrap().bounds().center();
                self.round
                    .begin_resolution(Projectile {
                        source,
                        target,
                        ability,
                        position,
                        previous_position: position,
                        speed,
                    })
                    .expect("attack checked in turn phase");
            }
        }
    }
    fn hit(
        &mut self,
        world: &mut Context<'_>,
        source: u64,
        target: u64,
        ability: AbilityId,
        opportunity: bool,
    ) {
        let effect = self.abilities[ability.0 as usize].1.effect;
        match effect {
            AbilityEffect::Damage { amount } => {
                let before = world
                    .item(target)
                    .unwrap()
                    .durability
                    .as_ref()
                    .unwrap()
                    .current();
                let outcome = world.apply(WorldCommand::Damage { target, amount });
                assert!(
                    !matches!(outcome, io_world::CommandOutcome::Rejected(_)),
                    "validated damage target"
                );
                let bounds = world.item(target).unwrap().bounds();
                if self.damage_numbers.len() == 256 {
                    self.damage_numbers.pop_front();
                }
                self.damage_numbers.push_back(DamageNumber {
                    target,
                    amount: amount.min(before),
                    position: Vec3::new(bounds.center().x, bounds.center().y, bounds.max.z + 0.3),
                    created_at: self.time,
                });
                if !Self::alive(world, target) {
                    self.death_sources
                        .insert(target, world.item(source).unwrap().transform.anchor);
                }
                self.record(
                    world,
                    CombatEvent::Hit {
                        source,
                        target,
                        damage: amount.min(before),
                        opportunity,
                    },
                );
            }
        }
    }
    fn record(&mut self, world: &mut Context<'_>, event: CombatEvent) {
        world.emit(event.clone());
        self.last_event = Some(event);
    }
    fn on_round_started(&mut self, world: &mut Context<'_>, round: u32) {
        for actor in &mut self.actors {
            actor.reaction_spent = false;
            actor.direction = Vec3::default();
        }
        self.record(world, CombatEvent::RoundStarted(round));
        self.npc_wait = 0.;
    }
    fn finish_if_over(&mut self, world: &dyn WorldView) -> bool {
        let factions: BTreeSet<_> = self
            .actors
            .iter()
            .filter(|a| Self::alive(world, a.item))
            .map(Combatant::faction)
            .collect();
        if factions.len() > 1 {
            return false;
        }
        self.round.finish(factions.first().copied());
        for actor in &mut self.actors {
            actor.direction = Vec3::default();
        }
        true
    }
    fn next_turn(&mut self, world: &mut Context<'_>) {
        if self.finish_if_over(world) {
            return;
        }
        match self.round.end_turn(|id| Self::alive(world, id)) {
            Ok(TurnProgress::NewRound(round)) => {
                if matches!(self.phase(), Phase::Movement { .. }) {
                    self.on_round_started(world, round);
                }
            }
            Ok(TurnProgress::Actor(_)) => {}
            Err(io_game::RoundError::Exhausted) => self.round.finish(None),
            Err(error) => panic!("invalid internal turn transition: {error:?}"),
        }
        self.npc_wait = 0.;
    }
    fn advance(&mut self, world: &mut Context<'_>, tick: Tick) -> Result<(), GameError> {
        if !self.enabled() {
            return Ok(());
        }
        let dt = tick.seconds();
        self.time += f64::from(dt);
        while self
            .damage_numbers
            .front()
            .is_some_and(|hit| self.time - hit.created_at > 1.5)
        {
            self.damage_numbers.pop_front();
        }
        if !matches!(self.phase(), Phase::Exploration | Phase::Finished { .. })
            && self.finish_if_over(world)
        {
            self.attach_deaths(world)?;
            return Ok(());
        }
        match self.phase().clone() {
            Phase::Exploration => self.move_actors(world, dt, false),
            Phase::Movement { remaining, .. } => {
                self.move_actors(world, dt.min(remaining as f32), true);
                if self.finish_if_over(world) {
                    self.attach_deaths(world)?;
                    self.refresh_engagements(world);
                    return Ok(());
                }
                let mut order = Vec::new();
                if remaining - f64::from(dt) <= 1e-7 {
                    let mut actors: Vec<_> = self
                        .actors
                        .iter()
                        .filter(|a| Self::alive(world, a.item))
                        .collect();
                    actors.sort_by(|a, b| {
                        b.template
                            .initiative
                            .cmp(&a.template.initiative)
                            .then(a.item.cmp(&b.item))
                    });
                    order = actors.iter().map(|a| a.item).collect();
                }
                if self
                    .round
                    .advance_movement(f64::from(dt), order)
                    .expect("validated duration and live turn order")
                {
                    for actor in &mut self.actors {
                        actor.direction = Vec3::default();
                    }
                }
            }
            Phase::Turns { .. } => {
                if let Some(id) = self.active_actor() {
                    if !Self::alive(world, id) {
                        self.next_turn(world);
                    } else if self.actor(id).unwrap().control() == Control::Npc {
                        self.npc_wait += f64::from(dt);
                        if self.npc_wait >= 0.5 {
                            let attack = self.enemies(world, id).into_iter().find_map(|target| {
                                self.actor(id)
                                    .unwrap()
                                    .abilities
                                    .iter()
                                    .copied()
                                    .find(|&a| self.check_attack(world, id, target, a).is_ok())
                                    .map(|a| (target, a))
                            });
                            match attack {
                                Some((target, ability)) => self.attack(world, id, target, ability),
                                None => {
                                    self.record(world, CombatEvent::Passed(id));
                                    self.next_turn(world);
                                }
                            }
                        }
                    }
                }
            }
            Phase::Resolving {
                resolution: mut projectile,
                ..
            } => {
                if !Self::alive(world, projectile.target) || !Self::alive(world, projectile.source)
                {
                    self.round.complete_resolution().expect("resolving phase");
                    self.next_turn(world);
                } else {
                    let target = world.item(projectile.target).unwrap().bounds().center();
                    let delta = target - projectile.position;
                    let distance = delta.dot(delta).sqrt();
                    if distance <= projectile.speed * dt {
                        self.hit(
                            world,
                            projectile.source,
                            projectile.target,
                            projectile.ability,
                            false,
                        );
                        self.round.complete_resolution().expect("resolving phase");
                        self.next_turn(world);
                    } else {
                        projectile.previous_position = projectile.position;
                        projectile.position =
                            projectile.position + delta.scaled(projectile.speed * dt / distance);
                        *self.round.resolution_mut().expect("resolving phase") = projectile;
                    }
                }
            }
            Phase::Ready { .. } | Phase::Finished { .. } => {}
        }
        self.attach_deaths(world)?;
        self.refresh_engagements(world);
        Ok(())
    }
    pub fn enemies(&self, world: &dyn WorldView, actor: u64) -> Vec<u64> {
        let Some(source) = self.actor(actor) else {
            return vec![];
        };
        let mut targets: Vec<_> = self
            .actors
            .iter()
            .filter(|a| a.faction() != source.faction() && Self::alive(world, a.item))
            .map(|a| a.item)
            .collect();
        targets.sort_by(|&a, &b| {
            distance(world, actor, a)
                .total_cmp(&distance(world, actor, b))
                .then(a.cmp(&b))
        });
        targets
    }
    fn validate_world(&self, world: &dyn WorldView) -> Result<(), GameError> {
        for actor in &self.actors {
            let item = world.item(actor.item).ok_or(GameError::InvalidWorld)?;
            let p = item.transform.anchor;
            let living = Self::alive(world, actor.item);
            if item.durability.is_none()
                || item.motion.is_some()
                || (living && item.physics_body.is_some())
                || !p.finite()
                || p.x.abs().max(p.y.abs()).max(p.z.abs()) > if living { 9e5 } else { 1e6 }
            {
                return Err(GameError::InvalidWorld);
            }
        }
        Ok(())
    }
    fn attach_deaths(&mut self, world: &mut Context<'_>) -> Result<(), GameError> {
        for actor in &self.actors {
            let Some(config) = &actor.template.death_physics else {
                continue;
            };
            let item = world.item(actor.item).ok_or(GameError::InvalidWorld)?;
            if Self::alive(world, actor.item) || item.physics_body.is_some() {
                continue;
            }
            let collider = config.collider();
            let anchor = item.transform.anchor;
            let source = self
                .death_sources
                .get(&actor.item)
                .copied()
                .unwrap_or(anchor - Vec3::new(1., 0., 0.));
            let delta = anchor - source;
            let length = (delta.x * delta.x + delta.y * delta.y).sqrt();
            let direction = if length > 1e-5 {
                Vec3::new(delta.x / length, delta.y / length, 0.)
            } else {
                Vec3::new(1., 0., 0.)
            };
            let impulse =
                direction.scaled(config.knockback_impulse) + Vec3::new(0., 0., config.lift_impulse);
            // A small above-center offset supplies a physical tipping torque.
            let point = anchor
                + item.transform.rotation.rotate(collider.offset)
                + Vec3::new(0., 0., config.half_extents[2] * 0.5);
            world
                .attach_dynamic_body(actor.item, config.body(), collider, impulse, point)
                .map_err(|_| GameError::InvalidWorld)?;
        }
        Ok(())
    }
    fn refresh_engagements(&mut self, world: &dyn WorldView) {
        self.engagements.clear();
        for source in &self.actors {
            let Some(ability) = source.reaction else {
                continue;
            };
            if !Self::alive(world, source.item) {
                continue;
            }
            for target in self.enemies(world, source.item) {
                if distance(world, source.item, target)
                    <= self.abilities[ability.0 as usize].1.range
                {
                    self.engagements.insert((source.item, target));
                }
            }
        }
    }
    fn move_actors(&mut self, world: &mut Context<'_>, dt: f32, reactions: bool) {
        let starts: Vec<_> = self
            .actors
            .iter()
            .map(|a| world.item(a.item).unwrap().transform.anchor)
            .collect();
        let mut ends = starts.clone();
        for (i, actor) in self.actors.iter().enumerate() {
            if !Self::alive(world, actor.item) {
                continue;
            }
            let direction = match actor.control() {
                Control::Player => actor.direction,
                Control::Npc if reactions => match self.enemies(world, actor.item).first() {
                    Some(&enemy) => {
                        let delta = world.item(enemy).unwrap().transform.anchor - starts[i];
                        let length = delta.dot(delta).sqrt();
                        let stop = actor
                            .reaction
                            .map_or(1., |a| self.abilities[a.0 as usize].1.range * 0.8);
                        if length > stop {
                            delta.scaled(
                                (length - stop).min(actor.template.movement_speed * dt)
                                    / length
                                    / (actor.template.movement_speed * dt),
                            )
                        } else {
                            Vec3::default()
                        }
                    }
                    None => Vec3::default(),
                },
                Control::Npc => Vec3::default(),
            };
            ends[i] = starts[i] + direction.scaled(actor.template.movement_speed * dt);
            ends[i].x = ends[i].x.clamp(-9e5, 9e5);
            ends[i].y = ends[i].y.clamp(-9e5, 9e5);
            ends[i] = io_world::ground_destination(world, actor.item, ends[i])
                .expect("validated movement");
            // Reserve grounded destinations in stable actor order before planning the next mover.
            if world.item(actor.item).unwrap().grounded.is_some() {
                let rotation = world.item(actor.item).unwrap().transform.rotation;
                world
                    .place(actor.item, ends[i], rotation)
                    .expect("validated grounded pose");
            }
        }
        if reactions {
            let mut exits = Vec::new();
            for (i, source) in self.actors.iter().enumerate() {
                let Some(ability) = source.reaction.filter(|_| !source.reaction_spent) else {
                    continue;
                };
                if !Self::alive(world, source.item) {
                    continue;
                }
                let range = self.abilities[ability.0 as usize].1.range;
                for (j, target) in self.actors.iter().enumerate() {
                    if source.faction() == target.faction()
                        || !Self::alive(world, target.item)
                        || ends[j] == starts[j]
                    {
                        continue;
                    }
                    if let Some(t) = exit_fraction(starts[j] - starts[i], ends[j] - ends[i], range)
                    {
                        exits.push((t, i, j, ability));
                    }
                }
            }
            exits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
            for (t, i, j, ability) in exits {
                let (source, target) = (self.actors[i].item, self.actors[j].item);
                if self.actors[i].reaction_spent
                    || !Self::alive(world, source)
                    || !Self::alive(world, target)
                {
                    continue;
                }
                self.actors[i].reaction_spent = true;
                self.hit(world, source, target, ability, true);
                if !Self::alive(world, target) {
                    ends[j] = starts[j] + (ends[j] - starts[j]).scaled(t);
                }
            }
        }
        for (i, actor) in self.actors.iter().enumerate() {
            if ends[i] != starts[i] {
                let rotation = world.item(actor.item).unwrap().transform.rotation;
                world
                    .place(actor.item, ends[i], rotation)
                    .expect("validated game-owned placement");
            }
        }
    }
}

fn distance(world: &dyn WorldView, a: u64, b: u64) -> f32 {
    match (world.item(a), world.item(b)) {
        (Some(a), Some(b)) => {
            let d = a.transform.anchor - b.transform.anchor;
            d.dot(d).sqrt()
        }
        _ => f32::INFINITY,
    }
}

// Swept relative motion detects entering AND leaving a threat sphere in one tick.
fn exit_fraction(start: Vec3, end: Vec3, radius: f32) -> Option<f32> {
    if end.dot(end) <= radius * radius {
        return None;
    }
    let delta = end - start;
    let a = delta.dot(delta);
    if a <= f32::EPSILON {
        return None;
    }
    let b = start.dot(delta);
    let c = start.dot(start) - radius * radius;
    let discriminant = b * b - a * c;
    if discriminant < 0. {
        return None;
    }
    let t = (-b + discriminant.sqrt()) / a;
    (0. ..=1.).contains(&t).then_some(t)
}

impl From<FrameworkError> for GameError {
    fn from(error: FrameworkError) -> Self {
        match error {
            FrameworkError::InvalidTimestep => Self::InvalidValue,
            FrameworkError::InvalidPlugin | FrameworkError::InvalidActiveItem => Self::InvalidWorld,
        }
    }
}

impl GamePlugin for Encounter {
    type Command = GameCommand;
    type Event = CombatEvent;
    type Error = GameError;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "io-encounter",
            version: 1,
        }
    }
    fn validate(&self, world: &dyn WorldView) -> Result<(), GameError> {
        self.validate_world(world)
    }
    fn active_items(&self) -> Vec<u64> {
        self.actors.iter().map(Combatant::item).collect()
    }
    fn command(&mut self, world: &mut Context<'_>, command: GameCommand) -> Result<(), GameError> {
        self.handle(world, command)
    }
    fn update(&mut self, world: &mut Context<'_>, tick: Tick) -> Result<(), GameError> {
        self.advance(world, tick)
    }
}

/// Application composition root registers this plugin; io-game never imports it.
pub fn register(
    definition: GameDefinition,
    world: &dyn WorldView,
    names: &BTreeMap<String, u64>,
) -> Result<Game, String> {
    let plugin = Encounter::new(definition, world, names)?;
    Game::register(plugin, world).map_err(|error| format!("encounter registration: {error:?}"))
}

#[cfg(test)]
mod tests;
