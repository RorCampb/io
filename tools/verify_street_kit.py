"""Round-trip generated GLBs through Blender and check geometry and the run loop."""

import json
import math
from pathlib import Path

import bpy


ROOT = Path(__file__).resolve().parents[1] / "assets" / "street-kit"
catalog = json.loads((ROOT / "catalog.json").read_text())
for name, info in catalog["assets"].items():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = 24
    bpy.ops.import_scene.gltf(filepath=str(ROOT / info["file"]))
    rigs = [obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE"]
    # Blender's importer also creates mesh widgets for displaying skeleton joints.
    widgets = {pb.custom_shape for rig in rigs for pb in rig.pose.bones if pb.custom_shape}
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH" and obj not in widgets]
    assert meshes, name
    assert all(math.isfinite(value) for obj in meshes for v in obj.data.vertices for value in v.co), name
    assert sum(len(obj.data.polygons) for obj in meshes) == info["triangles"], name
    if name == "runner":
        assert len(rigs) == 1
        rig = rigs[0]
        assert len(rig.data.bones) == 14
        assert all(any(mod.type == "ARMATURE" for mod in obj.modifiers) for obj in meshes)
        assert any(action.name.startswith("Run_") for action in bpy.data.actions)
        poses = []
        for frame in (1, 7, 13, 25):
            bpy.context.scene.frame_set(frame)
            bpy.context.view_layer.update()
            poses.append([tuple(v for row in pb.matrix for v in row) for pb in rig.pose.bones])
        difference = lambda a, b: max(abs(x - y) for pa, pb in zip(a, b) for x, y in zip(pa, pb))
        assert difference(poses[0], poses[1]) > 0.1, "Run clip must change the pose"
        assert difference(poses[0], poses[2]) > 0.1, "Legs must alternate"
        assert difference(poses[0], poses[3]) < 0.001, "Run loop endpoints must match"
    print("VERIFIED", name, info["triangles"], "triangles", flush=True)
print("All six exported assets round-trip successfully.", flush=True)
