#![deny(unsafe_op_in_unsafe_fn)]
mod app;
mod appearance;
#[cfg(test)]
mod benchmark;
mod camera;
mod config;
mod demo;
mod model;
#[cfg(test)]
mod package_tests;
mod projection;
#[cfg(test)]
mod variant_tests;

use app::{Action, App, CameraId};
pub use io_types::Vec3 as IoVec3;
use io_world::Axis;
pub use projection::Instance as IoInstance;
use std::ptr;

#[derive(Debug, serde::Serialize)]
pub struct ValidationReport {
    pub meshes: usize,
    pub appearances: usize,
    pub items: usize,
}

pub fn validate_scene(path: &std::path::Path) -> Result<ValidationReport, String> {
    let library = model::ModelLibrary::load(path)?;
    let world = demo::world(&library)?;
    Ok(ValidationReport {
        meshes: library.models.len(),
        appearances: library.config.appearances.len(),
        items: world.items().len(),
    })
}

pub fn validate_package(path: &std::path::Path) -> Result<ValidationReport, String> {
    let config = config::SceneConfig {
        version: 1,
        dimensions: [1.; 3],
        origin: [0.; 3],
        camera: config::CameraConfig {
            target: [0.; 3],
            zoom: 1.,
            follow: None,
            render_distance: 120.,
        },
        packages: vec![config::PackageImport {
            namespace: "package".into(),
            file: path.to_owned(),
        }],
        assets: Vec::new(),
        appearances: Vec::new(),
        items: Vec::new(),
    };
    let library = model::ModelLibrary::from_config(config, std::path::Path::new("."))?;
    Ok(ValidationReport {
        meshes: library.models.len(),
        appearances: library.config.appearances.len(),
        items: 0,
    })
}

pub struct IoApp {
    state: App,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoAction {
    pub kind: u32,
    pub x: f32,
    pub y: f32,
    pub delta: i32,
}
impl IoAction {
    fn decode(self) -> Option<Action> {
        Some(match self.kind {
            1 => Action::Orbit {
                yaw: self.x,
                pitch: self.y,
            },
            2 => Action::Zoom { steps: self.x },
            3 => Action::ResetView,
            4 => Action::ResizeAxis {
                axis: Axis::A,
                delta: self.delta,
            },
            5 => Action::ResizeAxis {
                axis: Axis::B,
                delta: self.delta,
            },
            6 => Action::ResizeAxis {
                axis: Axis::C,
                delta: self.delta,
            },
            7 => Action::NewCamera,
            8 => Action::NextCamera,
            9 => Action::Pan {
                dx: self.x,
                dy: self.y,
            },
            10 => Action::Distance { steps: self.x },
            11 => Action::ToggleFollow,
            12 => Action::ToggleGrid,
            _ => return None,
        })
    }
}
#[repr(C)]
pub struct IoFrame {
    pub instances: *const IoInstance,
    pub instance_count: usize,
    pub grid: *const IoVec3,
    pub grid_vertex_count: usize,
    pub clip_from_world: [f32; 16],
    pub serial: u64,
    pub world_items: u64,
    pub candidates: u64,
    pub active_simulations: u64,
    pub target: IoVec3,
    pub render_distance: f32,
    pub joint_matrices: *const f32,
    pub joint_count: usize,
}
impl Default for IoFrame {
    fn default() -> Self {
        Self {
            instances: ptr::null(),
            instance_count: 0,
            grid: ptr::null(),
            grid_vertex_count: 0,
            clip_from_world: [0.; 16],
            serial: 0,
            world_items: 0,
            candidates: 0,
            active_simulations: 0,
            target: IoVec3::default(),
            render_distance: 0.,
            joint_matrices: ptr::null(),
            joint_count: 0,
        }
    }
}
#[repr(C)]
pub struct IoModel {
    pub vertices: *const io_assets::Vertex,
    pub vertex_count: usize,
    pub indices: *const u32,
    pub index_count: usize,
    pub topology: u32,
}
impl Default for IoModel {
    fn default() -> Self {
        Self {
            vertices: ptr::null(),
            vertex_count: 0,
            indices: ptr::null(),
            index_count: 0,
            topology: 0,
        }
    }
}
#[repr(C)]
#[derive(Default)]
pub struct IoItemState {
    pub id: u64,
    pub anchor: IoVec3,
    pub size: IoVec3,
    pub health: u32,
    pub simulated_ticks: u64,
}

#[no_mangle]
pub extern "C" fn io_app_new() -> *mut IoApp {
    match App::new() {
        Ok(state) => Box::into_raw(Box::new(IoApp { state })),
        Err(error) => {
            eprintln!("Scene load failed: {error}");
            ptr::null_mut()
        }
    }
}
/// # Safety
/// Pass null or a live, exclusively accessed app handle, exactly once.
#[no_mangle]
pub unsafe extern "C" fn io_app_free(app: *mut IoApp) {
    if !app.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}
/// # Safety
/// A non-null app must be live and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn io_app_dispatch(app: *mut IoApp, action: IoAction) -> bool {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return false;
    };
    let Some(action) = action.decode() else {
        return false;
    };
    app.state.dispatch(action)
}
/// # Safety
/// app must be null or live and exclusively accessed. name must be null or a
/// readable, NUL-terminated string, with storage independent of the app.
#[no_mangle]
pub unsafe extern "C" fn io_app_set_visual_state(
    app: *mut IoApp,
    item_id: u64,
    name: *const std::ffi::c_char,
) -> bool {
    if name.is_null() {
        return false;
    }
    let Ok(name) = (unsafe { std::ffi::CStr::from_ptr(name) }).to_str() else {
        return false;
    };
    unsafe { app.as_mut() }.is_some_and(|a| a.state.set_visual_state(item_id, name))
}
/// # Safety
/// A non-null app must be live, with no concurrent mutation.
#[no_mangle]
pub unsafe extern "C" fn io_app_active_camera(app: *const IoApp) -> CameraId {
    unsafe { app.as_ref() }.map_or(0, |a| a.state.active_camera())
}
/// # Safety
/// A non-null app must be live and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn io_app_create_camera(app: *mut IoApp) -> CameraId {
    unsafe { app.as_mut() }
        .and_then(|a| a.state.create_camera())
        .unwrap_or(0)
}
/// # Safety
/// A non-null app must be live and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn io_app_select_camera(app: *mut IoApp, camera: CameraId) -> bool {
    unsafe { app.as_mut() }.is_some_and(|a| a.state.select_camera(camera))
}
/// # Safety
/// A non-null app must be live and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn io_app_set_viewport(
    app: *mut IoApp,
    camera: CameraId,
    w: i32,
    h: i32,
) -> bool {
    unsafe { app.as_mut() }.is_some_and(|a| a.state.set_viewport(camera, w, h))
}
/// # Safety
/// A non-null app must be live and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn io_app_set_camera_target(
    app: *mut IoApp,
    camera: CameraId,
    target: IoVec3,
) -> bool {
    unsafe { app.as_mut() }.is_some_and(|a| a.state.set_target(camera, target))
}
/// # Safety
/// A non-null app must be live and exclusively accessed.
#[no_mangle]
pub unsafe extern "C" fn io_app_set_render_distance(
    app: *mut IoApp,
    camera: CameraId,
    distance: f32,
) -> bool {
    unsafe { app.as_mut() }.is_some_and(|a| a.state.set_distance(camera, distance))
}
/// # Safety
/// A non-null app must be live and exclusively accessed. Consume borrowed frames before calling.
#[no_mangle]
pub unsafe extern "C" fn io_app_update(app: *mut IoApp, seconds: f32) {
    if let Some(app) = unsafe { app.as_mut() } {
        app.state.update(seconds);
    }
}
/// # Safety
/// app must be null or live and exclusively accessed; out must be null or separately writable.
/// Arrays are borrowed until the next mutable app call or free. Never free them from C.
#[no_mangle]
pub unsafe extern "C" fn io_app_frame(
    app: *mut IoApp,
    camera: CameraId,
    out: *mut IoFrame,
) -> bool {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return false;
    };
    *out = IoFrame::default();
    let Some(app) = (unsafe { app.as_mut() }) else {
        return false;
    };
    let Some(view) = app.state.camera(camera) else {
        return false;
    };
    let target = view.target();
    let render_distance = view.render_distance();
    let world_items = app.state.world().items().len() as u64;
    let active_simulations = app.state.active_count() as u64;
    let Some(frame) = app.state.frame(camera) else {
        return false;
    };
    *out = IoFrame {
        instances: frame.instances.as_ptr(),
        instance_count: frame.instances.len(),
        grid: frame.grid.as_ptr(),
        grid_vertex_count: frame.grid.len(),
        clip_from_world: frame.clip_from_world,
        serial: frame.serial,
        world_items,
        candidates: frame.candidate_count as u64,
        active_simulations,
        target,
        render_distance,
        joint_matrices: frame.joint_matrices.as_ptr().cast(),
        joint_count: frame.joint_matrices.len(),
    };
    true
}
/// # Safety
/// out must be null or writable. Returned model data are immutable and valid for the process lifetime.
#[no_mangle]
pub unsafe extern "C" fn io_model_get(id: u32, out: *mut IoModel) -> bool {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return false;
    };
    *out = IoModel::default();
    let Some(mesh) = model::ModelLibrary::global()
        .ok()
        .and_then(|library| library.mesh(id))
    else {
        return false;
    };
    *out = IoModel {
        vertices: mesh.vertices().as_ptr(),
        vertex_count: mesh.vertices().len(),
        indices: mesh.indices().as_ptr(),
        index_count: mesh.indices().len(),
        topology: 4,
    };
    true
}
/// # Safety
/// app must be null or live with no concurrent mutation. out must be null or separately writable.
#[no_mangle]
pub unsafe extern "C" fn io_app_item_state(
    app: *const IoApp,
    id: u64,
    out: *mut IoItemState,
) -> bool {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return false;
    };
    *out = IoItemState::default();
    let Some(app) = (unsafe { app.as_ref() }) else {
        return false;
    };
    let Some(item) = app.state.world().item(id) else {
        return false;
    };
    *out = IoItemState {
        id: item.id,
        anchor: item.transform.anchor,
        size: item.transform.size,
        health: item.durability.as_ref().map_or(0, |d| d.current()),
        simulated_ticks: item.simulated_ticks,
    };
    true
}
