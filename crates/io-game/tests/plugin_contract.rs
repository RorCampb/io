//! A second plugin using ONLY public framework APIs, with no combat or model dependency.
use io_game::{FrameworkError, GamePlugin, PluginInfo, PluginWorld, Session, Tick, EVENT_CAPACITY};
use io_types::Vec3;
use io_world::{
    BodyKind, Collider, ColliderShape, CommandError, CommandOutcome, Item, PhysicsBody, Space,
    World, WorldCommand, WorldView,
};

#[derive(Clone, Debug)]
struct Workshop {
    ticks: u32,
    info: PluginInfo,
    pre_impulse: bool,
}
impl Default for Workshop {
    fn default() -> Self {
        Self {
            ticks: 0,
            info: PluginInfo {
                id: "example-workshop",
                version: 1,
            },
            pre_impulse: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Input {
    Kick(u64),
    Teleport(u64),
}
#[derive(Clone, Debug, PartialEq)]
enum Event {
    Kicked(u64),
    Ticked(u32),
}
#[derive(Debug, PartialEq)]
enum Error {
    Framework(FrameworkError),
    Command(CommandError),
}
impl From<FrameworkError> for Error {
    fn from(e: FrameworkError) -> Self {
        Self::Framework(e)
    }
}
impl GamePlugin for Workshop {
    type Command = Input;
    type Event = Event;
    type Error = Error;
    fn info(&self) -> PluginInfo {
        self.info
    }
    fn validate(&self, _: &dyn WorldView) -> Result<(), Error> {
        Ok(())
    }
    fn active_items(&self) -> Vec<u64> {
        vec![1]
    }
    fn command(&mut self, world: &mut PluginWorld<'_, Event>, command: Input) -> Result<(), Error> {
        match command {
            Input::Kick(target) => {
                match world.apply(WorldCommand::ApplyImpulse {
                    target,
                    impulse: Vec3::new(1., 0., 0.),
                    point: Vec3::default(),
                }) {
                    CommandOutcome::Applied | CommandOutcome::Unchanged => {
                        world.emit(Event::Kicked(target))
                    }
                    CommandOutcome::Rejected(error) => return Err(Error::Command(error)),
                }
            }
            Input::Teleport(target) => {
                world
                    .place(target, Vec3::default(), Default::default())
                    .map_err(Error::Command)?;
            }
        }
        Ok(())
    }
    fn before_step(&mut self, world: &mut PluginWorld<'_, Event>, _: Tick) -> Result<(), Error> {
        if self.pre_impulse {
            self.command(world, Input::Kick(1))?;
        }
        Ok(())
    }
    fn update(&mut self, world: &mut PluginWorld<'_, Event>, tick: Tick) -> Result<(), Error> {
        assert!(tick.seconds() > 0.);
        if self.pre_impulse {
            assert!(
                world.item(1).unwrap().transform.anchor.x > 0.,
                "update observes integrated motion"
            );
        }
        self.ticks += 1;
        world.emit(Event::Ticked(self.ticks));
        Ok(())
    }
}

fn world() -> World {
    let mut body = PhysicsBody::new(BodyKind::Dynamic);
    body.gravity_scale = 0.;
    World::new(
        Space::new(Vec3::new(100., 100., 100.)),
        vec![Item {
            id: 1,
            physics_body: Some(body),
            collider: Some(Collider::new(ColliderShape::Sphere { radius: 0.5 })),
            ..Item::default()
        }],
    )
}

#[test]
fn independent_continuous_plugin_uses_components_without_models_or_rounds() {
    let mut world = world();
    let mut session = Session::register(Workshop::default(), &world).unwrap();
    session.command(&mut world, Input::Kick(1)).unwrap();
    let frozen = session.clone();
    let mut direct = World::new(world.space().clone(), world.items().to_vec());
    direct.simulate(&[0], 0.1);
    session.step(&mut world, &[], 0.1).unwrap();
    assert_eq!(
        world.items(),
        direct.items(),
        "host steps the world exactly once, even off-screen"
    );
    assert_eq!(session.ticks, 1);
    assert_eq!(frozen.ticks, 0);
    assert_eq!(frozen.events().len(), 1);
    assert_eq!(session.events().back().unwrap().payload, Event::Ticked(1));
    assert!(world.item(1).unwrap().renderable.is_none());
}

#[test]
fn invalid_requests_cannot_bypass_world_checks_or_advance_time() {
    let mut world = world();
    let before = world.items().to_vec();
    let mut session = Session::register(Workshop::default(), &world).unwrap();
    assert_eq!(
        session.command(&mut world, Input::Kick(99)),
        Err(Error::Command(CommandError::UnknownItem))
    );
    assert_eq!(
        session.command(&mut world, Input::Teleport(1)),
        Err(Error::Command(CommandError::WrongBodyKind))
    );
    for dt in [f32::NAN, f32::INFINITY, 0., -1., 0.3] {
        assert_eq!(
            session.step(&mut world, &[], dt),
            Err(Error::Framework(FrameworkError::InvalidTimestep))
        );
    }
    assert_eq!(
        session.step(&mut world, &[3], 0.1),
        Err(Error::Framework(FrameworkError::InvalidActiveItem))
    );
    assert_eq!(session.ticks, 0);
    assert!(session.events().is_empty());
    assert_eq!(world.items(), before);
    let mut missing = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![]);
    assert_eq!(
        session.step(&mut missing, &[], 0.1),
        Err(Error::Framework(FrameworkError::InvalidActiveItem))
    );
}

#[test]
fn event_history_is_bounded_ordered_and_reports_drops() {
    let mut world = world();
    let mut session = Session::register(Workshop::default(), &world).unwrap();
    for _ in 0..EVENT_CAPACITY + 4 {
        session.step(&mut world, &[], 0.01).unwrap();
    }
    assert_eq!(session.events().len(), EVENT_CAPACITY);
    assert_eq!(session.dropped_events(), 4);
    assert_eq!(session.events().front().unwrap().sequence, 5);
    assert_eq!(
        session.events().back().unwrap().sequence,
        (EVENT_CAPACITY + 4) as u64
    );
}

#[test]
fn registration_checks_contract_and_pre_step_runs_before_physics() {
    let mut world = world();
    for info in [
        PluginInfo { id: "", version: 1 },
        PluginInfo {
            id: "workshop",
            version: 2,
        },
        PluginInfo {
            id: "invalid/name",
            version: 1,
        },
    ] {
        assert!(matches!(
            Session::register(
                Workshop {
                    info,
                    ..Default::default()
                },
                &world
            ),
            Err(Error::Framework(FrameworkError::InvalidPlugin))
        ));
    }
    let mut session = Session::register(
        Workshop {
            pre_impulse: true,
            ..Default::default()
        },
        &world,
    )
    .unwrap();
    session.step(&mut world, &[], 0.1).unwrap();
    assert_eq!(session.events()[0].payload, Event::Kicked(1));
    assert_eq!(session.events()[1].payload, Event::Ticked(1));
}
