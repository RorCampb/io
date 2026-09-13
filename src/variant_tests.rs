use crate::{
    app::{Action, App},
    appearance::Appearance,
    camera::Camera,
    config::SceneConfig,
    demo,
    model::ModelLibrary,
    projection::Frame,
};
use io_types::{Bounds, Vec3};
use io_world::{Item, Space, World};
use serde_json::{json, Value};
use std::path::Path;

fn fixture() -> Value {
    json!({
        "version": 1, "dimensions": [1000,1000,100], "origin": [10,10,0],
        "camera": {"target": [0,0,0], "zoom": 1},
        "assets": [
            {"name": "high", "builtin": "box"},
            {"name": "low", "builtin": "box"},
            {"name": "broken", "builtin": "box"}
        ],
        "appearances": [{"name": "prop", "base_asset": "high", "hysteresis": 0.1, "lods": [
            {"asset": "high", "min_screen_pixels": 50},
            {"asset": "low", "min_screen_pixels": 0}
        ], "states": [{"name": "broken", "lods": [
            {"asset": "broken", "min_screen_pixels": 0}
        ]}]}],
        "items": [{"name": "prop", "appearance": "prop", "position": [0,0,0],
            "on_death_visual_state": "broken"}]
    })
}

fn load(value: Value) -> Result<ModelLibrary, String> {
    let config: SceneConfig = serde_json::from_value(value).map_err(|e| e.to_string())?;
    ModelLibrary::from_config(config, Path::new("."))
}

#[test]
fn model_less_moving_items_simulate_but_do_not_enter_render_packets() {
    let library = Box::leak(Box::new(load(fixture()).unwrap()));
    let world = demo::world(library).unwrap();
    let visible = world.items()[0].clone();
    let origin = visible.transform.anchor;
    let mut hidden = Item {
        id: 2,
        ..Item::default()
    };
    hidden.transform = io_world::Transform::new(origin, Vec3::new(1., 1., 1.), 0.).unwrap();
    hidden.motion =
        io_world::PathMotion::new(vec![origin, origin + Vec3::new(20., 0., 0.)], 2., 0.);
    let world = World::try_new(
        Space::new(Vec3::new(1000., 1000., 100.)),
        vec![visible, hidden],
    )
    .unwrap();
    let mut app = App::with_world(world, Camera::new(origin), None, library);
    app.update(0.2);
    assert_eq!(app.active_count(), 1);
    assert!(app.world().item(2).unwrap().transform.anchor.x > origin.x);
    assert!(!app.set_visual_state(2, "broken"));
    let frame = app.frame(1).unwrap();
    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.instances[0].item_id, 1);
    assert!(frame.joint_matrices.is_empty());
}

#[test]
fn frames_reject_unresolved_references_instead_of_silently_omitting_items() {
    let library = load(fixture()).unwrap();
    let item = demo::world(&library).unwrap().items()[0].clone();
    let camera = Camera::new(item.transform.anchor);
    let space = || Space::new(Vec3::new(1000., 1000., 100.));
    let mut invalid_appearance = item.clone();
    invalid_appearance
        .renderable
        .as_mut()
        .unwrap()
        .appearance_id = io_types::AppearanceId(999);
    let mut invalid_state = item.clone();
    invalid_state.renderable.as_mut().unwrap().visual_state = io_types::VisualStateId(2);
    invalid_state
        .renderable
        .as_mut()
        .unwrap()
        .visual_state_count = 3;
    for (invalid, message) in [
        (invalid_appearance, "unresolved appearance"),
        (invalid_state, "unresolved visual state"),
    ] {
        let world = World::try_new(space(), vec![invalid]).unwrap();
        let mut frame = Frame::default();
        assert!(frame
            .build(&world, &camera, 1, &library, 1., &[], false)
            .unwrap_err()
            .contains(message));
        assert_eq!(frame.serial, 0);
    }
    let world = World::try_new(space(), vec![item]).unwrap();
    assert!(Frame::default()
        .build(&world, &camera, 1, &library, f32::NAN, &[], false)
        .is_err());
}

#[test]
fn cameras_choose_independently_and_state_changes_invalidate_cached_frames() {
    let library = Box::leak(Box::new(load(fixture()).unwrap()));
    let world = demo::world(library).unwrap();
    let saved = world.items()[0].clone();
    let mut app = App::with_world(world, Camera::new(saved.transform.anchor), None, library);
    let serial = app.frame(1).unwrap().serial;
    assert_eq!(app.frame(1).unwrap().serial, serial);
    assert_eq!(app.frame(1).unwrap().instances[0].model_id, 2);
    let close = app.create_camera().unwrap();
    app.select_camera(close);
    app.dispatch(Action::Zoom { steps: 20. });
    assert_eq!(app.frame(close).unwrap().instances[0].model_id, 1);
    assert_eq!(app.frame(1).unwrap().instances[0].model_id, 2);
    assert_eq!(app.world().items()[0], saved);
    assert!(!app.set_visual_state(1, "missing"));
    assert!(!app.set_visual_state(999, "broken"));
    assert_eq!(app.frame(1).unwrap().serial, serial);
    assert!(app.set_visual_state(1, "broken"));
    assert!(!app.set_visual_state(1, "broken"));
    assert_eq!(app.frame(1).unwrap().instances[0].model_id, 3);
    assert_eq!(app.frame(close).unwrap().instances[0].model_id, 3);
    assert_eq!(app.world().items()[0].bounds(), saved.bounds());
    assert_eq!(
        app.world().items()[0]
            .durability
            .as_ref()
            .unwrap()
            .current(),
        saved.durability.as_ref().unwrap().current()
    );
}

#[test]
fn hysteresis_and_omission_have_stable_boundaries() {
    let mut config = fixture();
    config["appearances"][0]["lods"][1]["min_screen_pixels"] = json!(10);
    let library = load(config).unwrap();
    let group = &library.appearances[3].states[0];
    assert_eq!(group.select(50., None, 0.1), 0);
    assert_eq!(group.select(49., None, 0.1), 1);
    assert_eq!(group.select(49., Some(0), 0.1), 0);
    assert_eq!(group.select(44., Some(0), 0.1), 1);
    assert_eq!(group.select(51., Some(1), 0.1), 1);
    assert_eq!(group.select(56., Some(1), 0.1), 0);
    assert_eq!(group.select(8., Some(1), 0.1), 2);
    assert_eq!(group.select(10., Some(2), 0.1), 2);
    assert_eq!(group.select(12., Some(2), 0.1), 1);
    let world = demo::world(&library).unwrap();
    let original = world.items()[0].clone();
    let mut camera = Camera::new(original.transform.anchor);
    camera.set_zoom(0.025);
    let mut frame = Frame::default();
    frame
        .build(&world, &camera, 1, &library, 1., &[], false)
        .unwrap();
    assert!(frame.instances.is_empty());
    assert!(frame.joint_matrices.is_empty());
    camera.set_zoom(4.);
    frame
        .build(&world, &camera, 2, &library, 1., &[], false)
        .unwrap();
    assert_eq!(frame.instances[0].model_id, 1);
    assert_eq!(world.items()[0], original);
}

#[test]
fn variant_envelopes_preserve_visibility_and_death_preserves_occupancy() {
    let mut library = load(fixture()).unwrap();
    library.models[1] = io_assets::Model::box_with_bounds(Bounds {
        min: Vec3::new(-20., -20., -20.),
        max: Vec3::new(20., 20., 20.),
    })
    .unwrap();
    library.appearances[3] = Appearance::resolve(
        &library.config.appearances[0],
        &library.names,
        &library.models,
    )
    .unwrap();
    let mut item = demo::world(&library).unwrap().items()[0].clone();
    let camera = Camera::new(Vec3::new(100., 100., 0.));
    item.transform.anchor = camera.target() + camera.basis().0.scaled(camera.half_view().0 + 5.);
    let bounds = item.bounds();
    assert!(!camera.sees(bounds));
    assert!(camera.sees(item.visibility_bounds()));
    let mut world = World::new(Space::new(Vec3::new(1000., 1000., 100.)), vec![item]);
    let mut frame = Frame::default();
    frame
        .build(&world, &camera, 1, &library, 1., &[], false)
        .unwrap();
    assert_eq!(frame.instances.len(), 1);
    world.damage(1, 100);
    frame
        .build(&world, &camera, 2, &library, 1., &[], false)
        .unwrap();
    assert_eq!(frame.instances[0].model_id, 3);
    assert_eq!(world.items()[0].bounds(), bounds);
}

#[test]
fn malformed_variant_definitions_and_references_are_rejected() {
    for (pointer, replacement, message) in [
        (
            "/appearances/0/lods/1/min_screen_pixels",
            json!(50),
            "strictly decreasing",
        ),
        (
            "/appearances/0/lods/0/min_screen_pixels",
            json!(-1),
            "nonnegative",
        ),
        (
            "/appearances/0/lods/0/asset",
            json!("missing"),
            "unknown LOD asset",
        ),
        (
            "/appearances/0/base_asset",
            json!("missing"),
            "unknown base asset",
        ),
        (
            "/appearances/0/states/0/lods",
            json!([]),
            "at least one LOD",
        ),
        (
            "/appearances/0/states/0/name",
            json!("default"),
            "duplicate visual state",
        ),
        ("/appearances/0/hysteresis", json!(0.5), "hysteresis"),
    ] {
        let mut value = fixture();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(load(value).err().unwrap().contains(message));
    }
    let mut value = fixture();
    value["items"][0]["asset"] = json!("high");
    assert!(demo::world(&load(value).unwrap())
        .err()
        .unwrap()
        .contains("exactly one"));
    let mut value = fixture();
    value["items"][0]["visual_state"] = json!("missing");
    assert!(demo::world(&load(value).unwrap())
        .err()
        .unwrap()
        .contains("unknown visual state"));
}

#[test]
fn orthographic_quality_tracks_zoom_and_scale_not_camera_translation() {
    let item = Item::default();
    let mut camera = Camera::new(Vec3::default());
    let pixels = camera.projected_diameter(item.occupancy.local_bounds, item.transform.size);
    camera.set_target(Vec3::new(100., 100., 100.));
    assert_eq!(
        camera.projected_diameter(item.occupancy.local_bounds, item.transform.size),
        pixels
    );
    camera.set_zoom(2.);
    assert_eq!(
        camera.projected_diameter(item.occupancy.local_bounds, item.transform.size),
        pixels * 2.
    );
    assert_eq!(
        camera.projected_diameter(item.occupancy.local_bounds, item.transform.size.scaled(2.)),
        pixels * 4.
    );
}

#[test]
fn huge_district_loads_and_camera_detail_changes_without_losing_world_state() {
    let library = ModelLibrary::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/district/huge.json"),
    )
    .unwrap();
    assert_eq!(library.config.camera.render_distance, 1709.);
    let world = demo::world(&library).unwrap();
    assert_eq!(world.items().len(), 80446);
    assert_eq!(
        world.items().iter().filter(|i| i.motion.is_some()).count(),
        8192
    );
    let mut camera = Camera::new(Vec3::new(5000., 5000., 1.));
    camera.set_zoom(library.config.camera.zoom);
    camera.set_distance(library.config.camera.render_distance);
    let mut frame = Frame::default();
    frame
        .build(&world, &camera, 1, &library, 1., &[], false)
        .unwrap();
    let overview = frame.instances.len();
    assert!(overview > 30000);
    camera.set_zoom(2.2);
    camera.set_distance(120.);
    frame
        .build(&world, &camera, 2, &library, 1., &[], false)
        .unwrap();
    assert!(!frame.instances.is_empty() && frame.instances.len() < overview / 10);
    assert!(frame.candidate_count < world.items().len() / 10);
    assert_eq!(world.items().len(), 80446);
}

#[test]
fn authored_lods_reduce_geometry_and_keep_canonical_animation_and_events() {
    let library = ModelLibrary::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/street-kit/variants-demo.json"),
    )
    .unwrap();
    for name in ["cottage", "townhouse", "lamp", "tree", "runner"] {
        let appearance = library.appearance(library.appearance_names[name]).unwrap();
        let high = library.mesh(appearance.base_mesh).unwrap();
        let low = library
            .mesh(appearance.states[0].variants[1].mesh_id)
            .unwrap();
        assert!(
            low.indices().len() < high.indices().len() / 2,
            "{name} did not simplify"
        );
    }
    let original = demo::world(&library).unwrap();
    let mut runner = original
        .items()
        .iter()
        .find(|i| i.animation.is_some())
        .unwrap()
        .clone();
    runner.motion = None;
    let mut second = runner.clone();
    second.id = 900;
    second.transform.anchor.x += 3.;
    let state = second.animation.as_mut().unwrap();
    state.seek(0.25).unwrap();
    let mut world = World::new(
        Space::new(Vec3::new(10000., 10000., 256.)),
        vec![runner.clone(), second],
    );
    let mut camera = Camera::new(runner.transform.anchor);
    camera.set_zoom(0.025);
    let mut frame = Frame::default();
    frame
        .build(&world, &camera, 1, &library, 1., &[], false)
        .unwrap();
    let appearance = library
        .appearance(runner.renderable.as_ref().unwrap().appearance_id)
        .unwrap();
    let variant = &appearance.states[0].variants[1];
    let source = library.mesh(appearance.base_mesh).unwrap();
    let joints = variant.joint_mapping.len();
    assert_eq!(frame.instances.len(), 2);
    assert!(frame
        .instances
        .iter()
        .all(|i| i.model_id == variant.mesh_id));
    assert_eq!(frame.joint_matrices.len(), joints * 2);
    assert_ne!(
        frame.joint_matrices[..joints],
        frame.joint_matrices[joints..]
    );
    let canonical = source.palette(Some(runner.animation.as_ref().unwrap().clip()), 0.25);
    let expected: Vec<_> = variant
        .joint_mapping
        .iter()
        .map(|&i| canonical[i])
        .collect();
    let second = frame.instances.iter().find(|i| i.item_id == 900).unwrap();
    assert_eq!(
        &frame.joint_matrices[second.joint_offset as usize..second.joint_offset as usize + joints],
        expected
    );
    let saved = world.items().to_vec();
    camera.set_zoom(16.);
    frame
        .build(&world, &camera, 2, &library, 1., &[], false)
        .unwrap();
    assert!(frame
        .instances
        .iter()
        .all(|i| i.model_id == appearance.base_mesh));
    assert_eq!(world.items(), saved);

    // Gameplay events still advance when LOD policy omits the render completely.
    let mut config = fixture();
    config["appearances"][0]["lods"][1]["min_screen_pixels"] = json!(10);
    let hidden = load(config).unwrap();
    let mut actor = demo::world(&hidden).unwrap().items()[0].clone();
    let mut animation = io_world::AnimationState::looping(0);
    animation
        .set_events(
            io_world::AnimationEvents::new(
                1.,
                vec![io_world::AnimationEvent {
                    name: "hit".into(),
                    at: 0.5,
                    target: actor.id,
                    effect: io_world::EffectKind::Damage(io_world::Damage { amount: 100 }),
                }],
            )
            .unwrap(),
        )
        .unwrap();
    actor.animation = Some(animation);
    world = World::new(Space::new(Vec3::new(1000., 1000., 100.)), vec![actor]);
    camera = Camera::new(world.items()[0].transform.anchor);
    camera.set_zoom(0.025);
    frame = Frame::default();
    frame
        .build(&world, &camera, 3, &hidden, 1., &[0], false)
        .unwrap();
    assert!(frame.instances.is_empty());
    world.simulate(&[0], 0.5);
    assert_eq!(world.items()[0].durability.as_ref().unwrap().current(), 0);
    assert_eq!(
        world.items()[0].renderable.as_ref().unwrap().visual_state.0,
        1
    );
}
