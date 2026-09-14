//! Small, owned commands. Timing and delivery belong to the controller.
use crate::{Axis, BodyKind, World};
use io_types::{Rotation, Vec3, VisualStateId};

#[derive(Clone, Copy, Debug)]
pub enum WorldCommand {
    Damage {
        target: u64,
        amount: u32,
    },
    ApplyImpulse {
        target: u64,
        impulse: Vec3,
        point: Vec3,
    },
    SetGravityScale {
        target: u64,
        scale: f32,
    },
    SetKinematicTarget {
        target: u64,
        anchor: Vec3,
        rotation: Rotation,
    },
    SetVisualState {
        target: u64,
        state: VisualStateId,
    },
    ResizeAxis {
        axis: Axis,
        delta: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Collider, ColliderShape, Durability, Item, PhysicsBody, Space};
    use io_types::{Envelope, MessageId};

    #[test]
    fn compact_owned_envelopes_preserve_correlation() {
        fn transferable<T: Send + Sync + Copy + 'static>() {}
        transferable::<Envelope<WorldCommand>>();
        let message = Envelope::new(
            MessageId(42),
            WorldCommand::Damage {
                target: 1,
                amount: 2,
            },
        );
        assert_eq!(message.reply(CommandOutcome::Applied).id, message.id);
        assert!(std::mem::size_of::<Envelope<WorldCommand>>() <= 64);
        println!(
            "command envelope: {} bytes",
            std::mem::size_of::<Envelope<WorldCommand>>()
        );
    }

    #[test]
    fn command_validation_preserves_state_and_matches_direct_impulses() {
        let item = Item {
            id: 1,
            durability: Some(Durability::new(10, 10).unwrap()),
            physics_body: Some(PhysicsBody::new(BodyKind::Dynamic)),
            collider: Some(Collider::new(ColliderShape::Sphere { radius: 0.5 })),
            ..Item::default()
        };
        let mut world = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![item.clone()]);
        assert_eq!(
            world.apply_command(WorldCommand::Damage {
                target: 2,
                amount: 2
            }),
            CommandOutcome::Rejected(CommandError::UnknownItem)
        );
        assert_eq!(
            world.apply_command(WorldCommand::SetKinematicTarget {
                target: 1,
                anchor: Vec3::default(),
                rotation: Rotation::default()
            }),
            CommandOutcome::Rejected(CommandError::WrongBodyKind)
        );
        assert_eq!(
            world.apply_command(WorldCommand::ApplyImpulse {
                target: 1,
                impulse: Vec3::new(f32::NAN, 0., 0.),
                point: Vec3::default()
            }),
            CommandOutcome::Rejected(CommandError::InvalidValue)
        );
        assert_eq!(world.items(), &[item.clone()]);
        let frozen = world.snapshot();
        let mut direct = World::new(world.space().clone(), vec![item]);
        let impulse = Vec3::new(2., 0., 0.);
        assert!(direct.apply_impulse(1, impulse, Vec3::default()));
        assert_eq!(
            world.apply_command(WorldCommand::ApplyImpulse {
                target: 1,
                impulse,
                point: Vec3::default()
            }),
            CommandOutcome::Applied
        );
        assert_eq!(world.items(), direct.items());
        assert_eq!(
            world.apply_command(WorldCommand::Damage {
                target: 1,
                amount: 2
            }),
            CommandOutcome::Applied
        );
        assert_eq!(
            world.apply_command(WorldCommand::Damage {
                target: 1,
                amount: 0
            }),
            CommandOutcome::Unchanged
        );
        use crate::WorldView;
        assert_eq!(
            frozen
                .item(1)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            10
        );
        assert_eq!(
            frozen
                .item(1)
                .unwrap()
                .physics_body
                .as_ref()
                .unwrap()
                .velocity,
            Vec3::default()
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    UnknownItem,
    MissingComponent,
    WrongBodyKind,
    InvalidValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    Applied,
    Unchanged,
    Rejected(CommandError),
}

impl World {
    pub fn apply_command(&mut self, command: WorldCommand) -> CommandOutcome {
        use CommandError::*;
        use CommandOutcome::*;
        let target = match command {
            WorldCommand::Damage { target, .. }
            | WorldCommand::ApplyImpulse { target, .. }
            | WorldCommand::SetGravityScale { target, .. }
            | WorldCommand::SetKinematicTarget { target, .. }
            | WorldCommand::SetVisualState { target, .. } => target,
            WorldCommand::ResizeAxis { axis, delta } => {
                let before = self.revision();
                self.resize_units(axis, delta);
                return if before == self.revision() {
                    Unchanged
                } else {
                    Applied
                };
            }
        };
        let Some(item) = self.item(target) else {
            return Rejected(UnknownItem);
        };
        let accepted = match command {
            WorldCommand::Damage { amount, .. } => {
                if item.durability.is_none() {
                    return Rejected(MissingComponent);
                }
                return if self.damage(target, amount) {
                    Applied
                } else {
                    Unchanged
                };
            }
            WorldCommand::SetVisualState { state, .. } => {
                let Some(renderable) = &item.renderable else {
                    return Rejected(MissingComponent);
                };
                if state.0 >= renderable.visual_state_count {
                    return Rejected(InvalidValue);
                }
                return if self.set_visual_state(target, state) {
                    Applied
                } else {
                    Unchanged
                };
            }
            WorldCommand::ApplyImpulse { impulse, point, .. } => {
                let Some(body) = &item.physics_body else {
                    return Rejected(MissingComponent);
                };
                if body.kind != BodyKind::Dynamic {
                    return Rejected(WrongBodyKind);
                }
                self.apply_impulse(target, impulse, point)
            }
            WorldCommand::SetGravityScale { scale, .. } => {
                let Some(body) = &item.physics_body else {
                    return Rejected(MissingComponent);
                };
                if body.kind != BodyKind::Dynamic {
                    return Rejected(WrongBodyKind);
                }
                self.set_gravity_scale(target, scale)
            }
            WorldCommand::SetKinematicTarget {
                anchor, rotation, ..
            } => {
                let Some(body) = &item.physics_body else {
                    return Rejected(MissingComponent);
                };
                if body.kind != BodyKind::Kinematic {
                    return Rejected(WrongBodyKind);
                }
                self.set_kinematic_target(target, anchor, rotation)
            }
            WorldCommand::ResizeAxis { .. } => unreachable!("handled above"),
        };
        if accepted {
            Applied
        } else {
            Rejected(InvalidValue)
        }
    }
}
