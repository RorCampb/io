"""Author Collapse on the generated runner; leave the original Blender file intact.

Blender --background --python tools/add_runner_collapse.py
"""
import json
import math
import struct
from pathlib import Path

import bpy
from mathutils import Matrix, Quaternion, Vector


def add_collapse(scene, rig):
    run = bpy.data.actions["Run"]
    rig.animation_data.action = run
    scene.frame_set(13)
    bpy.context.view_layer.update()
    start = {pb.name: pb.matrix_basis.decompose() for pb in rig.pose.bones}
    action = bpy.data.actions.get("Collapse") or bpy.data.actions.new("Collapse")
    for curve in list(action.fcurves):
        action.fcurves.remove(curve)
    action.use_fake_user = True
    rig.animation_data.action = action
    meshes = [obj for obj in scene.objects if obj.type == "MESH"]

    def smooth(t):
        t = min(1, max(0, t))
        return t * t * (3 - 2 * t)

    for frame in range(1, 38):
        scene.frame_set(frame)
        t = (frame - 1) / 36
        relax = smooth(t / 0.8)
        buckle = math.sin(math.pi * min(1, t / 0.75))
        for pb in rig.pose.bones:
            location, rotation, scale = start[pb.name]
            angle = 0.
            if pb.name.startswith("thigh"):
                angle = 0.55 * buckle + 0.12 * relax
            elif pb.name.startswith("shin"):
                angle = -1.0 * buckle - 0.26 * relax
            elif pb.name.startswith("upper_arm"):
                angle = -0.35 * buckle - 0.15 * relax
            elif pb.name.startswith("forearm"):
                angle = -0.2 * relax
            elif pb.name == "head":
                angle = 0.10 * relax
            pb.rotation_mode = "QUATERNION"
            pb.location = location.lerp(Vector((0, 0, 0)), relax)
            pb.rotation_quaternion = rotation.slerp(Quaternion((1, 0, 0), angle), relax)
            pb.scale = scale.lerp(Vector((1, 1, 1)), relax)

        # Authored fall, not ragdoll physics. Correct root height from the actual mesh
        # so the body stays above the ground throughout the fall and settles on it.
        fall = smooth((t - 0.08) / 0.75)
        root = rig.pose.bones["root"]
        root.matrix = (Matrix.Translation((0, 0.18 * fall, 0))
                       @ Matrix.Rotation(-math.pi / 2 * fall, 4, "X")
                       @ rig.data.bones["root"].matrix_local)
        bpy.context.view_layer.update()
        depsgraph = bpy.context.evaluated_depsgraph_get()
        floor = math.inf
        for obj in meshes:
            evaluated = obj.evaluated_get(depsgraph)
            mesh = evaluated.to_mesh()
            floor = min(floor, min((evaluated.matrix_world @ v.co).z for v in mesh.vertices))
            evaluated.to_mesh_clear()
        matrix = root.matrix.copy()
        matrix.translation.z += 0.005 - floor
        root.matrix = matrix
        bpy.context.view_layer.update()
        for pb in rig.pose.bones:
            for channel in ("location", "rotation_quaternion", "scale"):
                pb.keyframe_insert(channel, frame=frame)
    for curve in action.fcurves:
        for key in curve.keyframe_points:
            key.interpolation = "LINEAR"
    rig.animation_data.action = run
    scene.frame_set(1)
    rig["clip_notes"] = "Run: looping 1s. Collapse: one-shot 1.5s; hold final pose."
    return action


if __name__ == "__main__":
    out = Path(__file__).resolve().parents[1] / "assets" / "street-kit"
    bpy.ops.wm.open_mainfile(filepath=str(out / "street-kit.blend"))
    bpy.context.preferences.filepaths.save_version = 0
    scene = bpy.data.scenes["runner"]
    bpy.context.window.scene = scene
    add_collapse(scene, bpy.data.objects["RunnerRig"])
    bpy.ops.export_scene.gltf(filepath=str(out / "runner.glb"), export_format="GLB",
        use_active_scene=True, export_animations=True, export_animation_mode="ACTIONS",
        export_force_sampling=True, export_frame_range=False, export_frame_step=1,
        export_skins=True, export_yup=True, export_materials="EXPORT",
        export_cameras=False, export_lights=False, export_extras=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(out / "runner-collapse.blend"))
    data = (out / "runner.glb").read_bytes()
    size = struct.unpack_from("<I", data, 12)[0]
    document = json.loads(data[20:20 + size])
    clips = [clip["name"] for clip in document["animations"]]
    assert "Run" in clips and "Collapse" in clips, clips
    catalog = json.loads((out / "catalog.json").read_text())
    catalog["assets"]["runner"].update(bytes=len(data), clips=clips, animations=clips)
    (out / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
    print("EXPORTED runner:", clips, flush=True)
