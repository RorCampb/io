"""Inspect/import matching-skeleton FBX packs using Blender's Python runtime.

blender --background --python tools/import_mixamo.py -- --source PATH --inspect
"""
import argparse
import json
import math
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fbx_pack_contract import check_skeleton, clip_id, planar_drift

import bpy
from mathutils import Matrix, Vector


def read_fbx(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = 30
    bpy.ops.import_scene.fbx(filepath=str(path), use_anim=True)
    rigs = [o for o in bpy.context.scene.objects if o.type == "ARMATURE"]
    if len(rigs) != 1:
        raise ValueError(f"{path.name}: expected one armature, found {len(rigs)}")
    return rigs[0]


def signature(rig):
    return {b.name: {"parent": b.parent.name if b.parent else None,
                     "matrix": [v for row in b.matrix_local for v in row]}
            for b in rig.data.bones}


def describe(path):
    rig = read_fbx(path)
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    points = [o.matrix_world @ Vector(p) for o in meshes for p in o.bound_box]
    actions = []
    root = next(b for b in rig.pose.bones if b.parent is None)
    for action in bpy.data.actions:
        rig.animation_data_create().action = action
        start, end = map(int, action.frame_range)
        positions = []
        for frame in [start, end]:
            bpy.context.scene.frame_set(frame)
            positions.append(list((rig.matrix_world @ root.matrix).translation))
        actions.append({"name": action.name, "frames": [start, end], "root_positions": positions})
    return {"file": path.name, "bones": signature(rig), "matrix": [v for row in rig.matrix_world for v in row],
            "meshes": [o.name for o in meshes], "actions": actions,
            "bounds": [[min(p[i] for p in points) for i in range(3)],
                       [max(p[i] for p in points) for i in range(3)]] if points else None,
            "materials": [{"name": m.name, "color": list(m.diffuse_color),
                           "textures": [n.image.name for n in m.node_tree.nodes if n.type == "TEX_IMAGE" and n.image] if m.use_nodes else []}
                          for m in bpy.data.materials]}


def in_place(rig, original, name):
    action = original.copy()
    action.name = name
    action.use_fake_user = True
    rig.animation_data.action = action
    root = next(b for b in rig.pose.bones if b.parent is None)
    start, end = map(int, action.frame_range)
    samples, positions = [], []
    for frame in range(start, end+1):
        bpy.context.scene.frame_set(frame)
        samples.append(root.location.copy())
        positions.append((rig.matrix_world @ root.matrix).translation.copy())
    local_from_world = (rig.matrix_world.to_3x3() @ root.bone.matrix_local.to_3x3()).inverted()
    values = [p - local_from_world @ Vector(planar_drift(positions[0], positions[-1], i/(end-start)))
              for i,p in enumerate(samples)]
    path = root.path_from_id("location")
    for curve in list(action.fcurves):
        if curve.data_path == path:
            action.fcurves.remove(curve)
    for axis in range(3):
        curve = action.fcurves.new(path, index=axis, action_group=root.name)
        for i,value in enumerate(values):
            key = curve.keyframe_points.insert(start+i, value[axis], options={"FAST"})
            key.interpolation = "LINEAR"
        curve.update()
    return action


def bake_clip(rig, source, source_action, name, rest_offset):
    start,end = map(int, source_action.frame_range)
    if end <= start:
        raise ValueError(f"{name}: empty clip")
    shift = Matrix.Translation(rest_offset)
    correction = {b.name: source.data.bones[b.name].matrix_local.inverted() @ shift.inverted() @ b.matrix_local
                  for b in rig.data.bones}
    samples = {b.name: [] for b in rig.data.bones}
    for frame in range(start,end+1):
        bpy.context.scene.frame_set(frame)
        poses = {b.name: shift @ source.pose.bones[b.name].matrix @ correction[b.name] for b in rig.data.bones}
        for bone in rig.data.bones:
            kwargs = {"parent_matrix":poses[bone.parent.name], "parent_matrix_local":bone.parent.matrix_local} if bone.parent else {}
            local = bone.convert_local_to_pose(poses[bone.name], bone.matrix_local, invert=True, **kwargs)
            location,rotation,scale = local.decompose()
            rotation.normalize()
            previous = samples[bone.name]
            if previous and sum(a*b for a,b in zip(rotation,previous[-1][3:7])) < 0:
                rotation.negate()
            samples[bone.name].append([*location,*rotation,*scale])
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    for bone in rig.pose.bones:
        bone.rotation_mode = "QUATERNION"
        for channel,offset,size in [("location",0,3),("rotation_quaternion",3,4),("scale",7,3)]:
            for axis in range(size):
                curve = action.fcurves.new(bone.path_from_id(channel),index=axis,action_group=bone.name)
                curve.keyframe_points.add(len(samples[bone.name]))
                curve.keyframe_points.foreach_set("co",[v for i,s in enumerate(samples[bone.name]) for v in (start+i,s[offset+axis])])
                for key in curve.keyframe_points:
                    key.interpolation = "LINEAR"
                curve.update()
    rig.animation_data.action = action
    max_error = 0.
    for frame in [start,(start+end)//2,end]:
        bpy.context.scene.frame_set(frame)
        for bone in rig.data.bones:
            desired = shift @ source.pose.bones[bone.name].matrix @ correction[bone.name]
            actual = rig.pose.bones[bone.name].matrix
            error = max(abs(a-b) for row_a,row_b in zip(desired,actual) for a,b in zip(row_a,row_b))
            max_error = max(max_error,error)
    if max_error > 0.01:
        raise ValueError(f"{name}: baked pose validation failed ({max_error})")
    return action,max_error


def scene_for(clips, package="package.json", spacing=7., columns=6):
    rows = math.ceil(len(clips)/columns)
    width, depth = columns*spacing, rows*spacing
    items = [{"name":"floor", "asset":"box", "position":[-spacing/2,-spacing/2,-.2],
              "scale":[width,depth,.15], "tint":[.13,.18,.20]}]
    for i,name in enumerate(clips):
        items.append({"name":name,"appearance":"mixamo/x_bot", "position":[i%columns*spacing,i//columns*spacing,0],
                      "animation":{"clip":name,"speed":1}})
    return {"version":1,"dimensions":[max(100,width),max(100,depth),30],"origin":[0,0,0],
            "simulation":{"tick_hz":60},
            "camera":{"target":[(columns-1)*spacing/2,(rows-1)*spacing/2,1],"zoom":min(5,35/max(width,depth)),"render_distance":120},
            "assets":[{"name":"box","builtin":"box"}],
            "packages":[{"namespace":"mixamo","file":package}],"items":items}


def build(args, files):
    character = args.source / args.character
    if character not in files:
        raise ValueError("character FBX is missing from the source folder")
    if args.output.resolve() == args.source.resolve() or args.output.resolve().is_relative_to(args.source.resolve()):
        raise ValueError("output must be outside the source folder")
    if args.output.exists() and any(args.output.iterdir()) and not args.overwrite:
        raise ValueError("output is not empty; use --overwrite to rebuild generated outputs")
    rig = read_fbx(character)
    base = signature(rig)
    matrix = rig.matrix_world.copy()
    scale = matrix.to_scale()
    if max(scale)-min(scale)>1e-6 or min(scale)<=0:
        raise ValueError("armature needs positive uniform scale")
    roots = [b for b in rig.data.bones if b.parent is None]
    if len(roots) != 1:
        raise ValueError("expected one root bone")
    for material in bpy.data.materials:
        if material.use_nodes and any(n.type == "TEX_IMAGE" for n in material.node_tree.nodes):
            raise ValueError("textured materials need an explicit material conversion; refusing to discard them")
        if material.diffuse_color[3] < 1:
            raise ValueError("transparent materials are not supported by the engine")
    rig.animation_data_clear()
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)
    rig.animation_data_create()
    actions, catalog = {}, []
    for path in files:
        if path == character:
            continue
        name = clip_id(path.stem)
        if name in actions:
            raise ValueError(f"duplicate normalized clip name: {name}")
        before = set(bpy.data.objects)
        bpy.ops.import_scene.fbx(filepath=str(path), use_anim=True)
        imported = set(bpy.data.objects)-before
        source_rigs = [o for o in imported if o.type == "ARMATURE"]
        if len(source_rigs) != 1:
            raise ValueError(f"{path.name}: expected one armature")
        source = source_rigs[0]
        rest_offset = check_skeleton(base, signature(source), meters_per_unit=scale.x)
        if any(abs(a-b)>1e-5 for row_a,row_b in zip(matrix,source.matrix_world) for a,b in zip(row_a,row_b)):
            raise ValueError(f"{path.name}: armature transforms differ")
        source_action = source.animation_data.action if source.animation_data else None
        if source_action is None:
            raise ValueError(f"{path.name}: missing animation")
        if any(not c.data_path.startswith('pose.bones[') for c in source_action.fcurves):
            raise ValueError(f"{path.name}: object animation requires explicit baking")
        action,pose_error = bake_clip(rig,source,source_action,name,rest_offset)
        start,end = map(int, action.frame_range)
        if end <= start:
            raise ValueError(f"{path.name}: empty clip")
        rig.animation_data.action = action
        root = rig.pose.bones[roots[0].name]
        points = []
        for frame in [start,end]:
            bpy.context.scene.frame_set(frame)
            points.append((rig.matrix_world @ root.matrix).translation.copy())
        fps = bpy.context.scene.render.fps / bpy.context.scene.render.fps_base
        duration = (end-start)/fps
        delta = points[-1]-points[0]
        catalog.append({"clip":name,"source":path.name,"duration":duration,"frames":[start,end],"fps":fps,
                        "planar_travel_m":math.hypot(delta.x,delta.y),"nominal_speed_mps":math.hypot(delta.x,delta.y)/duration,
                        "root_motion":"authored", "bind_origin_offset_armature_units":rest_offset,
                        "baked_pose_max_matrix_error":pose_error})
        actions[name] = action
        for obj in imported:
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.actions.remove(source_action)
    for name in args.in_place:
        if name not in actions or name+"_in_place" in actions:
            raise ValueError(f"unknown/conflicting in-place clip: {name}")
        adapted = name+"_in_place"
        actions[adapted] = in_place(rig, actions[name], adapted)
        entry = next(e for e in catalog if e["clip"] == name).copy()
        entry.update(clip=adapted, root_motion="linear_planar_drift_removed", source_clip=name)
        catalog.append(entry)
    rig.animation_data.action = None
    for bone in rig.pose.bones:
        bone.matrix_basis.identity()
    bpy.context.scene.frame_set(1)
    bpy.context.view_layer.update()
    args.output.mkdir(parents=True, exist_ok=True)
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(args.output / "source.blend"),compress=True)
    bpy.ops.export_scene.gltf(filepath=str(args.output / "x_bot.glb"), export_format="GLB",
                              export_animations=True, export_animation_mode="ACTIONS", export_force_sampling=True,
                              export_frame_range=False, export_frame_step=1, export_skins=True, export_yup=True,
                              export_materials="EXPORT", export_cameras=False, export_lights=False)
    package = {"format":"io.asset-package","version":1,"name":"mixamo-action-adventure",
               "coordinates":{"units":"meters","up_axis":"+Y","handedness":"right","origin":"authored"},
               "requires":["opaque_materials","skeletal_animation"],
               "assets":[{"name":"x_bot","file":"x_bot.glb","kind":"skinned","clips":sorted(actions)}],
               "appearances":[{"name":"x_bot","base_asset":"x_bot"}]}
    write = lambda path,data: path.write_text(json.dumps(data,indent=2)+"\n")
    write(args.output / "package.json",package)
    write(args.output / "catalog.json",{"character":character.name,"bones":len(base),"clips":catalog})
    write(args.output / "gallery.json",scene_for(sorted(actions)))
    preview = [name for name in ["idle","walking_in_place","running_in_place"] if name in actions]
    write(args.output / "preview.json",scene_for(preview or list(actions)[:3],spacing=3.,columns=3))
    (args.output / "clips").mkdir(exist_ok=True)
    for name in actions:
        write(args.output / "clips" / (name+".json"),scene_for([name],package="../package.json",spacing=7.,columns=1))
    for entry in catalog:
        print("CLIP",entry["clip"],f'{entry["duration"]:.3f}s',f'{entry["nominal_speed_mps"]:.3f}m/s',entry["root_motion"],flush=True)
    print(f"Exported {len(actions)} clips on {len(base)} bones to {args.output}",flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--inspect", action="store_true")
    parser.add_argument("--report", type=Path, default=Path("build/mixamo-inspect.json"))
    parser.add_argument("--output", type=Path, default=Path("assets/action-adventure"))
    parser.add_argument("--character", default="X Bot.fbx")
    parser.add_argument("--in-place", nargs="*", default=["walking","running"])
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])
    files = sorted(args.source.glob("*.fbx"))
    if not files:
        raise ValueError("source contains no FBX files")
    if not args.inspect:
        build(args, files)
        return
    report = [describe(path) for path in files]
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    for entry in report:
        print("FBX", entry["file"], "bones", len(entry["bones"]), "meshes", entry["meshes"], "actions", entry["actions"], "bounds", entry["bounds"], flush=True)


if __name__ == "__main__":
    main()
