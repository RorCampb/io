//! Timed test-fixture behavior, not an engine door type. Pose changes use PluginWorld.
use crate::Error;
use io_game::PluginWorld;
use io_types::Vec3;
use io_world::WorldView;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarrierDefinition {
    pub item: String,
    pub alternate: [f32; 3],
    pub interval_seconds: f32,
    /// Zero retains the original instantaneous shutter fixture.
    #[serde(default)]
    pub travel_seconds: f32,
}
impl BarrierDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if self.item.is_empty()
            || self.item.len() > 64
            || self.alternate.iter().any(|v| !v.is_finite())
            || !self.interval_seconds.is_finite()
            || !(0. ..=120.).contains(&self.interval_seconds)
            || !self.travel_seconds.is_finite()
            || !(0. ..=30.).contains(&self.travel_seconds)
            || self.interval_seconds + self.travel_seconds < 0.5
        {
            return Err("invalid playground barrier cycle".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Barrier {
    item: u64,
    positions: [Vec3; 2],
    next: usize,
    interval: f32,
    elapsed: f32,
    travel: f32,
    travel_elapsed: Option<f32>,
}
impl Barrier {
    pub fn new(d: &BarrierDefinition, item: u64, w: &dyn WorldView) -> Result<Self, String> {
        d.validate()?;
        let i = w.item(item).ok_or("unknown barrier")?;
        if i.collider.is_none()
            || !i
                .physics_body
                .as_ref()
                .is_some_and(|p| p.kind == io_world::BodyKind::Kinematic)
            || i.motion.is_some()
            || i.character_body.is_some()
        {
            return Err(
                "barrier requires a kinematic collider and exclusive plugin ownership".into(),
            );
        }
        Ok(Self {
            item,
            positions: [
                i.transform.anchor,
                Vec3::new(d.alternate[0], d.alternate[1], d.alternate[2]),
            ],
            next: 1,
            interval: d.interval_seconds,
            elapsed: 0.,
            travel: d.travel_seconds,
            travel_elapsed: None,
        })
    }
    pub fn item(&self) -> u64 {
        self.item
    }
    pub fn update<E>(
        &mut self,
        w: &mut PluginWorld<E>,
        dt: f32,
        safe_to_change: bool,
    ) -> Result<(), Error> {
        if self.travel_elapsed.is_none() {
            self.elapsed += dt;
            if self.elapsed < self.interval {
                return Ok(());
            }
        }
        if !safe_to_change {
            return Ok(());
        }
        let i = w.item(self.item).ok_or(Error::InvalidWorld)?;
        let rotation = i.transform.rotation;
        let elapsed = self.travel_elapsed.unwrap_or(0.) + dt;
        let fraction = if self.travel == 0. {
            1.
        } else {
            (elapsed / self.travel).min(1.)
        };
        let anchor = self.positions[1 - self.next]
            + (self.positions[self.next] - self.positions[1 - self.next]).scaled(fraction);
        let delta = anchor - i.transform.anchor;
        let mut bounds = i.spatial_bounds();
        if self.travel > 0. {
            // Conservative swept bounds prevent a moving fixture from crossing an actor.
            bounds.min = bounds.min + Vec3::new(delta.x.min(0.), delta.y.min(0.), delta.z.min(0.));
            bounds.max = bounds.max + Vec3::new(delta.x.max(0.), delta.y.max(0.), delta.z.max(0.));
        } else {
            bounds.min = bounds.min + delta;
            bounds.max = bounds.max + delta;
        }
        let occupied = w
            .items()
            .iter()
            .filter_map(|i| {
                i.character_body
                    .map(|body| body.bounds_at(i.transform.anchor))
            })
            .any(|b| {
                b.min.x < bounds.max.x
                    && b.max.x > bounds.min.x
                    && b.min.y < bounds.max.y
                    && b.max.y > bounds.min.y
                    && b.min.z < bounds.max.z
                    && b.max.z > bounds.min.z
            });
        if occupied {
            return Ok(());
        }
        match w.apply(io_world::WorldCommand::SetKinematicTarget {
            target: self.item,
            anchor,
            rotation,
        }) {
            io_world::CommandOutcome::Applied | io_world::CommandOutcome::Unchanged => {}
            io_world::CommandOutcome::Rejected(_) => return Err(Error::InvalidWorld),
        }
        if fraction >= 1. {
            self.next = 1 - self.next;
            self.elapsed = 0.;
            self.travel_elapsed = None;
        } else {
            self.travel_elapsed = Some(elapsed);
        }
        Ok(())
    }
}
