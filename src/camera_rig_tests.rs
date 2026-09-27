use super::*;
fn setup() -> (CameraRigs, Vec<io_world::Interior>) {
    let mut rigs = CameraRigs::default();
    rigs.interiors.insert(
        "Room".into(),
        CameraRig {
            yaw_degrees: 0.,
            pitch_degrees: 10.,
            zoom: 12.,
            min_zoom: 8.,
            yaw_limit_degrees: 15.,
            ..CameraRig::default()
        },
    );
    let region = io_world::Interior {
        name: "Room".into(),
        bounds: io_types::Bounds {
            min: Vec3::default(),
            max: Vec3::new(10., 10., 3.),
        },
        ceiling: Some(3.),
        entry_direction: Vec3::new(0., -1., 0.),
    };
    (rigs, vec![region])
}
#[test]
fn authored_blend_is_time_based_and_recovers_exterior() {
    let (rigs, regions) = setup();
    let mut a = RigController::new(&rigs.exterior);
    let mut b = a.clone();
    let p = Some(Vec3::new(5., 5., 0.));
    for _ in 0..60 {
        a.update(&rigs, &regions, p, 1. / 60.);
    }
    for _ in 0..120 {
        b.update(&rigs, &regions, p, 1. / 120.);
    }
    assert!((a.pose().pitch_degrees - b.pose().pitch_degrees).abs() < 1e-3);
    assert!(a.pose().pitch_degrees < 11.);
    for _ in 0..300 {
        a.update(&rigs, &regions, None, 1. / 60.);
    }
    assert!((a.pose().pitch_degrees - rigs.exterior.pitch_degrees).abs() < 1e-3);
}
#[test]
fn manual_controls_are_bounded_and_cameras_are_independent() {
    let (rigs, regions) = setup();
    let mut a = RigController::new(&rigs.exterior);
    let b = a.clone();
    for _ in 0..300 {
        a.update(&rigs, &regions, Some(Vec3::new(5., 5., 0.)), 1. / 60.);
    }
    a.orbit(20., 20.);
    a.zoom(-100.);
    assert!((a.pose().yaw_degrees - 15.).abs() < 0.01);
    assert!((a.pose().zoom - 8.).abs() < 0.01);
    assert_eq!(a.pose().pitch_degrees, 85.);
    assert_eq!(b.pose(), rigs.exterior);
}

#[test]
fn manual_framing_holds_while_crawling_then_releases_in_the_next_room() {
    let (mut rigs, mut regions) = setup();
    {
        let r = rigs.interiors.get_mut("Room").unwrap();
        r.min_zoom = 0.5;
        r.yaw_limit_degrees = 180.;
        r.fov_degrees = 50.;
    }
    let mut second = regions[0].clone();
    second.name = "Chamber".into();
    second.bounds.min.x = 10.;
    second.bounds.max.x = 20.;
    regions.push(second);
    rigs.interiors.insert(
        "Chamber".into(),
        CameraRig {
            yaw_degrees: 90.,
            zoom: 6.,
            fov_degrees: 75.,
            ..CameraRig::default()
        },
    );
    let mut c = RigController::new(&rigs.exterior);
    for _ in 0..300 {
        c.update(&rigs, &regions, Some(Vec3::new(5., 5., 0.)), 1. / 60.);
    }
    c.zoom(-20.);
    c.orbit(1., 0.1);
    let manual = c.pose();
    assert!(manual.zoom < 1.5);
    for i in 0..300 {
        c.update(
            &rigs,
            &regions,
            Some(Vec3::new(5. + i as f32 / 100., 5., 0.)),
            1. / 60.,
        );
        assert!((c.pose().zoom - manual.zoom).abs() < 1e-4);
        assert!(angle_delta(c.pose().yaw_degrees, manual.yaw_degrees).abs() < 1e-4);
    }
    c.update(&rigs, &regions, Some(Vec3::new(15., 5., 0.)), 1. / 60.);
    assert!(
        (c.pose().zoom - manual.zoom).abs() < 0.2,
        "no snap to authored zoom"
    );
    for _ in 0..300 {
        c.update(&rigs, &regions, Some(Vec3::new(15., 5., 0.)), 1. / 60.);
    }
    let p = c.pose();
    assert!((p.zoom - 6.).abs() < 0.001);
    assert!((p.yaw_degrees - 90.).abs() < 0.001);
    assert!((p.fov_degrees - 75.).abs() < 0.001);
}

#[test]
fn full_orbit_wraps_and_new_input_wins_over_a_transition() {
    let (mut rigs, regions) = setup();
    let r = rigs.interiors.get_mut("Room").unwrap();
    r.yaw_limit_degrees = 180.;
    r.min_zoom = 0.5;
    let mut c = RigController::new(&rigs.exterior);
    for _ in 0..150 {
        let before = c.pose().yaw_degrees;
        c.orbit(0.1, 0.);
        assert!((angle_delta(c.pose().yaw_degrees, before) - 0.1_f32.to_degrees()).abs() < 0.001);
    }
    c.zoom(-10.);
    let p = c.pose();
    c.update(&rigs, &regions, Some(Vec3::new(5., 5., 0.)), 1. / 60.);
    for _ in 0..300 {
        c.update(&rigs, &regions, Some(Vec3::new(5., 5., 0.)), 1. / 60.);
    }
    assert!((c.pose().zoom - p.zoom).abs() < 0.001);
    assert!(angle_delta(c.pose().yaw_degrees, p.yaw_degrees).abs() < 0.001);
}

#[test]
fn changing_fov_expands_view_without_dollying_and_stays_time_based() {
    use crate::camera::{Camera, Projection};
    let projection = Projection::ZoomPerspective {
        start_zoom: 1.5,
        end_zoom: 5.,
        vertical_fov_degrees: 45.,
        near_clip: 0.05,
        smoothing_seconds: 0.16,
    };
    let mut camera = Camera::new(Vec3::default());
    camera.set_projection(projection);
    let mut rig = CameraRig {
        zoom: 12.,
        ..CameraRig::default()
    };
    camera.preview_rig(&rig, Vec3::default());
    let narrow = camera.view();
    rig.fov_degrees = 75.;
    camera.preview_rig(&rig, Vec3::default());
    let wide = camera.view();
    assert!(wide.half_width > narrow.half_width * 1.8);
    assert!((wide.convergence - narrow.convergence).abs() < 1e-5);
    let p = wide.right.scaled(0.5);
    let projected = wide.project(p).unwrap();
    assert!(
        (projected.0 - wide.width * 0.5).abs()
            < (narrow.project(p).unwrap().0 - narrow.width * 0.5).abs()
    );
    let bounds = io_types::Bounds {
        min: p - Vec3::new(0.1, 0.1, 0.1),
        max: p + Vec3::new(0.1, 0.1, 0.1),
    };
    assert!(camera
        .pick_depth(projected.0, projected.1, bounds)
        .is_some());
    assert!(wide.sees(bounds));
    let (mut rigs, regions) = setup();
    rigs.interiors.get_mut("Room").unwrap().fov_degrees = 75.;
    let mut a = RigController::new(&rigs.exterior);
    let mut b = a.clone();
    for _ in 0..30 {
        a.update(&rigs, &regions, Some(Vec3::new(5., 5., 0.)), 1. / 60.);
    }
    for _ in 0..60 {
        b.update(&rigs, &regions, Some(Vec3::new(5., 5., 0.)), 1. / 120.);
    }
    assert!(a.pose().fov_degrees > 45. && a.pose().fov_degrees < 75.);
    assert!((a.pose().fov_degrees - b.pose().fov_degrees).abs() < 0.001);
}

#[test]
fn envelope_regions_preserve_player_angles_and_zoom() {
    let (mut rigs, regions) = setup();
    rigs.motion = io_scene::RigMotion::InteriorEnvelope {
        clearance: 0.12,
        close_distance: 0.6,
        max_retreat: 8.,
        max_rise: 3.,
        max_fov_degrees: 95.,
    };
    let mut controller = RigController::new(&rigs.exterior);
    controller.orbit(1.5, 0.2);
    controller.zoom(-5.);
    let requested = controller.pose();
    for point in [
        Some(Vec3::new(5., 5., 0.)),
        None,
        Some(Vec3::new(5., 5., 0.)),
    ] {
        for _ in 0..300 {
            controller.update(&rigs, &regions, point, 1. / 60.);
        }
        let pose = controller.pose();
        assert!((pose.zoom - requested.zoom).abs() < 1e-5);
        assert!((pose.yaw_degrees - requested.yaw_degrees).abs() < 1e-5);
        assert!((pose.pitch_degrees - requested.pitch_degrees).abs() < 1e-5);
        assert_eq!(pose.min_zoom, rigs.exterior.min_zoom);
    }
}
