//! Application composition: select a plugin and adapt its input/presentation to this native demo.
use io_encounter::{Encounter, GameCommand, GameError};
use io_world::World;
#[derive(Clone, Debug)]
pub enum Game {
    Encounter(io_encounter::Game),
    Village(io_village::Game),
}
impl Default for Game {
    fn default() -> Self {
        Self::Encounter(Default::default())
    }
}
impl std::ops::Deref for Game {
    type Target = Encounter;
    fn deref(&self) -> &Encounter {
        match self {
            Self::Encounter(g) => g,
            Self::Village(g) => g.encounter(),
        }
    }
}
impl Game {
    pub fn village(&self) -> Option<&io_village::Village> {
        match self {
            Self::Village(g) => Some(g),
            Self::Encounter(_) => None,
        }
    }
    pub fn command(&mut self, world: &mut World, command: GameCommand) -> Result<(), GameError> {
        self.explore_command(world, io_village::Command::Combat(command))
    }
    pub fn explore_command(
        &mut self,
        world: &mut World,
        command: io_village::Command,
    ) -> Result<(), GameError> {
        match self {
            Self::Village(g) => g.command(world, command),
            Self::Encounter(g) => match command {
                io_village::Command::Combat(c) => g.command(world, c),
                _ => Err(GameError::WrongPhase),
            },
        }
    }
    pub fn step(&mut self, world: &mut World, active: &[usize], dt: f32) -> Result<(), GameError> {
        match self {
            Self::Encounter(g) => g.step(world, active, dt),
            Self::Village(g) => g.step(world, active, dt),
        }
    }
}
