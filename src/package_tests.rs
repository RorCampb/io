use crate::{config::SceneConfig, demo, model::ModelLibrary};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "io-packages-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, value: &Value) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn package() -> Value {
    json!({"format":"io.asset-package","version":1,"name":"test-kit",
        "coordinates":{"units":"meters","up_axis":"+Y","handedness":"right","origin":"authored"},
        "requires":["opaque_materials"],
        "assets":[{"name":"box","builtin":"box","kind":"static","clips":[]}],
        "appearances":[{"name":"prop","base_asset":"box","occupancy_bounds":{"min":[-1,-1,0],"max":[1,1,2]}}]})
}
fn scene() -> Value {
    json!({"version":1,"dimensions":[100,100,100],"origin":[0,0,0],"camera":{"target":[0,0,0],"zoom":1},
        "packages":[{"namespace":"first","file":"../kit/package.json"},{"namespace":"second","file":"../kit/package.json"}],
        "items":[{"name":"a","appearance":"first/prop","position":[1,1,0]},
                 {"name":"b","appearance":"second/prop","position":[5,5,0]}]})
}

#[test]
fn namespaced_packages_are_reusable_and_paths_are_scene_relative() {
    let f = Fixture::new();
    f.write("kit/package.json", &package());
    f.write("scenes/a.json", &scene());
    let library = ModelLibrary::load(&f.0.join("scenes/a.json")).unwrap();
    let world = demo::world(&library).unwrap();
    assert_ne!(
        world.items()[0].renderable.as_ref().unwrap().appearance_id,
        world.items()[1].renderable.as_ref().unwrap().appearance_id
    );
    assert_eq!(world.items()[0].occupancy.local_bounds.min.x, -1.);
    assert_eq!(
        world.items()[0]
            .renderable
            .as_ref()
            .unwrap()
            .local_bounds
            .min
            .x,
        0.
    );
    f.write("scenes/b.json", &scene());
    assert_eq!(
        crate::validate_scene(&f.0.join("scenes/b.json"))
            .unwrap()
            .items,
        2
    );
    assert_eq!(
        crate::validate_package(&f.0.join("kit/package.json"))
            .unwrap()
            .meshes,
        1
    );
}

#[test]
fn unsupported_contract_categories_and_mismatched_content_fail_before_world_creation() {
    let f = Fixture::new();
    for (pointer, value) in [
        ("/version", json!(2)),
        ("/format", json!("other")),
        ("/coordinates/units", json!("feet")),
        ("/coordinates/up_axis", json!("+Z")),
        ("/requires/0", json!("textures")),
        ("/assets/0/builtin", json!("unknown")),
        ("/assets/0/kind", json!("moving")),
        ("/assets/0/kind", json!("skinned")),
        ("/assets/0/clips", json!(["Run"])),
        ("/appearances/0/base_asset", json!("missing")),
        ("/appearances/0/occupancy_bounds/min", json!([10, 10, 10])),
    ] {
        let mut data = package();
        *data.pointer_mut(pointer).unwrap() = value;
        f.write("package.json", &data);
        assert!(
            crate::validate_package(&f.0.join("package.json")).is_err(),
            "accepted {pointer}"
        );
    }
    let mut data = package();
    data["requires"] = json!(["opaque_materials", "skeletal_animation"]);
    data["assets"][0]["kind"] = json!("skinned");
    f.write("package.json", &data);
    assert!(crate::validate_package(&f.0.join("package.json"))
        .err()
        .unwrap()
        .contains("skeleton"));
    data = package();
    data["assets"][0]["file"] = json!("box.glb");
    f.write("package.json", &data);
    assert!(crate::validate_package(&f.0.join("package.json")).is_err());
}

#[test]
fn namespace_collisions_and_package_path_escapes_are_rejected() {
    let f = Fixture::new();
    f.write("kit/package.json", &package());
    let mut data = scene();
    data["packages"][1]["namespace"] = json!("first");
    f.write("scenes/a.json", &data);
    assert!(ModelLibrary::load(&f.0.join("scenes/a.json"))
        .err()
        .unwrap()
        .contains("duplicate package namespace"));
    let mut data = package();
    data["assets"][0].as_object_mut().unwrap().remove("builtin");
    data["assets"][0]["file"] = json!("../escape.glb");
    f.write("kit/package.json", &data);
    assert!(crate::validate_package(&f.0.join("kit/package.json")).is_err());
}

#[test]
fn in_memory_scene_loading_cannot_bypass_numeric_validation() {
    let value = json!({"version":1,"dimensions":[-1,100,100],"origin":[0,0,0],
        "camera":{"target":[0,0,0],"zoom":1},"assets":[],"items":[]});
    let config: SceneConfig = serde_json::from_value(value).unwrap();
    assert!(ModelLibrary::from_config(config, Path::new(".")).is_err());
}

#[test]
fn package_gltf_dependencies_cannot_escape_the_package_directory() {
    let f = Fixture::new();
    let mut data = package();
    data["assets"][0].as_object_mut().unwrap().remove("builtin");
    data["assets"][0]["file"] = json!("mesh.gltf");
    f.write("kit/package.json", &data);
    for uri in [
        "../outside.bin",
        "/tmp/outside.bin",
        "https://example.invalid/x.bin",
        "%2e%2e/outside.bin",
    ] {
        f.write(
            "kit/mesh.gltf",
            &json!({"asset":{"version":"2.0"},
            "buffers":[{"uri":uri,"byteLength":4}]}),
        );
        assert!(crate::validate_package(&f.0.join("kit/package.json"))
            .unwrap_err()
            .contains("dependency must be"));
    }
    #[cfg(unix)]
    {
        f.write("outside.bin", &json!(123));
        std::os::unix::fs::symlink(f.0.join("outside.bin"), f.0.join("kit/link.bin")).unwrap();
        f.write(
            "kit/mesh.gltf",
            &json!({"asset":{"version":"2.0"},
            "buffers":[{"uri":"link.bin","byteLength":4}]}),
        );
        assert!(crate::validate_package(&f.0.join("kit/package.json"))
            .unwrap_err()
            .contains("dependency leaves package"));
    }
}

#[test]
fn actual_blender_package_preserves_scene_state_and_animation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let old = ModelLibrary::load(&root.join("assets/street-kit/variants-demo.json")).unwrap();
    let packaged = ModelLibrary::load(&root.join("assets/street-kit/package-demo.json")).unwrap();
    let mut a = demo::world(&old).unwrap();
    let mut b = demo::world(&packaged).unwrap();
    let active: Vec<_> = (0..a.items().len()).collect();
    a.simulate(&active, 0.2);
    b.simulate(&active, 0.2);
    assert_eq!(a.items(), b.items());
}
