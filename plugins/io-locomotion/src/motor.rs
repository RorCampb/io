//! Shared execution for player and AI intentions; planning never moves world items directly.
use crate::animation::AnimationDriver;
use crate::paths::{self, Action, PlannedMotion, Profile, Settings};
use crate::Error;
use io_game::PluginWorld;
use io_traversal::{navigation::Waypoint, Progress};
use io_types::{Rotation, Vec3};
use io_world::{
    character_fits, character_support, sweep_character, walk_character, CharacterSweep,
    SupportProbe, WorldView,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;

pub use crate::animation::{ClipDefinition, Motion};
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locomotion {
    pub walk_speed: f32,
    pub crouch_speed: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    pub standing_height: f32,
    pub crouch_height: f32,
    pub turn_speed: f32,
    pub blend_seconds: f32,
    pub clips: BTreeMap<Motion, ClipDefinition>,
}
impl Locomotion {
    pub fn validate(&self) -> Result<(), String> {
        if ![
            self.walk_speed,
            self.crouch_speed,
            self.jump_speed,
            self.gravity,
            self.standing_height,
            self.crouch_height,
            self.turn_speed,
            self.blend_seconds,
        ]
        .iter()
        .all(|v| v.is_finite())
            || !(0.1..=12.).contains(&self.walk_speed)
            || !(0.1..=self.walk_speed).contains(&self.crouch_speed)
            || !(0.1..=15.).contains(&self.jump_speed)
            || !(0.1..=50.).contains(&self.gravity)
            || !(0.5..=4.).contains(&self.standing_height)
            || !(0.2..self.standing_height).contains(&self.crouch_height)
            || !(0.1..=30.).contains(&self.turn_speed)
            || !(0.01..=0.5).contains(&self.blend_seconds)
            || self.clips.len() != Motion::ALL.len()
        {
            return Err("invalid traversal settings".into());
        }
        for m in Motion::ALL {
            let c = self
                .clips
                .get(&m)
                .ok_or("missing locomotion clip binding")?;
            if c.clip.is_empty() || !c.speed.is_finite() || !(0. ..=8.).contains(&c.speed) {
                return Err("invalid locomotion clip".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Move { x: f32, y: f32 },
    Crouch(bool),
    Jump,
}
#[derive(Clone, Copy, Debug)]
pub enum MotionEvent {
    Jumped,
    Landed,
    Stance { crouched: bool },
}
#[derive(Clone, Debug)]
/// Optional upright-body movement implementation, bound to an existing Item.
/// Direct commands and planned actions share collision, pose, gravity and presentation.
pub struct Motor {
    walk_scratch: crate::pipeline::WalkScratch,
    definition: Arc<Locomotion>,
    actor: u64,
    animation: AnimationDriver,
    planned: Option<Box<PlannedMotion>>,
    progress: Progress,
    direction: Vec3,
    crouch_intent: bool,
    jump_intent: bool,
    crouched: bool,
    grounded: bool,
    vertical_speed: f32,
    landing: f32,
    yaw: f32,
}
impl Motor {
    pub fn new(
        definition: Locomotion,
        actor: u64,
        clips: BTreeMap<Motion, (usize, f64)>,
        world: &dyn WorldView,
    ) -> Result<Self, String> {
        definition.validate()?;
        let definition = Arc::new(definition);
        let animation = AnimationDriver::new(definition.clone(), clips)?;
        let p = Self {
            walk_scratch: Default::default(),
            definition,
            actor,
            animation,
            planned: None,
            progress: Progress::Running,
            direction: Vec3::default(),
            crouch_intent: false,
            jump_intent: false,
            crouched: false,
            grounded: false,
            vertical_speed: 0.,
            landing: 0.,
            yaw: {
                let forward = world
                    .item(actor)
                    .ok_or("unknown motor item")?
                    .transform
                    .rotation
                    .rotate(Vec3::new(0., -1., 0.));
                forward.x.atan2(-forward.y)
            },
        };
        p.validate(world)
            .map_err(|e| format!("invalid traversal binding: {e:?}"))?;
        Ok(p)
    }
    pub fn actor(&self) -> u64 {
        self.actor
    }
    pub fn settings(&self) -> &Locomotion {
        &self.definition
    }
    pub fn motion(&self) -> Motion {
        self.animation.motion()
    }
    pub fn crouched(&self) -> bool {
        self.crouched
    }
    pub fn grounded(&self) -> bool {
        self.grounded
    }
    pub fn vertical_speed(&self) -> f32 {
        self.vertical_speed
    }
    pub fn cancel(&mut self) {
        self.planned = None;
        self.direction = Vec3::default();
        self.jump_intent = false;
        self.progress = Progress::Running;
    }
    pub fn traversal_profile(
        &self,
        w: &dyn WorldView,
        settings: Settings,
    ) -> Result<Profile, Error> {
        settings.validate().map_err(|_| Error::InvalidInput)?;
        Ok(Profile {
            actor: self.actor,
            body: w
                .item(self.actor)
                .and_then(|i| i.character_body)
                .ok_or(Error::InvalidWorld)?,
            speed: self.definition.walk_speed,
            gravity: self.definition.gravity,
            jump_speed: self.definition.jump_speed,
            step_height: settings.step_height,
            jump_distance: settings.jump_distance,
            max_drop: settings.max_drop,
        })
    }
    pub fn begin_traversal(
        &mut self,
        w: &dyn WorldView,
        settings: Settings,
        step: Waypoint<Action>,
    ) -> Result<Progress, Error> {
        let profile = self.traversal_profile(w, settings)?;
        let start = w
            .item(self.actor)
            .ok_or(Error::InvalidWorld)?
            .transform
            .anchor;
        self.cancel();
        self.crouch_intent = false;
        self.planned = paths::path(w, profile, start, step.position, step.action).map(|path| {
            Box::new(PlannedMotion {
                path,
                profile,
                elapsed: 0.,
            })
        });
        self.progress = if self.planned.is_some() {
            Progress::Running
        } else {
            Progress::Blocked
        };
        Ok(self.progress)
    }
    pub fn traversal_progress(&self) -> Progress {
        self.progress
    }
    pub fn feedback(
        &self,
        w: &dyn WorldView,
        requested_velocity: Vec3,
        actual_velocity: Vec3,
    ) -> Result<crate::MotorFeedback, Error> {
        Ok(crate::MotorFeedback {
            actor: self.actor,
            position: w
                .item(self.actor)
                .ok_or(Error::InvalidWorld)?
                .transform
                .anchor,
            requested_velocity,
            actual_velocity,
            grounded: self.grounded,
            crouched: self.crouched,
            motion: self.motion(),
        })
    }
    pub fn validate(&self, w: &dyn WorldView) -> Result<(), Error> {
        let i = w.item(self.actor).ok_or(Error::InvalidWorld)?;
        if i.character_body.is_none()
            || i.animation.is_none()
            || i.motion.is_some()
            || i.physics_body.is_some()
        {
            return Err(Error::InvalidWorld);
        }
        Ok(())
    }
    pub fn command(&mut self, c: Command) -> Result<(), Error> {
        match c {
            Command::Move { x, y } => {
                if !x.is_finite() || !y.is_finite() || x.abs() > 1. || y.abs() > 1. {
                    return Err(Error::InvalidInput);
                }
                self.direction = Vec3::new(x, y, 0.).scaled(1. / x.hypot(y).max(1.));
            }
            Command::Crouch(v) => self.crouch_intent = v,
            Command::Jump => self.jump_intent = true,
        }
        Ok(())
    }
    pub fn update<E>(
        &mut self,
        w: &mut PluginWorld<E>,
        dt: f32,
    ) -> Result<Vec<MotionEvent>, Error> {
        if !dt.is_finite() || dt <= 0. || dt > 0.25 {
            return Err(Error::InvalidInput);
        }
        self.validate(w)?;
        let mut events = Vec::new();
        let item = w.item(self.actor).ok_or(Error::InvalidWorld)?;
        let start = item.transform.anchor;
        let mut shape = item.character_body.ok_or(Error::InvalidWorld)?;
        let desired = if self.crouch_intent {
            self.definition.crouch_height
        } else {
            self.definition.standing_height
        };
        let mut probe = shape;
        probe.height = desired;
        if character_fits(w, self.actor, start, probe)
            && w.set_character_height(self.actor, desired)
        {
            shape = probe;
        }
        let crouched = shape.height < self.definition.standing_height - 0.01;
        if crouched != self.crouched {
            events.push(MotionEvent::Stance { crouched });
            self.crouched = crouched;
        }
        let mut stepping = false;
        let movement = if let Some(planned) = &mut self.planned {
            let result = planned.advance(w, start, dt)?;
            self.progress = result.progress;
            stepping = result.stepping;
            if let Some(v) = result.vertical_speed {
                self.vertical_speed = v;
            }
            if result.jumped {
                events.push(MotionEvent::Jumped);
            }
            if self.progress != Progress::Running {
                self.planned = None;
            }
            result.movement
        } else {
            let supported =
                character_support(w, Some(self.actor), start, shape, SupportProbe::CONTACT)
                    .map_err(|_| Error::InvalidWorld)?
                    .is_some();
            if self.jump_intent && supported && !crouched {
                self.vertical_speed = self.definition.jump_speed;
                events.push(MotionEvent::Jumped);
            }
            self.jump_intent = false;
            let dz = self.vertical_speed * dt - 0.5 * self.definition.gravity * dt * dt;
            self.vertical_speed = (self.vertical_speed - self.definition.gravity * dt).max(-50.);
            let speed = if crouched {
                self.definition.crouch_speed
            } else {
                self.definition.walk_speed
            };
            let delta = self.direction.scaled(speed * dt) + Vec3::new(0., 0., dz);
            if supported && self.vertical_speed <= 0. {
                let planar = Vec3::new(delta.x, delta.y, 0.);
                let walk = match crate::pipeline::walk(
                    w,
                    Some(self.actor),
                    start,
                    planar,
                    shape,
                    &mut self.walk_scratch.0,
                ) {
                    Ok(walk) => walk,
                    // A live actor changes the admissible prefix. Preserve the existing
                    // contact/refinement behavior only for this blocked case.
                    Err(
                        crate::pipeline::WalkError::ActorBlocked
                        | crate::pipeline::WalkError::UnsupportedStart,
                    ) => walk_character(w, Some(self.actor), start, planar, shape)
                        .map_err(|_| Error::InvalidWorld)?,
                    Err(crate::pipeline::WalkError::Invalid(_)) => return Err(Error::InvalidWorld),
                };
                if walk.reached {
                    CharacterSweep {
                        position: walk.position,
                        grounded: true,
                        hit_ceiling: false,
                    }
                } else {
                    // A player may walk off an edge or slide against a wall. AI route
                    // validation rejects unsupported edges before issuing movement.
                    let remainder = Vec3::new(
                        start.x + delta.x - walk.position.x,
                        start.y + delta.y - walk.position.y,
                        delta.z,
                    );
                    sweep_character(w, self.actor, walk.position, remainder, shape)
                        .map_err(|_| Error::InvalidWorld)?
                }
            } else {
                sweep_character(w, self.actor, start, delta, shape)
                    .map_err(|_| Error::InvalidWorld)?
            }
        };
        self.landing = (self.landing - dt).max(0.);
        if movement.grounded && !self.grounded && self.vertical_speed < -1. {
            self.landing = 0.18;
            events.push(MotionEvent::Landed);
        }
        self.grounded = movement.grounded;
        if movement.hit_ceiling || movement.grounded {
            self.vertical_speed = 0.;
        }
        let displacement = movement.position - start;
        let moving = displacement.x.hypot(displacement.y) > 0.0001;
        if moving {
            let target = displacement.x.atan2(-displacement.y);
            let error = (target - self.yaw + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.yaw += error.clamp(
                -self.definition.turn_speed * dt,
                self.definition.turn_speed * dt,
            );
        }
        w.place(
            self.actor,
            movement.position,
            Rotation::yaw(self.yaw).map_err(|_| Error::InvalidInput)?,
        )
        .map_err(|_| Error::InvalidWorld)?;
        let next = match (
            self.grounded || stepping,
            crouched,
            moving,
            self.landing > 0.,
        ) {
            (false, _, _, _) => {
                if self.vertical_speed > 0. {
                    Motion::Jump
                } else {
                    Motion::Fall
                }
            }
            (true, true, true, _) => Motion::CrouchWalk,
            (true, true, false, _) => Motion::CrouchIdle,
            (true, false, _, true) => Motion::Land,
            (true, false, true, false) => Motion::Walk,
            _ => Motion::Idle,
        };
        self.animation.update(w, self.actor, next)?;
        Ok(events)
    }
}
