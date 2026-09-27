//! Editor-only sampling of the runtime rig controller, never a navigation solver.
#![forbid(unsafe_code)]
use crate::camera::{Camera, Projection};
use io_types::{Bounds, Vec3};

#[derive(Clone, PartialEq)]
struct Key {
    draft: io_editor::Draft,
    selected: usize,
    viewport: (i32, i32),
    show: bool,
    projection: Projection,
    speed: f32,
}
#[derive(Default)]
pub struct GuideCache {
    key: Option<Key>,
    pub vertices: Vec<Vec3>,
    pub ends: [usize; 3],
    pub bounds: Option<Bounds>,
}
pub struct Request<'a> {
    pub draft: &'a io_editor::Draft,
    pub regions: &'a [io_world::Interior],
    pub portals: &'a [io_world::Portal],
    pub selected: usize,
    pub viewport: (i32, i32),
    pub projection: Projection,
    pub speed: f32,
    pub show: bool,
}
fn line(out: &mut Vec<Vec3>, a: Vec3, b: Vec3) {
    out.extend([a, b]);
}
fn dot(out: &mut Vec<Vec3>, p: Vec3, r: f32) {
    for d in [
        Vec3::new(r, 0., 0.),
        Vec3::new(0., r, 0.),
        Vec3::new(0., 0., r),
    ] {
        line(out, p - d, p + d);
    }
}
fn portal_guide(out: &mut Vec<Vec3>, p: &io_world::Portal) -> Bounds {
    let a = p.tangent().scaled(p.width * 0.5);
    let z = Vec3::new(0., 0., p.height * 0.5);
    let corners = [
        p.center - a - z,
        p.center + a - z,
        p.center + a + z,
        p.center - a + z,
    ];
    for i in 0..4 {
        line(out, corners[i], corners[(i + 1) % 4]);
    }
    line(out, p.center, p.center + p.normal);
    let mut b = Bounds {
        min: p.center,
        max: p.center,
    };
    for c in corners {
        b = b.union(Bounds { min: c, max: c });
    }
    b.min = b.min - Vec3::new(0.2, 0.2, 0.2);
    b.max = b.max + Vec3::new(0.2, 0.2, 0.2);
    b
}
impl GuideCache {
    pub fn refresh(&mut self, r: Request<'_>) {
        let key = Key {
            draft: r.draft.clone(),
            selected: r.selected,
            viewport: r.viewport,
            show: r.show,
            projection: r.projection,
            speed: r.speed,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.vertices.clear();
        self.ends = [0; 3];
        self.bounds = None;
        if let Some(p) = r
            .draft
            .portal_index(r.selected)
            .and_then(|i| r.portals.get(i))
        {
            self.bounds = Some(portal_guide(&mut self.vertices, p));
            self.ends = [self.vertices.len(); 3];
            return;
        }
        let Some(region) = r.selected.checked_sub(1).and_then(|i| r.regions.get(i)) else {
            return;
        };
        let b = region.bounds;
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            )
        };
        for i in 0..8 {
            for axis in [1, 2, 4] {
                if i & axis == 0 {
                    line(&mut self.vertices, corner(i), corner(i | axis));
                }
            }
        }
        for p in r.portals {
            let location =
                io_world::SpaceLocation::Interior(io_world::InteriorId((r.selected - 1) as u32));
            if p.from == location || p.to == location {
                portal_guide(&mut self.vertices, p);
            }
        }
        self.ends = [self.vertices.len(); 3];
        if r.show {
            self.sample(&r, region);
        }
        let mut bounds = b;
        for p in &self.vertices {
            bounds.min = Vec3::new(
                bounds.min.x.min(p.x),
                bounds.min.y.min(p.y),
                bounds.min.z.min(p.z),
            );
            bounds.max = Vec3::new(
                bounds.max.x.max(p.x),
                bounds.max.y.max(p.y),
                bounds.max.z.max(p.z),
            );
        }
        self.bounds = Some(bounds);
    }
    fn sample(&mut self, r: &Request<'_>, region: &io_world::Interior) {
        let direction = region.entry_direction;
        let extent = region.bounds.extent();
        let half = (if direction.x.abs() > 1e-5 {
            extent.x / direction.x.abs()
        } else {
            f32::INFINITY
        })
        .min(if direction.y.abs() > 1e-5 {
            extent.y / direction.y.abs()
        } else {
            f32::INFINITY
        });
        let approach = r.draft.rig(r.selected).map_or(3., |rig| rig.approach);
        let length = 2. * (half + approach + 2.);
        let mut center = region.bounds.center();
        center.z = region.bounds.min.z;
        let start = center - direction.scaled(length * 0.5);
        let mut camera = Camera::new(start);
        camera.set_projection(r.projection);
        camera.set_viewport(r.viewport.0, r.viewport.1);
        camera.preview_rig(&r.draft.rigs.exterior, start);
        let speed = r.speed.max(0.1);
        let dt = length / (512. * speed);
        let mut feet = Vec::new();
        let mut views = Vec::new();
        let mut location = io_world::locate_space(r.regions, start);
        let mut previous = start;
        for step in 0..=512 {
            let p = start + direction.scaled(length * step as f32 / 512.);
            location = io_world::advance_location(r.portals, location, previous, p);
            previous = p;
            camera.track_space(
                crate::camera::SpaceView {
                    rigs: &r.draft.rigs,
                    regions: r.regions,
                    portals: r.portals,
                    location,
                    point: p,
                },
                if step == 0 { 0. } else { dt },
            );
            if step % 8 == 0 {
                feet.push(p + Vec3::new(0., 0., 0.05));
                views.push((camera.view(), camera.fov_degrees()));
            }
        }
        for pair in feet.windows(2) {
            line(&mut self.vertices, pair[0], pair[1]);
        }
        for &p in &feet {
            dot(&mut self.vertices, p, 0.08);
        }
        self.ends[1] = self.vertices.len();
        let mut previous = None;
        for (i, (view, fov)) in views.iter().enumerate() {
            // Orthographic has no finite eye. Cap distant/infinite markers at 80 m.
            let distance = if view.convergence > 0. {
                (1. / view.convergence).min(80.)
            } else {
                80.
            };
            let eye = view.target + view.forward.scaled(distance);
            if let Some(p) = previous {
                line(&mut self.vertices, p, eye);
            }
            previous = Some(eye);
            dot(&mut self.vertices, eye, 0.18);
            if i % 16 == 0 {
                let c = eye - view.forward.scaled(2.5);
                let hh = 2.5 * (fov.to_radians() * 0.5).tan();
                let hw = hh * view.width / view.height;
                let corners = [
                    c - view.right.scaled(hw) - view.up.scaled(hh),
                    c + view.right.scaled(hw) - view.up.scaled(hh),
                    c + view.right.scaled(hw) + view.up.scaled(hh),
                    c - view.right.scaled(hw) + view.up.scaled(hh),
                ];
                for j in 0..4 {
                    line(&mut self.vertices, eye, corners[j]);
                    line(&mut self.vertices, corners[j], corners[(j + 1) % 4]);
                }
            }
        }
        self.ends[2] = self.vertices.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampled_guides_are_bounded_finite_and_refresh_with_lens_and_speed() {
        let mut rigs = io_scene::CameraRigs::default();
        rigs.interiors.insert(
            "Room".into(),
            io_scene::CameraRig {
                zoom: 12.,
                pitch_degrees: 10.,
                ..io_scene::CameraRig::default()
            },
        );
        let mut draft = io_editor::Draft {
            portals: Vec::new(),
            rigs,
            regions: vec![io_editor::Region {
                name: "Room".into(),
                min: [0.; 3],
                max: [4., 8., 3.],
                ceiling: Some(3.),
                entry_direction: [0., -1., 0.],
            }],
        };
        let regions = [io_world::Interior {
            name: "Room".into(),
            bounds: Bounds {
                min: Vec3::default(),
                max: Vec3::new(4., 8., 3.),
            },
            ceiling: Some(3.),
            entry_direction: Vec3::new(0., -1., 0.),
        }];
        let projection = Projection::ZoomPerspective {
            start_zoom: 1.5,
            end_zoom: 5.,
            vertical_fov_degrees: 45.,
            near_clip: 0.05,
            smoothing_seconds: 0.16,
        };
        fn request<'a>(
            draft: &'a io_editor::Draft,
            regions: &'a [io_world::Interior],
            projection: Projection,
            speed: f32,
        ) -> Request<'a> {
            Request {
                portals: &[],
                draft,
                regions,
                selected: 1,
                viewport: (1280, 800),
                projection,
                speed,
                show: true,
            }
        }
        let mut cache = GuideCache::default();
        cache.refresh(request(&draft, &regions, projection, 2.));
        assert_eq!(cache.ends[0], 24);
        assert!(cache.ends[1] > 24 && cache.ends[2] > cache.ends[1]);
        assert!(cache.vertices.len() < 3000);
        assert!(cache.vertices.iter().all(|p| p.finite()));
        let first = cache.vertices.clone();
        cache.refresh(request(&draft, &regions, projection, 2.));
        assert_eq!(cache.vertices, first);
        cache.refresh(request(&draft, &regions, projection, 0.5));
        assert_ne!(cache.vertices, first);
        let slow = cache.vertices.clone();
        draft.rigs.interiors.get_mut("Room").unwrap().fov_degrees = 90.;
        cache.refresh(request(&draft, &regions, projection, 0.5));
        assert_ne!(cache.vertices, slow);
        let mut r = request(&draft, &regions, projection, 0.5);
        r.projection = Projection::Orthographic {};
        cache.refresh(r);
        assert!(cache.vertices.iter().all(|p| p.finite()));
    }
}
