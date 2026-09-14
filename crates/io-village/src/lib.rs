#![forbid(unsafe_code)]
//! Exploration example plugin. Composes encounter rules on the same world, not another simulation.
use io_encounter::{CombatEvent, Encounter, GameCommand, GameDefinition, GameError, Phase};
use io_game::{GamePlugin, PluginInfo, PluginWorld, Tick};
use io_types::{Rotation, Vec3};
use io_world::{ground_destination, line_of_sight, WorldView};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

pub type Game = io_game::Session<Village>;
type Context<'a> = PluginWorld<'a, CombatEvent>;
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Combat(GameCommand),
    Talk { target: Option<u64> },
    Recruit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogueGroup {
    Men,
    Women,
    Children,
    Parents,
    Shop,
    Civic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Resident,
    Companion,
    Raider,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settlement {
    pub name: String,
    pub center: [f32; 2],
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NpcDefinition {
    pub item: String,
    pub name: String,
    pub settlement: usize,
    pub role: Role,
    pub family: String,
    pub home: String,
    pub job: String,
    pub groups: Vec<DialogueGroup>,
    pub route: Vec<[f32; 2]>,
    pub speed: f32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub player: String,
    pub settlements: Vec<Settlement>,
    pub npcs: Vec<NpcDefinition>,
    pub dialogues: BTreeMap<DialogueGroup, Vec<String>>,
    pub combat: GameDefinition,
}
impl Definition {
    pub fn validate(&self) -> Result<(), String> {
        self.combat.validate()?;
        let text =
            |s: &str| !s.is_empty() && s.is_ascii() && !s.bytes().any(|b| b < 32) && s.len() <= 180;
        if self.version != 1
            || !text(&self.player)
            || self.settlements.is_empty()
            || self.settlements.len() > 32
            || self.npcs.len() > 4096
        {
            return Err("invalid exploration header".into());
        }
        for s in &self.settlements {
            if !text(&s.name) || s.center.iter().any(|v| !v.is_finite()) {
                return Err("invalid settlement".into());
            }
        }
        for group in [
            DialogueGroup::Men,
            DialogueGroup::Women,
            DialogueGroup::Children,
            DialogueGroup::Parents,
            DialogueGroup::Shop,
            DialogueGroup::Civic,
        ] {
            let lines = self
                .dialogues
                .get(&group)
                .ok_or("missing dialogue category")?;
            if lines.is_empty() || lines.len() > 1000 || lines.iter().any(|s| !text(s)) {
                return Err("invalid dialogue lines".into());
            }
        }
        let mut names = BTreeSet::new();
        for n in &self.npcs {
            if !names.insert(&n.item)
                || n.item == self.player
                || n.settlement >= self.settlements.len()
                || [&n.item, &n.name, &n.family, &n.home, &n.job]
                    .iter()
                    .any(|s| !text(s))
                || n.groups.is_empty()
                || n.groups.len() > 6
                || n.route.len() < 2
                || n.route.len() > 32
                || n.route
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 100_000.)
                || !n.speed.is_finite()
                || !(0.1..=8.).contains(&n.speed)
            {
                return Err(format!("invalid NPC {}", n.item));
            }
            for i in 0..n.route.len() {
                let a = n.route[i];
                let b = n.route[(i + 1) % n.route.len()];
                if (a[0] - b[0]).hypot(a[1] - b[1]) < 0.5 {
                    return Err("route legs must be at least half a meter".into());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
struct Npc {
    id: u64,
    definition: usize,
    segment: usize,
    elapsed: f32,
    visits: u64,
    recruited: bool,
    lines: usize,
    trail_cursor: u64,
}
#[derive(Clone, Debug)]
pub struct Village {
    definition: Arc<Definition>,
    names: Arc<BTreeMap<String, u64>>,
    player: u64,
    npcs: Vec<Npc>,
    encounter: Encounter,
    in_combat: bool,
    direction: Vec3,
    dialogue: Option<(String, String)>,
    pub schedule_updates: u64,
    trail: VecDeque<Vec3>,
    trail_start: u64,
    last_round: u32,
    exit_delay: f32,
    lost_sight: f32,
}
impl Village {
    pub fn new(
        definition: Definition,
        world: &dyn WorldView,
        names: BTreeMap<String, u64>,
    ) -> Result<Self, String> {
        definition.validate()?;
        if world.terrain().is_none() {
            return Err("exploration requires shared terrain".into());
        }
        let player = *names
            .get(&definition.player)
            .ok_or("unknown exploration player")?;
        let player_binding = definition
            .combat
            .combatants
            .iter()
            .find(|a| a.item == definition.player)
            .ok_or("player needs combat binding")?;
        let player_template = &definition.combat.templates[&player_binding.template];
        if player_template.control != io_encounter::Control::Player {
            return Err("exploration player needs player control".into());
        }
        let mut unique_ids = BTreeSet::from([player]);
        let mut npcs = Vec::new();
        for (i, n) in definition.npcs.iter().enumerate() {
            let id = *names.get(&n.item).ok_or("unknown NPC item")?;
            if !unique_ids.insert(id) {
                return Err("duplicate exploration item identity".into());
            }
            if n.role != Role::Resident {
                let binding = definition
                    .combat
                    .combatants
                    .iter()
                    .find(|a| a.item == n.item)
                    .ok_or("combat NPC needs binding")?;
                let template = &definition.combat.templates[&binding.template];
                if template.control != io_encounter::Control::Npc
                    || (template.faction == player_template.faction) != (n.role == Role::Companion)
                {
                    return Err("invalid NPC combat faction/control".into());
                }
            }
            if !names.contains_key(&n.home) {
                return Err(format!("unknown home {}", n.home));
            }
            for point in &n.route {
                if world
                    .terrain()
                    .unwrap()
                    .height(point[0], point[1])
                    .is_none()
                {
                    return Err("NPC route outside terrain".into());
                }
            }
            npcs.push(Npc {
                id,
                definition: i,
                segment: 1,
                elapsed: 0.,
                visits: 0,
                recruited: false,
                lines: 0,
                trail_cursor: 0,
            });
        }
        let mut idle = definition.combat.clone();
        idle.combatants.retain(|c| c.item == definition.player);
        let encounter = Encounter::new(idle, world, &names)?;
        let plugin = Self {
            definition: Arc::new(definition),
            names: Arc::new(names),
            player,
            npcs,
            encounter,
            in_combat: false,
            direction: Vec3::default(),
            dialogue: None,
            schedule_updates: 0,
            trail: VecDeque::from([world.item(player).unwrap().transform.anchor]),
            trail_start: 0,
            last_round: 0,
            exit_delay: 0.,
            lost_sight: 0.,
        };
        plugin
            .validate(world)
            .map_err(|e| format!("village bindings: {e:?}"))?;
        Ok(plugin)
    }
    pub fn encounter(&self) -> &Encounter {
        &self.encounter
    }
    pub fn exploring(&self) -> bool {
        !self.in_combat
    }
    pub fn player(&self) -> u64 {
        self.player
    }
    pub fn companion_count(&self) -> usize {
        self.npcs.iter().filter(|n| n.recruited).count()
    }
    pub fn npc_count(&self) -> usize {
        self.npcs.len()
    }
    pub fn scheduled_visits(&self) -> u64 {
        self.npcs.iter().map(|n| n.visits).sum()
    }
    pub fn interactive(&self, id: u64) -> bool {
        self.npcs
            .iter()
            .any(|n| n.id == id && self.definition.npcs[n.definition].role != Role::Raider)
    }
    fn nearest(&self, world: &dyn WorldView, role: Option<Role>, range: f32) -> Option<usize> {
        self.npcs
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                let r = self.definition.npcs[n.definition].role;
                r != Role::Raider
                    && role.is_none_or(|wanted| wanted == r)
                    && Encounter::alive(world, n.id)
            })
            .map(|(i, n)| (i, distance(world, self.player, n.id)))
            .filter(|(_, d)| *d <= range)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(i, _)| i)
    }
    fn walk(world: &mut Context<'_>, id: u64, delta: Vec3) -> Result<(), GameError> {
        let item = world.item(id).ok_or(GameError::InvalidWorld)?;
        let start = item.transform.anchor;
        let requested = start + delta;
        let end = ground_destination(world, id, requested).map_err(|_| GameError::InvalidValue)?;
        let d = end - start;
        let yaw = if d.x * d.x + d.y * d.y > 0.00001 {
            Rotation::yaw(d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2).unwrap()
        } else {
            item.transform.rotation
        };
        world
            .place(id, end, yaw)
            .map_err(|_| GameError::InvalidWorld)
    }
    fn start_combat(&mut self, world: &mut Context<'_>) -> Result<(), GameError> {
        if self.in_combat {
            return Err(GameError::WrongPhase);
        }
        let hostiles: BTreeSet<_> = self
            .npcs
            .iter()
            .filter(|n| {
                self.definition.npcs[n.definition].role == Role::Raider
                    && Encounter::alive(world, n.id)
                    && distance(world, self.player, n.id) <= 20.
                    && line_of_sight(world, self.player, n.id).unwrap_or(false)
            })
            .map(|n| n.id)
            .collect();
        if hostiles.is_empty() {
            return Err(GameError::OutOfRange);
        }
        let mut members = BTreeSet::from([self.player]);
        for n in &self.npcs {
            if Encounter::alive(world, n.id)
                && ((n.recruited && distance(world, self.player, n.id) <= 30.)
                    || hostiles.contains(&n.id))
            {
                members.insert(n.id);
            }
        }
        let mut config = self.definition.combat.clone();
        config.combatants.retain(|a| {
            self.names
                .get(&a.item)
                .is_some_and(|id| members.contains(id))
        });
        let first = self
            .last_round
            .checked_add(1)
            .ok_or(GameError::InvalidValue)?;
        let mut encounter = Encounter::new_at_round(config, world, &self.names, first)
            .map_err(|_| GameError::InvalidWorld)?;
        encounter.command(world, GameCommand::StartCombat)?;
        self.encounter = encounter;
        self.in_combat = true;
        self.exit_delay = 0.;
        self.lost_sight = 0.;
        self.last_round = first;
        self.direction = Vec3::default();
        self.dialogue = None;
        Ok(())
    }
    pub fn lines(&self, world: &dyn WorldView) -> Vec<String> {
        let p = world.item(self.player).unwrap().transform.anchor;
        let settlement = self
            .definition
            .settlements
            .iter()
            .min_by(|a, b| {
                (p.x - a.center[0])
                    .hypot(p.y - a.center[1])
                    .total_cmp(&(p.x - b.center[0]).hypot(p.y - b.center[1]))
            })
            .unwrap();
        let mut lines = vec![
            format!("WANDERING - NEAR {}", settlement.name.to_ascii_uppercase()),
            format!(
                "1 SQ KM - X {:.0} Y {:.0} - PARTY {}",
                p.x,
                p.y,
                self.companion_count() + 1
            ),
            "WASD MOVE - E TALK - R RECRUIT - M MAP".into(),
            "ARROWS OR RIGHT DRAG ORBIT - SCROLL ZOOMS".into(),
            format!(
                "{} NPCS - {} SCHEDULE UPDATES",
                self.npc_count(),
                self.schedule_updates
            ),
        ];
        if let Some(i) = self.nearest(world, None, 4.) {
            let n = &self.definition.npcs[self.npcs[i].definition];
            lines.push(format!("NEARBY {} - {}", n.name, n.job).to_ascii_uppercase());
        }
        if let Some((speaker, line)) = &self.dialogue {
            // Give dialogue the bounded HUD's space instead of truncating it behind diagnostics.
            lines.truncate(2);
            lines.push(if speaker.len() > 58 {
                format!("{}...", &speaker[..55])
            } else {
                speaker.to_ascii_uppercase()
            });
            let mut current = String::new();
            for word in line
                .split_whitespace()
                .flat_map(|word| word.as_bytes().chunks(58))
            {
                let word = std::str::from_utf8(word).expect("dialogue is validated ASCII");
                if !current.is_empty() && current.len() + word.len() + 1 > 58 {
                    lines.push(current);
                    current = String::new();
                }
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
            if !current.is_empty() {
                lines.push(current);
            }
            lines.push("E NEXT LINE - R RECRUIT COMPANION".into());
        }
        lines
    }
}
impl GamePlugin for Village {
    type Command = Command;
    type Event = CombatEvent;
    type Error = GameError;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "io-village",
            version: 1,
        }
    }
    fn validate(&self, world: &dyn WorldView) -> Result<(), GameError> {
        for id in std::iter::once(self.player).chain(self.npcs.iter().map(|n| n.id)) {
            let item = world.item(id).ok_or(GameError::InvalidWorld)?;
            if item.durability.is_none() || (Encounter::alive(world, id) && item.grounded.is_none())
            {
                return Err(GameError::InvalidWorld);
            }
        }
        Ok(())
    }
    fn active_items(&self) -> Vec<u64> {
        if self.in_combat {
            self.encounter.active_items()
        } else {
            vec![self.player]
        }
    }
    fn command(&mut self, world: &mut Context<'_>, command: Command) -> Result<(), GameError> {
        if self.in_combat {
            return match command {
                Command::Combat(c) => self.encounter.command(world, c),
                Command::Talk { .. } | Command::Recruit => Err(GameError::WrongPhase),
            };
        }
        if !Encounter::alive(world, self.player) {
            return Err(GameError::Dead);
        }
        match command {
            Command::Combat(GameCommand::Move {
                actor,
                window,
                x,
                y,
            }) => {
                if actor != self.player {
                    return Err(GameError::NotPlayer);
                }
                if window != io_game::MovementWindow::Exploration {
                    return Err(GameError::WrongPhase);
                }
                if !x.is_finite() || !y.is_finite() || x.abs() > 1. || y.abs() > 1. {
                    return Err(GameError::InvalidValue);
                }
                let len = (x * x + y * y).sqrt().max(1.);
                self.direction = Vec3::new(x / len, y / len, 0.);
                Ok(())
            }
            Command::Combat(GameCommand::StartCombat) => self.start_combat(world),
            Command::Combat(_) => Err(GameError::WrongPhase),
            Command::Talk { target } => {
                let i = match target {
                    Some(id) => self
                        .npcs
                        .iter()
                        .position(|n| {
                            n.id == id
                                && self.interactive(id)
                                && Encounter::alive(world, id)
                                && distance(world, self.player, id) <= 4.
                        })
                        .ok_or(GameError::OutOfRange)?,
                    None => self.nearest(world, None, 4.).ok_or(GameError::OutOfRange)?,
                };
                let n = &mut self.npcs[i];
                let def = &self.definition.npcs[n.definition];
                let group = def.groups[n.lines % def.groups.len()];
                let lines = &self.definition.dialogues[&group];
                let line = lines[(n.lines / def.groups.len() + n.definition) % lines.len()].clone();
                n.lines += 1;
                self.dialogue = Some((
                    format!("{} / {} FAMILY / {}", def.name, def.family, def.job),
                    if def.role == Role::Companion && !n.recruited {
                        format!("{line} I can travel with you. Press R to recruit me.")
                    } else {
                        line
                    },
                ));
                Ok(())
            }
            Command::Recruit => {
                let i = self
                    .nearest(world, Some(Role::Companion), 4.)
                    .ok_or(GameError::OutOfRange)?;
                self.npcs[i].recruited = true;
                self.npcs[i].trail_cursor = self.trail_start + self.trail.len() as u64 - 1;
                self.dialogue = Some((
                    self.definition.npcs[self.npcs[i].definition].name.clone(),
                    "I am with you. I will follow and fight at your side.".into(),
                ));
                Ok(())
            }
        }
    }
    fn update(&mut self, world: &mut Context<'_>, tick: Tick) -> Result<(), GameError> {
        let dt = tick.seconds();
        if self.in_combat {
            match self.encounter.phase() {
                Phase::Ready { round }
                | Phase::Movement { round, .. }
                | Phase::Turns { round, .. }
                | Phase::Resolving { round, .. } => self.last_round = *round,
                _ => {}
            }
            self.encounter.update(world, tick)?;
            // Keep close engagements and brief occlusion from flickering combat on/off.
            // Resolve attacks normally; disengagement is evaluated only during movement.
            if matches!(self.encounter.phase(), Phase::Movement { .. }) {
                let contact = self.encounter.actors().iter().any(|a| {
                    a.faction() == self.encounter.actor(self.player).unwrap().faction()
                        && Encounter::alive(world, a.item())
                        && self
                            .encounter
                            .enemies(world, a.item())
                            .iter()
                            .any(|&enemy| {
                                let d = distance(world, a.item(), enemy);
                                d <= 5.
                                    || (d <= 30.
                                        && line_of_sight(world, a.item(), enemy).unwrap_or(false))
                            })
                });
                self.lost_sight = if contact { 0. } else { self.lost_sight + dt };
            } else {
                self.lost_sight = 0.;
            }
            if matches!(self.encounter.phase(), Phase::Finished { .. }) {
                self.exit_delay += dt;
            }
            if Encounter::alive(world, self.player)
                && ((matches!(self.encounter.phase(), Phase::Finished { .. })
                    && self.exit_delay >= 1.6)
                    || self.lost_sight >= 2.)
            {
                let mut config = self.definition.combat.clone();
                config
                    .combatants
                    .retain(|a| a.item == self.definition.player);
                self.encounter = Encounter::new(config, world, &self.names)
                    .map_err(|_| GameError::InvalidWorld)?;
                self.in_combat = false;
                self.direction = Vec3::default();
                self.trail.clear();
                self.trail
                    .push_back(world.item(self.player).unwrap().transform.anchor);
                self.trail_start = 0;
                for npc in &mut self.npcs {
                    npc.trail_cursor = 0;
                }
                self.dialogue = Some((
                    "THE ROAD IS QUIET".into(),
                    "The fight is over. Your companions continue with you.".into(),
                ));
            }
        } else if Encounter::alive(world, self.player) {
            let speed = self.encounter.actor(self.player).unwrap().movement_speed();
            Self::walk(world, self.player, self.direction.scaled(speed * dt))?;
        }
        let player = world.item(self.player).unwrap().transform.anchor;
        if !self.in_combat {
            let last = *self.trail.back().unwrap();
            let d = player - last;
            if d.dot(d) > 64. * 64. {
                self.trail.clear();
                self.trail.push_back(player);
                self.trail_start = 0;
                for n in &mut self.npcs {
                    n.trail_cursor = 0;
                }
            } else if d.dot(d) > 0.75 * 0.75 {
                if self.trail.len() == 8192 {
                    self.trail.pop_front();
                    self.trail_start += 1;
                }
                self.trail.push_back(player);
            }
        }
        let mut follower = 0;
        for n in &mut self.npcs {
            if !Encounter::alive(world, n.id)
                || (self.in_combat && self.encounter.actor(n.id).is_some())
            {
                continue;
            }
            let def = &self.definition.npcs[n.definition];
            if def.role == Role::Raider {
                continue;
            }
            let pos = world.item(n.id).unwrap().transform.anchor;
            if n.recruited {
                follower += 1;
                n.trail_cursor = n.trail_cursor.max(self.trail_start);
                let last = self.trail_start + self.trail.len() as u64 - 1;
                n.trail_cursor = n.trail_cursor.min(last);
                let mut target = self.trail[(n.trail_cursor - self.trail_start) as usize];
                while n.trail_cursor < last && (target - pos).dot(target - pos) < 0.6 * 0.6 {
                    n.trail_cursor += 1;
                    target = self.trail[(n.trail_cursor - self.trail_start) as usize];
                }
                let d = Vec3::new(target.x - pos.x, target.y - pos.y, 0.);
                let length = d.dot(d).sqrt();
                let separation = (player - pos).dot(player - pos).sqrt();
                if length > 0.05 && (n.trail_cursor < last || separation > 1.5 + follower as f32) {
                    let speed = self.encounter.actor(self.player).unwrap().movement_speed() * 1.25;
                    Self::walk(world, n.id, d.scaled((speed * dt).min(length) / length))?;
                }
                continue;
            }
            n.elapsed += dt;
            if (player - pos).dot(player - pos) > 80. * 80. && n.elapsed < 1. {
                continue;
            }
            let mut budget = def.speed * n.elapsed;
            n.elapsed = 0.;
            self.schedule_updates += 1;
            for _ in 0..32 {
                if budget <= 0.0001 {
                    break;
                }
                let pos = world.item(n.id).unwrap().transform.anchor;
                let dest = def.route[n.segment];
                let d = Vec3::new(dest[0] - pos.x, dest[1] - pos.y, 0.);
                let length = d.dot(d).sqrt();
                if length < 0.25 {
                    n.segment = (n.segment + 1) % def.route.len();
                    n.visits += 1;
                    continue;
                }
                let step = budget.min(length);
                Self::walk(world, n.id, d.scaled(step / length))?;
                budget -= step;
                if length > step {
                    break;
                }
            }
        }
        if !self.in_combat
            && Encounter::alive(world, self.player)
            && self.npcs.iter().any(|n| {
                self.definition.npcs[n.definition].role == Role::Raider
                    && Encounter::alive(world, n.id)
                    && distance(world, self.player, n.id) <= 20.
                    && line_of_sight(world, self.player, n.id).unwrap_or(false)
            })
        {
            self.start_combat(world)?;
        }
        Ok(())
    }
}
fn distance(world: &dyn WorldView, a: u64, b: u64) -> f32 {
    let d = world.item(a).unwrap().transform.anchor - world.item(b).unwrap().transform.anchor;
    d.dot(d).sqrt()
}
