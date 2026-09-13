//! Limited in-house rigid-body solver. No mesh, camera, or renderer dependency.
mod collision;
use crate::Item;
use io_types::{Bounds, Rotation, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind { Static, Dynamic, Kinematic }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShape { Box { half_extents: Vec3 }, Sphere { radius: f32 } }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    pub shape: ColliderShape,
    /// Physical local units, independent of render scale. Also the center of mass.
    pub offset: Vec3,
    pub friction: f32,
    pub restitution: f32,
    pub memberships: u32,
    pub filter: u32,
}
impl Collider {
    pub fn new(shape: ColliderShape) -> Self {
        Self { shape, offset: Vec3::default(), friction: 0.6, restitution: 0., memberships: 1, filter: u32::MAX }
    }
    pub fn validate(&self) -> Result<(), String> {
        let dimensions = match self.shape {
            ColliderShape::Box { half_extents } => [half_extents.x, half_extents.y, half_extents.z],
            ColliderShape::Sphere { radius } => [radius; 3],
        };
        if dimensions.iter().any(|v| !v.is_finite() || !(0.01..=10000.).contains(v))
            || !bounded(self.offset, 10000.) || !self.friction.is_finite() || !(0.0..=2.).contains(&self.friction)
            || !self.restitution.is_finite() || !(0.0..=1.).contains(&self.restitution)
        { return Err("invalid collider dimensions, offset, friction, or restitution".into()); }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsBody {
    pub kind: BodyKind,
    pub mass: f32,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub(crate) target: Option<(Vec3, Rotation)>,
}
impl PhysicsBody {
    pub fn new(kind: BodyKind) -> Self {
        Self { kind, mass: 1., gravity_scale: 1., linear_damping: 0., angular_damping: 0.05,
            velocity: Vec3::default(), angular_velocity: Vec3::default(), target: None }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.mass.is_finite() || !(0.01..=1e6).contains(&self.mass)
            || !self.gravity_scale.is_finite() || !(-10.0..=10.).contains(&self.gravity_scale)
            || [self.linear_damping, self.angular_damping].iter().any(|v| !v.is_finite() || !(0.0..=100.).contains(v))
            || !bounded(self.velocity, 10000.) || !bounded(self.angular_velocity, 1000.)
        { return Err("invalid body mass, gravity multiplier, damping, or velocity".into()); }
        if self.kind == BodyKind::Static && (self.velocity != Vec3::default() || self.angular_velocity != Vec3::default()) {
            return Err("static bodies cannot have velocity".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsSettings {
    pub gravity: Vec3,
    pub substeps: u32,
    pub iterations: u32,
}
impl Default for PhysicsSettings {
    fn default() -> Self { Self { gravity: Vec3::new(0., 0., -9.81), substeps: 4, iterations: 12 } }
}
impl PhysicsSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !bounded(self.gravity, 1000.) || !(1..=32).contains(&self.substeps) || !(1..=64).contains(&self.iterations) {
            return Err("physics requires finite gravity within +/-1000, 1..32 substeps, and 1..64 iterations".into());
        }
        Ok(())
    }
}
pub(crate) fn bounded(v: Vec3, limit: f32) -> bool {
    v.finite() && v.x.abs() <= limit && v.y.abs() <= limit && v.z.abs() <= limit
}

/// One solved contact for this world tick. Normal points from a to b.
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    pub a: u64, pub b: u64, pub point: Vec3, pub normal: Vec3, pub impulse: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsStats {
    pub bodies: usize, pub moving: usize, pub candidate_pairs: usize, pub contacts: usize,
    pub step_seconds: f64,
}

#[derive(Clone)]
struct Body {
    item: usize, id: u64, center: Vec3, rotation: Rotation, collider: Collider,
    inverse_mass: f32, inverse_inertia: Vec3, velocity: Vec3, omega: Vec3,
    kind: BodyKind, gravity_scale: f32, damping: f32, angular_damping: f32,
}
impl Body {
    fn from_item(index: usize, item: &Item) -> Self {
        let p = item.physics_body.as_ref().expect("validated body index");
        let collider = item.collider.expect("validated body requires collider");
        let inverse_mass = if p.kind == BodyKind::Dynamic { 1. / p.mass } else { 0. };
        let inverse_inertia = match collider.shape {
            ColliderShape::Sphere { radius } => {
                let i = 2.5 * inverse_mass / (radius * radius);
                Vec3::new(i, i, i)
            }
            ColliderShape::Box { half_extents: h } => Vec3::new(
                3. * inverse_mass / (h.y*h.y + h.z*h.z),
                3. * inverse_mass / (h.x*h.x + h.z*h.z),
                3. * inverse_mass / (h.x*h.x + h.y*h.y)),
        };
        Self { item: index, id: item.id, center: item.transform.anchor + item.transform.rotation.rotate(collider.offset),
            rotation: item.transform.rotation, collider, inverse_mass, inverse_inertia,
            velocity: p.velocity, omega: p.angular_velocity, kind: p.kind, gravity_scale: p.gravity_scale,
            damping: p.linear_damping, angular_damping: p.angular_damping }
    }
    fn inertia(&self, v: Vec3) -> Vec3 {
        let v = self.rotation.inverse_rotate(v);
        self.rotation.rotate(Vec3::new(v.x*self.inverse_inertia.x, v.y*self.inverse_inertia.y, v.z*self.inverse_inertia.z))
    }
    fn impulse(&mut self, impulse: Vec3, arm: Vec3) {
        self.velocity = self.velocity + impulse.scaled(self.inverse_mass);
        self.omega = self.omega + self.inertia(arm.cross(impulse));
    }
    fn bounds(&self) -> Bounds {
        let ext = match self.collider.shape {
            ColliderShape::Sphere { radius } => Vec3::new(radius, radius, radius),
            ColliderShape::Box { half_extents: h } => {
                let x = self.rotation.rotate(Vec3::new(h.x, 0., 0.));
                let y = self.rotation.rotate(Vec3::new(0., h.y, 0.));
                let z = self.rotation.rotate(Vec3::new(0., 0., h.z));
                Vec3::new(x.x.abs()+y.x.abs()+z.x.abs(), x.y.abs()+y.y.abs()+z.y.abs(), x.z.abs()+y.z.abs()+z.z.abs())
            }
        };
        Bounds { min: self.center - ext, max: self.center + ext }
    }
}

pub(crate) fn apply_impulse(item: &mut Item, impulse: Vec3, point: Vec3) -> bool {
    if !bounded(impulse, 1e7) || !bounded(point, 1e6)
        || !item.physics_body.as_ref().is_some_and(|b| b.kind == BodyKind::Dynamic) { return false; }
    let mut body = Body::from_item(0, item);
    body.impulse(impulse, point - body.center);
    if !bounded(body.velocity, 10000.) || !bounded(body.omega, 1000.) { return false; }
    let target = item.physics_body.as_mut().unwrap();
    target.velocity = body.velocity;
    target.angular_velocity = body.omega;
    true
}

struct Constraint { a: usize, b: usize, point: Vec3, normal: Vec3, bias: f32, friction: f32,
    normal_impulse: f32, tangent_impulse: Vec3 }

/// Solves against scratch state, committing only finite, supported results.
pub(crate) fn step(items: &mut [Item], indices: &[usize], settings: PhysicsSettings, dt: f32)
    -> Result<(PhysicsStats, Vec<ContactEvent>), String>
{
    let start = std::time::Instant::now();
    let mut bodies: Vec<_> = indices.iter().map(|&i| Body::from_item(i, &items[i])).collect();
    let initial = bodies.clone();
    let mut stats = PhysicsStats { bodies: bodies.len(), moving: bodies.iter().filter(|b| b.kind != BodyKind::Static).count(), ..PhysicsStats::default() };
    let mut events = Vec::new();
    let h = dt / settings.substeps as f32;
    for substep in 0..settings.substeps {
        for (i, body) in bodies.iter_mut().enumerate() {
            match body.kind {
                BodyKind::Static => {}
                BodyKind::Dynamic => {
                    body.velocity = (body.velocity + settings.gravity.scaled(body.gravity_scale*h)).scaled(1. / (1. + body.damping*h));
                    body.omega = body.omega.scaled(1. / (1. + body.angular_damping*h));
                    body.center = body.center + body.velocity.scaled(h);
                    body.rotation = body.rotation.integrate(body.omega, h)?;
                }
                BodyKind::Kinematic => {
                    if let Some((anchor, rotation)) = items[body.item].physics_body.as_ref().unwrap().target {
                        let center = anchor + rotation.rotate(body.collider.offset);
                        body.velocity = (center - initial[i].center).scaled(1. / dt);
                        body.omega = initial[i].rotation.angular_velocity_to(rotation, dt);
                        let alpha = (substep + 1) as f32 / settings.substeps as f32;
                        body.center = initial[i].center.scaled(1.-alpha) + center.scaled(alpha);
                        body.rotation = initial[i].rotation.interpolate(rotation, alpha);
                    } else { body.velocity = Vec3::default(); body.omega = Vec3::default(); }
                }
            }
        }
        let mut bounds: Vec<_> = bodies.iter().enumerate().map(|(i,b)| (i,b.bounds())).collect();
        bounds.sort_by(|a,b| a.1.min.x.total_cmp(&b.1.min.x).then(a.0.cmp(&b.0)));
        let mut constraints = Vec::new();
        for x in 0..bounds.len() {
            let (a, ba) = bounds[x];
            for &(b, bb) in &bounds[x+1..] {
                if bb.min.x > ba.max.x { break; }
                if ba.max.y < bb.min.y || bb.max.y < ba.min.y || ba.max.z < bb.min.z || bb.max.z < ba.min.z { continue; }
                let (aa, ab) = (&bodies[a], &bodies[b]);
                if aa.inverse_mass + ab.inverse_mass == 0.
                    || aa.collider.memberships & ab.collider.filter == 0 || ab.collider.memberships & aa.collider.filter == 0 { continue; }
                stats.candidate_pairs += 1;
                for contact in collision::contacts(aa, ab) {
                    let ra = contact.point - aa.center;
                    let rb = contact.point - ab.center;
                    let speed = (ab.velocity + ab.omega.cross(rb) - aa.velocity - aa.omega.cross(ra)).dot(contact.normal);
                    let bounce = if speed < -1. { -speed * aa.collider.restitution.max(ab.collider.restitution) } else { 0. };
                    let bias = (0.15 * (contact.depth - 0.002).max(0.) / h).min(3.).max(bounce);
                    constraints.push(Constraint { a, b, point: contact.point, normal: contact.normal, bias,
                        friction: (aa.collider.friction * ab.collider.friction).sqrt(), normal_impulse: 0., tangent_impulse: Vec3::default() });
                }
            }
        }
        stats.contacts += constraints.len();
        for _ in 0..settings.iterations {
            for c in &mut constraints {
                let (a,b) = pair_mut(&mut bodies, c.a, c.b);
                let ra = c.point - a.center;
                let rb = c.point - b.center;
                let velocity = b.velocity + b.omega.cross(rb) - a.velocity - a.omega.cross(ra);
                let k = effective_mass(a,b,ra,rb,c.normal);
                if k <= 1e-10 { continue; }
                let impulse = ((c.bias - velocity.dot(c.normal)) / k + c.normal_impulse).max(0.);
                let delta = c.normal.scaled(impulse - c.normal_impulse);
                c.normal_impulse = impulse;
                a.impulse(delta.scaled(-1.), ra); b.impulse(delta, rb);
                let velocity = b.velocity + b.omega.cross(rb) - a.velocity - a.omega.cross(ra);
                let tangent = velocity - c.normal.scaled(velocity.dot(c.normal));
                let length = tangent.dot(tangent).sqrt();
                if length > 1e-6 {
                    let direction = tangent.scaled(1. / length);
                    let kt = effective_mass(a,b,ra,rb,direction);
                    let proposed = c.tangent_impulse - direction.scaled(length / kt.max(1e-10));
                    let magnitude = proposed.dot(proposed).sqrt();
                    let limit = c.friction*c.normal_impulse;
                    let next = proposed.scaled(if magnitude > limit { limit / magnitude } else { 1. });
                    let delta = next - c.tangent_impulse;
                    c.tangent_impulse = next;
                    a.impulse(delta.scaled(-1.), ra); b.impulse(delta, rb);
                }
            }
        }
        for c in constraints {
            if c.normal_impulse > 0. {
                events.push(ContactEvent { a: bodies[c.a].id, b: bodies[c.b].id, point: c.point, normal: c.normal, impulse: c.normal_impulse });
            }
        }
    }
    for b in &bodies {
        let anchor = b.center - b.rotation.rotate(b.collider.offset);
        if !bounded(anchor, 1e6) || !bounded(b.velocity, 10000.) || !bounded(b.omega, 1000.) {
            return Err(format!("physics result outside supported numeric range for item {}", b.id));
        }
    }
    for b in bodies {
        let item = &mut items[b.item];
        item.transform.snap();
        item.transform.anchor = b.center - b.rotation.rotate(b.collider.offset);
        item.transform.rotation = b.rotation;
        let state = item.physics_body.as_mut().unwrap();
        state.velocity = b.velocity; state.angular_velocity = b.omega; state.target = None;
    }
    stats.step_seconds = start.elapsed().as_secs_f64();
    Ok((stats, events))
}
fn effective_mass(a: &Body, b: &Body, ra: Vec3, rb: Vec3, n: Vec3) -> f32 {
    a.inverse_mass + b.inverse_mass + n.dot(a.inertia(ra.cross(n)).cross(ra) + b.inertia(rb.cross(n)).cross(rb))
}
fn pair_mut(values: &mut [Body], a: usize, b: usize) -> (&mut Body, &mut Body) {
    if a < b { let (left,right) = values.split_at_mut(b); (&mut left[a], &mut right[0]) }
    else { let (left,right) = values.split_at_mut(a); (&mut right[0], &mut left[b]) }
}
