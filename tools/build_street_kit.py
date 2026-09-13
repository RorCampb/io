"""Build original, editable street-demo assets with Blender 4.2.

Run with Blender --background --python tools/build_street_kit.py.
Outputs are confined to assets/street-kit. No existing user Blender files are read.
"""

import json
import math
import struct
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from add_runner_collapse import add_collapse


OUT = Path(__file__).resolve().parents[1] / "assets" / "street-kit"
OUT.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.context.preferences.filepaths.save_version = 0
ASSETS = {}


def material(name, hex_color, metallic=0.0, emission=0.0):
    rgb = [int(hex_color[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    linear = [v / 12.92 if v < 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in rgb]
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*linear, 1)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*linear, 1)
    bsdf.inputs["Roughness"].default_value = 0.65
    bsdf.inputs["Metallic"].default_value = metallic
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*linear, 1)
        bsdf.inputs["Emission Strength"].default_value = emission
    return mat


M = {
    "cream": material("Warm limestone", "E9DBBC"),
    "sage": material("Sage plaster", "94AD9A"),
    "ochre": material("Apricot plaster", "D89B75"),
    "roof": material("Terracotta roof", "995646"),
    "roof_dark": material("Roof seams", "754236"),
    "glass": material("Blue window glass", "365660", metallic=0.18),
    "wood": material("Walnut doors", "674A3A"),
    "metal": material("Painted iron", "283F40", metallic=0.5),
    "brass": material("Brass details", "C79853", metallic=0.6),
    "lamp": material("Warm lamp lens", "FFE6AC", emission=1.5),
    "road": material("Blue grey asphalt", "526269"),
    "paving": material("Sandstone paving", "C4B9A3"),
    "marking": material("Road marking", "E6D8AA"),
    "leaf": material("Olive leaves", "6D8860"),
    "leaf_light": material("Young leaves", "9AAE70"),
    "trunk": material("Tree bark", "75513B"),
    "skin": material("Runner skin", "BB805C"),
    "jacket": material("Mustard windbreaker", "E4B44E"),
    "pants": material("Ink blue trousers", "304955"),
    "shoe": material("Teal trainers", "4A8F88"),
    "sole": material("Shoe soles", "E5DBC9"),
    "hair": material("Dark hair", "3D302C"),
    "ground": material("Display plinth", "AAB6A2"),
}


def activate(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def finish(obj, name, mat, bevel=0, smooth=False):
    obj.name = name
    obj.data.materials.append(mat)
    activate(obj)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        modifier = obj.modifiers.new("Soft manufactured edges", "BEVEL")
        modifier.width = bevel
        modifier.segments = 2
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    for polygon in obj.data.polygons:
        polygon.use_smooth = smooth
    return obj


def box(name, center, size, mat, bevel=0.02):
    bpy.ops.mesh.primitive_cube_add(size=1, location=center)
    obj = bpy.context.object
    obj.scale = size
    return finish(obj, name, mat, bevel)


def sphere(name, center, scale, mat):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=16, ring_count=10, location=center)
    obj = bpy.context.object
    obj.scale = scale
    return finish(obj, name, mat, smooth=True)


def rod(name, a, b, radius, mat, vertices=12):
    a, b = Vector(a), Vector(b)
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius,
                                      depth=(b - a).length, location=(a + b) / 2)
    obj = bpy.context.object
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = (b - a).to_track_quat("Z", "Y")
    return finish(obj, name, mat, bevel=min(radius * 0.15, 0.018), smooth=True)


def capsule(name, a, b, width, mat):
    a, b = Vector(a), Vector(b)
    obj = sphere(name, (a + b) / 2, (width, width, (b - a).length / 2 + width * 0.35), mat)
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = (b - a).to_track_quat("Z", "Y")
    return obj


def asset_scene(name):
    scene = bpy.data.scenes.new(name)
    bpy.context.window.scene = scene
    scene.unit_settings.system = "METRIC"
    scene.render.fps = 24
    scene.frame_end = 24
    return scene


def export_asset(name, animated=False):
    scene = bpy.context.scene
    objects = list(scene.objects)
    rigs = [obj for obj in objects if obj.type == "ARMATURE"]
    for rig in rigs:
        rig.data.pose_position = "REST"
    bpy.context.view_layer.update()
    points = [obj.matrix_world @ Vector(v) for obj in objects if obj.type == "MESH" for v in obj.bound_box]
    lo = [min(p[i] for p in points) for i in range(3)]
    hi = [max(p[i] for p in points) for i in range(3)]
    dimensions = [hi[i] - lo[i] for i in range(3)]
    ASSETS[name] = {"file": name + ".glb", "scene": scene.name,
                    "bounds_min": lo, "bounds_max": hi, "dimensions": dimensions,
                    "uniform_import_size": max(dimensions),
                    "clips": ["Run", "Collapse"] if animated else []}
    for rig in rigs:
        rig.data.pose_position = "POSE"
    bpy.context.view_layer.update()
    bpy.ops.export_scene.gltf(filepath=str(OUT / (name + ".glb")), export_format="GLB",
        use_active_scene=True, export_animations=animated, export_animation_mode="ACTIONS",
        export_force_sampling=True, export_frame_range=False, export_frame_step=1,
        export_skins=animated, export_yup=True, export_materials="EXPORT",
        export_cameras=False, export_lights=False, export_extras=True)
    print("EXPORTED", name, flush=True)
    return scene


def roof(width, depth, base, ridge):
    w, d = width / 2, depth / 2
    vertices = [(-w, -d, base), (w, -d, base), (0, -d, ridge),
                (-w, d, base), (w, d, base), (0, d, ridge)]
    faces = [(0, 2, 1), (3, 4, 5), (0, 3, 5, 2), (1, 2, 5, 4), (0, 1, 4, 3)]
    mesh = bpy.data.meshes.new("Gabled roof mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("Gabled terracotta roof", mesh)
    bpy.context.collection.objects.link(obj)
    obj.data.materials.append(M["roof"])
    for side in (-1, 1):
        for step in range(1, 6):
            x = side * w * step / 6
            z = ridge + (base - ridge) * step / 6 + 0.018
            rod("Roof tile course", (x, -d, z), (x, d, z), 0.015, M["roof_dark"], 6)
    rod("Ridge cap", (0, -d, ridge), (0, d, ridge), 0.08, M["roof"], 10)


def window(x, y, z, w=0.9, h=1.1, shutters=False):
    box("Window surround", (x, y, z), (w + 0.16, 0.16, h + 0.16), M["cream"])
    box("Window glass", (x, y - 0.10, z), (w, 0.035, h), M["glass"], 0.005)
    box("Window mullion", (x, y - 0.13, z), (0.045, 0.035, h), M["cream"], 0.005)
    box("Window crossbar", (x, y - 0.13, z), (w, 0.035, 0.045), M["cream"], 0.005)
    box("Window sill", (x, y - 0.10, z - h / 2 - 0.09), (w + 0.28, 0.32, 0.10), M["cream"])
    if shutters:
        for side in (-1, 1):
            sx = x + side * (w / 2 + 0.25)
            box("Sage shutter", (sx, y - 0.08, z), (0.30, 0.09, h), M["metal"])
            for level in range(6):
                box("Shutter slat", (sx, y - 0.14, z - h * 0.4 + level * h * 0.16),
                    (0.25, 0.025, 0.035), M["sage"], 0.003)


def house(name, tall=False):
    asset_scene(name)
    height = 5.6 if tall else 3.1
    width, depth = (4.8, 4.3) if tall else (5.4, 4.3)
    front = -depth / 2
    box("Foundation", (0, 0, 0.16), (width + 0.2, depth + 0.2, 0.32), M["cream"], 0.04)
    box("Plaster walls", (0, 0, height / 2 + 0.25), (width, depth, height), M["ochre" if tall else "sage"], 0.04)
    for x in (-width / 2 + 0.07, width / 2 - 0.07):
        box("Corner trim", (x, front - 0.035, height / 2 + 0.25), (0.14, 0.1, height), M["cream"])
    box("Front door", (0, front - 0.08, 1.24), (0.94, 0.15, 1.95), M["wood"], 0.025)
    for z in (0.80, 1.55):
        box("Door inset panel", (0, front - 0.17, z), (0.69, 0.035, 0.52), M["roof_dark"], 0.012)
    sphere("Brass door handle", (0.31, front - 0.23, 1.22), (0.055, 0.045, 0.055), M["brass"])
    box("Door step", (0, front - 0.35, 0.12), (1.35, 0.65, 0.24), M["cream"])
    for x in (-1.55, 1.55):
        window(x, front - 0.08, 1.65, w=0.80, shutters=not tall)
    for level in ([1.65, 4.25] if tall else [1.65]):
        for side in (-1, 1):
            for y in (-0.95, 0.95):
                previous = set(bpy.context.scene.objects)
                window(0, 0, 0, w=0.75, h=1.0)
                transform = Matrix.Translation((side * (width / 2 + 0.08), y, level))
                transform @= Matrix.Rotation(side * math.pi / 2, 4, "Z")
                for obj in set(bpy.context.scene.objects) - previous:
                    obj.matrix_world = transform @ obj.matrix_world
        for x in (-1.35, 1.35):
            previous = set(bpy.context.scene.objects)
            window(0, 0, 0, w=0.85, h=1.0)
            transform = Matrix.Translation((x, depth / 2 + 0.08, level))
            transform @= Matrix.Rotation(math.pi, 4, "Z")
            for obj in set(bpy.context.scene.objects) - previous:
                obj.matrix_world = transform @ obj.matrix_world
    if tall:
        box("Floor cornice", (0, 0, 3.05), (width + 0.18, depth + 0.18, 0.16), M["cream"])
        for x in (-1.5, 0, 1.5):
            window(x, front - 0.08, 4.25, w=0.78, h=1.25)
        box("Door canopy", (0, front - 0.38, 2.43), (1.4, 0.85, 0.14), M["roof"])
    roof(width + 0.50, depth + 0.45, height + 0.25, height + 1.8)
    box("Chimney", (width * 0.28, 0.7, height + 1.20), (0.50, 0.55, 1.65), M["ochre"], 0.025)
    box("Chimney cap", (width * 0.28, 0.7, height + 2.02), (0.66, 0.71, 0.12), M["cream"])
    return export_asset(name)


def street():
    asset_scene("road_straight")
    box("Road slab", (0, 0, -0.16), (7.2, 10, 0.32), M["road"], 0)
    for side in (-1, 1):
        box("Sidewalk", (side * 4.65, 0, 0.02), (2.1, 10, 0.24), M["paving"], 0)
        for y in range(10):
            box("Curb block", (side * 3.68, y - 4.5, 0.09), (0.20, 0.975, 0.28), M["cream"], 0.01)
            box("Paving seam", (side * 4.73, y - 4.5, 0.143), (1.75, 0.018, 0.005), M["wood"], 0)
    for y in (-4, -2, 0, 2, 4):
        box("Dashed center line", (0, y, 0.008), (0.10, 0.95, 0.015), M["marking"], 0)
    for side in (-1, 1):
        box("Road edge line", (side * 3.32, 0, 0.008), (0.055, 10, 0.015), M["marking"], 0)
    return export_asset("road_straight")


def lamp():
    asset_scene("street_lamp")
    box("Lamp footing", (0, 0, 0.10), (0.45, 0.45, 0.20), M["cream"], 0.04)
    rod("Flared base", (0, 0, 0.12), (0, 0, 0.6), 0.15, M["metal"])
    rod("Lamp pole", (0, 0, 0.3), (0, 0, 3.9), 0.055, M["metal"])
    rod("Brass collar", (0, 0, 3.38), (0, 0, 3.48), 0.075, M["brass"])
    rod("Lantern arm", (0, 0, 3.9), (0.8, 0, 3.9), 0.055, M["metal"])
    rod("Diagonal brace", (0, 0, 3.48), (0.48, 0, 3.9), 0.028, M["metal"])
    rod("Lantern hanger", (0.76, 0, 3.9), (0.76, 0, 3.65), 0.035, M["metal"])
    box("Lantern lid", (0.76, 0, 3.64), (0.49, 0.49, 0.12), M["metal"], 0.06)
    box("Warm lantern lens", (0.76, 0, 3.39), (0.32, 0.32, 0.43), M["lamp"], 0.015)
    box("Lantern base", (0.76, 0, 3.16), (0.41, 0.41, 0.09), M["metal"])
    for x in (-0.18, 0.18):
        for y in (-0.18, 0.18):
            rod("Lantern frame", (0.76 + x, y, 3.18), (0.76 + x, y, 3.60), 0.018, M["metal"], 6)
    return export_asset("street_lamp")


def tree():
    asset_scene("street_tree")
    box("Tree planter", (0, 0, 0.18), (1.25, 1.25, 0.36), M["cream"], 0.06)
    box("Planter soil", (0, 0, 0.36), (1.07, 1.07, 0.04), M["wood"], 0.02)
    rod("Tree trunk", (0, 0, 0.3), (0, 0, 2.35), 0.11, M["trunk"], 10)
    rod("Branch", (0, 0, 1.5), (-0.40, 0, 2.4), 0.065, M["trunk"], 8)
    for index, (x, y, z, radius) in enumerate([(-0.45, 0, 2.7, 0.7), (0.43, 0.08, 2.9, 0.75), (0, 0, 3.43, 0.72)]):
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=radius, location=(x, y, z))
        finish(bpy.context.object, "Rounded crown", M["leaf_light" if index == 2 else "leaf"], smooth=True)
    return export_asset("street_tree")


def runner():
    scene = asset_scene("runner")
    bones = {"root": ((0, 0, 0), (0, 0, 0.2), None),
             "hips": ((0, 0, 0.91), (0, 0, 1.08), "root"),
             "spine": ((0, 0, 1.08), (0, 0, 1.43), "hips"),
             "head": ((0, 0, 1.43), (0, 0, 1.83), "spine")}
    for side, sign in (("L", 1), ("R", -1)):
        x = sign * 0.145
        bones.update({
            "thigh." + side: ((x, 0, 0.91), (x, 0, 0.51), "hips"),
            "shin." + side: ((x, 0, 0.51), (x, 0, 0.11), "thigh." + side),
            "foot." + side: ((x, 0, 0.11), (x, -0.18, 0.11), "shin." + side),
            "upper_arm." + side: ((sign * 0.27, 0, 1.38), (sign * 0.39, 0, 1.10), "spine"),
            "forearm." + side: ((sign * 0.39, 0, 1.10), (sign * 0.40, -0.06, 0.85), "upper_arm." + side),
        })
    armature = bpy.data.armatures.new("Runner skeleton")
    rig = bpy.data.objects.new("RunnerRig", armature)
    scene.collection.objects.link(rig)
    activate(rig)
    bpy.ops.object.mode_set(mode="EDIT")
    for name, (head, tail, parent) in bones.items():
        bone = armature.edit_bones.new(name)
        bone.head, bone.tail = head, tail
        if parent:
            bone.parent = armature.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    rig.show_in_front = True
    parts = []

    def bind(obj, bone):
        group = obj.vertex_groups.new(name=bone)
        group.add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
        parts.append(obj)
        return obj

    bind(sphere("Jacket torso", (0, 0, 1.22), (0.29, 0.17, 0.27), M["jacket"]), "spine")
    bind(sphere("Trouser hips", (0, 0, 0.94), (0.24, 0.15, 0.16), M["pants"]), "hips")
    bind(box("Jacket zip", (0, -0.168, 1.22), (0.022, 0.025, 0.34), M["cream"], 0.005), "spine")
    bind(sphere("Head", (0, -0.01, 1.66), (0.22, 0.20, 0.25), M["skin"]), "head")
    bind(sphere("Hair cap", (0, 0.005, 1.80), (0.225, 0.195, 0.12), M["hair"]), "head")
    bind(sphere("Nose", (0, -0.211, 1.65), (0.041, 0.045, 0.05), M["skin"]), "head")
    for sign in (-1, 1):
        bind(sphere("Ear", (sign * 0.22, 0, 1.66), (0.042, 0.048, 0.065), M["skin"]), "head")
        bind(sphere("Eye", (sign * 0.078, -0.193, 1.71), (0.022, 0.017, 0.027), M["hair"]), "head")
    for side, sign in (("L", 1), ("R", -1)):
        for prefix, width, material_name in (("thigh", 0.105, "pants"), ("shin", 0.083, "pants"),
                                              ("upper_arm", 0.091, "jacket"), ("forearm", 0.072, "jacket")):
            name = prefix + "." + side
            bind(capsule(name, bones[name][0], bones[name][1], width, M[material_name]), name)
        x = sign * 0.145
        bind(box("Trainer sole", (x, -0.085, 0.041), (0.21, 0.36, 0.075), M["sole"], 0.035), "foot." + side)
        bind(sphere("Trainer upper", (x, -0.07, 0.113), (0.104, 0.173, 0.094), M["shoe"]), "foot." + side)
        bind(sphere("Hand", bones["forearm." + side][1], (0.067, 0.069, 0.086), M["skin"]), "forearm." + side)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in parts:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    mesh = bpy.context.object
    mesh.name = "RunnerBody"
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    mesh.parent = rig
    modifier = mesh.modifiers.new("Skeleton deformation", "ARMATURE")
    modifier.object = rig
    rig.animation_data_create()
    action = bpy.data.actions.new("Run")
    rig.animation_data.action = action
    action.use_fake_user = True

    def pose(name, head, tail):
        bone = rig.data.bones[name]
        rest_direction = bone.tail_local - bone.head_local
        rotation = rest_direction.rotation_difference(Vector(tail) - Vector(head))
        matrix = Matrix.Translation(Vector(head)) @ rotation.to_matrix().to_4x4()
        matrix @= bone.matrix_local.to_quaternion().to_matrix().to_4x4()
        pb = rig.pose.bones[name]
        pb.rotation_mode = "QUATERNION"
        pb.matrix = matrix
        bpy.context.view_layer.update()
        pb.keyframe_insert("location")
        pb.keyframe_insert("rotation_quaternion")
        pb.keyframe_insert("scale")

    # A one-second in-place loop. Analytic two-bone legs keep the stance foot level.
    for frame in range(1, 26):
        scene.frame_set(frame)
        phase = (frame - 1) / 24 * math.tau
        bob = 0.02 * math.cos(2 * phase)
        flight = 0.055 * max(0, 1 - 3 * abs(math.cos(phase)))
        hip_z = 0.79 + bob + flight
        pose("root", (0, 0, 0), (0, 0, 0.2))
        pose("hips", (0, 0, hip_z), (0, -0.015, hip_z + 0.17))
        pose("spine", (0, -0.015, hip_z + 0.17), (0, -0.065, hip_z + 0.52))
        pose("head", (0, -0.065, hip_z + 0.52), (0, -0.065, hip_z + 0.92))
        for side, sign in (("L", 1), ("R", -1)):
            p = phase + (0 if sign == 1 else math.pi)
            x = sign * 0.145
            hip = Vector((x, 0, hip_z))
            foot = Vector((x, 0.28 * math.sin(p), 0.11 + flight + 0.22 * max(0, math.cos(p))))
            delta = foot - hip
            midpoint = (hip + foot) / 2
            bend = Vector((0, delta.z, -delta.y)).normalized()
            knee = midpoint + bend * math.sqrt(max(0, 0.4 ** 2 - delta.length_squared / 4))
            pose("thigh." + side, hip, knee)
            pose("shin." + side, knee, foot)
            pose("foot." + side, foot, foot + Vector((0, -0.18, 0)))
            shoulder = Vector((sign * 0.27, -0.055, hip_z + 0.47))
            swing = 0.65 * math.sin(p)
            elbow = shoulder + Vector((sign * 0.075, -0.29 * math.sin(swing), -0.29 * math.cos(swing)))
            hand = elbow + Vector((sign * 0.015, -0.24 * math.cos(swing), 0.24 * math.sin(swing)))
            pose("upper_arm." + side, shoulder, elbow)
            pose("forearm." + side, elbow, hand)
    for curve in action.fcurves:
        for point in curve.keyframe_points:
            point.interpolation = "LINEAR"
    scene.frame_end = 25
    scene.frame_set(1)
    rig["clip_notes"] = "Run: 1 second, in place, 24 fps; frame 25 repeats frame 1."
    rig["forward_axis"] = "-Y in Blender; +Z in glTF"
    add_collapse(scene, rig)
    export_asset("runner", animated=True)
    return scene


def instance(scene, name, location, yaw=0, scale=1):
    obj = bpy.data.objects.new(name + " instance", None)
    obj.instance_type = "COLLECTION"
    obj.instance_collection = bpy.data.scenes[name].collection
    obj.location = location
    obj.rotation_euler.z = yaw
    obj.scale = (scale,) * 3
    scene.collection.objects.link(obj)
    return obj


def studio(scene, camera_location, target, ortho, resolution):
    bpy.context.window.scene = scene
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.eevee.taa_render_samples = 32
    scene.render.resolution_x, scene.render.resolution_y = resolution
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.view_settings.view_transform = "AgX"
    scene.world = bpy.data.worlds.new(scene.name + " sky")
    scene.world.use_nodes = True
    scene.world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.65, 0.75, 0.85, 1)
    scene.world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.45
    data = bpy.data.cameras.new(scene.name + " camera")
    camera = bpy.data.objects.new(data.name, data)
    scene.collection.objects.link(camera)
    camera.location = camera_location
    camera.rotation_euler = (Vector(target) - camera.location).to_track_quat("-Z", "Y").to_euler()
    data.type = "ORTHO"
    data.ortho_scale = ortho
    scene.camera = camera
    sun_data = bpy.data.lights.new(scene.name + " sun", "SUN")
    sun_data.energy = 2.5
    sun_data.angle = math.radians(18)
    sun = bpy.data.objects.new(sun_data.name, sun_data)
    scene.collection.objects.link(sun)
    sun.rotation_euler = (math.radians(28), math.radians(-22), math.radians(-28))
    fill_data = bpy.data.lights.new(scene.name + " softbox", "AREA")
    fill_data.energy = 500 if ortho < 5 else 2200
    fill_data.shape = "DISK"
    fill_data.size = 6 if ortho < 5 else 15
    fill = bpy.data.objects.new(fill_data.name, fill_data)
    scene.collection.objects.link(fill)
    fill.location = (2, -4, 6) if ortho < 5 else (0, -5, 15)
    fill.rotation_euler = (Vector(target) - fill.location).to_track_quat("-Z", "Y").to_euler()


def render(scene, filename, frame=1):
    bpy.context.window.scene = scene
    scene.frame_set(frame)
    scene.render.filepath = str(OUT / filename)
    bpy.ops.render.render(write_still=True)
    print("RENDERED", filename, flush=True)


house("house_cottage")
house("house_townhouse", tall=True)
street()
lamp()
tree()
runner()

preview = asset_scene("Street preview")
box("Neighborhood plinth", (0, 0, -0.5), (27, 32, 0.65), M["ground"], 0.35)
for y in (-10, 0, 10):
    instance(preview, "road_straight", (0, y, 0))
for side in (-1, 1):
    for index, y in enumerate((-9, 0, 9)):
        instance(preview, "house_cottage" if index % 2 == 0 else "house_townhouse",
                 (side * 8.35, y, 0), -side * math.pi / 2)
    for y in (-12, -3, 6):
        instance(preview, "street_lamp", (side * 4.4, y, 0.14), math.pi if side == 1 else 0)
    for y in (-4.5, 4.5, 12):
        instance(preview, "street_tree", (side * 6.4, y, 0))
instance(preview, "runner", (1.3, -5.0, 0))
studio(preview, (25, -31, 28), (0, 0, 1), 43, (1440, 1200))

character_preview = asset_scene("Runner preview")
box("Character platform", (0, 0, -0.08), (3, 3, 0.16), M["cream"], 0.1)
instance(character_preview, "runner", (0, 0, 0))
studio(character_preview, (3, -5, 2.6), (0, 0, 0.95), 2.65, (800, 800))
character_preview.frame_end = 24

# Store export statistics from the actual GLB, including skin and animation presence.
for name, info in ASSETS.items():
    data = (OUT / info["file"]).read_bytes()
    size, chunk_type = struct.unpack_from("<II", data, 12)
    document = json.loads(data[20:20 + size])
    info["bytes"] = len(data)
    info["triangles"] = sum(document["accessors"][p["indices"]]["count"] // 3
        for mesh in document.get("meshes", []) for p in mesh["primitives"])
    info["skins"] = len(document.get("skins", []))
    info["animations"] = [a.get("name", "unnamed") for a in document.get("animations", [])]
    assert info["triangles"] > 0
    if name == "runner":
        assert info["skins"] == 1 and "Run" in info["animations"], info
        assert all("JOINTS_0" in p["attributes"] and "WEIGHTS_0" in p["attributes"]
                   for mesh in document["meshes"] for p in mesh["primitives"])
catalog = {"units": "meters", "bounds_basis": "bind pose, Blender axes",
           "source_up": "+Z", "source_forward": "-Y",
           "gltf_up": "+Y", "gltf_forward": "+Z", "assets": ASSETS}
(OUT / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
bpy.context.window.scene = preview
preview.frame_set(1)
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / "street-kit.blend"))
render(preview, "street-preview.png", 4)
render(character_preview, "runner-preview.png", 4)
character_preview.render.resolution_x = character_preview.render.resolution_y = 480
character_preview.eevee.taa_render_samples = 16
for frame in range(1, 25):
    render(character_preview, "run-frames/run_%03d.png" % frame, frame)
print("STREET KIT COMPLETE", flush=True)
