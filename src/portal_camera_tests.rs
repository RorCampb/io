use crate::camera::{Camera, SpaceView};
use io_types::Vec3;
use io_world::{Interior, InteriorId, Portal, SpaceLocation};
fn setup() -> (Camera, io_scene::CameraRigs, Vec<Interior>, Vec<Portal>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/entry.json");
    let config = crate::config::SceneConfig::read(&path).unwrap();
    let (r, p) =
        io_scene::resolve_layout(&config.interiors, &config.portals, config.origin).unwrap();
    let mut c = Camera::new(Vec3::new(50., 58., 1.));
    c.set_projection(config.camera.projection);
    (c, config.camera.rigs.unwrap(), r, p)
}
fn settle(
    c: &mut Camera,
    rigs: &io_scene::CameraRigs,
    r: &[Interior],
    p: &[Portal],
    location: SpaceLocation,
    point: Vec3,
) {
    for _ in 0..360 {
        c.track_space(
            SpaceView {
                rigs,
                regions: r,
                portals: p,
                location,
                point,
            },
            1. / 60.,
        );
    }
}
#[test]
fn exterior_walls_do_not_change_fov_but_the_opening_can_anticipate() {
    let (mut c, rigs, r, p) = setup();
    for point in [
        Vec3::new(48., 46., 0.),
        Vec3::new(52., 46., 0.),
        Vec3::new(43.5, 37., 0.),
        Vec3::new(56.5, 37., 0.),
        Vec3::new(50., 30., 0.),
        Vec3::new(53., 50.5, 0.),
    ] {
        settle(&mut c, &rigs, &r, &p, SpaceLocation::Exterior, point);
        assert!((c.fov_degrees() - 45.).abs() < 0.01, "wall at {point:?}");
        assert!((c.capture_rig().1 - rigs.exterior.pitch_degrees).abs() < 0.01);
        assert!(!c.toggle_room_anchor());
    }
    settle(
        &mut c,
        &rigs,
        &r,
        &p,
        SpaceLocation::Exterior,
        Vec3::new(50., 50.5, 0.),
    );
    assert!(c.fov_degrees() > 48.);
    assert!(
        !c.toggle_room_anchor(),
        "approach does not grant room membership"
    );
    settle(
        &mut c,
        &rigs,
        &r,
        &p,
        SpaceLocation::Exterior,
        Vec3::new(53., 50.5, 0.),
    );
    assert!((c.fov_degrees() - 45.).abs() < 0.01);
}
#[test]
fn room_anchor_ignores_character_motion_and_restores_both_views() {
    let (mut c, rigs, r, p) = setup();
    let room = SpaceLocation::Interior(InteriorId(1));
    let a = Vec3::new(50., 37., 0.);
    settle(&mut c, &rigs, &r, &p, room, a);
    c.orbit(0.3, 0.);
    c.zoom_by(-2.);
    settle(&mut c, &rigs, &r, &p, room, a);
    let follow = c.capture_rig();
    assert!(c.toggle_room_anchor());
    settle(&mut c, &rigs, &r, &p, room, a);
    assert!(c.room_anchored());
    c.orbit(0.4, 0.1);
    c.zoom_by(3.);
    settle(&mut c, &rigs, &r, &p, room, a);
    let anchored = c.view().matrix();
    let target = c.target();
    settle(&mut c, &rigs, &r, &p, room, Vec3::new(47., 34., 0.));
    assert_eq!(c.target(), target);
    for (a, b) in anchored.iter().zip(c.view().matrix()) {
        assert!((a - b).abs() < 0.001);
    }
    assert!(c.toggle_room_anchor());
    settle(&mut c, &rigs, &r, &p, room, a);
    let restored = c.capture_rig();
    assert!((restored.0 - follow.0).abs() < 0.01 && (restored.2 - follow.2).abs() < 0.01);
    assert!(c.toggle_room_anchor());
    settle(&mut c, &rigs, &r, &p, room, a);
    for (a, b) in anchored.iter().zip(c.view().matrix()) {
        assert!((a - b).abs() < 0.001);
    }
    settle(
        &mut c,
        &rigs,
        &r,
        &p,
        SpaceLocation::Exterior,
        Vec3::new(50., 58., 0.),
    );
    assert!(!c.room_anchored());
    assert!((c.capture_rig().0 - follow.0).abs() < 0.01);
}
#[test]
fn anchored_orbit_and_lens_remain_finite_and_camera_local() {
    let (mut c, rigs, r, p) = setup();
    let room = SpaceLocation::Interior(InteriorId(1));
    let point = Vec3::new(50., 37., 0.);
    settle(&mut c, &rigs, &r, &p, room, point);
    let untouched = c.clone();
    assert!(c.toggle_room_anchor());
    settle(&mut c, &rigs, &r, &p, room, point);
    let before = c.view().matrix();
    for i in 0..180 {
        c.orbit(
            std::f32::consts::TAU / 180.,
            if i < 90 { 0.004 } else { -0.004 },
        );
        c.zoom_by(if i < 90 { 0.015 } else { -0.015 });
        c.track_space(
            SpaceView {
                rigs: &rigs,
                regions: &r,
                portals: &p,
                location: room,
                point,
            },
            1. / 60.,
        );
        let v = c.view();
        assert!(v.matrix().iter().all(|v| v.is_finite()));
        assert!(v.project(v.target).is_some());
    }
    assert_ne!(before, c.view().matrix());
    assert!(!untouched.room_anchored());
}
