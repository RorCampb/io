#![forbid(unsafe_code)]
//! One world owner; bounded commands and immutable, latest-only publications.
use crate::timing::SimulationTiming;
use io_types::{Envelope, MessageId, Vec3};
use io_world::{CommandOutcome, World, WorldCommand, WorldSnapshot, WorldView};
use std::collections::HashSet;
use std::ops::Deref;
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::{self, JoinHandle};
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

pub const COMMAND_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub center: Vec3,
    pub radius: f32,
    pub follow: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    CommandCompleted {
        before_tick: u64,
        outcome: CommandOutcome,
    },
    GameCompleted {
        before_tick: u64,
        outcome: Result<(), io_encounter::GameError>,
    },
}

#[derive(Clone, Copy, Debug)]
enum Command {
    World(WorldCommand),
    Game(io_village::Command),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitError {
    Full,
    Stopped,
    IdExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Stopped,
    PhysicsFailed,
    Panicked,
}

pub struct Publication {
    pub game: crate::game::Game,
    pub world: WorldSnapshot,
    pub active: Arc<[usize]>,
    pub tick: u64,
    pub completed_at: Instant,
    pub simulation_ms: f64,
    pub snapshot_ms: f64,
    pub overruns: u64,
}

pub struct Worker {
    timing: SimulationTiming,
    commands: Option<mpsc::SyncSender<Envelope<Command>>>,
    events: mpsc::Receiver<Envelope<Event>>,
    latest: Arc<Mutex<Arc<Publication>>>,
    regions: Arc<Mutex<Arc<[Region]>>>,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
    thread: Option<JoinHandle<()>>,
    next_id: u64,
    outstanding: usize,
    pub current: Arc<Publication>,
}

pub fn select_active(world: &World, regions: &[Region]) -> Vec<usize> {
    let mut active: HashSet<_> = world.physics_indices().iter().copied().collect();
    for region in regions {
        if let Some(id) = region.follow.filter(|&id| id < world.items().len()) {
            active.insert(id);
        }
        for id in world.query(region.center, region.radius) {
            let item = &world.items()[id];
            if item.needs_simulation()
                && item
                    .visibility_bounds()
                    .within_radius(region.center, region.radius)
            {
                active.insert(id);
            }
        }
    }
    let mut active: Vec<_> = active.into_iter().collect();
    active.sort_unstable();
    active
}

impl Worker {
    pub fn spawn(world: World, regions: Vec<Region>) -> Result<Self, String> {
        Self::spawn_with_timing(world, regions, SimulationTiming::default())
    }

    pub fn spawn_with_timing(
        world: World,
        regions: Vec<Region>,
        timing: SimulationTiming,
    ) -> Result<Self, String> {
        Self::spawn_with_game(world, regions, timing, crate::game::Game::default())
    }

    pub fn spawn_with_game(
        world: World,
        regions: Vec<Region>,
        timing: SimulationTiming,
        game: crate::game::Game,
    ) -> Result<Self, String> {
        let current = Arc::new(Publication {
            game: game.clone(),
            active: select_active(&world, &regions).into(),
            world: world.snapshot(),
            tick: 0,
            completed_at: Instant::now(),
            simulation_ms: 0.,
            snapshot_ms: 0.,
            overruns: 0,
        });
        let latest = Arc::new(Mutex::new(current.clone()));
        let regions: Arc<Mutex<Arc<[Region]>>> = Arc::new(Mutex::new(regions.into()));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(AtomicU8::new(0));
        let (tx, rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (event_tx, event_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let task = Task {
            game,
            timing,
            world,
            commands: rx,
            events: event_tx,
            latest: latest.clone(),
            regions: regions.clone(),
            stop: stop.clone(),
            status: status.clone(),
        };
        let failure_status = status.clone();
        let thread = thread::Builder::new()
            .name("io-simulation".into())
            .spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run())).is_err() {
                    failure_status.store(3, Ordering::Release);
                }
            })
            .map_err(|e| format!("cannot start simulation worker: {e}"))?;
        Ok(Self {
            timing,
            commands: Some(tx),
            events: event_rx,
            latest,
            regions,
            stop,
            status,
            thread: Some(thread),
            next_id: 1,
            outstanding: 0,
            current,
        })
    }

    pub fn status(&self) -> Status {
        match self.status.load(Ordering::Acquire) {
            0 => Status::Running,
            1 => Status::Stopped,
            2 => Status::PhysicsFailed,
            _ => Status::Panicked,
        }
    }

    pub fn timing(&self) -> SimulationTiming {
        self.timing
    }

    pub fn submit(&mut self, payload: WorldCommand) -> Result<MessageId, SubmitError> {
        self.send(Command::World(payload))
    }
    pub fn submit_game(
        &mut self,
        payload: io_encounter::GameCommand,
    ) -> Result<MessageId, SubmitError> {
        self.submit_exploration(io_village::Command::Combat(payload))
    }
    pub fn submit_exploration(
        &mut self,
        payload: io_village::Command,
    ) -> Result<MessageId, SubmitError> {
        self.send(Command::Game(payload))
    }
    fn send(&mut self, payload: Command) -> Result<MessageId, SubmitError> {
        if self.status() != Status::Running {
            return Err(SubmitError::Stopped);
        }
        if self.outstanding == COMMAND_CAPACITY {
            return Err(SubmitError::Full);
        }
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(SubmitError::IdExhausted)?;
        let id = MessageId(self.next_id);
        let sender = self.commands.as_ref().ok_or(SubmitError::Stopped)?;
        sender
            .try_send(Envelope::new(id, payload))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => SubmitError::Full,
                mpsc::TrySendError::Disconnected(_) => SubmitError::Stopped,
            })?;
        self.next_id = next;
        self.outstanding += 1;
        Ok(id)
    }

    pub fn poll_event(&mut self) -> Option<Envelope<Event>> {
        let event = self.events.try_recv().ok()?;
        self.outstanding -= 1;
        Some(event)
    }

    pub fn set_regions(&self, regions: Vec<Region>) {
        let replacement: Arc<[Region]> = regions.into();
        let old = { std::mem::replace(&mut *self.regions.lock().unwrap(), replacement) };
        drop(old);
    }

    /// Only the publication pointer is locked, never the world or the physics solve.
    pub fn poll_snapshot(&mut self) -> bool {
        let newest = match self.latest.try_lock() {
            Ok(slot) => slot.clone(),
            Err(_) => return false,
        };
        if Arc::ptr_eq(&newest, &self.current) {
            return false;
        }
        self.current = newest;
        true
    }

    pub fn alpha(&self, now: Instant) -> f32 {
        if self.current.tick == 0 {
            return 1.;
        }
        self.timing.alpha(
            now.saturating_duration_since(self.current.completed_at)
                .as_secs_f64(),
        )
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Task {
    game: crate::game::Game,
    timing: SimulationTiming,
    world: World,
    commands: mpsc::Receiver<Envelope<Command>>,
    events: mpsc::SyncSender<Envelope<Event>>,
    latest: Arc<Mutex<Arc<Publication>>>,
    regions: Arc<Mutex<Arc<[Region]>>>,
    stop: Arc<AtomicBool>,
    status: Arc<AtomicU8>,
}

impl Task {
    fn run(mut self) {
        let dt = self.timing.period();
        let mut deadline = Instant::now() + dt;
        let mut tick = 0_u64;
        let mut overruns = 0;
        let mut pending = Vec::with_capacity(COMMAND_CAPACITY);
        loop {
            while Instant::now() < deadline && !self.stop.load(Ordering::Acquire) {
                match self
                    .commands
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(command) => pending.push(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        self.status.store(1, Ordering::Release);
                        return;
                    }
                }
            }
            if self.stop.load(Ordering::Acquire) {
                self.status.store(1, Ordering::Release);
                return;
            }
            pending.extend(
                self.commands
                    .try_iter()
                    .take(COMMAND_CAPACITY - pending.len()),
            );
            tick = tick.checked_add(1).expect("simulation tick exhausted");
            for command in pending.drain(..) {
                let event = match command.payload {
                    Command::World(payload) => Event::CommandCompleted {
                        before_tick: tick,
                        outcome: self.world.apply_command(payload),
                    },
                    Command::Game(payload) => Event::GameCompleted {
                        before_tick: tick,
                        outcome: self.game.explore_command(&mut self.world, payload),
                    },
                };
                // Credits cover queued, executing AND unread replies, so this cannot fill.
                if self.events.try_send(command.reply(event)).is_err() {
                    self.status.store(1, Ordering::Release);
                    return;
                }
            }
            let started = Instant::now();
            let regions = self.regions.lock().unwrap().clone();
            let active = select_active(&self.world, &regions);
            self.game
                .step(&mut self.world, &active, self.timing.seconds() as f32)
                .expect("validated simulation timestep");
            let simulation_ms = started.elapsed().as_secs_f64() * 1000.;
            let copying = Instant::now();
            let snapshot = self.world.snapshot();
            let game = self.game.clone();
            let snapshot_ms = copying.elapsed().as_secs_f64() * 1000.;
            let completed_at = Instant::now();
            deadline += dt;
            // Slow physics slows simulation time. Never pile up catch-up work on rendering.
            if completed_at > deadline {
                overruns += 1;
                deadline = completed_at;
            }
            let failed = self.world.physics_error().is_some();
            let publication = Arc::new(Publication {
                game,
                world: snapshot,
                active: active.into(),
                tick,
                completed_at,
                simulation_ms,
                snapshot_ms,
                overruns,
            });
            let old = { std::mem::replace(&mut *self.latest.lock().unwrap(), publication) };
            drop(old);
            if failed {
                self.status.store(2, Ordering::Release);
                return;
            }
        }
    }
}

pub enum Simulation {
    Inline(Box<World>),
    Threaded(Worker),
}

impl Deref for Simulation {
    type Target = dyn WorldView;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Inline(world) => world.as_ref(),
            Self::Threaded(worker) => &worker.current.world,
        }
    }
}

impl Simulation {
    /// Inline: applied/unchanged. Threaded: accepted for later execution, not an acknowledgement.
    pub fn command(&mut self, command: WorldCommand) -> bool {
        match self {
            Self::Inline(world) => {
                !matches!(world.apply_command(command), CommandOutcome::Rejected(_))
            }
            Self::Threaded(worker) => worker.submit(command).is_ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_world::{BodyKind, Collider, ColliderShape, Durability, Item, PhysicsBody, Space};

    fn world() -> World {
        let mut physics = PhysicsBody::new(BodyKind::Dynamic);
        physics.gravity_scale = 0.;
        physics.velocity.x = 1.;
        World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![Item {
                id: 1,
                durability: Some(Durability::new(1000, 1000).unwrap()),
                physics_body: Some(physics),
                collider: Some(Collider::new(ColliderShape::Sphere { radius: 0.5 })),
                ..Item::default()
            }],
        )
    }
    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "worker did not progress");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn bounded_commands_include_unread_replies_and_apply_once_in_order() {
        let mut worker = Worker::spawn(world(), Vec::new()).unwrap();
        let frozen = worker.current.clone();
        for id in 1..=COMMAND_CAPACITY {
            assert_eq!(
                worker.submit(WorldCommand::Damage {
                    target: 1,
                    amount: 1
                }),
                Ok(MessageId(id as u64))
            );
        }
        assert_eq!(
            worker.submit(WorldCommand::Damage {
                target: 1,
                amount: 1
            }),
            Err(SubmitError::Full)
        );
        wait_until(|| {
            worker.poll_snapshot();
            worker.current.tick >= 2
        });
        assert_eq!(
            worker
                .current
                .world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            936
        );
        assert_eq!(
            worker.submit(WorldCommand::Damage {
                target: 1,
                amount: 1
            }),
            Err(SubmitError::Full)
        );
        for id in 1..=COMMAND_CAPACITY {
            let event = worker.poll_event().unwrap();
            assert_eq!(event.id, MessageId(id as u64));
            assert!(matches!(
                event.payload,
                Event::CommandCompleted {
                    outcome: CommandOutcome::Applied,
                    ..
                }
            ));
        }
        assert!(worker.poll_event().is_none());
        assert!(worker
            .submit(WorldCommand::Damage {
                target: 999,
                amount: 1
            })
            .is_ok());
        let mut rejection = None;
        wait_until(|| {
            rejection = worker.poll_event();
            rejection.is_some()
        });
        assert!(matches!(
            rejection.unwrap().payload,
            Event::CommandCompleted {
                outcome: CommandOutcome::Rejected(io_world::CommandError::UnknownItem),
                ..
            }
        ));
        assert_eq!(
            frozen
                .world
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            1000
        );
        assert_eq!(frozen.world.item(1).unwrap().transform.anchor.x, 0.);
    }

    #[test]
    fn frozen_consumer_does_not_block_publication_and_physics_matches_inline() {
        let mut reference = world();
        let timing = SimulationTiming::new(144).unwrap();
        let mut worker = Worker::spawn_with_timing(world(), Vec::new(), timing).unwrap();
        wait_until(|| worker.latest.lock().unwrap().tick >= 4);
        assert_eq!(worker.current.tick, 0);
        assert!(worker.poll_snapshot());
        let snapshot = worker.current.clone();
        for _ in 0..snapshot.tick {
            reference.simulate(&[0], worker.timing().seconds() as f32);
        }
        assert_eq!(reference.items(), snapshot.world.items());
        assert_eq!(worker.alpha(snapshot.completed_at), 0.);
        assert!((worker.alpha(snapshot.completed_at + timing.period() / 2) - 0.5).abs() < 0.0001);
        assert_eq!(
            worker.alpha(snapshot.completed_at + Duration::from_secs(10)),
            1.
        );
        assert_eq!(
            worker.alpha(snapshot.completed_at - Duration::from_millis(1)),
            0.
        );
        assert!(std::mem::size_of::<Envelope<Event>>() <= 32);
    }

    #[test]
    fn blocked_tick_does_not_lock_snapshot_reader_and_shutdown_joins() {
        let mut worker = Worker::spawn(world(), Vec::new()).unwrap();
        let regions = worker.regions.clone();
        let gate = regions.lock().unwrap();
        worker
            .submit(WorldCommand::Damage {
                target: 1,
                amount: 1,
            })
            .unwrap();
        // The reply precedes region selection, which is deliberately blocked here.
        wait_until(|| worker.poll_event().is_some());
        for _ in 0..1000 {
            worker.poll_snapshot();
            assert_eq!(worker.current.tick, 0);
            assert_eq!(worker.current.world.items().len(), 1);
        }
        let stopped = worker.status.clone();
        drop(gate);
        drop(worker);
        assert_eq!(stopped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn physics_failure_is_observable_and_rejects_new_commands() {
        let mut broken = world();
        assert!(broken
            .set_physics_settings(io_world::PhysicsSettings {
                gravity: Vec3::new(1000., 0., 0.),
                ..Default::default()
            })
            .is_ok());
        assert!(broken.set_gravity_scale(1, 10.));
        assert!(broken.apply_impulse(1, Vec3::new(9998., 0., 0.), Vec3::default()));
        let mut worker = Worker::spawn(broken, Vec::new()).unwrap();
        wait_until(|| worker.status() != Status::Running);
        worker.poll_snapshot();
        assert_eq!(worker.status(), Status::PhysicsFailed);
        assert!(worker.current.world.physics_error().is_some());
        assert_eq!(
            worker.submit(WorldCommand::Damage {
                target: 1,
                amount: 1
            }),
            Err(SubmitError::Stopped)
        );
    }

    #[test]
    fn game_commands_and_world_snapshots_share_the_worker_tick() {
        let library =
            crate::model::ModelLibrary::load(std::path::Path::new("assets/game/encounter.json"))
                .unwrap();
        let world = crate::demo::world(&library).unwrap();
        let game = crate::demo::game(&library, &world).unwrap();
        let mut worker =
            Worker::spawn_with_game(world, vec![], SimulationTiming::new(60).unwrap(), game)
                .unwrap();
        let frozen = worker.current.clone();
        let command = worker
            .submit_game(io_encounter::GameCommand::StartCombat)
            .unwrap();
        wait_until(|| {
            worker.poll_snapshot();
            matches!(
                worker.current.game.phase(),
                io_encounter::Phase::Movement { .. }
            )
        });
        let event = worker.poll_event().unwrap();
        assert_eq!(event.id, command);
        assert!(matches!(
            event.payload,
            Event::GameCompleted {
                outcome: Ok(()),
                ..
            }
        ));
        assert_eq!(frozen.game.phase(), &io_encounter::Phase::Exploration);
        let old = worker.current.world.item(2).unwrap().transform.anchor;
        worker
            .submit_game(io_encounter::GameCommand::Move {
                actor: 2,
                window: io_encounter::MovementWindow::Round(1),
                x: -1.,
                y: 0.,
            })
            .unwrap();
        wait_until(|| {
            worker.poll_snapshot();
            worker.current.world.item(2).unwrap().transform.anchor.x < old.x
        });
        assert!(matches!(
            worker.current.game.phase(),
            io_encounter::Phase::Movement { .. }
        ));
        assert_eq!(frozen.world.item(2).unwrap().transform.anchor.x, -3.);
    }
}
