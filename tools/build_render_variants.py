"""Run with Blender --background --python tools/build_render_variants.py.

Generate demonstration LODs from the existing authored scenes. Source .blend
files and high-detail GLBs are retained; only lods/ and variants-demo.json change.
"""
import json
from pathlib import Path

import bpy


ROOT = Path(__file__).resolve().parents[1]
KIT = ROOT / "assets" / "street-kit"
OUT = KIT / "lods"
OUT.mkdir(exist_ok=True)
ASSETS = {
    "cottage": "house_cottage",
    "townhouse": "house_townhouse",
    "lamp": "street_lamp",
    "tree": "street_tree",
    "runner": "runner",
}

for name, scene_name in ASSETS.items():
    source = "runner-collapse.blend" if name == "runner" else "street-kit.blend"
    bpy.ops.wm.open_mainfile(filepath=str(KIT / source))
    scene = bpy.data.scenes[scene_name]
    bpy.context.window.scene = scene
    for obj in scene.objects:
        if obj.type != "MESH":
            continue
        modifier = obj.modifiers.new("LOD simplification", "DECIMATE")
        modifier.ratio = 0.2
        modifier.use_collapse_triangulate = True
        # Simplify the bind mesh before armature deformation.
        with bpy.context.temp_override(object=obj, active_object=obj):
            bpy.ops.object.modifier_move_to_index(modifier=modifier.name, index=0)
    bpy.ops.export_scene.gltf(
        filepath=str(OUT / (scene_name + "_low.glb")), export_format="GLB",
        use_active_scene=True, export_apply=True,
        export_animations=name == "runner", export_animation_mode="ACTIONS",
        export_force_sampling=True, export_frame_range=False, export_frame_step=1,
        export_skins=name == "runner", export_yup=True, export_materials="EXPORT",
        export_cameras=False, export_lights=False, export_extras=True,
    )

scene = json.loads((KIT / "demo.json").read_text())
scene["appearances"] = []
for name, scene_name in ASSETS.items():
    scene["assets"].append({"name": name + "_low", "file": "lods/" + scene_name + "_low.glb"})
    scene["appearances"].append({
        "name": name,
        "base_asset": name,
        "hysteresis": 0.1,
        "lods": [
            {"asset": name, "min_screen_pixels": 250},
            {"asset": name + "_low", "min_screen_pixels": 0},
        ],
    })
for item in scene["items"]:
    if item.get("asset") in ASSETS:
        item["appearance"] = item.pop("asset")
(KIT / "variants-demo.json").write_text(json.dumps(scene, indent=2) + "\n")
print("Generated low meshes and assets/street-kit/variants-demo.json", flush=True)
