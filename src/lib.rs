mod camera;
mod scene;
mod space;

use scene::SceneState;
use std::ptr;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoLine {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoPoint {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

pub struct IoWorld {
    scene: SceneState,
}

#[no_mangle]
pub extern "C" fn io_world_new() -> *mut IoWorld {
    Box::into_raw(Box::new(IoWorld {
        scene: SceneState::demo(),
    }))
}

#[no_mangle]
pub extern "C" fn io_world_free(world: *mut IoWorld) {
    if world.is_null() {
        return;
    }

    unsafe {
        drop(Box::from_raw(world));
    }
}

#[no_mangle]
pub extern "C" fn io_world_resize_units_a(world: *mut IoWorld, delta: i32) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.resize_units_a(delta);
    }
}

#[no_mangle]
pub extern "C" fn io_world_resize_units_b(world: *mut IoWorld, delta: i32) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.resize_units_b(delta);
    }
}

#[no_mangle]
pub extern "C" fn io_world_resize_units_c(world: *mut IoWorld, delta: i32) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.resize_units_c(delta);
    }
}

#[no_mangle]
pub extern "C" fn io_world_orbit_camera(world: *mut IoWorld, yaw_delta: f32, pitch_delta: f32) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.orbit_camera(yaw_delta, pitch_delta);
    }
}

#[no_mangle]
pub extern "C" fn io_world_zoom_camera(world: *mut IoWorld, steps: f32) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.zoom_camera(steps);
    }
}

#[no_mangle]
pub extern "C" fn io_world_set_viewport(world: *mut IoWorld, width: i32, height: i32) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.set_viewport(width, height);
    }
}

#[no_mangle]
pub extern "C" fn io_world_reset_camera(world: *mut IoWorld) {
    if let Some(world) = unsafe { world.as_mut() } {
        world.scene.reset_camera();
    }
}

#[no_mangle]
pub extern "C" fn io_world_lines(world: *mut IoWorld, out_count: *mut usize) -> *const IoLine {
    if out_count.is_null() {
        return ptr::null();
    }

    let Some(world) = (unsafe { world.as_mut() }) else {
        unsafe {
            *out_count = 0;
        }
        return ptr::null();
    };

    let lines = world.scene.lines();
    unsafe {
        *out_count = lines.len();
    }
    lines.as_ptr()
}

#[no_mangle]
pub extern "C" fn io_world_points(world: *mut IoWorld, out_count: *mut usize) -> *const IoPoint {
    if out_count.is_null() {
        return ptr::null();
    }

    let Some(world) = (unsafe { world.as_mut() }) else {
        unsafe {
            *out_count = 0;
        }
        return ptr::null();
    };

    let points = world.scene.points();
    unsafe {
        *out_count = points.len();
    }
    points.as_ptr()
}
