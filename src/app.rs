#![forbid(unsafe_code)]
use crate::camera::Camera;
use crate::model::ModelLibrary;
use crate::projection::Frame;
use io_types::Vec3;
use io_world::{Axis, World};
use std::collections::{BTreeMap, HashSet};
use std::ops::Bound::{Excluded, Unbounded};

pub type CameraId = u64;
pub enum Action {
    Orbit { yaw: f32, pitch: f32 },
    Zoom { steps: f32 },
    Pan { dx: f32, dy: f32 },
    Distance { steps: f32 },
    ResetView,
    ResizeAxis { axis: Axis, delta: i32 },
    NewCamera,
    NextCamera,
    ToggleFollow,
    ToggleGrid,
}
struct View {
    camera: Camera,
    frame: Frame,
    world_revision: Option<u64>,
    dirty: bool,
    follow: Option<usize>,
    home: Camera,
    home_follow: Option<usize>,
    follow_offset: Vec3,
}
impl View {
    fn new(camera: Camera, follow: Option<usize>) -> Self {
        Self {
            home: camera.clone(),
            follow,
            home_follow: follow,
            follow_offset: Vec3::default(),
            camera,
            frame: Frame::default(),
            world_revision: None,
            dirty: true,
        }
    }
}
pub struct App {
    world: World,
    cameras: BTreeMap<CameraId, View>,
    active_camera: CameraId,
    next_camera: CameraId,
    serial: u64,
    accumulator: f64,
    active_simulation: Vec<usize>,
    simulation_dirty: bool,
    spatial_revision: u64,
    library: &'static ModelLibrary,
    grid: bool,
}
impl App {
    pub fn new() -> Result<Self, String> {
        let library = ModelLibrary::global().map_err(str::to_owned)?;
        let world = crate::demo::world(library)?;
        let config = &library.config;
        let mut camera = Camera::new(Vec3::new(
            config.origin[0] + config.camera.target[0],
            config.origin[1] + config.camera.target[1],
            config.origin[2] + config.camera.target[2],
        ));
        camera.set_zoom(config.camera.zoom);
        camera.set_distance(config.camera.render_distance);
        let follow = config
            .camera
            .follow
            .as_ref()
            .and_then(|name| config.items.iter().position(|i| &i.name == name));
        Ok(Self::with_world(world, camera, follow, library))
    }
    pub(crate) fn with_world(
        world: World,
        camera: Camera,
        follow: Option<usize>,
        library: &'static ModelLibrary,
    ) -> Self {
        let mut view = View::new(camera, follow);
        if let Some(id) = follow {
            view.follow_offset = view.camera.target() - world.items()[id].transform.anchor;
        }
        Self {
            world,
            cameras: BTreeMap::from([(1, view)]),
            active_camera: 1,
            next_camera: 2,
            serial: 0,
            accumulator: 0.,
            active_simulation: Vec::new(),
            simulation_dirty: true,
            spatial_revision: 0,
            library,
            grid: false,
        }
    }
    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn set_visual_state(&mut self, item_id: u64, name: &str) -> bool {
        let Some(item) = self.world.item(item_id) else {
            return false;
        };
        let Some(renderable) = &item.renderable else {
            return false;
        };
        let Some(appearance) = self.library.appearance(renderable.appearance_id) else {
            return false;
        };
        let Some(&state) = appearance.state_names.get(name) else {
            return false;
        };
        self.world.set_visual_state(item_id, state)
    }
    pub fn camera(&self, id: CameraId) -> Option<&Camera> {
        self.cameras.get(&id).map(|v| &v.camera)
    }
    pub fn active_camera(&self) -> CameraId {
        self.active_camera
    }
    pub fn active_count(&self) -> usize {
        self.active_simulation.len()
    }
    pub fn create_camera(&mut self) -> Option<CameraId> {
        let id = self.next_camera;
        self.next_camera = id.checked_add(1)?;
        let view = &self.cameras[&self.active_camera];
        let mut copy = View::new(view.camera.clone(), view.follow);
        copy.follow_offset = view.follow_offset;
        copy.home_follow = view.home_follow;
        self.cameras.insert(id, copy);
        self.simulation_dirty = true;
        Some(id)
    }
    pub fn select_camera(&mut self, id: CameraId) -> bool {
        if !self.cameras.contains_key(&id) {
            return false;
        }
        self.active_camera = id;
        true
    }
    pub fn set_target(&mut self, id: CameraId, target: Vec3) -> bool {
        if !target.finite() {
            return false;
        }
        let Some(view) = self.cameras.get_mut(&id) else {
            return false;
        };
        view.camera
            .set_target(self.world.space().clamp_target(target));
        view.follow = None;
        view.dirty = true;
        self.simulation_dirty = true;
        true
    }
    pub fn set_distance(&mut self, id: CameraId, distance: f32) -> bool {
        let Some(view) = self.cameras.get_mut(&id) else {
            return false;
        };
        let changed = view.camera.set_distance(distance);
        view.dirty |= changed;
        self.simulation_dirty |= changed;
        changed
    }
    pub fn dispatch(&mut self, action: Action) -> bool {
        match action {
            Action::ToggleGrid => {
                self.grid = !self.grid;
                for view in self.cameras.values_mut() {
                    view.dirty = true;
                }
                true
            }
            Action::ToggleFollow => self.update_active_view(|view, _| {
                view.follow = if view.follow.is_some() {
                    None
                } else {
                    view.home_follow
                };
                true
            }),
            Action::ResizeAxis { axis, delta } => {
                self.world.resize_units(axis, delta);
                true
            }
            Action::NewCamera => {
                let Some(id) = self.create_camera() else {
                    return false;
                };
                self.active_camera = id;
                true
            }
            Action::NextCamera => {
                self.active_camera = *self
                    .cameras
                    .range((Excluded(self.active_camera), Unbounded))
                    .next()
                    .or_else(|| self.cameras.first_key_value())
                    .unwrap()
                    .0;
                true
            }
            Action::Orbit { yaw, pitch } => {
                self.update_active_view(|view, _| view.camera.orbit(yaw, pitch))
            }
            Action::Zoom { steps } => self.update_active_view(|view, _| view.camera.zoom_by(steps)),
            Action::Pan { dx, dy } => self.update_active_view(|view, space| {
                if !view.camera.pan(dx, dy, space) {
                    return false;
                }
                view.follow = None;
                true
            }),
            Action::Distance { steps } => self.update_active_view(|view, _| {
                if !steps.is_finite() {
                    return false;
                }
                view.camera.set_distance(
                    view.camera.render_distance() * (steps.clamp(-100., 100.) * 0.2).exp(),
                )
            }),
            Action::ResetView => self.update_active_view(|view, _| {
                let old = view.camera.clone();
                view.camera = view.home.clone();
                view.camera.copy_viewport(&old);
                view.follow = view.home_follow;
                true
            }),
        }
    }
    fn update_active_view(
        &mut self,
        update: impl FnOnce(&mut View, &io_world::Space) -> bool,
    ) -> bool {
        let view = self
            .cameras
            .get_mut(&self.active_camera)
            .expect("active camera is retained and selection validates IDs");
        let changed = update(view, self.world.space());
        view.dirty |= changed;
        self.simulation_dirty |= changed;
        changed
    }
    pub fn set_viewport(&mut self, id: CameraId, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        let Some(view) = self.cameras.get_mut(&id) else {
            return false;
        };
        view.dirty |= view.camera.set_viewport(w, h);
        true
    }
    fn refresh_simulation(&mut self) {
        if self.simulation_dirty || self.spatial_revision != self.world.spatial_revision() {
            let mut active = HashSet::new();
            for view in self.cameras.values() {
                if let Some(index) = view.follow {
                    active.insert(index);
                }
                for id in self
                    .world
                    .query(view.camera.target(), view.camera.render_distance())
                {
                    let item = &self.world.items()[id];
                    if item.needs_simulation()
                        && item
                            .visibility_bounds()
                            .within_radius(view.camera.target(), view.camera.render_distance())
                    {
                        active.insert(id);
                    }
                }
            }
            self.active_simulation = active.into_iter().collect();
            self.active_simulation.sort_unstable();
            self.simulation_dirty = false;
            self.spatial_revision = self.world.spatial_revision();
        }
    }
    pub fn update(&mut self, seconds: f32) {
        if !seconds.is_finite() || seconds <= 0. {
            return;
        }
        // Fixed 30 Hz simulation, at most eight ticks per call after a stall.
        self.accumulator += (seconds as f64).min(0.25);
        let dt = 1. / 30.;
        let mut ticks = 0;
        while self.accumulator >= dt && ticks < 8 {
            self.refresh_simulation();
            self.world.simulate(&self.active_simulation, dt as f32);
            self.accumulator -= dt;
            ticks += 1;
        }
        let alpha = (self.accumulator / dt) as f32;
        let interpolating = self.active_simulation.iter().any(|&id| {
            let item = &self.world.items()[id];
            item.motion.is_some() || item.animation.is_some()
        });
        for view in self.cameras.values_mut() {
            if let Some(id) = view.follow {
                view.camera
                    .set_target(self.world.items()[id].render_pose(alpha).0 + view.follow_offset);
                view.dirty = true;
                self.simulation_dirty = true;
            }
            view.dirty |= interpolating;
        }
    }
    pub fn frame(&mut self, id: CameraId) -> Option<&Frame> {
        let view = self.cameras.get_mut(&id)?;
        if view.dirty || view.world_revision != Some(self.world.revision()) {
            self.serial = self.serial.wrapping_add(1);
            if let Err(error) = view.frame.build(
                &self.world,
                &view.camera,
                self.serial,
                self.library,
                (self.accumulator * 30.) as f32,
                &self.active_simulation,
                self.grid,
            ) {
                eprintln!("Frame rejected: {error}");
                return None;
            }
            view.world_revision = Some(self.world.revision());
            view.dirty = false;
        }
        Some(&view.frame)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_pan_keeps_following_and_cached_frame() {
        let mut app = App::new().unwrap();
        let serial = app.frame(1).unwrap().serial;
        let follow = app.cameras[&1].follow;
        assert!(follow.is_some());
        let target = app.camera(1).unwrap().target();
        assert!(!app.dispatch(Action::Pan {
            dx: f32::NAN,
            dy: 0.
        }));
        assert_eq!(app.cameras[&1].follow, follow);
        assert_eq!(app.camera(1).unwrap().target(), target);
        assert_eq!(app.frame(1).unwrap().serial, serial);
    }
    #[test]
    fn street_motion_interpolates_and_stops_when_all_cameras_leave() {
        let mut app = App::new().unwrap();
        let index = app
            .world
            .items()
            .iter()
            .position(|i| i.motion.is_some())
            .unwrap();
        let id = app.world.items()[index].id;
        let before = app.world.items()[index].transform.anchor;
        app.update(1. / 30.);
        let pose = app.frame(1).unwrap().joint_matrices.clone();
        let authoritative = app.world.items()[index].transform.anchor;
        assert_ne!(before, authoritative);
        app.update(1. / 120.);
        assert_eq!(app.world.items()[index].transform.anchor, authoritative);
        assert_ne!(app.frame(1).unwrap().joint_matrices, pose);
        app.set_target(1, Vec3::new(9000., 9000., 0.));
        let retained = app.world.item(id).unwrap().clone();
        app.update(0.2);
        app.update(0.2);
        assert_eq!(app.world.item(id).unwrap(), &retained);
        assert!(app.frame(1).unwrap().joint_matrices.is_empty());
        app.set_target(1, retained.transform.anchor);
        app.update(0.1);
        assert!(app.world.item(id).unwrap().simulated_ticks > retained.simulated_ticks);
    }
    #[test]
    fn two_characters_share_geometry_but_have_independent_poses() {
        let library = ModelLibrary::global().unwrap();
        let original = crate::demo::world(library).unwrap();
        let mut items = original.items().to_vec();
        let mut second = items
            .iter()
            .find(|i| i.animation.is_some())
            .unwrap()
            .clone();
        second.id = 900;
        second.transform.anchor.x -= 2.;
        second.motion = None;
        let animation = second.animation.as_mut().unwrap();
        animation.seek(0.25).unwrap();
        items.push(second);
        let world = World::new(io_world::Space::new(Vec3::new(10000., 10000., 256.)), items);
        let mut app = App::with_world(
            world,
            Camera::new(Vec3::new(5000., 5000., 0.)),
            None,
            library,
        );
        let frame = app.frame(1).unwrap();
        assert_eq!(frame.joint_matrices.len(), 28);
        assert_ne!(&frame.joint_matrices[..14], &frame.joint_matrices[14..]);
        let offsets: Vec<_> = frame
            .instances
            .iter()
            .filter(|i| library.mesh(i.model_id).unwrap().joint_count() > 0)
            .map(|i| i.joint_offset)
            .collect();
        assert_eq!(offsets, vec![0, 14]);
    }
    fn benchmark_app() -> App {
        App::with_world(
            crate::benchmark::world(),
            Camera::new(Vec3::new(5000., 5000., 0.)),
            None,
            ModelLibrary::global().unwrap(),
        )
    }
    #[test]
    fn large_world_queries_small_regions_and_zoom_changes_visibility() {
        let mut a = benchmark_app();
        let count = a.world.items().len();
        let first = a.frame(1).unwrap();
        let visible = first.instances.len();
        assert!(visible > 0 && first.candidate_count < count / 10);
        a.dispatch(Action::Zoom { steps: -12. });
        assert!(a.frame(1).unwrap().instances.len() > visible);
        assert!(a.frame(1).unwrap().grid.len() <= 1000);
    }
    #[test]
    fn world_subdivisions_do_not_change_instances() {
        let mut a = benchmark_app();
        let original = a.frame(1).unwrap().instances.clone();
        a.dispatch(Action::ResizeAxis {
            axis: Axis::A,
            delta: 10,
        });
        assert_eq!(a.frame(1).unwrap().instances, original);
    }
    #[test]
    fn cameras_are_independent_and_unviewed_state_is_retained() {
        let mut a = benchmark_app();
        let original = a.frame(1).unwrap().instances.clone();
        let copy = a.create_camera().unwrap();
        a.set_target(copy, Vec3::new(100., 100., 0.));
        assert_ne!(a.frame(copy).unwrap().instances, original);
        assert_eq!(a.frame(1).unwrap().instances, original);
        let before = a.world.items()[0].clone();
        a.set_target(copy, Vec3::new(9000., 9000., 0.));
        a.update(0.1);
        assert_eq!(a.world.items()[0], before);
        a.set_target(copy, Vec3::new(5., 5., 0.));
        a.update(0.1);
        assert!(a.world.items()[0].simulated_ticks > before.simulated_ticks);
        let retained = a.world.items()[0].clone();
        a.set_target(copy, Vec3::new(9000., 9000., 0.));
        a.update(0.1);
        assert_eq!(a.world.items()[0], retained);
        assert_eq!(a.world.items().len(), 40400);
    }
    #[test]
    fn overlapping_cameras_simulate_once_and_tick_rate_is_fixed() {
        let mut a = benchmark_app();
        let mut b = benchmark_app();
        a.create_camera();
        a.update(0.2);
        for _ in 0..6 {
            b.update(1. / 30.);
        }
        assert_eq!(a.world.items(), b.world.items());
        assert!(a.active_count() > 0);
        assert!(!a.select_camera(0));
        assert!(!a.set_distance(1, f32::NAN));
        assert!(!a.set_target(1, Vec3::new(f32::INFINITY, 0., 0.)));
        assert!(a.frame(999).is_none());
    }
    #[test]
    fn distance_controls_candidates_without_deleting_items() {
        let mut a = benchmark_app();
        a.dispatch(Action::Zoom { steps: -30. });
        a.set_distance(1, 200.);
        let wide = a.frame(1).unwrap().instances.len();
        a.set_distance(1, 16.);
        assert!(a.frame(1).unwrap().instances.len() < wide);
        assert_eq!(a.world.items().len(), 40400);
    }
}
