use crate::{
    AnimationState, ColorMode, DepletionAnimation, DepletionResponse, Durability, Occupancy,
    PathMotion, Renderable, Transform,
};
use io_types::{Bounds, Rotation, Vec3};

/// Editable assembly before insertion; World owns accepted items and exposes shared references.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: u64,
    pub grounded: Option<crate::Grounded>,
    pub transform: Transform,
    pub occupancy: Occupancy,
    pub renderable: Option<Renderable>,
    pub durability: Option<Durability>,
    pub depletion_response: Option<DepletionResponse>,
    pub motion: Option<PathMotion>,
    pub animation: Option<AnimationState>,
    pub simulated_ticks: u64,
    pub physics_body: Option<crate::PhysicsBody>,
    pub collider: Option<crate::Collider>,
}
impl Item {
    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0 {
            return Err("item ID must be nonzero".into());
        }
        self.transform.validate()?;
        if let Some(motor) = self.grounded {
            motor.validate()?;
            if self.physics_body.is_some() || self.motion.is_some() {
                return Err("grounded movement needs exclusive pose ownership".into());
            }
        }
        match (&self.physics_body, &self.collider) {
            (None, None) => {}
            (Some(body), Some(collider)) => {
                body.validate()?;
                collider.validate()?;
                if !crate::physics::bounded(self.transform.anchor, 1e6) {
                    return Err("physics position exceeds supported range".into());
                }
                if self.motion.is_some() && body.kind != crate::BodyKind::Kinematic {
                    return Err("route movement on physics bodies requires kinematic mode".into());
                }
            }
            _ => return Err("physics body and collider must be supplied together".into()),
        }
        if let Some(renderable) = &self.renderable {
            renderable.validate()?;
        }
        if self.animation.is_some() && self.renderable.is_none() {
            return Err("model animation requires a renderable".into());
        }
        if let Some(response) = &self.depletion_response {
            if self.durability.is_none() {
                return Err("depletion response requires durability".into());
            }
            response.validate(self.renderable.as_ref())?;
        }
        if !self.occupancy.local_bounds.valid()
            || !self.bounds().valid()
            || !self.visibility_bounds().valid()
        {
            return Err("invalid occupancy or transformed bounds".into());
        }
        Ok(())
    }
    pub fn needs_simulation(&self) -> bool {
        self.motion.is_some()
            || self.animation.is_some()
            || self
                .renderable
                .as_ref()
                .is_some_and(|r| r.color_mode == ColorMode::Pulse)
    }
    pub(crate) fn apply_depletion(&mut self) {
        let Some(response) = self.depletion_response.take() else {
            return;
        };
        if let Some(state) = response.visual_state {
            if let Some(renderable) = &mut self.renderable {
                renderable.visual_state = state;
            }
        }
        if response.stop_motion {
            self.motion = None;
            if let Some(body) = &mut self.physics_body {
                body.target = None;
            }
            self.transform.snap();
        }
        match response.animation {
            DepletionAnimation::Keep => {}
            DepletionAnimation::Freeze => {
                if let Some(animation) = &mut self.animation {
                    animation.freeze();
                }
            }
            DepletionAnimation::PlayOnce(animation) => self.animation = Some(animation),
        }
    }
    pub fn render_pose(&self, alpha: f32) -> (Vec3, Rotation) {
        if (self.motion.is_none() && self.physics_body.is_none()) || self.simulated_ticks == 0 {
            (self.transform.anchor, self.transform.rotation)
        } else {
            self.transform.pose(alpha)
        }
    }
    pub fn interpolates_pose(&self) -> bool {
        self.simulated_ticks != 0
            && (self.motion.is_some() || self.physics_body.is_some())
            && (self.transform.anchor != self.transform.previous_anchor
                || self.transform.rotation != self.transform.previous_rotation)
    }
    pub fn transform(&self) -> [f32; 16] {
        self.render_transform(1.)
    }
    pub fn render_transform(&self, alpha: f32) -> [f32; 16] {
        let (anchor, rotation) = self.render_pose(alpha);
        self.transform.matrix(anchor, rotation)
    }
    pub fn bounds(&self) -> Bounds {
        self.transform.bounds(self.occupancy.local_bounds)
    }
    pub fn visibility_bounds(&self) -> Bounds {
        // Include occupancy even when visuals are smaller or absent.
        let b = self
            .renderable
            .as_ref()
            .map_or(self.occupancy.local_bounds, |r| {
                r.local_bounds.union(self.occupancy.local_bounds)
            });
        if self.motion.is_none() && self.physics_body.is_none() {
            return self.transform.bounds(b);
        }
        let t = &self.transform;
        let ext = Vec3::new(
            b.min.x.abs().max(b.max.x.abs()) * t.size.x,
            b.min.y.abs().max(b.max.y.abs()) * t.size.y,
            b.min.z.abs().max(b.max.z.abs()) * t.size.z,
        );
        let r = ext.dot(ext).sqrt();
        let e = Vec3::new(r, r, r);
        Bounds {
            min: Vec3::new(
                t.anchor.x.min(t.previous_anchor.x),
                t.anchor.y.min(t.previous_anchor.y),
                t.anchor.z.min(t.previous_anchor.z),
            ) - e,
            max: Vec3::new(
                t.anchor.x.max(t.previous_anchor.x),
                t.anchor.y.max(t.previous_anchor.y),
                t.anchor.z.max(t.previous_anchor.z),
            ) + e,
        }
    }
}
