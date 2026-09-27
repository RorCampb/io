use super::*;
use crate::camera::{Camera, Projection};

fn room() -> io_world::Interior {
    io_world::Interior {
        name: "Room".into(),
        bounds: Bounds {
            min: Vec3::new(-6., -6., 10.),
            max: Vec3::new(6., 6., 14.),
        },
        ceiling: Some(14.),
        entry_direction: Vec3::new(0., -1., 0.),
    }
}
pub(super) fn motion() -> RigMotion {
    RigMotion::InteriorEnvelope {
        clearance: 0.12,
        close_distance: 0.6,
        max_retreat: 8.,
        max_rise: 3.,
        max_fov_degrees: 95.,
    }
}
fn camera() -> (Camera, io_scene::CameraRigs, Vec<io_world::Interior>) {
    let mut rigs = io_scene::CameraRigs {
        motion: motion(),
        ..Default::default()
    };
    rigs.interiors.insert("Room".into(), CameraRig::default());
    let mut c = Camera::new(Vec3::new(0., 0., 11.));
    c.set_projection(Projection::ZoomPerspective {
        start_zoom: 1.5,
        end_zoom: 5.,
        vertical_fov_degrees: 45.,
        near_clip: 0.05,
        smoothing_seconds: 0.16,
    });
    (c, rigs, vec![room()])
}
fn settle(
    c: &mut Camera,
    rigs: &io_scene::CameraRigs,
    regions: &[io_world::Interior],
    foot: Option<Vec3>,
) {
    for _ in 0..360 {
        let h = c.track_rigs(rigs, regions, foot, 1. / 60.);
        if let Some(p) = foot {
            c.set_target(p + Vec3::new(0., 0., h));
        }
    }
}
#[test]
fn orbit_slice_and_zoom_path_stay_in_a_translated_upper_floor() {
    let b = room().bounds;
    let pivot = Vec3::new(1., -2., 11.);
    for yaw in 0..360 {
        let mut rig = CameraRig {
            yaw_degrees: yaw as f32,
            ..Default::default()
        };
        for pitch in [2., 35., 85.] {
            rig.pitch_degrees = pitch;
            let l = limits(b, pivot, rig.yaw_degrees.to_radians(), 0.25, 8., 3., 0.6).unwrap();
            let mut previous = Pose {
                retreat: 0.,
                rise: 0.,
                fov: 0.,
            };
            for step in 0..=100 {
                let zoom = rig.max_zoom * (rig.min_zoom / rig.max_zoom).powf(step as f32 / 100.);
                let p = path(l, zoom, &rig, 95.);
                assert!(
                    p.retreat + 1e-5 >= previous.retreat
                        && p.rise + 1e-5 >= previous.rise
                        && p.fov + 1e-5 >= previous.fov
                );
                let (s, c) = rig.yaw_degrees.to_radians().sin_cos();
                let eye = pivot + Vec3::new(s * p.retreat, c * p.retreat, p.rise);
                assert!(eye.x >= b.min.x + 0.249 && eye.x <= b.max.x - 0.249);
                assert!(eye.y >= b.min.y + 0.249 && eye.y <= b.max.y - 0.249);
                assert!(eye.z >= b.min.z + 0.249 && eye.z <= b.max.z - 0.249);
                previous = p;
            }
        }
    }
}
#[test]
fn insufficient_space_is_rejected_without_dividing_by_parallel_planes() {
    let b = room().bounds;
    for yaw in [0_f32, 90., 180., 270.] {
        let l = limits(
            b,
            Vec3::new(0., 0., 11.),
            yaw.to_radians(),
            0.25,
            8.,
            3.,
            0.6,
        )
        .unwrap();
        assert!((l.retreat - 5.75).abs() < 1e-4);
    }
    assert!(limits(b, Vec3::default(), 0., 0.25, 8., 3., 0.6).is_none());
    assert!(limits(b, Vec3::new(0., 0., 11.), 0., 8., 8., 3., 0.6).is_none());
}
#[test]
fn zoom_remains_reversible_and_exit_recovers_requested_orthographic_view() {
    let (mut c, rigs, regions) = camera();
    let foot = Some(Vec3::new(0., 0., 10.));
    settle(&mut c, &rigs, &regions, foot);
    c.zoom_by(100.);
    settle(&mut c, &rigs, &regions, foot);
    let close = c.view();
    c.zoom_by(-100.);
    settle(&mut c, &rigs, &regions, foot);
    let wide = c.view();
    assert!(wide.half_height > close.half_height * 4.);
    assert!(wide.convergence > 0. && c.fov_degrees() > 94.9);
    c.zoom_by(100.);
    settle(&mut c, &rigs, &regions, foot);
    assert!((c.view().half_height - close.half_height).abs() < 0.001);
    c.zoom_by(-100.);
    c.orbit(1., 0.);
    settle(&mut c, &rigs, &regions, None);
    assert!(c.view().convergence < 1e-8);
    assert!((c.capture_rig().0 - 102.29578).abs() < 0.01);
    assert!((c.capture_rig().2 - 0.5).abs() < 0.001);
}
#[test]
fn full_orbit_projection_and_picking_share_the_resolved_view() {
    let (mut c, rigs, regions) = camera();
    let foot = Some(Vec3::new(0., 0., 10.));
    settle(&mut c, &rigs, &regions, foot);
    for i in 0..72 {
        c.orbit(std::f32::consts::TAU / 72., 0.);
        c.zoom_by(if i < 36 { -0.2 } else { 0.2 });
        c.track_rigs(&rigs, &regions, foot, 1. / 60.);
        let v = c.view();
        assert!(v.matrix().iter().all(|x| x.is_finite()));
        let p = v.target;
        let screen = v.project(p).unwrap();
        let b = Bounds {
            min: p - Vec3::new(0.1, 0.1, 0.1),
            max: p + Vec3::new(0.1, 0.1, 0.1),
        };
        assert!(v.pick_depth(screen.0, screen.1, b).is_some());
        assert!((v.half_height - c.half_view().1).abs() < 1e-5);
    }
}
#[test]
fn low_room_and_upper_floor_do_not_activate_each_other() {
    let (mut c, rigs, regions) = camera();
    settle(&mut c, &rigs, &regions, Some(Vec3::default()));
    let outside = c.view().convergence;
    settle(&mut c, &rigs, &regions, Some(Vec3::new(0., 0., 10.)));
    assert!((c.view().convergence - outside).abs() > 0.01);
    settle(&mut c, &rigs, &regions, Some(Vec3::default()));
    assert!((c.view().convergence - outside).abs() < 1e-5);
}

#[test]
fn adjoining_low_passage_limits_eye_height_without_lending_its_width() {
    let mut chamber = room();
    chamber.bounds = Bounds {
        min: Vec3::new(-6., -12., 0.),
        max: Vec3::new(6., 0., 4.),
    };
    chamber.ceiling = Some(4.);
    let passage = io_world::Interior {
        name: "Passage".into(),
        bounds: Bounds {
            min: Vec3::new(-1.5, 0., 0.),
            max: Vec3::new(1.5, 7., 1.5),
        },
        ceiling: Some(1.5),
        entry_direction: Vec3::new(0., -1., 0.),
    };
    let volumes = spaces(&[chamber, passage], &io_scene::CameraRigs::default());
    assert_eq!(
        volumes[0].max.y, 0.,
        "no tall virtual vestibule over the low passage"
    );
    assert_eq!(
        volumes[1].max.y, 10.,
        "exterior entrance can anticipate outside"
    );
    let pivot = Vec3::new(0., -1., 1.);
    let mut p = Pose {
        retreat: 5.,
        rise: 2.,
        fov: 70.,
    };
    constrain(&mut p, &volumes, pivot, 0., 0.2);
    assert!(pivot.y + p.retreat + 0.2 <= 1e-5 || pivot.z + p.rise <= 1.3 + 1e-5);
    assert!(p.retreat < 1.);
    let mut p = Pose {
        retreat: 5.,
        rise: 0.,
        fov: 70.,
    };
    constrain(&mut p, &volumes, pivot, 0., 0.2);
    assert_eq!(
        p.retreat, 5.,
        "a low view can pass through the shared entrance"
    );
    constrain(&mut p, &volumes, pivot, std::f32::consts::FRAC_PI_4, 0.2);
    assert!(
        p.retreat < 2.,
        "diagonal view cannot cross the passage side wall"
    );
    let gap = [
        Bounds {
            min: Vec3::new(-1., -1., 0.),
            max: Vec3::new(1., 1., 1.),
        },
        Bounds {
            min: Vec3::new(-1., -1., 2.),
            max: Vec3::new(1., 1., 3.),
        },
    ];
    assert_eq!(
        union_exit(&gap, Vec3::new(0., 0., 0.5), Vec3::new(0., 0., 1.)),
        Some(0.5)
    );
}

#[test]
fn envelope_contract_is_strict_and_rejects_invalid_limits() {
    let value = serde_json::to_value(motion()).unwrap();
    let decoded: RigMotion = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, motion());
    assert!(decoded.validate().is_ok());
    for (field, bad) in [
        ("clearance", 0.),
        ("close_distance", 4.),
        ("max_retreat", 0.1),
        ("max_rise", 0.),
        ("max_fov_degrees", 180.),
    ] {
        let mut v = value.clone();
        v[field] = serde_json::json!(bad);
        assert!(serde_json::from_value::<RigMotion>(v)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut v = value;
    v["typo"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RigMotion>(v).is_err());
    let mut legacy = serde_json::to_value(io_scene::CameraRigs::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("motion");
    assert_eq!(
        serde_json::from_value::<io_scene::CameraRigs>(legacy)
            .unwrap()
            .motion,
        RigMotion::Authored
    );
}

#[test]
fn crossing_a_shared_doorway_keeps_the_resolved_eye_in_connected_space() {
    let regions = vec![
        io_world::Interior {
            name: "Passage".into(),
            bounds: Bounds {
                min: Vec3::new(-1.5, 0., 0.),
                max: Vec3::new(1.5, 7., 1.5),
            },
            ceiling: Some(1.5),
            entry_direction: Vec3::new(0., -1., 0.),
        },
        io_world::Interior {
            name: "Room".into(),
            bounds: Bounds {
                min: Vec3::new(-6., -12., 0.),
                max: Vec3::new(6., 0., 4.),
            },
            ceiling: Some(4.),
            entry_direction: Vec3::new(0., -1., 0.),
        },
    ];
    for yaw in [0., 45., 90., 180.] {
        for zoom in [0.5, 3., 16.] {
            let (mut c, mut rigs, _) = camera();
            rigs.exterior.yaw_degrees = yaw;
            rigs.exterior.zoom = zoom;
            rigs.interiors.insert(
                "Passage".into(),
                CameraRig {
                    target_height: 0.7,
                    ..CameraRig::default()
                },
            );
            let spaces = spaces(&regions, &rigs);
            settle(&mut c, &rigs, &regions, Some(Vec3::new(0., 3., 0.)));
            for i in (0..800).chain((0..800).rev()) {
                let foot = Vec3::new(0., 3. - i as f32 * 0.01, 0.);
                let h = c.track_rigs(&rigs, &regions, Some(foot), 1. / 60.);
                c.set_target(foot + Vec3::new(0., 0., h));
                let v = c.view();
                assert!(v.matrix().iter().all(|x| x.is_finite()));
                let eye = v.target + v.forward.scaled(1. / v.convergence);
                assert!(
                    spaces.iter().any(|b| eye.x >= b.min.x - 0.02
                        && eye.x <= b.max.x + 0.02
                        && eye.y >= b.min.y - 0.02
                        && eye.y <= b.max.y + 0.02
                        && eye.z >= b.min.z - 0.02
                        && eye.z <= b.max.z + 0.02),
                    "yaw={yaw} zoom={zoom} foot={foot:?} eye={eye:?}"
                );
            }
        }
    }
}
