"""Build a simplified adventurer from Rory's drawing, without changing engine code.

Blender --background --python tools/build_adventurer.py
Writes only assets/adventurer; the original reference and encounter stay untouched.
"""

import json
import math
from pathlib import Path

import bpy
from mathutils import Vector


OUT = Path(__file__).resolve().parents[1] / "assets" / "adventurer"


def material(name, color, metallic=0):
    rgb = [int(color[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    rgb = [v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in rgb]
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*rgb, 1)
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = (*rgb, 1)
    shader.inputs["Metallic"].default_value = metallic
    shader.inputs["Roughness"].default_value = 0.65 if not metallic else 0.3
    return mat


def finish(obj, name, mat, smooth=False, bevel=0):
    obj.name = name
    obj.data.materials.append(mat)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel:
        mod = obj.modifiers.new("Rounded edges", "BEVEL")
        mod.width, mod.segments = bevel, 2
        bpy.ops.object.modifier_apply(modifier=mod.name)
    for polygon in obj.data.polygons:
        polygon.use_smooth = smooth
    return obj


def ellipsoid(name, center, size, mat, smooth=True):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=12, ring_count=8, location=center)
    obj = bpy.context.object
    obj.scale = size
    return finish(obj, name, mat, smooth)


def box(name, center, size, mat, bevel=0.015):
    bpy.ops.mesh.primitive_cube_add(size=1, location=center)
    obj = bpy.context.object
    obj.scale = size
    return finish(obj, name, mat, bevel=bevel)


def limb(name, a, b, radius, mat):
    a, b = Vector(a), Vector(b)
    obj = ellipsoid(name, (a + b) / 2, (radius, radius, (b - a).length / 2 + radius * 0.3), mat)
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = (b - a).to_track_quat("Z", "Y")
    return obj


def rings(name, levels, mat, sides=12, smooth=False):
    # Each ring is (z, x radius, y radius, x offset, y offset).
    vertices = [(cx + rx * math.cos(i * math.tau / sides),
                 cy + ry * math.sin(i * math.tau / sides), z)
                for z, rx, ry, cx, cy in levels for i in range(sides)]
    faces = [tuple(reversed(range(sides)))]
    for ring in range(len(levels) - 1):
        for i in range(sides):
            a, b = ring * sides + i, ring * sides + (i + 1) % sides
            faces.append((a, b, b + sides, a + sides))
    faces.append(tuple(range((len(levels) - 1) * sides, len(vertices))))
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.data.materials.append(mat)
    for polygon in mesh.polygons:
        polygon.use_smooth = smooth
    return obj


def character(cloth, dark, m):
    skin, leather, gold = m["skin"], m["leather"], m["gold"]
    for side in (-1, 1):
        x = side * 0.155
        limb("Stocking", (x, 0, 0.34), (x, 0, 0.77), 0.069, m["stocking"])
        rings("Tall boot", [(0.08, .081, .088, x, 0), (.30, .068, .074, x, 0),
                             (.47, .085, .085, x, 0)], leather, smooth=True)
        rings("Boot cuff", [(.43, .087, .089, x, 0), (.48, .098, .098, x, 0)], gold)
        ellipsoid("Pointed boot toe", (x, -.082, .075), (.087, .178, .073), leather)
        rings("Loose shorts", [(.64, .125, .123, x, 0), (.72, .147, .14, x, 0),
                                 (.93, .137, .148, x * .8, 0)], dark, smooth=True)
        rings("Shorts hem", [(.64, .127, .125, x, 0), (.68, .135, .133, x, 0)], cloth)
    rings("Bloused tunic", [(.89, .255, .166, 0, 0), (.96, .29, .19, 0, 0),
                            (1.05, .218, .145, 0, 0), (1.19, .23, .147, 0, 0),
                            (1.36, .265, .155, 0, 0), (1.42, .18, .12, 0, 0)], cloth, smooth=True)
    rings("Leather belt", [(1.005, .241, .161, 0, 0), (1.067, .224, .151, 0, 0)], leather)
    box("Belt buckle", (0, -.165, 1.035), (.09, .03, .066), gold, .008)
    box("Buckle center", (0, -.184, 1.035), (.05, .01, .035), leather, .004)
    ellipsoid("Belt pouch", (-.255, -.008, .96), (.096, .098, .12), leather)
    box("Pouch flap", (-.264, -.091, .995), (.125, .026, .09), m["hair"])
    ellipsoid("Pouch stud", (-.264, -.112, .99), (.012, .008, .012), gold)
    limb("Neck", (0, 0, 1.39), (0, 0, 1.57), .085, skin)
    for side in (-1, 1):
        limb("Cream collar", (side * .13, -.117, 1.40), (0, -.162, 1.285), .022, m["stocking"])
    ellipsoid("Collar clasp", (0, -.169, 1.275), (.022, .013, .026), gold)
    # Relaxed left arm, raised right arm holding the sword over the shoulder.
    for side, elbow, hand in [(-1, (-.355, -.015, 1.10), (-.42, -.065, .91)),
                              (1, (.44, .025, 1.34), (.335, -.01, 1.63))]:
        shoulder = (side * .25, 0, 1.35)
        sleeve_end = Vector(shoulder).lerp(Vector(elbow), .43)
        limb("Short tunic sleeve", shoulder, sleeve_end, .115, cloth)
        limb("Upper arm", sleeve_end, elbow, .064, skin)
        limb("Forearm", elbow, hand, .056, skin)
        wrist = Vector(elbow).lerp(Vector(hand), .78)
        limb("Leather wrist wrap", wrist, Vector(elbow).lerp(Vector(hand), .94), .060, leather)
        ellipsoid("Hand", hand, (.06, .06, .077), skin)
    ellipsoid("Hair silhouette", (0, .025, 1.64), (.164, .137, .20), m["hair"])
    rings("Tapered face", [(1.46, .042, .06, 0, -.035), (1.50, .089, .09, 0, -.028),
                           (1.61, .13, .116, 0, -.013), (1.72, .132, .112, 0, 0),
                           (1.78, .095, .08, 0, 0)], skin, smooth=True)
    for side in (-1, 1):
        ellipsoid("Ear", (side * .137, 0, 1.64), (.033, .029, .058), skin)
        ellipsoid("Eye white", (side * .052, -.118, 1.652), (.031, .013, .021), m["stocking"])
        ellipsoid("Eye", (side * .051, -.130, 1.651), (.013, .007, .016), m["hair"])
        limb("Brow", (side * .028, -.13, 1.687), (side * .083, -.117, 1.692), .009, m["hair"])
        limb("Sideburn", (side * .121, -.045, 1.73), (side * .125, -.033, 1.60), .026, m["hair"])
    ellipsoid("Nose", (0, -.133, 1.608), (.027, .032, .039), skin)
    limb("Mouth", (-.03, -.114, 1.546), (.03, -.114, 1.546), .006, m["lip"])
    rings("Folded cap brim", [(1.749, .176, .149, 0, .012), (1.805, .166, .141, 0, .018)], dark)
    rings("Pointed cap", [(1.795, .163, .138, 0, .016), (1.88, .128, .112, .016, .02),
                          (2.00, .075, .068, .046, .032), (2.14, .003, .003, .073, .045)], cloth)
    # Diamond-section blade: sharp silhouette without textures or engine features.
    vertices = [(-.77, .026, 1.54), (-.55, .026, 1.625), (.21, .026, 1.69),
                (.21, .026, 1.56), (-.55, .026, 1.495),
                (-.37, -.003, 1.588), (-.37, .055, 1.588)]
    faces = [(i, (i + 1) % 5, 5) for i in range(5)]
    faces += [((i + 1) % 5, i, 6) for i in range(5)]
    mesh = bpy.data.meshes.new("Diamond sword blade")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    blade = bpy.data.objects.new("Shoulder sword", mesh)
    bpy.context.collection.objects.link(blade)
    blade.data.materials.append(m["steel"])
    limb("Sword crossguard", (.235, .026, 1.50), (.235, .026, 1.76), .024, gold)
    limb("Sword grip", (.255, .026, 1.635), (.445, .026, 1.65), .027, leather)
    ellipsoid("Sword pommel", (.465, .026, 1.653), (.042, .035, .042), gold)


def point_at(obj, target):
    obj.rotation_euler = (Vector(target) - obj.location).to_track_quat("-Z", "Y").to_euler()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.preferences.filepaths.save_version = 0
    m = {key: material(key, color, metal) for key, color, metal in [
        ("skin", "C9916D", 0), ("leather", "543A30", 0), ("gold", "CCA35B", .5),
        ("stocking", "E8D8B4", 0), ("hair", "302A29", 0), ("lip", "815244", 0),
        ("steel", "B8CCD2", .7)]}
    variants = []
    stats = {}
    for name, color, shadow in [("blue", "387EC5", "24456E"), ("red", "C55049", "7D303B")]:
        scene = bpy.data.scenes.new(name)
        bpy.context.window.scene = scene
        scene.unit_settings.system = "METRIC"
        character(material(name + " cloth", color), material(name + " trim", shadow), m)
        objects = list(scene.objects)
        for obj in objects:
            obj.data.calc_loop_triangles()
        points = [obj.matrix_world @ Vector(v) for obj in objects for v in obj.bound_box]
        stats[name] = {"triangles": sum(len(obj.data.loop_triangles) for obj in objects),
                       "bounds_min": [min(p[i] for p in points) for i in range(3)],
                       "bounds_max": [max(p[i] for p in points) for i in range(3)], "clips": []}
        bpy.ops.export_scene.gltf(filepath=str(OUT / f"{name}.glb"), export_format="GLB",
                                  use_active_scene=True, export_animations=False,
                                  export_skins=False, export_yup=True,
                                  export_cameras=False, export_lights=False)
        variants.append(objects)
    package = {"format": "io.asset-package", "version": 1, "name": "adventurer",
               "coordinates": {"units": "meters", "up_axis": "+Y", "handedness": "right", "origin": "authored"},
               "requires": ["opaque_materials"],
               "assets": [{"name": n, "file": f"{n}.glb", "kind": "static", "clips": []} for n in stats],
               "appearances": [{"name": n, "base_asset": n} for n in stats]}
    (OUT / "package.json").write_text(json.dumps(package, indent=2) + "\n")
    (OUT / "catalog.json").write_text(json.dumps(stats, indent=2) + "\n")
    encounter = json.loads((OUT.parent / "game" / "encounter.json").read_text())
    encounter["packages"] = [{"namespace": "adventurer", "file": "package.json"}]
    for item in encounter["items"]:
        if item["name"] == "floor":
            continue
        item.pop("asset")
        item["appearance"] = "adventurer/" + ("blue" if item["name"] == "hero" else "red")
        item["scale"] = [1, 1, 1]
        item["tint"] = [1, 1, 1]
    for template in encounter["game"]["templates"].values():
        template["death_physics"]["half_extents"] = [.29, .19, 1.07]
        template["death_physics"]["offset"] = [0, 0, 1.07]
    (OUT / "encounter.json").write_text(json.dumps(encounter, indent=2) + "\n")
    # Separate presentation scene: no plinth, lights or camera in either GLB.
    preview = bpy.data.scenes.new("Preview")
    bpy.context.window.scene = preview
    for objects, x, yaw in zip(variants, (-.83, .83), (-.12, .12)):
        parent = bpy.data.objects.new("Display placement", None)
        preview.collection.objects.link(parent)
        parent.location.x = x
        parent.rotation_euler.z = yaw
        for original in objects:
            obj = original.copy()
            preview.collection.objects.link(obj)
            obj.parent = parent
    stone = material("Charcoal stage", "283C43")
    floor = material("Backdrop", "18272E")
    for x in (-.83, .83):
        bpy.ops.mesh.primitive_cylinder_add(vertices=64, radius=.68, depth=.10, location=(x, 0, -.055))
        finish(bpy.context.object, "Display plinth", stone, bevel=.025)
    box("Studio floor", (0, 0, -.13), (200, 200, .05), floor, 0)
    world = bpy.data.worlds.new("Soft studio")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (.16, .21, .27, 1)
    world.node_tree.nodes["Background"].inputs[1].default_value = .4
    preview.world = world
    for name, location, energy, size, color in [
        ("Key", (-3, -4, 6), 650, 4, (1, .88, .74)),
        ("Fill", (4, -2, 3), 400, 3, (.68, .82, 1)),
        ("Rim", (0, 3, 4), 850, 3, (1, .80, .56))]:
        data = bpy.data.lights.new(name, "AREA")
        data.energy, data.shape, data.size, data.color = energy, "DISK", size, color
        obj = bpy.data.objects.new(name, data)
        preview.collection.objects.link(obj)
        obj.location = location
        point_at(obj, (0, 0, 1))
    data = bpy.data.cameras.new("Portrait camera")
    camera = bpy.data.objects.new("Portrait camera", data)
    preview.collection.objects.link(camera)
    camera.location = (2.6, -8, 3.5)
    point_at(camera, (0, 0, 1.05))
    data.type, data.ortho_scale = "ORTHO", 4.15
    preview.camera = camera
    preview.render.engine = "CYCLES"
    preview.cycles.samples = 48
    preview.cycles.use_denoising = True
    preview.render.resolution_x, preview.render.resolution_y = 1500, 1100
    preview.render.resolution_percentage = 100
    preview.render.image_settings.file_format = "PNG"
    preview.render.filepath = str(OUT / "preview.png")
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT / "adventurer.blend"))
    bpy.ops.render.render(write_still=True)
    print(json.dumps(stats, indent=2), flush=True)


if __name__ == "__main__":
    main()
