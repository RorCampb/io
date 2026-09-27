//! Camera orbit intent is independent from the collision-limited distance.
use io_types::Vec3;
use io_world::WorldView;

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitCamera {
    pub target_height_fraction: Option<f32>,
    pub avoidance: Option<crate::camera_steering::Avoidance>,
    pub min_distance: f32,
    pub max_distance: f32,
    pub clearance: f32,
    pub response_seconds: f32,
    pub lookahead_seconds: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_world::{BodyKind, Collider, ColliderShape, Item, PhysicsBody, Space, Transform, World};
    fn settings() -> OrbitCamera {
        OrbitCamera {
            target_height_fraction: None,
            avoidance: None,
            min_distance: 1.,
            max_distance: 20.,
            clearance: 0.2,
            response_seconds: 0.2,
            lookahead_seconds: 0.25,
        }
    }
    fn empty() -> World {
        World::new(Space::new(Vec3::new(100., 100., 100.)), Vec::new())
    }
    fn step<'a>(world: &'a World, seconds: f32) -> BoomStep<'a> {
        BoomStep {
            world,
            pivot: Vec3::default(),
            direction: Vec3::new(1., 0., 0.),
            requested: 10.,
            velocity: Vec3::default(),
            exclude: None,
            seconds,
            near_radius: 0.1,
        }
    }
    #[test]
    fn obstruction_clamps_immediately_then_returns_without_forgetting_zoom() {
        let w = World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![Item {
                id: 1,
                transform: Transform::new(Vec3::new(4., 0., 0.), Vec3::new(1., 1., 1.), 0.)
                    .unwrap(),
                physics_body: Some(PhysicsBody::new(BodyKind::Static)),
                collider: Some(Collider::new(ColliderShape::Box {
                    half_extents: Vec3::new(0.01, 2., 2.),
                })),
                ..Item::default()
            }],
        );
        let mut a = CameraBoom::new(settings());
        a.update(step(&w, 0.));
        assert!((a.distance(10.) - 3.785).abs() < 1e-4);
        let mut b = a.clone();
        let clear = empty();
        a.update(step(&clear, 0.5));
        for _ in 0..72 {
            b.update(step(&clear, 0.5 / 72.));
        }
        assert!((a.distance(10.) - b.distance(10.)).abs() < 1e-4);
        assert!(a.distance(10.) > 3.785 && a.distance(10.) < 10.);
        assert_eq!(a.distance(2.), 2.);
        for _ in 0..100 {
            a.update(step(&clear, 0.05));
        }
        assert!((a.distance(10.) - 10.).abs() < 0.01);
    }
    #[test]
    fn collision_limits_every_axis_without_steering_or_predictive_zoom() {
        for direction in [
            Vec3::new(1., 0., 0.),
            Vec3::new(-1., 0., 0.),
            Vec3::new(0., 1., 0.),
            Vec3::new(0., -1., 0.),
            Vec3::new(0., 0., 1.),
            Vec3::new(0., 0., -1.),
        ] {
            let w = World::new(
                Space::new(Vec3::new(100., 100., 100.)),
                vec![Item {
                    id: 1,
                    transform: Transform::new(direction.scaled(4.), Vec3::new(1., 1., 1.), 0.)
                        .unwrap(),
                    collider: Some(Collider::new(ColliderShape::Box {
                        half_extents: Vec3::new(0.1, 0.1, 0.1),
                    })),
                    physics_body: Some(PhysicsBody::new(BodyKind::Static)),
                    ..Item::default()
                }],
            );
            let mut boom = CameraBoom::new(OrbitCamera {
                lookahead_seconds: 0.,
                ..settings()
            });
            let mut s = step(&w, 0.1);
            s.direction = direction;
            s.velocity = direction.scaled(100.);
            boom.update(s);
            assert!((boom.distance(10.) - 3.695).abs() < 1e-4);
            assert_eq!(boom.direction(), None);
            let mut s = step(&w, 0.);
            s.exclude = Some(1);
            s.direction = direction;
            s.seconds = 10.;
            boom.update(s);
            assert!((boom.distance(10.) - 10.).abs() < 1e-4);
        }
    }
    #[test]
    fn orbit_contract_rejects_bad_limits_and_unknown_fields() {
        assert!(settings().valid());
        assert!(!OrbitCamera {
            min_distance: 30.,
            ..settings()
        }
        .valid());
        assert!(!OrbitCamera {
            clearance: f32::NAN,
            ..settings()
        }
        .valid());
        assert!(serde_json::from_str::<OrbitCamera>(r#"{"min_distance":1,"max_distance":20,"clearance":0.2,"response_seconds":0.2,"lookahead_seconds":0.25,"extra":0}"#).is_err());
    }
}
impl OrbitCamera {
    pub fn valid(self) -> bool {
        [
            self.min_distance,
            self.max_distance,
            self.clearance,
            self.response_seconds,
            self.lookahead_seconds,
        ]
        .iter()
        .all(|v| v.is_finite())
            && (0.2..=100.).contains(&self.min_distance)
            && (self.min_distance..=200.).contains(&self.max_distance)
            && (0.02..=1.).contains(&self.clearance)
            && (0.01..=2.).contains(&self.response_seconds)
            && (0. ..=0.5).contains(&self.lookahead_seconds)
            && self.avoidance.is_none_or(|a| a.valid())
            && self
                .target_height_fraction
                .is_none_or(|h| h.is_finite() && (0.1..=0.9).contains(&h))
    }
}

#[derive(Clone, Debug)]
pub struct CameraBoom {
    pub settings: OrbitCamera,
    distance: Option<f32>,
    steering: Option<crate::camera_steering::Steering>,
}
pub struct BoomStep<'a> {
    pub world: &'a dyn WorldView,
    pub pivot: Vec3,
    pub direction: Vec3,
    pub requested: f32,
    pub velocity: Vec3,
    pub exclude: Option<u64>,
    pub seconds: f32,
    pub near_radius: f32,
}
impl CameraBoom {
    pub fn collision_radius(&self, near_radius: f32) -> f32 {
        self.settings.clearance.max(near_radius)
    }
    pub fn place_eye(&mut self, distance: f32) {
        self.distance = Some(distance);
    }
    pub fn new(settings: OrbitCamera) -> Self {
        Self {
            steering: settings
                .avoidance
                .map(crate::camera_steering::Steering::new),
            settings,
            distance: None,
        }
    }
    pub fn distance(&self, requested: f32) -> f32 {
        self.distance.unwrap_or(requested).min(requested)
    }
    pub fn direction(&self) -> Option<Vec3> {
        self.steering.as_ref().and_then(|s| s.direction)
    }
    pub fn manual_orbit(&mut self) -> Option<Vec3> {
        if let Some(s) = &mut self.steering {
            let rebase = if s.manual_active() { None } else { s.direction };
            s.manual_orbit();
            return rebase;
        }
        None
    }
    pub fn update(&mut self, step: BoomStep<'_>) -> bool {
        let BoomStep {
            world,
            pivot,
            direction,
            requested,
            velocity,
            exclude,
            seconds,
            near_radius,
        } = step;
        if !seconds.is_finite() || seconds < 0. {
            return false;
        }
        let radius = self.collision_radius(near_radius);
        let limit = |start, direction: Vec3| {
            io_world::cast_sphere(
                world,
                start,
                start + direction.scaled(requested),
                radius,
                exclude,
            )
            .map_or(0., |t| {
                if t == 1. {
                    requested
                } else {
                    (t * requested - 0.005).max(0.)
                }
            })
        };
        let predicted_pivot = pivot + velocity.scaled(self.settings.lookahead_seconds);
        // Do not anticipate an impossible player position through a wall.
        let predict = seconds > 0.
            && (predicted_pivot - pivot).dot(predicted_pivot - pivot) > 1e-8
            && io_world::cast_sphere(world, pivot, predicted_pivot, radius, exclude)
                .is_ok_and(|t| t == 1.);
        let usable = |d: Vec3| {
            let safe = limit(pivot, d);
            if predict {
                safe.min(limit(predicted_pivot, d))
            } else {
                safe
            }
        };
        let old_direction = self.direction();
        let direction = match &mut self.steering {
            Some(s) => s.update(direction, seconds, self.settings.response_seconds, |d| {
                usable(d) / requested
            }),
            None => direction,
        };
        let safe = limit(pivot, direction);
        let predicted = usable(direction);
        let goal = safe.min(predicted).min(requested);
        let old = self.distance.unwrap_or(requested);
        // A normalized exponential step is stable across presentation rates. Clamp
        // after easing: smoothing must never keep the eye on the far side of a wall.
        let next = (old + (goal - old) * -(-seconds / self.settings.response_seconds).exp_m1())
            .min(safe)
            .min(requested)
            .max(0.);
        self.distance = Some(next);
        (next - old).abs() > 1e-6 || old_direction != self.direction()
    }
}
