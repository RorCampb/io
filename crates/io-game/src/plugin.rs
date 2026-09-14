//! Statically linked, trusted Rust plugins. No dynamic loader or untyped message bus.
use io_types::{Rotation, Vec3};
use io_world::{
    Collider, CommandError, CommandOutcome, Item, PhysicsBody, PhysicsStats, Space, World,
    WorldCommand, WorldView,
};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::Deref;

pub const EVENT_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: &'static str,
    pub version: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameworkError {
    InvalidPlugin,
    InvalidTimestep,
    InvalidActiveItem,
}

#[derive(Clone, Copy, Debug)]
pub struct Tick {
    seconds: f32,
}
impl Tick {
    pub fn seconds(self) -> f32 {
        self.seconds
    }
}

/// Read-only world access plus checked operations; no mutable Items or simulation stepping.
pub struct PluginWorld<'a, E> {
    world: &'a mut World,
    events: &'a mut VecDeque<PluginEvent<E>>,
    sequence: &'a mut u64,
    dropped: &'a mut u64,
}

#[derive(Clone, Debug)]
pub struct PluginEvent<E> {
    pub sequence: u64,
    pub payload: E,
}

impl<E> PluginWorld<'_, E> {
    pub fn apply(&mut self, command: WorldCommand) -> CommandOutcome {
        self.world.apply_command(command)
    }
    /// Explicit placement, not collision-aware locomotion. Physics-owned Items cannot teleport here.
    pub fn place(
        &mut self,
        target: u64,
        anchor: Vec3,
        rotation: Rotation,
    ) -> Result<(), CommandError> {
        let item = self.world.item(target).ok_or(CommandError::UnknownItem)?;
        if item.physics_body.is_some() || item.motion.is_some() {
            return Err(CommandError::WrongBodyKind);
        }
        if self.world.set_pose_3d(target, anchor, rotation) {
            Ok(())
        } else {
            Err(CommandError::InvalidValue)
        }
    }
    pub fn attach_dynamic_body(
        &mut self,
        target: u64,
        body: PhysicsBody,
        collider: Collider,
        impulse: Vec3,
        point: Vec3,
    ) -> Result<(), String> {
        self.world
            .attach_dynamic_body(target, body, collider, impulse, point)
    }
    /// Bounded presentation/history stream, not a reliable command transport.
    pub fn emit(&mut self, payload: E) {
        let Some(next) = self.sequence.checked_add(1) else {
            *self.dropped = self.dropped.saturating_add(1);
            return;
        };
        if self.events.len() == EVENT_CAPACITY {
            self.events.pop_front();
            *self.dropped = self.dropped.saturating_add(1);
        }
        *self.sequence = next;
        self.events.push_back(PluginEvent {
            sequence: *self.sequence,
            payload,
        });
    }
}

impl<E> WorldView for PluginWorld<'_, E> {
    fn terrain(&self) -> Option<&io_world::HeightField> {
        self.world.terrain()
    }
    fn space(&self) -> &Space {
        self.world.space()
    }
    fn items(&self) -> &[Item] {
        self.world.items()
    }
    fn item(&self, id: u64) -> Option<&Item> {
        self.world.item(id)
    }
    fn query(&self, center: Vec3, radius: f32) -> Vec<usize> {
        self.world.query(center, radius)
    }
    fn revision(&self) -> u64 {
        self.world.revision()
    }
    fn spatial_revision(&self) -> u64 {
        self.world.spatial_revision()
    }
    fn physics_stats(&self) -> PhysicsStats {
        self.world.physics_stats()
    }
    fn physics_error(&self) -> Option<&str> {
        self.world.physics_error()
    }
}

/// One authoritative mode plugin per session. Plugins can compose their own rule modules.
/// Associated enums are the plugin's contract; adding one never extends an engine enum.
pub trait GamePlugin: Clone + Debug + Send + Sync + 'static {
    type Command: Send + 'static;
    type Event: Clone + Debug + Send + Sync + 'static;
    type Error: From<FrameworkError>;
    fn info(&self) -> PluginInfo;
    fn validate(&self, world: &dyn WorldView) -> Result<(), Self::Error>;
    fn active_items(&self) -> Vec<u64> {
        vec![]
    }
    fn command(
        &mut self,
        world: &mut PluginWorld<'_, Self::Event>,
        command: Self::Command,
    ) -> Result<(), Self::Error>;
    /// Submit physics targets/forces before integration; inspect actual results in update.
    fn before_step(
        &mut self,
        _world: &mut PluginWorld<'_, Self::Event>,
        _tick: Tick,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Runs after the host's single world step. dt is validated and independent of rendering.
    fn update(
        &mut self,
        world: &mut PluginWorld<'_, Self::Event>,
        tick: Tick,
    ) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug)]
pub struct Session<P: GamePlugin> {
    plugin: P,
    events: VecDeque<PluginEvent<P::Event>>,
    sequence: u64,
    dropped: u64,
}
impl<P: GamePlugin + Default> Default for Session<P> {
    fn default() -> Self {
        Self {
            plugin: P::default(),
            events: VecDeque::new(),
            sequence: 0,
            dropped: 0,
        }
    }
}
impl<P: GamePlugin> Deref for Session<P> {
    type Target = P;
    fn deref(&self) -> &P {
        &self.plugin
    }
}
impl<P: GamePlugin> Session<P> {
    fn validate_info(plugin: &P) -> Result<(), P::Error> {
        let info = plugin.info();
        if info.version != 1
            || info.id.is_empty()
            || info.id.len() > 64
            || !info
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(FrameworkError::InvalidPlugin.into());
        }
        Ok(())
    }
    pub fn register(plugin: P, world: &dyn WorldView) -> Result<Self, P::Error> {
        Self::validate_info(&plugin)?;
        plugin.validate(world)?;
        Ok(Self {
            plugin,
            events: VecDeque::new(),
            sequence: 0,
            dropped: 0,
        })
    }
    pub fn events(&self) -> &VecDeque<PluginEvent<P::Event>> {
        &self.events
    }
    pub fn dropped_events(&self) -> u64 {
        self.dropped
    }
    pub fn command(&mut self, world: &mut World, command: P::Command) -> Result<(), P::Error> {
        Self::validate_info(&self.plugin)?;
        self.plugin.validate(world)?;
        self.plugin.command(
            &mut PluginWorld {
                world,
                events: &mut self.events,
                sequence: &mut self.sequence,
                dropped: &mut self.dropped,
            },
            command,
        )
    }
    pub fn step(&mut self, world: &mut World, active: &[usize], dt: f32) -> Result<(), P::Error> {
        if !dt.is_finite() || dt <= 0. || dt > 0.25 {
            return Err(FrameworkError::InvalidTimestep.into());
        }
        Self::validate_info(&self.plugin)?;
        self.plugin.validate(world)?;
        if active.iter().any(|&i| i >= world.items().len()) {
            return Err(FrameworkError::InvalidActiveItem.into());
        }
        let required: std::collections::BTreeSet<_> =
            self.plugin.active_items().into_iter().collect();
        let mut selected = active.to_vec();
        let mut found = 0;
        for (i, item) in world.items().iter().enumerate() {
            if required.contains(&item.id) {
                selected.push(i);
                found += 1;
            }
        }
        if found != required.len() {
            return Err(FrameworkError::InvalidActiveItem.into());
        }
        selected.sort_unstable();
        selected.dedup();
        self.plugin.before_step(
            &mut PluginWorld {
                world,
                events: &mut self.events,
                sequence: &mut self.sequence,
                dropped: &mut self.dropped,
            },
            Tick { seconds: dt },
        )?;
        world.simulate(&selected, dt);
        self.plugin.update(
            &mut PluginWorld {
                world,
                events: &mut self.events,
                sequence: &mut self.sequence,
                dropped: &mut self.dropped,
            },
            Tick { seconds: dt },
        )
    }
}
