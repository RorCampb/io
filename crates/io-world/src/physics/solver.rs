use super::{effective_mass, pair_mut, warm_start, Body, Constraint};
use io_types::Vec3;

/// Constants valid only while this substep's body poses remain unchanged.
pub(super) struct PreparedContact {
    pub(super) ra: Vec3,
    pub(super) rb: Vec3,
    pub(super) normal_response: f32,
}

pub(super) fn prepare(
    bodies: &[Body],
    contacts: &[Constraint],
    prepared: &mut Vec<PreparedContact>,
) {
    prepared.clear();
    prepared.extend(contacts.iter().map(|c| {
        let (a, b) = (&bodies[c.a.index()], &bodies[c.b.index()]);
        let ra = c.point - a.center;
        let rb = c.point - b.center;
        PreparedContact {
            ra,
            rb,
            normal_response: effective_mass(a, b, ra, rb, c.normal),
        }
    }));
}

pub(super) fn solve(
    bodies: &mut [Body],
    contacts: &mut [Constraint],
    prepared: &[PreparedContact],
    iterations: u32,
) {
    assert_eq!(contacts.len(), prepared.len());
    for _ in 0..iterations {
        for (c, p) in contacts.iter_mut().zip(prepared) {
            let (a, b) = pair_mut(bodies, c.a.index(), c.b.index());
            solve_contact(a, b, c, p);
        }
    }
}

pub(super) fn solve_contact(a: &mut Body, b: &mut Body, c: &mut Constraint, p: &PreparedContact) {
    if p.normal_response <= 1e-10 {
        return;
    }
    let velocity = b.velocity + b.omega.cross(p.rb) - a.velocity - a.omega.cross(p.ra);
    let impulse =
        ((c.bias - velocity.dot(c.normal)) / p.normal_response + c.normal_impulse).max(0.);
    let delta = c.normal.scaled(impulse - c.normal_impulse);
    c.normal_impulse = impulse;
    a.impulse(delta.scaled(-1.), p.ra);
    b.impulse(delta, p.rb);

    let velocity = b.velocity + b.omega.cross(p.rb) - a.velocity - a.omega.cross(p.ra);
    let tangent = velocity - c.normal.scaled(velocity.dot(c.normal));
    let length = tangent.dot(tangent).sqrt();
    let mut proposed = c.tangent_impulse;
    if length > 1e-6 {
        let direction = tangent.scaled(1. / length);
        // Slip direction changes between passes, unlike the normal.
        let k = effective_mass(a, b, p.ra, p.rb, direction);
        proposed = proposed - direction.scaled(length / k.max(1e-10));
    }
    let next = warm_start::clamp_friction(proposed, c.normal, c.friction * c.normal_impulse);
    let delta = next - c.tangent_impulse;
    c.tangent_impulse = next;
    a.impulse(delta.scaled(-1.), p.ra);
    b.impulse(delta, p.rb);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{effective_mass, BodyIndex};
    use crate::{BodyKind, Collider, ColliderShape, Item, PhysicsBody, Transform};
    use io_types::Rotation;

    fn body(index: usize, kind: BodyKind, seed: f32) -> Body {
        let mut transform = Transform::new(
            Vec3::new(seed.sin(), seed.cos(), index as f32),
            Vec3::new(1., 1., 1.),
            0.,
        )
        .unwrap();
        transform.rotation = Rotation::from_xyzw([seed.sin(), 0.3, seed.cos(), 1.]).unwrap();
        let mut physics = PhysicsBody::new(kind);
        physics.mass = 0.5 + seed.abs();
        if kind != BodyKind::Static {
            physics.velocity = Vec3::new(seed.cos(), -seed.sin(), -1.);
            physics.angular_velocity = Vec3::new(0.1, seed.sin(), 0.2);
        }
        Body::from_item(
            index,
            &Item {
                id: index as u64 + 1,
                transform,
                physics_body: Some(physics),
                collider: Some(Collider::new(ColliderShape::Box {
                    half_extents: Vec3::new(0.2, 0.5, 0.9),
                })),
                ..Item::default()
            },
        )
    }

    fn contact(seed: f32) -> Constraint {
        let n = Vec3::new(seed.sin(), seed.cos(), 1.);
        Constraint {
            a: BodyIndex::try_from(0).unwrap(),
            b: BodyIndex::try_from(1).unwrap(),
            point: Vec3::new(0.3, -0.2, 0.5),
            normal: n.scaled(1. / n.dot(n).sqrt()),
            bias: 0.1,
            friction: 0.6,
            normal_impulse: 0.,
            tangent_impulse: Vec3::default(),
        }
    }

    // The pre-preparation scalar solver is kept as an independent regression oracle.
    fn scalar_solve(bodies: &mut [Body], contacts: &mut [Constraint], iterations: u32) {
        for _ in 0..iterations {
            for c in contacts.iter_mut() {
                let (a, b) = pair_mut(bodies, c.a.index(), c.b.index());
                let ra = c.point - a.center;
                let rb = c.point - b.center;
                let velocity = b.velocity + b.omega.cross(rb) - a.velocity - a.omega.cross(ra);
                let k = effective_mass(a, b, ra, rb, c.normal);
                if k <= 1e-10 {
                    continue;
                }
                let impulse = ((c.bias - velocity.dot(c.normal)) / k + c.normal_impulse).max(0.);
                let delta = c.normal.scaled(impulse - c.normal_impulse);
                c.normal_impulse = impulse;
                a.impulse(delta.scaled(-1.), ra);
                b.impulse(delta, rb);
                let velocity = b.velocity + b.omega.cross(rb) - a.velocity - a.omega.cross(ra);
                let tangent = velocity - c.normal.scaled(velocity.dot(c.normal));
                let length = tangent.dot(tangent).sqrt();
                let mut proposed = c.tangent_impulse;
                if length > 1e-6 {
                    let direction = tangent.scaled(1. / length);
                    let kt = effective_mass(a, b, ra, rb, direction);
                    proposed = proposed - direction.scaled(length / kt.max(1e-10));
                }
                // Normal corrections can shrink the cone even at zero slip.
                let next =
                    warm_start::clamp_friction(proposed, c.normal, c.friction * c.normal_impulse);
                let delta = next - c.tangent_impulse;
                c.tangent_impulse = next;
                a.impulse(delta.scaled(-1.), ra);
                b.impulse(delta, rb);
            }
        }
    }

    fn close(a: f32, b: f32) {
        assert_eq!(a, b, "prepared versus scalar");
    }

    fn close_vector(a: Vec3, b: Vec3) {
        close(a.x, b.x);
        close(a.y, b.y);
        close(a.z, b.z);
    }

    #[test]
    fn isotropic_inertia_is_exact_and_independent_of_orientation() {
        for i in 0..32 {
            let mut body = body(0, BodyKind::Dynamic, i as f32 * 0.27);
            for scale in [0., 0.1, 6., 25.] {
                body.inverse_inertia = Vec3::new(scale, scale, scale);
                let v = Vec3::new(-0.3, 0.7, 1.2);
                assert_eq!(body.inertia(v), v.scaled(scale));
            }
        }
    }

    #[test]
    fn prepared_normals_match_scalar_mass_for_rotated_anisotropic_bodies() {
        for kind in [BodyKind::Static, BodyKind::Dynamic, BodyKind::Kinematic] {
            for i in 0..32 {
                let seed = i as f32 * 0.27;
                let bodies = [body(0, kind, seed), body(1, BodyKind::Dynamic, seed + 0.7)];
                let c = contact(seed);
                let mut prepared = Vec::new();
                prepare(&bodies, &[c.clone()], &mut prepared);
                let p = &prepared[0];
                assert_eq!(
                    p.normal_response,
                    effective_mass(&bodies[0], &bodies[1], p.ra, p.rb, c.normal)
                );
                for j in 0..12 {
                    let angle = j as f32 * 0.61;
                    let v = Vec3::new(angle.cos(), angle.sin(), 0.3);
                    let n = v.scaled(1. / v.dot(v).sqrt());
                    let mut c = c.clone();
                    c.normal = n;
                    prepare(&bodies, &[c], &mut prepared);
                    let p = &prepared[0];
                    assert!(p.normal_response > 0.);
                    assert_eq!(
                        p.normal_response,
                        effective_mass(&bodies[0], &bodies[1], p.ra, p.rb, n)
                    );
                }
            }
        }
    }

    #[test]
    fn prepared_passes_match_scalar_solver_including_warm_impulses_and_friction() {
        for kind in [BodyKind::Static, BodyKind::Dynamic, BodyKind::Kinematic] {
            for i in 0..32 {
                let seed = i as f32 * 0.27;
                let initial = vec![body(0, kind, seed), body(1, BodyKind::Dynamic, seed + 0.7)];
                for friction in [0., 0.6, 2.] {
                    let mut contacts: Vec<_> = (0..4)
                        .map(|j| {
                            let mut c = contact(seed);
                            c.point = c.point + Vec3::new(j as f32 * 0.1, 0.2, 0.);
                            c.friction = friction;
                            c.normal_impulse = 0.15;
                            c.tangent_impulse = c.normal.cross(Vec3::new(0.03, 0.02, 0.01));
                            c
                        })
                        .collect();
                    let mut actual = initial.clone();
                    for c in &contacts {
                        let (a, b) = pair_mut(&mut actual, c.a.index(), c.b.index());
                        let impulse = c.normal.scaled(c.normal_impulse) + c.tangent_impulse;
                        a.impulse(impulse.scaled(-1.), c.point - a.center);
                        b.impulse(impulse, c.point - b.center);
                    }
                    let mut expected = actual.clone();
                    let mut reference = contacts.clone();
                    let mut prepared = Vec::new();
                    prepare(&actual, &contacts, &mut prepared);
                    solve(&mut actual, &mut contacts, &prepared, 12);
                    scalar_solve(&mut expected, &mut reference, 12);
                    for (a, b) in actual.iter().zip(&expected) {
                        close_vector(a.velocity, b.velocity);
                        close_vector(a.omega, b.omega);
                        assert_eq!(a.center, b.center);
                        assert_eq!(a.rotation, b.rotation);
                    }
                    for (a, b) in contacts.iter().zip(&reference) {
                        close(a.normal_impulse, b.normal_impulse);
                        close_vector(a.tangent_impulse, b.tangent_impulse);
                        assert!(a.normal_impulse >= 0.);
                        assert!(
                            a.tangent_impulse.dot(a.tangent_impulse).sqrt()
                                <= a.friction * a.normal_impulse + 1e-5
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn prepared_storage_is_reused_but_pose_dependent_values_are_refreshed() {
        let mut bodies = [
            body(0, BodyKind::Static, 0.),
            body(1, BodyKind::Dynamic, 1.),
        ];
        let contacts = vec![contact(0.); 8];
        let mut prepared = Vec::new();
        prepare(&bodies, &contacts, &mut prepared);
        let capacity = prepared.capacity();
        let old_arm = prepared[0].rb;
        prepare(&bodies, &[], &mut prepared);
        assert!(prepared.is_empty());
        assert_eq!(prepared.capacity(), capacity);
        bodies[1].center = bodies[1].center + Vec3::new(0.1, 0.2, 0.3);
        bodies[1].rotation = Rotation::yaw(0.7).unwrap();
        prepare(&bodies, &contacts[..1], &mut prepared);
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared.capacity(), capacity);
        assert_ne!(prepared[0].rb, old_arm);
        assert_eq!(prepared[0].rb, contacts[0].point - bodies[1].center);
        close(
            prepared[0].normal_response,
            effective_mass(
                &bodies[0],
                &bodies[1],
                prepared[0].ra,
                prepared[0].rb,
                contacts[0].normal,
            ),
        );
    }

    #[test]
    fn immovable_pair_has_zero_response_and_does_not_change_velocities() {
        let mut bodies = [
            body(0, BodyKind::Static, 0.),
            body(1, BodyKind::Kinematic, 1.),
        ];
        let initial = bodies.clone();
        let mut contacts = [contact(0.)];
        let mut prepared = Vec::new();
        prepare(&bodies, &contacts, &mut prepared);
        assert_eq!(prepared[0].normal_response, 0.);
        solve(&mut bodies, &mut contacts, &prepared, 12);
        for (a, b) in bodies.iter().zip(initial) {
            assert_eq!(a.velocity, b.velocity);
            assert_eq!(a.omega, b.omega);
        }
    }

    #[test]
    fn diagnostics_replay_is_observational_and_detects_neighbor_reactivation() {
        use crate::physics::diagnostics::{self, ContactDiagnostics};
        let mut initial: Vec<_> = (0..3)
            .map(|i| {
                let kind = if i == 0 {
                    BodyKind::Static
                } else {
                    BodyKind::Dynamic
                };
                let mut b = body(i, kind, 1.);
                b.center = Vec3::new(0., 0., i as f32);
                b.velocity = Vec3::default();
                b.omega = Vec3::default();
                b.inverse_mass = if i == 0 { 0. } else { 1. };
                b
            })
            .collect();
        initial[2].velocity.z = -1.;
        let contacts: Vec<_> = (0..2)
            .map(|i| {
                let mut c = contact(0.);
                c.a = BodyIndex::try_from(i).unwrap();
                c.b = BodyIndex::try_from(i + 1).unwrap();
                c.point = Vec3::new(0., 0., i as f32 + 0.5);
                c.normal = Vec3::new(0., 0., 1.);
                c.bias = 0.;
                c
            })
            .collect();
        let mut prepared = Vec::new();
        prepare(&initial, &contacts, &mut prepared);
        let mut actual = initial.clone();
        let mut expected = initial;
        let mut actual_contacts = contacts.clone();
        let mut expected_contacts = contacts;
        let mut report = ContactDiagnostics::default();
        report.begin_tick(2);
        diagnostics::solve(&mut actual, &mut actual_contacts, &prepared, &mut report);
        solve(&mut expected, &mut expected_contacts, &prepared, 2);
        for (a, b) in actual.iter().zip(&expected) {
            assert_eq!(a.velocity, b.velocity);
            assert_eq!(a.omega, b.omega);
            assert_eq!(a.center, b.center);
            assert_eq!(a.rotation, b.rotation);
        }
        for (a, b) in actual_contacts.iter().zip(&expected_contacts) {
            assert_eq!(a.normal_impulse, b.normal_impulse);
            assert_eq!(a.tangent_impulse, b.tangent_impulse);
        }
        assert!(report.passes[0].small_but_unsettled > 0);
        assert!(report.passes[1].small_then_large > 0);
        for p in &report.passes {
            assert_eq!(p.visits, 2);
            assert_eq!(p.correction_histogram.iter().sum::<u64>(), p.visits);
            assert_eq!(p.end_pass_histogram.iter().sum::<u64>(), p.visits);
            assert_eq!(p.invalid_samples, 0);
        }
        report.begin_tick(1);
        assert_eq!(report.passes.len(), 1);
        assert_eq!(report.passes[0].visits, 0);
    }

    #[test]
    fn diagnostics_do_not_label_cached_support_as_an_absent_contact() {
        use crate::physics::diagnostics::{self, ContactDiagnostics};
        let mut bodies = [
            body(0, BodyKind::Static, 0.),
            body(1, BodyKind::Dynamic, 1.),
        ];
        for b in &mut bodies {
            b.velocity = Vec3::default();
            b.omega = Vec3::default();
        }
        let mut c = contact(0.);
        c.bias = 0.;
        c.normal_impulse = 1.;
        let mut contacts = [c];
        let mut prepared = Vec::new();
        prepare(&bodies, &contacts, &mut prepared);
        let mut report = ContactDiagnostics::default();
        report.begin_tick(2);
        diagnostics::solve(&mut bodies, &mut contacts, &prepared, &mut report);
        for p in report.passes {
            assert_eq!(p.correction_histogram, [1, 0, 0, 0, 0, 0]);
            assert_eq!(p.end_pass_histogram, [1, 0, 0, 0, 0, 0]);
            assert_eq!(p.supporting_small, 1);
            assert_eq!(p.small_but_unsettled, 0);
        }
        assert_eq!(contacts[0].normal_impulse, 1.);
    }

    #[test]
    fn compact_records_keep_f32_precision_and_checked_indices() {
        use std::mem::size_of;
        assert_eq!(size_of::<BodyIndex>(), 4);
        assert_eq!(size_of::<Vec3>(), 12);
        assert_eq!(size_of::<Constraint>(), 56);
        assert_eq!(size_of::<PreparedContact>(), 28);
        assert_eq!(BodyIndex::try_from(0).unwrap().index(), 0);
        let max = u32::MAX as usize;
        assert_eq!(BodyIndex::try_from(max).unwrap().index(), max);
        if let Some(overflow) = max.checked_add(1) {
            assert!(BodyIndex::try_from(overflow).is_err());
        }
    }
}
