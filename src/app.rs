#![forbid(unsafe_code)]
use crate::camera::Camera;
use crate::model::ModelLibrary;
use crate::projection::Frame;
use crate::timing::SimulationTiming;
use crate::worker::{Event, Region, Simulation, Worker};
use io_types::Vec3;
use io_world::{Axis, CommandOutcome, World, WorldCommand, WorldView};
use std::collections::{BTreeMap, HashSet};
use std::ops::Bound::{Excluded, Unbounded};

pub type CameraId = u64;
pub enum Action {
    Orbit { yaw: f32, pitch: f32 },
    Zoom { steps: f32 },
    Pan { dx: f32, dy: f32 },
    Distance { steps: f32 },
    ResetView,
    ResizeAxis { axis: Axis, delta: i32 },
    NewCamera,
    NextCamera,
    ToggleFollow,
    ToggleGrid,
}
struct View {
    camera: Camera,
    frame: Frame,
    world_revision: Option<u64>,
    dirty: bool,
    follow: Option<usize>,
    home: Camera,
    home_follow: Option<usize>,
    follow_offset: Vec3,
}
impl View {
    fn new(camera: Camera, follow: Option<usize>) -> Self {
        Self {
            home: camera.clone(),
            follow,
            home_follow: follow,
            follow_offset: Vec3::default(),
            camera,
            frame: Frame::default(),
            world_revision: None,
            dirty: true,
        }
    }
}
pub struct App {
    game: crate::game::Game,
    game_target: Option<u64>,
    game_player: Option<u64>,
    game_ability: Option<(u64, io_encounter::AbilityId)>,
    timing: SimulationTiming,
    world: Simulation,
    cameras: BTreeMap<CameraId, View>,
    active_camera: CameraId,
    next_camera: CameraId,
    serial: u64,
    accumulator: f64,
    active_simulation: std::sync::Arc<[usize]>,
    last_alpha: f32,
    simulation_dirty: bool,
    spatial_revision: u64,
    library: &'static ModelLibrary,
    grid: bool,
    map_view: bool,
}
impl App {
    pub fn new_threaded() -> Result<Self, String> {
        Self::new()?.into_threaded()
    }
    pub(crate) fn into_threaded(mut self) -> Result<Self, String> {
        let regions = self.regions();
        self.world = match self.world {
            Simulation::Inline(world) => Simulation::Threaded(Worker::spawn_with_game(
                *world,
                regions,
                self.timing,
                self.game.clone(),
            )?),
            Simulation::Threaded(worker) => Simulation::Threaded(worker),
        };
        self.simulation_dirty = false;
        Ok(self)
    }
    fn regions(&self) -> Vec<Region> {
        self.cameras
            .values()
            .map(|view| Region {
                center: view.camera.target(),
                radius: view.camera.render_distance(),
                follow: view.follow,
            })
            .collect()
    }
    pub fn worker(&self) -> Option<&Worker> {
        match &self.world {
            Simulation::Threaded(worker) => Some(worker),
            Simulation::Inline(_) => None,
        }
    }
    pub fn new() -> Result<Self, String> {
        let library = ModelLibrary::global().map_err(str::to_owned)?;
        let world = crate::demo::world(library)?;
        let config = &library.config;
        let mut camera = Camera::new(Vec3::new(
            config.origin[0] + config.camera.target[0],
            config.origin[1] + config.camera.target[1],
            config.origin[2] + config.camera.target[2],
        ));
        camera.set_zoom(config.camera.zoom);
        camera.set_distance(config.camera.render_distance);
        camera.set_coverage(config.camera.coverage);
        let follow = config
            .camera
            .follow
            .as_ref()
            .and_then(|name| config.items.iter().position(|i| &i.name == name));
        let game = crate::demo::game(library, &world)?;
        let mut app = Self::with_world(world, camera, follow, library);
        app.game = game;
        Ok(app)
    }
    pub(crate) fn with_world(
        world: World,
        camera: Camera,
        follow: Option<usize>,
        library: &'static ModelLibrary,
    ) -> Self {
        let mut view = View::new(camera, follow);
        if let Some(id) = follow {
            view.follow_offset = view.camera.target() - world.items()[id].transform.anchor;
        }
        Self {
            game: crate::game::Game::default(),
            game_target: None,
            game_player: None,
            game_ability: None,
            timing: library.config.simulation,
            world: Simulation::Inline(Box::new(world)),
            cameras: BTreeMap::from([(1, view)]),
            active_camera: 1,
            next_camera: 2,
            serial: 0,
            accumulator: 0.,
            active_simulation: std::sync::Arc::from([]),
            last_alpha: 0.,
            simulation_dirty: true,
            spatial_revision: 0,
            library,
            grid: false,
            map_view: false,
        }
    }
    pub fn world(&self) -> &dyn WorldView {
        &*self.world
    }
    pub fn game(&self) -> &crate::game::Game {
        self.worker().map_or(&self.game, |w| &w.current.game)
    }
    pub fn controlled_actor(&self) -> Option<u64> {
        let game = self.game();
        let player = |id| {
            game.actor(id)
                .is_some_and(|a| a.control() == io_encounter::Control::Player)
                && io_encounter::Encounter::alive(self.world(), id)
        };
        game.active_actor()
            .filter(|&id| player(id))
            .or(self.game_player.filter(|&id| player(id)))
            .or_else(|| {
                game.actors()
                    .iter()
                    .find(|a| player(a.item()))
                    .map(|a| a.item())
            })
    }
    fn target_actor(&self, actor: u64) -> Option<u64> {
        let targets = self.game().enemies(self.world(), actor);
        self.game_target
            .filter(|id| targets.contains(id))
            .or_else(|| targets.first().copied())
    }
    pub fn game_action(&mut self, kind: u32, slot: u32, x: f32, y: f32) -> bool {
        use io_encounter::GameCommand;
        if !self.game().enabled() {
            return false;
        }
        let command = match kind {
            1 => GameCommand::StartCombat,
            2 => {
                let Some(actor) = self.controlled_actor() else {
                    return false;
                };
                let Some(window) = self.game().movement_window() else {
                    return x == 0. && y == 0.;
                };
                let Some(direction) = self
                    .camera(self.active_camera)
                    .and_then(|c| c.ground_direction(x, y))
                else {
                    return false;
                };
                GameCommand::Move {
                    actor,
                    window,
                    x: direction.x.clamp(-1., 1.),
                    y: direction.y.clamp(-1., 1.),
                }
            }
            3 => {
                let Some(actor) = self.controlled_actor() else {
                    return false;
                };
                let Some(&ability) = self
                    .game()
                    .actor(actor)
                    .unwrap()
                    .abilities()
                    .get(slot as usize)
                else {
                    return false;
                };
                self.game_ability = Some((actor, ability));
                return true;
            }
            4 => {
                let Some(actor) = self.controlled_actor() else {
                    return false;
                };
                GameCommand::EndTurn { actor }
            }
            5 => {
                let Some(actor) = self.controlled_actor() else {
                    return false;
                };
                let targets = self.game().enemies(self.world(), actor);
                if targets.is_empty() {
                    return false;
                }
                let current = self.target_actor(actor);
                let next = targets
                    .iter()
                    .position(|&id| Some(id) == current)
                    .map_or(0, |i| (i + 1) % targets.len());
                self.game_target = Some(targets[next]);
                return true;
            }
            6 => {
                if !matches!(
                    self.game().phase(),
                    io_encounter::Phase::Exploration | io_encounter::Phase::Movement { .. }
                ) {
                    return false;
                }
                let actors: Vec<_> = self
                    .game()
                    .actors()
                    .iter()
                    .filter(|a| {
                        a.control() == io_encounter::Control::Player
                            && io_encounter::Encounter::alive(self.world(), a.item())
                    })
                    .map(|a| a.item())
                    .collect();
                if actors.is_empty() {
                    return false;
                }
                let previous = self.controlled_actor();
                let next = actors
                    .iter()
                    .position(|&id| Some(id) == previous)
                    .map_or(0, |i| (i + 1) % actors.len());
                if let Some(actor) = previous {
                    self.send_game(GameCommand::Move {
                        actor,
                        window: self.game().movement_window().unwrap(),
                        x: 0.,
                        y: 0.,
                    });
                }
                self.game_player = Some(actors[next]);
                self.game_target = None;
                return true;
            }
            7 => return self.click_game_target(x, y),
            8 => return self.dispatch(Action::Orbit { yaw: x, pitch: y }),
            9 => return self.send_exploration(io_village::Command::Talk { target: None }),
            10 => return self.send_exploration(io_village::Command::Recruit),
            11 => {
                let Some(village) = self.game().village() else {
                    return false;
                };
                let player = village.player();
                let index = self
                    .world()
                    .items()
                    .iter()
                    .position(|i| i.id == player)
                    .unwrap();
                let position = self.world().item(player).unwrap().transform.anchor;
                self.map_view = !self.map_view;
                let view = self.cameras.get_mut(&self.active_camera).unwrap();
                view.follow = if self.map_view { None } else { Some(index) };
                view.follow_offset = Vec3::new(0., 0., 1.);
                view.camera.set_target(if self.map_view {
                    Vec3::new(0., 0., 15.)
                } else {
                    position + view.follow_offset
                });
                view.camera
                    .set_zoom(if self.map_view { 0.055 } else { 0.65 });
                view.dirty = true;
                self.simulation_dirty = true;
                return true;
            }
            _ => return false,
        };
        self.send_game(command)
    }
    fn click_game_target(&mut self, x: f32, y: f32) -> bool {
        let Some(camera) = self.camera(self.active_camera) else {
            return false;
        };
        let alpha = self.timing.alpha(self.accumulator);
        let picked = self
            .world()
            .query(camera.target(), camera.render_radius())
            .into_iter()
            .filter_map(|index| {
                let item = &self.world().items()[index];
                if self.game().village().is_some_and(|v| v.exploring()) && item.grounded.is_none() {
                    return None;
                }
                let renderable = item.renderable.as_ref()?;
                let (anchor, rotation) = item.render_pose(alpha);
                let mut transform = item.transform;
                transform.anchor = anchor;
                transform.rotation = rotation;
                let bounds = transform.bounds(renderable.local_bounds);
                camera
                    .pick_depth(x, y, bounds)
                    .map(|depth| (depth, item.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id)| id);
        let Some(target) = picked else {
            return false;
        };
        if self
            .game()
            .village()
            .is_some_and(|v| v.exploring() && v.interactive(target))
        {
            return self.send_exploration(io_village::Command::Talk {
                target: Some(target),
            });
        }
        let Some(target_actor) = self.game().actor(target) else {
            return false;
        };
        if !io_encounter::Encounter::alive(self.world(), target) {
            return false;
        }
        if target_actor.control() == io_encounter::Control::Player
            && self.game().movement_window().is_some()
        {
            if let Some(actor) = self.controlled_actor() {
                self.send_game(io_encounter::GameCommand::Move {
                    actor,
                    window: self.game().movement_window().unwrap(),
                    x: 0.,
                    y: 0.,
                });
            }
            self.game_player = Some(target);
            return true;
        }
        let Some(actor) = self.controlled_actor() else {
            return false;
        };
        if !self.game().enemies(self.world(), actor).contains(&target) {
            return false;
        }
        self.game_target = Some(target);
        if !matches!(self.game().phase(), io_encounter::Phase::Turns { .. })
            || self.game().active_actor() != Some(actor)
        {
            return true;
        }
        let Some((owner, ability)) = self.game_ability else {
            return false;
        };
        if owner != actor
            || self
                .game()
                .check_attack(self.world(), actor, target, ability)
                .is_err()
        {
            return false;
        }
        self.send_game(io_encounter::GameCommand::Attack {
            actor,
            target,
            ability,
        })
    }
    fn send_game(&mut self, command: io_encounter::GameCommand) -> bool {
        match &mut self.world {
            Simulation::Inline(world) => self.game.command(world, command).is_ok(),
            Simulation::Threaded(worker) => worker.submit_game(command).is_ok(),
        }
    }
    fn send_exploration(&mut self, command: io_village::Command) -> bool {
        match &mut self.world {
            Simulation::Inline(world) => self.game.explore_command(world, command).is_ok(),
            Simulation::Threaded(worker) => worker.submit_exploration(command).is_ok(),
        }
    }
    pub fn game_lines(&self) -> Vec<String> {
        use io_encounter::{AbilityEffect, CombatEvent, Phase};
        let game = self.game();
        if let Some(village) = game.village().filter(|v| v.exploring()) {
            return village.lines(self.world());
        }
        if !game.enabled() {
            return vec![];
        }
        if let Phase::Ready { round } = game.phase() {
            return vec![
                "ROUND BEGIN [ENTER]".into(),
                format!("ROUND {round} - MOVEMENT PAUSED"),
                "ADJUST CAMERA THEN PRESS ENTER TO MOVE".into(),
                "ARROWS OR RIGHT DRAG ORBIT - SCROLL ZOOMS".into(),
            ];
        }
        let name = |id| {
            game.actor(id)
                .map_or("UNKNOWN", |a| a.name())
                .to_ascii_uppercase()
        };
        let health = |id| {
            self.world()
                .item(id)
                .and_then(|i| i.durability.as_ref())
                .map_or(0, |d| d.current())
        };
        let mut lines = vec![match game.phase() {
            Phase::Exploration => "EXPLORATION - ENTER STARTS COMBAT".into(),
            Phase::Ready { .. } => unreachable!("ready prompt returned above"),
            Phase::Movement { round, remaining } => {
                format!("ROUND {round} MOVE {remaining:.1} SECONDS")
            }
            Phase::Turns { round, .. } => format!(
                "ROUND {round} TURN {}",
                game.active_actor().map_or("NONE".into(), name)
            ),
            Phase::Resolving { round, .. } => {
                format!("ROUND {round} SPELL IN FLIGHT - MOVEMENT LOCKED")
            }
            Phase::Finished { outcome: winner } => format!("COMBAT FINISHED - FACTION {winner:?}"),
        }];
        let Some(actor) = self.controlled_actor() else {
            lines.push("NO LIVING PLAYER CHARACTERS".into());
            return lines;
        };
        lines.push(format!("PLAYER {} HP {}", name(actor), health(actor)));
        let target = self.target_actor(actor);
        lines.push(target.map_or("NO TARGET".into(), |id| {
            format!("TARGET {} HP {} - T TO CYCLE", name(id), health(id))
        }));
        lines.push(format!(
            "THREATS {} - REACTION {}",
            game.engagements()
                .iter()
                .filter(|(_, id)| *id == actor)
                .count(),
            if game.actor(actor).unwrap().reaction_available() {
                "READY"
            } else {
                "SPENT"
            }
        ));
        lines.push(match game.phase() {
            Phase::Exploration | Phase::Movement { .. } => {
                "WASD CAMERA MOVE - C SWITCH PLAYER".into()
            }
            Phase::Turns { .. } if game.active_actor() == Some(actor) => {
                "MOVEMENT LOCKED - SELECT THEN CLICK - SPACE PASS".into()
            }
            Phase::Turns { .. } => "MOVEMENT LOCKED - WAITING FOR NPC TURN".into(),
            Phase::Resolving { .. } => "WAIT FOR IMPACT".into(),
            Phase::Finished { .. } => "REOPEN SCENE TO PLAY AGAIN".into(),
            Phase::Ready { .. } => unreachable!("ready prompt returned above"),
        });
        for (slot, &ability) in game.actor(actor).unwrap().abilities().iter().enumerate() {
            let (label, definition) = game.ability(ability).unwrap();
            let AbilityEffect::Damage { amount } = definition.effect;
            let available = matches!(game.phase(), Phase::Turns { .. })
                && game.active_actor() == Some(actor)
                && target
                    .is_some_and(|id| game.check_attack(self.world(), actor, id, ability).is_ok());
            lines.push(format!(
                "{} {} HIT {} RANGE {:.1} {} {}",
                slot + 1,
                label.to_ascii_uppercase(),
                amount,
                definition.range,
                if available { "READY" } else { "LOCKED" },
                if self.game_ability == Some((actor, ability)) {
                    "SELECTED"
                } else {
                    ""
                }
            ));
        }
        if let Some(event) = game.last_event() {
            lines.push(match event {
                CombatEvent::RoundStarted(round) => format!("ROUND {round} STARTED"),
                CombatEvent::Passed(id) => format!("{} PASSED", name(*id)),
                CombatEvent::Hit {
                    source,
                    target,
                    damage,
                    opportunity,
                } => format!(
                    "{}{} HIT {} FOR {damage}",
                    if *opportunity { "REACTION " } else { "" },
                    name(*source),
                    name(*target)
                ),
            });
        }
        lines
    }
    pub fn timing(&self) -> SimulationTiming {
        self.timing
    }
    pub fn set_visual_state(&mut self, item_id: u64, name: &str) -> bool {
        let Some(item) = self.world.item(item_id) else {
            return false;
        };
        let Some(renderable) = &item.renderable else {
            return false;
        };
        let Some(appearance) = self.library.appearance(renderable.appearance_id) else {
            return false;
        };
        let Some(&state) = appearance.state_names.get(name) else {
            return false;
        };
        if renderable.visual_state == state {
            return false;
        }
        self.world.command(WorldCommand::SetVisualState {
            target: item_id,
            state,
        })
    }
    pub fn camera(&self, id: CameraId) -> Option<&Camera> {
        self.cameras.get(&id).map(|v| &v.camera)
    }
    pub fn active_camera(&self) -> CameraId {
        self.active_camera
    }
    pub fn active_count(&self) -> usize {
        self.active_simulation.len()
    }
    pub fn create_camera(&mut self) -> Option<CameraId> {
        let id = self.next_camera;
        self.next_camera = id.checked_add(1)?;
        let view = &self.cameras[&self.active_camera];
        let mut copy = View::new(view.camera.clone(), view.follow);
        copy.follow_offset = view.follow_offset;
        copy.home_follow = view.home_follow;
        self.cameras.insert(id, copy);
        self.simulation_dirty = true;
        Some(id)
    }
    pub fn select_camera(&mut self, id: CameraId) -> bool {
        if !self.cameras.contains_key(&id) {
            return false;
        }
        self.active_camera = id;
        true
    }
    pub fn set_target(&mut self, id: CameraId, target: Vec3) -> bool {
        if !target.finite() {
            return false;
        }
        let Some(view) = self.cameras.get_mut(&id) else {
            return false;
        };
        view.camera
            .set_target(self.world.space().clamp_target(target));
        view.follow = None;
        view.dirty = true;
        self.simulation_dirty = true;
        true
    }
    pub fn set_distance(&mut self, id: CameraId, distance: f32) -> bool {
        let Some(view) = self.cameras.get_mut(&id) else {
            return false;
        };
        let changed = view.camera.set_distance(distance);
        view.dirty |= changed;
        self.simulation_dirty |= changed;
        changed
    }
    pub fn dispatch(&mut self, action: Action) -> bool {
        match action {
            Action::ToggleGrid => {
                self.grid = !self.grid;
                for view in self.cameras.values_mut() {
                    view.dirty = true;
                }
                true
            }
            Action::ToggleFollow => self.update_active_view(|view, _| {
                view.follow = if view.follow.is_some() {
                    None
                } else {
                    view.home_follow
                };
                true
            }),
            Action::ResizeAxis { axis, delta } => {
                self.world.command(WorldCommand::ResizeAxis { axis, delta })
            }
            Action::NewCamera => {
                let Some(id) = self.create_camera() else {
                    return false;
                };
                self.active_camera = id;
                true
            }
            Action::NextCamera => {
                self.active_camera = *self
                    .cameras
                    .range((Excluded(self.active_camera), Unbounded))
                    .next()
                    .or_else(|| self.cameras.first_key_value())
                    .unwrap()
                    .0;
                true
            }
            Action::Orbit { yaw, pitch } => {
                self.update_active_view(|view, _| view.camera.orbit(yaw, pitch))
            }
            Action::Zoom { steps } => self.update_active_view(|view, _| view.camera.zoom_by(steps)),
            Action::Pan { dx, dy } => self.update_active_view(|view, space| {
                if !view.camera.pan(dx, dy, space) {
                    return false;
                }
                view.follow = None;
                true
            }),
            Action::Distance { steps } => self.update_active_view(|view, _| {
                if !steps.is_finite() {
                    return false;
                }
                view.camera.set_distance(
                    view.camera.render_distance() * (steps.clamp(-100., 100.) * 0.2).exp(),
                )
            }),
            Action::ResetView => self.update_active_view(|view, _| {
                let old = view.camera.clone();
                view.camera = view.home.clone();
                view.camera.copy_viewport(&old);
                view.follow = view.home_follow;
                true
            }),
        }
    }
    fn update_active_view(
        &mut self,
        update: impl FnOnce(&mut View, &io_world::Space) -> bool,
    ) -> bool {
        let view = self
            .cameras
            .get_mut(&self.active_camera)
            .expect("active camera is retained and selection validates IDs");
        let changed = update(view, self.world.space());
        view.dirty |= changed;
        self.simulation_dirty |= changed;
        changed
    }
    pub fn set_viewport(&mut self, id: CameraId, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        let Some(view) = self.cameras.get_mut(&id) else {
            return false;
        };
        view.dirty |= view.camera.set_viewport(w, h);
        true
    }
    fn refresh_simulation(&mut self) {
        let Simulation::Inline(world) = &self.world else {
            return;
        };
        if self.simulation_dirty || self.spatial_revision != self.world.spatial_revision() {
            let mut active = HashSet::new();
            active.extend(world.physics_indices().iter().copied());
            for view in self.cameras.values() {
                if let Some(index) = view.follow {
                    active.insert(index);
                }
                for id in self
                    .world
                    .query(view.camera.target(), view.camera.render_distance())
                {
                    let item = &self.world.items()[id];
                    if item.needs_simulation()
                        && item
                            .visibility_bounds()
                            .within_radius(view.camera.target(), view.camera.render_distance())
                    {
                        active.insert(id);
                    }
                }
            }
            let mut active: Vec<_> = active.into_iter().collect();
            active.sort_unstable();
            self.active_simulation = active.into();
            self.simulation_dirty = false;
            self.spatial_revision = self.world.spatial_revision();
        }
    }
    pub fn update(&mut self, seconds: f32) {
        if !seconds.is_finite() || seconds < 0. || (seconds == 0. && self.worker().is_none()) {
            return;
        }
        if self.simulation_dirty && self.worker().is_some() {
            let regions = self.regions();
            if let Simulation::Threaded(worker) = &self.world {
                worker.set_regions(regions);
            }
            self.simulation_dirty = false;
        }
        match &mut self.world {
            Simulation::Threaded(worker) => {
                worker.poll_snapshot();
                self.active_simulation = worker.current.active.clone();
                self.accumulator =
                    f64::from(worker.alpha(std::time::Instant::now())) * self.timing.seconds();
                while let Some(event) = worker.poll_event() {
                    match event.payload {
                        Event::CommandCompleted {
                            outcome: CommandOutcome::Rejected(error),
                            ..
                        } => eprintln!("Command {:?} rejected: {:?}", event.id, error),
                        Event::CommandCompleted { .. } => {}
                        Event::GameCompleted {
                            outcome: Err(error),
                            ..
                        } => eprintln!("Game command {:?} rejected: {:?}", event.id, error),
                        Event::GameCompleted {
                            outcome: Ok(()), ..
                        } => {}
                    }
                }
            }
            Simulation::Inline(_) => {
                // Deterministic stepping for tests, captures and throughput benchmarks.
                self.accumulator += (seconds as f64).min(0.25);
                let dt = self.timing.seconds();
                // Roundoff at an exact tick boundary must not defer a whole tick.
                let ticks = ((self.accumulator / dt + 1e-9).floor() as u32)
                    .min(self.timing.max_catch_up_ticks());
                for _ in 0..ticks {
                    self.refresh_simulation();
                    if let Simulation::Inline(world) = &mut self.world {
                        self.game
                            .step(world, &self.active_simulation, dt as f32)
                            .expect("validated simulation timestep");
                    }
                }
                self.accumulator = (self.accumulator - f64::from(ticks) * dt).max(0.);
            }
        }
        let alpha = self.timing.alpha(self.accumulator);
        let interpolating = self.active_simulation.iter().any(|&id| {
            let item = &self.world.items()[id];
            item.motion.is_some() || item.animation.is_some() || item.interpolates_pose()
        });
        for view in self.cameras.values_mut() {
            if let Some(id) = view.follow {
                view.camera
                    .set_target(self.world.items()[id].render_pose(alpha).0 + view.follow_offset);
                view.dirty = true;
                self.simulation_dirty = true;
            }
            view.dirty |= interpolating && alpha != self.last_alpha;
        }
        self.last_alpha = alpha;
    }
    pub fn frame(&mut self, id: CameraId) -> Option<&Frame> {
        let view = self.cameras.get_mut(&id)?;
        if view.dirty || view.world_revision != Some(self.world.revision()) {
            self.serial = self.serial.wrapping_add(1);
            if let Err(error) = view.frame.build(
                &*self.world,
                &view.camera,
                self.serial,
                self.library,
                self.timing.alpha(self.accumulator),
                &self.active_simulation,
                self.grid,
            ) {
                eprintln!("Frame rejected: {error}");
                return None;
            }
            view.world_revision = Some(self.world.revision());
            view.dirty = false;
        }
        Some(&view.frame)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_prompt_locks_movement_but_not_camera_controls() {
        let library = Box::leak(Box::new(
            ModelLibrary::load(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets/villages/world.json"),
            )
            .unwrap(),
        ));
        let mut world = crate::demo::world(library).unwrap();
        let id = |name: &str| {
            library
                .config
                .items
                .iter()
                .position(|i| i.name == name)
                .unwrap() as u64
                + 1
        };
        let hero = id("hero");
        let p = world.item(id("v0-raider0")).unwrap().transform.anchor - Vec3::new(10., 0., 0.);
        world.set_pose(hero, p, 0.);
        let game = crate::demo::game(library, &world).unwrap();
        let mut app = App::with_world(world, Camera::new(p), None, library);
        app.game = game;
        app.update(0.05);
        assert_eq!(app.game_lines()[0], "ROUND BEGIN [ENTER]");
        assert!(app.game().movement_window().is_none());
        assert!(!app.game_action(2, 0, 0., 1.));
        assert!(app.game_action(8, 0, 0.7, 0.1));
        assert!(app.dispatch(Action::Zoom { steps: 1. }));
        app.update(0.25);
        assert_eq!(app.world().item(hero).unwrap().transform.anchor, p);
        assert!(app.game_action(1, 0, 0., 0.));
        assert_eq!(
            app.game().movement_window(),
            Some(io_game::MovementWindow::Round(1))
        );
        assert!(app.game_action(2, 0, 0., 1.));
        app.update(0.1);
        assert_ne!(app.world().item(hero).unwrap().transform.anchor, p);
    }
    #[test]
    fn configured_rate_advances_same_time_and_survives_worker_handoff() {
        for hz in [30, 60, 144, 1000] {
            let mut app = App::new().unwrap();
            app.timing = SimulationTiming::new(hz).unwrap();
            let id = app
                .world
                .items()
                .iter()
                .find(|i| i.motion.is_some())
                .unwrap()
                .id;
            let before = app.world.item(id).unwrap().simulated_ticks;
            // Equal exact elapsed time with different caller cadences, including
            // enough ticks to catch a hardcoded eight-tick limit at high rates.
            app.update(0.25);
            let expected = hz / 4;
            assert_eq!(
                app.world.item(id).unwrap().simulated_ticks - before,
                u64::from(expected)
            );
            let mut split = App::new().unwrap();
            split.timing = app.timing;
            split.update(0.125);
            split.update(0.125);
            assert_eq!(app.world.items(), split.world.items());
            assert!(app.timing.alpha(app.accumulator) <= 1.);
            let threaded = app.into_threaded().unwrap();
            assert_eq!(threaded.worker().unwrap().timing().tick_hz(), hz);
        }
    }

    #[test]
    fn sleeping_physics_reuses_the_render_packet_between_ticks() {
        use io_world::{BodyKind, Collider, ColliderShape, PhysicsBody};
        let library = ModelLibrary::global().unwrap();
        let original = crate::demo::world(library).unwrap();
        let mut item = original
            .items()
            .iter()
            .find(|i| i.animation.is_some())
            .unwrap()
            .clone();
        item.motion = None;
        item.animation = None;
        item.physics_body = Some(PhysicsBody::new(BodyKind::Dynamic));
        item.physics_body.as_mut().unwrap().gravity_scale = 0.;
        item.collider = Some(Collider::new(ColliderShape::Sphere { radius: 0.5 }));
        let anchor = item.transform.anchor;
        let mut app = App::with_world(
            World::new(original.space().clone(), vec![item]),
            Camera::new(anchor),
            None,
            library,
        );
        for _ in 0..90 {
            app.update(1. / 30.);
        }
        assert_eq!(app.world.physics_stats().sleeping, 1);
        let frame = app.frame(1).unwrap().serial;
        for _ in 0..10 {
            app.update(1. / 120.);
            assert_eq!(app.frame(1).unwrap().serial, frame);
        }
    }
    #[test]
    fn physical_items_keep_simulating_offscreen_and_interpolate_rotation() {
        use io_world::{BodyKind, Collider, ColliderShape, PhysicsBody};
        let library = ModelLibrary::global().unwrap();
        let original = crate::demo::world(library).unwrap();
        let mut item = original
            .items()
            .iter()
            .find(|i| i.animation.is_some())
            .unwrap()
            .clone();
        item.motion = None;
        item.animation = None;
        item.physics_body = Some(PhysicsBody::new(BodyKind::Dynamic));
        let body = item.physics_body.as_mut().unwrap();
        body.velocity.x = 2.;
        body.angular_velocity.x = 1.;
        body.gravity_scale = 0.;
        item.collider = Some(Collider::new(ColliderShape::Sphere { radius: 0.5 }));
        let id = item.id;
        let anchor = item.transform.anchor;
        let mut app = App::with_world(
            World::new(original.space().clone(), vec![item]),
            Camera::new(anchor),
            None,
            library,
        );
        let initial = app.frame(1).unwrap().instances.clone();
        assert_eq!(initial.len(), 1);
        app.update(1. / 30.);
        let authoritative = app.world.item(id).unwrap().transform;
        app.update(1. / 120.);
        assert_eq!(app.world.item(id).unwrap().transform, authoritative);
        assert_ne!(app.frame(1).unwrap().instances, initial);
        app.set_target(1, Vec3::new(9000., 9000., 0.));
        app.update(0.2);
        assert!(app.frame(1).unwrap().instances.is_empty());
        assert!(app.world.item(id).unwrap().transform.anchor.x > authoritative.anchor.x);
        assert_ne!(
            app.world.item(id).unwrap().transform.rotation,
            authoritative.rotation
        );
        assert!(app.world.physics_error().is_none());
        app.set_target(1, app.world.item(id).unwrap().transform.anchor);
        assert_eq!(app.frame(1).unwrap().instances.len(), 1);
    }
    #[test]
    fn rejected_pan_keeps_following_and_cached_frame() {
        let mut app = App::new().unwrap();
        let serial = app.frame(1).unwrap().serial;
        let follow = app.cameras[&1].follow;
        assert!(follow.is_some());
        let target = app.camera(1).unwrap().target();
        assert!(!app.dispatch(Action::Pan {
            dx: f32::NAN,
            dy: 0.
        }));
        assert_eq!(app.cameras[&1].follow, follow);
        assert_eq!(app.camera(1).unwrap().target(), target);
        assert_eq!(app.frame(1).unwrap().serial, serial);
    }
    #[test]
    fn street_motion_interpolates_and_stops_when_all_cameras_leave() {
        let mut app = App::new().unwrap();
        let index = app
            .world
            .items()
            .iter()
            .position(|i| i.motion.is_some())
            .unwrap();
        let id = app.world.items()[index].id;
        let before = app.world.items()[index].transform.anchor;
        app.update(1. / 30.);
        let pose = app.frame(1).unwrap().joint_matrices.clone();
        let authoritative = app.world.items()[index].transform.anchor;
        assert_ne!(before, authoritative);
        app.update(1. / 120.);
        assert_eq!(app.world.items()[index].transform.anchor, authoritative);
        assert_ne!(app.frame(1).unwrap().joint_matrices, pose);
        app.set_target(1, Vec3::new(9000., 9000., 0.));
        let retained = app.world.item(id).unwrap().clone();
        app.update(0.2);
        app.update(0.2);
        assert_eq!(app.world.item(id).unwrap(), &retained);
        assert!(app.frame(1).unwrap().joint_matrices.is_empty());
        app.set_target(1, retained.transform.anchor);
        app.update(0.1);
        assert!(app.world.item(id).unwrap().simulated_ticks > retained.simulated_ticks);
    }
    #[test]
    fn two_characters_share_geometry_but_have_independent_poses() {
        let library = ModelLibrary::global().unwrap();
        let original = crate::demo::world(library).unwrap();
        let mut items = original.items().to_vec();
        let mut second = items
            .iter()
            .find(|i| i.animation.is_some())
            .unwrap()
            .clone();
        second.id = 900;
        second.transform.anchor.x -= 2.;
        second.motion = None;
        let animation = second.animation.as_mut().unwrap();
        animation.seek(0.25).unwrap();
        items.push(second);
        let world = World::new(io_world::Space::new(Vec3::new(10000., 10000., 256.)), items);
        let mut app = App::with_world(
            world,
            Camera::new(Vec3::new(5000., 5000., 0.)),
            None,
            library,
        );
        let frame = app.frame(1).unwrap();
        assert_eq!(frame.joint_matrices.len(), 28);
        assert_ne!(&frame.joint_matrices[..14], &frame.joint_matrices[14..]);
        let offsets: Vec<_> = frame
            .instances
            .iter()
            .filter(|i| library.mesh(i.model_id).unwrap().joint_count() > 0)
            .map(|i| i.joint_offset)
            .collect();
        assert_eq!(offsets, vec![0, 14]);
    }
    fn benchmark_app() -> App {
        App::with_world(
            crate::benchmark::world(),
            Camera::new(Vec3::new(5000., 5000., 0.)),
            None,
            ModelLibrary::global().unwrap(),
        )
    }
    #[test]
    fn large_world_queries_small_regions_and_zoom_changes_visibility() {
        let mut a = benchmark_app();
        let count = a.world.items().len();
        let first = a.frame(1).unwrap();
        let visible = first.instances.len();
        assert!(visible > 0 && first.candidate_count < count / 10);
        a.dispatch(Action::Zoom { steps: -12. });
        assert!(a.frame(1).unwrap().instances.len() > visible);
        assert!(a.frame(1).unwrap().grid.len() <= 1000);
    }
    #[test]
    fn world_subdivisions_do_not_change_instances() {
        let mut a = benchmark_app();
        let original = a.frame(1).unwrap().instances.clone();
        a.dispatch(Action::ResizeAxis {
            axis: Axis::A,
            delta: 10,
        });
        assert_eq!(a.frame(1).unwrap().instances, original);
    }
    #[test]
    fn cameras_are_independent_and_unviewed_state_is_retained() {
        let mut a = benchmark_app();
        let original = a.frame(1).unwrap().instances.clone();
        let copy = a.create_camera().unwrap();
        a.set_target(copy, Vec3::new(100., 100., 0.));
        assert_ne!(a.frame(copy).unwrap().instances, original);
        assert_eq!(a.frame(1).unwrap().instances, original);
        let before = a.world.items()[0].clone();
        a.set_target(copy, Vec3::new(9000., 9000., 0.));
        a.update(0.1);
        assert_eq!(a.world.items()[0], before);
        a.set_target(copy, Vec3::new(5., 5., 0.));
        a.update(0.1);
        assert!(a.world.items()[0].simulated_ticks > before.simulated_ticks);
        let retained = a.world.items()[0].clone();
        a.set_target(copy, Vec3::new(9000., 9000., 0.));
        a.update(0.1);
        assert_eq!(a.world.items()[0], retained);
        assert_eq!(a.world.items().len(), 40400);
    }
    #[test]
    fn overlapping_cameras_simulate_once_and_tick_rate_is_fixed() {
        let mut a = benchmark_app();
        let mut b = benchmark_app();
        a.create_camera();
        a.update(0.2);
        for _ in 0..6 {
            b.update(1. / 30.);
        }
        assert_eq!(a.world.items(), b.world.items());
        assert!(a.active_count() > 0);
        assert!(!a.select_camera(0));
        assert!(!a.set_distance(1, f32::NAN));
        assert!(!a.set_target(1, Vec3::new(f32::INFINITY, 0., 0.)));
        assert!(a.frame(999).is_none());
    }
    #[test]
    fn distance_controls_candidates_without_deleting_items() {
        let mut a = benchmark_app();
        a.dispatch(Action::Zoom { steps: -30. });
        a.set_distance(1, 200.);
        let wide = a.frame(1).unwrap().instances.len();
        a.set_distance(1, 16.);
        assert!(a.frame(1).unwrap().instances.len() < wide);
        assert_eq!(a.world.items().len(), 40400);
    }
}
