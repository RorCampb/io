use crate::Interior;
use io_types::Vec3;
#[cfg(test)]
#[path = "portal_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InteriorId(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpaceLocation {
    #[default]
    Exterior,
    Interior(InteriorId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Portal {
    pub name: String,
    pub from: SpaceLocation,
    pub to: SpaceLocation,
    pub center: Vec3,
    /// Horizontal unit normal pointing from `from` toward `to`.
    pub normal: Vec3,
    pub width: f32,
    pub height: f32,
}
fn contains(r: &Interior, p: Vec3, tolerance: f32) -> bool {
    let b = r.bounds;
    p.x >= b.min.x - tolerance
        && p.x <= b.max.x + tolerance
        && p.y >= b.min.y - tolerance
        && p.y <= b.max.y + tolerance
        && p.z >= b.min.z - tolerance
        && p.z <= r.ceiling.unwrap_or(b.max.z) + tolerance
}
pub fn locate_space(regions: &[Interior], point: Vec3) -> SpaceLocation {
    regions
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            contains(r, point, 0.001) && point.z < r.ceiling.unwrap_or(r.bounds.max.z) - 1e-4
        })
        .min_by(|(ai, a), (bi, b)| {
            let a = a.bounds.extent();
            let b = b.bounds.extent();
            (a.x * a.y * a.z)
                .total_cmp(&(b.x * b.y * b.z))
                .then(ai.cmp(bi))
        })
        .map_or(SpaceLocation::Exterior, |(i, _)| {
            SpaceLocation::Interior(InteriorId(i as u32))
        })
}
impl Portal {
    pub fn tangent(&self) -> Vec3 {
        Vec3::new(-self.normal.y, self.normal.x, 0.)
    }
    fn in_opening(&self, p: Vec3) -> bool {
        let delta = p - self.center;
        delta.dot(self.tangent()).abs() <= self.width * 0.5 + 1e-4
            && delta.z.abs() <= self.height * 0.5 + 1e-4
    }
    pub fn validate(&self, regions: &[Interior]) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.len() > 64
            || self.from == self.to
            || !self.center.finite()
            || !self.normal.finite()
            || self.normal.z.abs() > 1e-5
            || (self.normal.dot(self.normal) - 1.).abs() > 1e-4
            || !self.width.is_finite()
            || !self.height.is_finite()
            || !(0.1..=1000.).contains(&self.width)
            || !(0.1..=1000.).contains(&self.height)
        {
            return Err("invalid portal geometry or endpoints".into());
        }
        for (location, sign) in [(self.from, -1.), (self.to, 1.)] {
            if let SpaceLocation::Interior(InteriorId(id)) = location {
                let r = regions.get(id as usize).ok_or("unknown portal interior")?;
                let inside = self.center + self.normal.scaled(sign * 0.01);
                let outside = self.center - self.normal.scaled(sign * 0.01);
                if !contains(r, inside, 0.001) || contains(r, outside, 0.001) {
                    return Err(
                        "portal must lie on the boundary and point into its destination".into(),
                    );
                }
                for x in [-0.5, 0.5] {
                    for z in [-0.5, 0.5] {
                        let p = self.center
                            + self.tangent().scaled(x * self.width)
                            + Vec3::new(0., 0., z * self.height);
                        if !contains(r, p, 0.001) {
                            return Err("portal opening extends beyond its interior".into());
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn crossing(
        &self,
        location: SpaceLocation,
        start: Vec3,
        end: Vec3,
    ) -> Option<(f32, SpaceLocation)> {
        let (sign, destination) = if location == self.from {
            (1., self.to)
        } else if location == self.to {
            (-1., self.from)
        } else {
            return None;
        };
        let a = (start - self.center).dot(self.normal) * sign;
        let b = (end - self.center).dot(self.normal) * sign;
        if a > 1e-5 || b <= 1e-5 || b <= a {
            return None;
        }
        let t = (-a / (b - a)).clamp(0., 1.);
        self.in_opening(start + (end - start).scaled(t))
            .then_some((t, destination))
    }
    /// Entrance-aligned anticipation, independent of membership and camera settings.
    pub fn approach(
        &self,
        location: SpaceLocation,
        point: Vec3,
        distance: f32,
    ) -> Option<(SpaceLocation, f32)> {
        let (sign, destination) = if location == self.from {
            (1., self.to)
        } else if location == self.to {
            (-1., self.from)
        } else {
            return None;
        };
        if !distance.is_finite() || distance <= 0. || !point.finite() || !self.in_opening(point) {
            return None;
        }
        let d = -(point - self.center).dot(self.normal) * sign;
        if d < 0. || d >= distance {
            return None;
        }
        let t = 1. - d / distance;
        Some((destination, t * t * (3. - 2. * t)))
    }
}
pub fn validate_layout(regions: &[Interior], portals: &[Portal]) -> Result<(), String> {
    if regions.len() > 4096 || portals.len() > 8192 {
        return Err("too many spaces or portals".into());
    }
    let mut names = std::collections::HashSet::new();
    for r in regions {
        r.validate()?;
        if !names.insert(&r.name) {
            return Err("duplicate interior name".into());
        }
    }
    names.clear();
    for p in portals {
        p.validate(regions)?;
        if !names.insert(&p.name) {
            return Err("duplicate portal name".into());
        }
    }
    Ok(())
}
/// Process all connected crossings in segment order, including a fast multi-room move.
pub fn advance_location(
    portals: &[Portal],
    mut location: SpaceLocation,
    start: Vec3,
    end: Vec3,
) -> SpaceLocation {
    let mut after = -1.;
    for _ in 0..portals.len() {
        let next = portals
            .iter()
            .filter_map(|p| p.crossing(location, start, end))
            .filter(|(t, _)| *t > after + 1e-6)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((t, destination)) = next else {
            break;
        };
        after = t;
        location = destination;
    }
    location
}
