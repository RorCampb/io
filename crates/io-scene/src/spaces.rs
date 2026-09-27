use io_types::{Bounds, Vec3};
use io_world::{Interior, InteriorId, Portal, SpaceLocation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteriorDefinition {
    pub name: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub ceiling: Option<f32>,
    pub entry_direction: [f32; 3],
}
impl InteriorDefinition {
    pub fn entrance(
        &self,
        regions: &[InteriorDefinition],
        name: String,
    ) -> Result<PortalDefinition, String> {
        let resolved: Vec<_> = regions
            .iter()
            .map(|r| r.resolve([0.; 3]))
            .collect::<Result<_, _>>()?;
        let room = self.resolve([0.; 3])?;
        let e = room.bounds.extent();
        let d = room.entry_direction;
        let distance = (if d.x.abs() > 1e-5 {
            e.x / d.x.abs()
        } else {
            f32::INFINITY
        })
        .min(if d.y.abs() > 1e-5 {
            e.y / d.y.abs()
        } else {
            f32::INFINITY
        });
        let mut center = room.bounds.center() - d.scaled(distance);
        let mut height = (room.ceiling.unwrap_or(room.bounds.max.z) - room.bounds.min.z).min(2.);
        center.z = room.bounds.min.z + height * 0.5;
        let mut probe = center - d.scaled(0.02);
        probe.z = room.bounds.min.z + 0.01;
        let mut width = (2. * (e.x * d.y.abs() + e.y * d.x.abs())).min(2.);
        let from = match io_world::locate_space(&resolved, probe) {
            SpaceLocation::Exterior => SpaceReference::Exterior,
            SpaceLocation::Interior(InteriorId(i)) => {
                let neighbor = &resolved[i as usize];
                let bottom = room.bounds.min.z.max(neighbor.bounds.min.z);
                let top = room
                    .ceiling
                    .unwrap_or(room.bounds.max.z)
                    .min(neighbor.ceiling.unwrap_or(neighbor.bounds.max.z));
                height = height.min(top - bottom);
                center.z = bottom + height * 0.5;
                let e = neighbor.bounds.extent();
                width = width.min(2. * (e.x * d.y.abs() + e.y * d.x.abs()));
                SpaceReference::Interior(resolved[i as usize].name.clone())
            }
        };
        let portal = PortalDefinition {
            name,
            from,
            to: SpaceReference::Interior(self.name.clone()),
            center: [center.x, center.y, center.z],
            normal: self.entry_direction,
            width,
            height,
        };
        portal.resolve(&resolved, [0.; 3])?;
        Ok(portal)
    }
    pub fn validate(&self) -> Result<(), String> {
        self.resolve([0.; 3]).map(|_| ())
    }
    pub fn resolve(&self, origin: [f32; 3]) -> Result<Interior, String> {
        if self
            .min
            .iter()
            .chain(self.max.iter())
            .chain(origin.iter())
            .any(|v| !v.is_finite() || v.abs() > 100_000.)
            || (0..3).any(|i| self.max[i] - self.min[i] < 0.1)
        {
            return Err("invalid interior bounds".into());
        }
        let p = |a: [f32; 3]| Vec3::new(a[0] + origin[0], a[1] + origin[1], a[2] + origin[2]);
        let r = Interior {
            name: self.name.clone(),
            bounds: Bounds {
                min: p(self.min),
                max: p(self.max),
            },
            ceiling: self.ceiling.map(|z| z + origin[2]),
            entry_direction: Vec3::new(
                self.entry_direction[0],
                self.entry_direction[1],
                self.entry_direction[2],
            ),
        };
        r.validate()?;
        Ok(r)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "name",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpaceReference {
    Exterior,
    Interior(String),
}
impl SpaceReference {
    fn resolve(&self, regions: &[Interior]) -> Result<SpaceLocation, String> {
        match self {
            Self::Exterior => Ok(SpaceLocation::Exterior),
            Self::Interior(name) => regions
                .iter()
                .position(|r| r.name == *name)
                .map(|i| SpaceLocation::Interior(InteriorId(i as u32)))
                .ok_or_else(|| format!("unknown portal interior: {name}")),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortalDefinition {
    pub name: String,
    pub from: SpaceReference,
    pub to: SpaceReference,
    pub center: [f32; 3],
    pub normal: [f32; 3],
    pub width: f32,
    pub height: f32,
}
impl PortalDefinition {
    pub fn resolve(&self, regions: &[Interior], origin: [f32; 3]) -> Result<Portal, String> {
        let p = Portal {
            name: self.name.clone(),
            from: self.from.resolve(regions)?,
            to: self.to.resolve(regions)?,
            center: Vec3::new(
                self.center[0] + origin[0],
                self.center[1] + origin[1],
                self.center[2] + origin[2],
            ),
            normal: Vec3::new(self.normal[0], self.normal[1], self.normal[2]),
            width: self.width,
            height: self.height,
        };
        p.validate(regions)?;
        Ok(p)
    }
}
pub fn resolve_layout(
    regions: &[InteriorDefinition],
    portals: &[PortalDefinition],
    origin: [f32; 3],
) -> Result<(Vec<Interior>, Vec<Portal>), String> {
    let regions: Vec<_> = regions
        .iter()
        .map(|r| r.resolve(origin))
        .collect::<Result<_, _>>()?;
    let portals: Vec<_> = portals
        .iter()
        .map(|p| p.resolve(&regions, origin))
        .collect::<Result<_, _>>()?;
    io_world::validate_layout(&regions, &portals)?;
    Ok((regions, portals))
}
