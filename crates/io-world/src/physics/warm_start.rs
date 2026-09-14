use super::{pair_mut, Body, ColliderShape, Constraint};
use io_types::Vec3;
use std::collections::HashMap;

#[derive(Clone)]
struct CachedContact {
    local_a: Vec3,
    local_b: Vec3,
    normal: Vec3,
    normal_impulse: f32,
    tangent_impulse: Vec3,
}

#[derive(Default)]
pub(super) struct Cache {
    pairs: HashMap<(u64, u64), Vec<CachedContact>>,
    substep_seconds: f32,
}

fn radius(shape: ColliderShape) -> f32 {
    match shape {
        ColliderShape::Sphere { radius } => radius,
        ColliderShape::Box { half_extents: h } => h.x.min(h.y).min(h.z),
    }
}

pub(super) fn clamp_friction(impulse: Vec3, normal: Vec3, limit: f32) -> Vec3 {
    let tangent = impulse - normal.scaled(impulse.dot(normal));
    let length = tangent.dot(tangent).sqrt();
    if length > limit {
        tangent.scaled(limit / length)
    } else {
        tangent
    }
}

impl Cache {
    pub fn clear(&mut self) {
        self.pairs.clear();
    }

    pub fn invalidate_body(&mut self, id: u64) {
        self.pairs.retain(|&(a, b), _| a != id && b != id);
    }

    pub fn restore(&mut self, bodies: &mut [Body], contacts: &mut [Constraint], h: f32) -> usize {
        let hits = self.seed(bodies, contacts, h);
        for contact in contacts {
            if contact.normal_impulse > 0. {
                let (a, b) = pair_mut(bodies, contact.a.index(), contact.b.index());
                let impulse =
                    contact.normal.scaled(contact.normal_impulse) + contact.tangent_impulse;
                a.impulse(impulse.scaled(-1.), contact.point - a.center);
                b.impulse(impulse, contact.point - b.center);
            }
        }
        hits
    }

    /// Seeds freshly zeroed constraints without applying impulses to velocities.
    pub fn seed(&mut self, bodies: &[Body], contacts: &mut [Constraint], h: f32) -> usize {
        // Conservative timestep policy: do not reuse guesses from a different h.
        if self.substep_seconds != h {
            self.clear();
        }
        let mut hits = 0;
        for contact in contacts {
            let (a, b) = (&bodies[contact.a.index()], &bodies[contact.b.index()]);
            let Some(previous) = self.pairs.get_mut(&(a.id, b.id)) else {
                continue;
            };
            let local_a = a.rotation.inverse_rotate(contact.point - a.center);
            let local_b = b.rotation.inverse_rotate(contact.point - b.center);
            let tolerance =
                (0.1 * radius(a.collider.shape).min(radius(b.collider.shape))).min(0.02);
            let matched = previous
                .iter()
                .enumerate()
                .filter_map(|(i, old)| {
                    let da = old.local_a - local_a;
                    let db = old.local_b - local_b;
                    let da = da.dot(da);
                    let db = db.dot(db);
                    (old.normal.dot(contact.normal) >= 0.98
                        && da <= tolerance * tolerance
                        && db <= tolerance * tolerance)
                        .then_some((i, da + db))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let Some((index, _)) = matched else {
                continue;
            };
            // Consume each match once: a manifold point cannot seed two contacts.
            let old = previous.remove(index);
            contact.normal_impulse = old.normal_impulse;
            contact.tangent_impulse = clamp_friction(
                old.tangent_impulse,
                contact.normal,
                contact.friction * old.normal_impulse,
            );
            hits += 1;
        }
        hits
    }

    pub fn replace(&mut self, bodies: &[Body], contacts: &[Constraint], h: f32) -> usize {
        // Only the immediately preceding substep survives. The sleeping/support
        // graph is separate and must not expire when this cache does.
        self.clear();
        self.substep_seconds = h;
        let mut count = 0;
        for contact in contacts {
            if contact.normal_impulse <= 0.
                || !contact.normal_impulse.is_finite()
                || !contact.tangent_impulse.finite()
            {
                continue;
            }
            let (a, b) = (&bodies[contact.a.index()], &bodies[contact.b.index()]);
            self.pairs
                .entry((a.id, b.id))
                .or_default()
                .push(CachedContact {
                    local_a: a.rotation.inverse_rotate(contact.point - a.center),
                    local_b: b.rotation.inverse_rotate(contact.point - b.center),
                    normal: contact.normal,
                    normal_impulse: contact.normal_impulse,
                    tangent_impulse: contact.tangent_impulse,
                });
            count += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BodyKind, Collider, Item, PhysicsBody, Transform};

    fn bodies() -> Vec<Body> {
        (0..2)
            .map(|i| {
                Body::from_item(
                    i,
                    &Item {
                        id: i as u64 + 1,
                        transform: Transform::new(
                            Vec3::new(0., 0., i as f32 * 2.),
                            Vec3::new(1., 1., 1.),
                            0.,
                        )
                        .unwrap(),
                        physics_body: Some(PhysicsBody::new(BodyKind::Dynamic)),
                        collider: Some(Collider::new(ColliderShape::Sphere { radius: 1. })),
                        ..Item::default()
                    },
                )
            })
            .collect()
    }
    fn contact() -> Constraint {
        Constraint {
            a: super::super::BodyIndex::try_from(0).unwrap(),
            b: super::super::BodyIndex::try_from(1).unwrap(),
            point: Vec3::new(0., 0., 1.),
            normal: Vec3::new(0., 0., 1.),
            bias: 0.,
            friction: 0.5,
            normal_impulse: 0.,
            tangent_impulse: Vec3::default(),
        }
    }
    fn cache(bodies: &[Body]) -> Cache {
        let mut cache = Cache::default();
        let mut old = contact();
        old.normal_impulse = 2.;
        old.tangent_impulse = Vec3::new(0.5, 0., 0.);
        assert_eq!(cache.replace(bodies, &[old], 0.01), 1);
        cache
    }

    #[test]
    fn matches_are_one_to_one_and_apply_impulses_not_just_accumulators() {
        let mut bodies = bodies();
        let mut cache = cache(&bodies);
        let mut contacts = [contact(), contact()];
        contacts[0].friction = 0.1;
        assert_eq!(cache.restore(&mut bodies, &mut contacts, 0.01), 1);
        assert_eq!(contacts[0].normal_impulse, 2.);
        assert_eq!(contacts[1].normal_impulse, 0.);
        assert!((contacts[0].tangent_impulse.x - 0.2).abs() < 1e-6);
        assert_eq!(bodies[0].velocity.z, -2.);
        assert_eq!(bodies[1].velocity.z, 2.);
        assert!((bodies[0].velocity.x + bodies[1].velocity.x).abs() < 1e-6);
        assert!(bodies[0].omega.dot(bodies[0].omega) > 0.);
    }

    #[test]
    fn changed_geometry_ids_and_timesteps_cannot_reuse_stale_contacts() {
        for case in 0..4 {
            let mut bodies = bodies();
            let mut cache = cache(&bodies);
            let mut current = contact();
            let mut h = 0.01;
            match case {
                0 => current.point.x = 0.1,
                1 => current.normal = Vec3::new(1., 0., 0.),
                2 => bodies[1].id = 99,
                3 => h = 0.02,
                _ => unreachable!(),
            }
            assert_eq!(cache.restore(&mut bodies, &mut [current], h), 0);
            assert_eq!(bodies[0].velocity, Vec3::default());
        }
    }

    #[test]
    fn contacts_expire_after_one_missing_substep_and_explicit_invalidation() {
        let mut bodies = bodies();
        let mut cache = cache(&bodies);
        assert_eq!(cache.replace(&bodies, &[], 0.01), 0);
        assert_eq!(cache.restore(&mut bodies, &mut [contact()], 0.01), 0);
        let mut old = contact();
        old.normal_impulse = 2.;
        cache.replace(&bodies, &[old], 0.01);
        cache.invalidate_body(1);
        assert_eq!(cache.restore(&mut bodies, &mut [contact()], 0.01), 0);
    }

    #[test]
    fn normal_solver_can_back_out_an_excessive_warm_guess() {
        let mut bodies = bodies();
        let mut cache = cache(&bodies);
        let mut contacts = [contact()];
        contacts[0].friction = 0.;
        cache.restore(&mut bodies, &mut contacts, 0.01);
        let c = &mut contacts[0];
        let (a, b) = pair_mut(&mut bodies, c.a.index(), c.b.index());
        let ra = c.point - a.center;
        let rb = c.point - b.center;
        let velocity = b.velocity + b.omega.cross(rb) - a.velocity - a.omega.cross(ra);
        let k = super::super::effective_mass(a, b, ra, rb, c.normal);
        let next = (c.normal_impulse - velocity.dot(c.normal) / k).max(0.);
        let delta = c.normal.scaled(next - c.normal_impulse);
        a.impulse(delta.scaled(-1.), ra);
        b.impulse(delta, rb);
        assert_eq!(next, 0.);
        assert_eq!(a.velocity, Vec3::default());
        assert_eq!(b.velocity, Vec3::default());
        assert_eq!(
            clamp_friction(Vec3::new(2., 0., 1.), c.normal, 0.),
            Vec3::default()
        );
    }
}
