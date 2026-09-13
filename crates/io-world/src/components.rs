use crate::{AnimationState, Playback};
use io_types::{AppearanceId, Bounds, Rotation, Vec3, VisualStateId};

/// Authoritative placement; interpolation history is maintained by World.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub anchor: Vec3,
    pub size: Vec3,
    pub rotation: Rotation,
    pub(crate) previous_anchor: Vec3,
    pub(crate) previous_rotation: Rotation,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            anchor: Vec3::default(),
            size: Vec3::new(1., 1., 1.),
            rotation: Rotation::default(),
            previous_anchor: Vec3::default(),
            previous_rotation: Rotation::default(),
        }
    }
}
impl Transform {
    pub fn new(anchor: Vec3, size: Vec3, yaw: f32) -> Result<Self, String> {
        Self::oriented(anchor, size, Rotation::yaw(yaw)?)
    }
    pub fn oriented(anchor: Vec3, size: Vec3, rotation: Rotation) -> Result<Self, String> {
        let mut transform = Self {
            anchor,
            size,
            rotation,
            ..Self::default()
        };
        transform.validate()?;
        transform.snap();
        Ok(transform)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.anchor.finite()
            || !self.size.finite()
            || self.size.x <= 0.
            || self.size.y <= 0.
            || self.size.z <= 0.
        {
            return Err("transform must be finite with positive scale".into());
        }
        Ok(())
    }
    pub(crate) fn snap(&mut self) {
        self.previous_anchor = self.anchor;
        self.previous_rotation = self.rotation;
    }
    pub(crate) fn pose(&self, alpha: f32) -> (Vec3, Rotation) {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0., 1.)
        } else {
            1.
        };
        (
            self.previous_anchor.scaled(1. - alpha) + self.anchor.scaled(alpha),
            self.previous_rotation.interpolate(self.rotation, alpha),
        )
    }
    pub fn matrix(&self, anchor: Vec3, rotation: Rotation) -> [f32; 16] {
        let x = rotation.rotate(Vec3::new(self.size.x, 0., 0.));
        let y = rotation.rotate(Vec3::new(0., self.size.y, 0.));
        let z = rotation.rotate(Vec3::new(0., 0., self.size.z));
        [
            x.x, x.y, x.z, 0., y.x, y.y, y.z, 0., z.x, z.y, z.z, 0., anchor.x, anchor.y, anchor.z,
            1.,
        ]
    }
    pub fn bounds(&self, bounds: Bounds) -> Bounds {
        let c = bounds.center();
        let e = bounds.extent();
        let center = self.anchor
            + self.rotation.rotate(Vec3::new(
                c.x * self.size.x,
                c.y * self.size.y,
                c.z * self.size.z,
            ));
        let x = self.rotation.rotate(Vec3::new(e.x * self.size.x, 0., 0.));
        let y = self.rotation.rotate(Vec3::new(0., e.y * self.size.y, 0.));
        let z = self.rotation.rotate(Vec3::new(0., 0., e.z * self.size.z));
        let extent = Vec3::new(
            x.x.abs() + y.x.abs() + z.x.abs(),
            x.y.abs() + y.y.abs() + z.y.abs(),
            x.z.abs() + y.z.abs() + z.z.abs(),
        );
        Bounds {
            min: center - extent,
            max: center + extent,
        }
    }
}

/// Gameplay/spatial extent, not a physics collider or rendering requirement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occupancy {
    pub local_bounds: Bounds,
}
impl Default for Occupancy {
    fn default() -> Self {
        Self {
            local_bounds: Bounds {
                min: Vec3::default(),
                max: Vec3::new(1., 1., 1.),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ColorMode {
    #[default]
    Tint,
    /// Explicit legacy debug visualization, not a character classification.
    Pulse,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Renderable {
    pub appearance_id: AppearanceId,
    pub visual_state: VisualStateId,
    pub visual_state_count: u32,
    /// Envelope of all visual states/LODs, independent of gameplay occupancy.
    pub local_bounds: Bounds,
    pub tint: [f32; 3],
    pub color_mode: ColorMode,
}
impl Default for Renderable {
    fn default() -> Self {
        Self {
            appearance_id: AppearanceId::default(),
            visual_state: VisualStateId::default(),
            visual_state_count: 1,
            local_bounds: Occupancy::default().local_bounds,
            tint: [1.; 3],
            color_mode: ColorMode::Tint,
        }
    }
}
impl Renderable {
    pub fn validate(&self) -> Result<(), String> {
        if self.appearance_id.0 == 0
            || self.visual_state.0 >= self.visual_state_count
            || !self.local_bounds.valid()
            || self.tint.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("invalid renderable appearance, visual state, bounds, or tint".into());
        }
        Ok(())
    }
    pub fn color(&self, ticks: u64) -> [f32; 3] {
        match self.color_mode {
            ColorMode::Tint => self.tint,
            ColorMode::Pulse => [1., 0.75 + 0.25 * (ticks as f32 * 0.08).sin(), 0.15],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Durability {
    current: u32,
    maximum: u32,
}
impl Durability {
    pub fn new(current: u32, maximum: u32) -> Result<Self, String> {
        if maximum == 0 || current > maximum {
            return Err("durability requires 0 <= current <= maximum and maximum > 0".into());
        }
        Ok(Self { current, maximum })
    }
    pub fn current(&self) -> u32 {
        self.current
    }
    pub fn maximum(&self) -> u32 {
        self.maximum
    }
    pub(crate) fn damage(&mut self, amount: u32) -> bool {
        let next = self.current.saturating_sub(amount);
        let changed = next != self.current;
        self.current = next;
        changed
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum DepletionAnimation {
    #[default]
    Keep,
    Freeze,
    PlayOnce(AnimationState),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DepletionResponse {
    pub stop_motion: bool,
    pub visual_state: Option<VisualStateId>,
    pub animation: DepletionAnimation,
}
impl DepletionResponse {
    pub(crate) fn validate(&self, renderable: Option<&Renderable>) -> Result<(), String> {
        if self
            .visual_state
            .is_some_and(|state| renderable.is_none_or(|r| state.0 >= r.visual_state_count))
        {
            return Err("depletion visual state requires a renderable with that state".into());
        }
        if let DepletionAnimation::PlayOnce(player) = &self.animation {
            if renderable.is_none()
                || !matches!(player.playback(), Playback::Once { .. })
                || player.speed() <= 0.
                || player.time() != 0.
            {
                return Err("depletion playback requires a renderable and an unstarted positive-speed one-shot".into());
            }
        }
        Ok(())
    }
}
