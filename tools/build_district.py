"""Generate a large, deterministic scene and shared opaque glTF props.

python3 tools/build_district.py
Content generation lives here; the engine loads the resulting ordinary scene JSON.
"""
import argparse
import base64
from collections import Counter
import copy
import json
import math
import os
from pathlib import Path
import random
import struct

ROOT = Path(__file__).resolve().parents[1]
KIT = ROOT / "assets/street-kit"
PITCH = 64


class Mesh:
    def __init__(self):
        self.positions = []
        self.colors = []

    def triangle(self, a, b, c, color):
        for x, y, z in (a, b, c):
            self.positions.append((x, z, -y))  # Engine Z-up to glTF Y-up.
            self.colors.append((*color, 1))

    def box(self, center, size, color):
        x, y, z = center
        sx, sy, sz = (v / 2 for v in size)
        p = [(x + a * sx, y + b * sy, z + c * sz)
             for a, b, c in [(-1,-1,-1),(1,-1,-1),(1,1,-1),(-1,1,-1),
                             (-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]]
        for a,b,c,d in [(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)]:
            self.triangle(p[a], p[b], p[c], color)
            self.triangle(p[a], p[c], p[d], color)

    def cone(self, radius, bottom, top, color, segments=10, top_radius=0):
        for i in range(segments):
            a, b = i * math.tau / segments, (i + 1) * math.tau / segments
            p = (radius * math.cos(a), radius * math.sin(a), bottom)
            q = (radius * math.cos(b), radius * math.sin(b), bottom)
            r = (top_radius * math.cos(b), top_radius * math.sin(b), top)
            s = (top_radius * math.cos(a), top_radius * math.sin(a), top)
            self.triangle(p, q, r, color)
            if top_radius:
                self.triangle(p, r, s, color)
                self.triangle((0,0,top), s, r, color)
            self.triangle((0,0,bottom), q, p, color)

    def save(self, path):
        positions = b"".join(struct.pack("<3f", *p) for p in self.positions)
        colors = b"".join(struct.pack("<4f", *c) for c in self.colors)
        data = positions + colors
        document = {
            "asset": {"version": "2.0", "generator": "io district generator"},
            "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "COLOR_0": 1}}]}],
            "buffers": [{"byteLength": len(data), "uri": "data:application/octet-stream;base64," + base64.b64encode(data).decode()}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": len(positions)},
                            {"buffer": 0, "byteOffset": len(positions), "byteLength": len(colors)}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": len(self.positions), "type": "VEC3",
                           "min": [min(p[i] for p in self.positions) for i in range(3)],
                           "max": [max(p[i] for p in self.positions) for i in range(3)]},
                          {"bufferView": 1, "componentType": 5126, "count": len(self.colors), "type": "VEC4"}],
        }
        path.write_text(json.dumps(document, separators=(",", ":")) + "\n")


def props(directory):
    wood, metal = (0.34,0.19,0.09), (0.12,0.19,0.17)
    meshes = {}
    bench = meshes["bench"] = Mesh()
    bench.box((0,0,0.52), (1.8,0.48,0.12), wood)
    bench.box((0,0.23,0.92), (1.8,0.10,0.5), wood)
    for x in (-0.65,0.65):
        bench.box((x,0,0.25), (0.10,0.4,0.5), metal)
    crate = meshes["crate"] = Mesh()
    crate.box((0,0,0.34), (0.68,0.68,0.68), (0.48,0.31,0.15))
    for x in (-0.27,0.27):
        crate.box((x,0,0.7), (0.07,0.72,0.04), wood)
    barrel = meshes["barrel"] = Mesh()
    barrel.cone(0.34,0,0.86,wood,12,0.34)
    for z in (0.15,0.65):
        barrel.cone(0.355,z,z+0.08,metal,12,0.355)
    for label, color in [("stall_red", (0.72,0.22,0.12)), ("stall_gold", (0.85,0.60,0.2))]:
        stall = meshes[label] = Mesh()
        stall.box((0,0,0.55), (2.8,1.4,1.1), wood)
        for x in (-1.35,1.35):
            for y in (-0.65,0.65):
                stall.box((x,y,1.35), (0.08,0.08,2.7), metal)
        stall.box((0,0,2.65), (3.2,1.9,0.15), color)
        for x in (-1,-0.5,0,0.5,1):
            stall.box((x,0,1.18), (0.35,0.8,0.16), (0.57,0.65,0.17))
    for label, low in [("pine_high", False), ("pine_low", True)]:
        tree = meshes[label] = Mesh()
        tree.cone(0.18,0,4.5,wood,5 if low else 10,0.12)
        for radius, bottom, top, color in [(1.8,1.5,5,(0.16,0.31,0.18)),
                                           (1.4,3,6.2,(0.19,0.37,0.20)),
                                           (0.9,4.8,7.2,(0.27,0.43,0.22))]:
            tree.cone(radius,bottom,top,color,5 if low else 16)
    road = meshes["road_low"] = Mesh()
    road.box((0,0,-0.16), (7.2,10,0.32), (0.19,0.24,0.27))
    for side in (-1,1):
        road.box((side*4.65,0,0.02), (2.1,10,0.24), (0.61,0.60,0.51))
    for y in (-4,-2,0,2,4):
        road.box((0,y,0.008), (0.1,0.95,0.015), (0.86,0.84,0.57))
    for name, mesh in meshes.items():
        mesh.save(directory / (name + ".gltf"))
    return list(meshes)


def generate(args):
    out = args.output.resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    mesh_dir = out.parent / "props"
    mesh_dir.mkdir(exist_ok=True)
    scene = json.loads((KIT / "variants-demo.json").read_text())
    for asset in scene["assets"]:
        if "file" in asset:
            asset["file"] = os.path.relpath(KIT / asset["file"], out.parent)
    for name in props(mesh_dir):
        scene["assets"].append({"name": name, "file": "props/" + name + ".gltf"})
    scene["appearances"].extend([
        {"name": "pine", "base_asset": "pine_high", "lods": [
            {"asset": "pine_high", "min_screen_pixels": 180}, {"asset": "pine_low", "min_screen_pixels": 0}]},
        {"name": "road", "base_asset": "road", "lods": [
            {"asset": "road", "min_screen_pixels": 250}, {"asset": "road_low", "min_screen_pixels": 0}]},
    ])
    for name in ("crate", "barrel", "bench"):
        scene["appearances"].append({"name": name, "base_asset": name, "lods": [
            {"asset": name, "min_screen_pixels": 2}]})
    rng = random.Random(args.seed)
    items, counts = [], Counter()

    def put(kind, x, y, z=0, **kwargs):
        name = f"{kind}-{counts[kind]}"
        counts[kind] += 1
        item = {"name": name, "position": [round(x,3),round(y,3),round(z,3)], **kwargs}
        item["appearance" if kind in appearance_names else "asset"] = kind
        items.append(item)
        return item

    appearance_names = {a["name"] for a in scene["appearances"]}
    half = args.blocks * PITCH / 2
    forest_inner = math.sqrt(2) * half + 30
    forest_outer = forest_inner + 200
    put("ground", -forest_outer-20, -forest_outer-20, -0.55,
        scale=[2*forest_outer+40,2*forest_outer+40,0.2], tint=[0.22,0.31,0.19])
    # Continuous avenues with separate asphalt intersections, avoiding overlapping sidewalks.
    for i in range(args.blocks + 1):
        line = i * PITCH - half
        for j in range(args.blocks):
            start = j * PITCH - half
            for offset in (12,22,32,42,52):
                put("road", line, start + offset)
                put("road", start + offset, line, yaw_degrees=90)
        for j in range(args.blocks + 1):
            y = j * PITCH - half
            put("ground", line-7, y-7, -0.32, scale=[14,14,0.32], tint=[0.19,0.24,0.27])
    for bx in range(args.blocks):
        for by in range(args.blocks):
            x, y = (bx+0.5)*PITCH-half, (by+0.5)*PITCH-half
            park = (bx + 2*by) % 9 == 0
            put("ground", x-26.3, y-26.3, -0.3, scale=[52.6,52.6,0.3],
                tint=[0.32,0.42,0.24] if park else [0.51,0.51,0.41])
            if park:
                for _ in range(32):
                    scale = rng.uniform(0.7,1.5)
                    put("pine", x+rng.uniform(-22,22), y+rng.uniform(-22,22), scale=[scale]*3)
            else:
                for side in (-1,1):
                    for offset in (-18,-6,6,18):
                        size = rng.uniform(0.95,1.2)
                        put("cottage" if rng.random()<0.45 else "townhouse", x+side*19,y+offset,
                            yaw_degrees=side*90, scale=[size,size,rng.uniform(0.95,1.35)],
                            tint=[rng.uniform(0.85,1),rng.uniform(0.85,1),rng.uniform(0.85,1)])
                for side in (-1,1):
                    for offset in (-18,0,18):
                        put("tree", x+side*10, y+offset)
                if (bx+by)%3 == 0:
                    for sx in (-4,4):
                        for sy in (-16,-8,0,8,16):
                            put("stall_red" if rng.random()<0.5 else "stall_gold", x+sx,y+sy)
                            for k in range(3):
                                put("crate",x+sx+1.8,y+sy+(k-1)*0.8)
                            put("barrel",x+sx-1.9,y+sy+0.4)
            for side in (-1,1):
                for offset in (-16,16):
                    put("lamp", x+side*27.1,y+offset,0.14,yaw_degrees=90 if side<0 else -90)
                    put("bench", x+offset,y+side*24.5,0,yaw_degrees=180 if side<0 else 0)
            route = [[x-27.2,y-27.2,0.14],[x-27.2,y+27.2,0.14],
                     [x+27.2,y+27.2,0.14],[x+27.2,y-27.2,0.14]]
            for _ in range(args.npcs_per_block):
                path = route if rng.random()<0.5 else list(reversed(route))
                put("runner",x,y,0.14, animation={"clip":"Run", "speed":rng.uniform(0.8,1.2)},
                    motion={"route":copy.deepcopy(path),"speed":rng.uniform(1.1,2.4),
                            "start_distance":rng.uniform(0,217.6)},
                    tint=[rng.uniform(0.45,1),rng.uniform(0.45,1),rng.uniform(0.65,1)])
    for _ in range(args.forest_trees):
        angle = rng.uniform(0,math.tau)
        radius = math.sqrt(rng.uniform(forest_inner**2,forest_outer**2))
        scale = rng.uniform(0.8,1.8)
        put("pine", math.cos(angle)*radius,math.sin(angle)*radius,
            scale=[scale,scale,scale*rng.uniform(0.8,1.2)],yaw_degrees=rng.uniform(0,360),
            tint=[rng.uniform(0.8,1.1),rng.uniform(0.8,1.1),rng.uniform(0.8,1)])
    scene["items"] = items
    scene["camera"] = {"target":[0,0,1],"zoom":max(0.025,min(0.8,20/(half+20))),
                       "render_distance":math.ceil(forest_outer+30)}
    out.write_text(json.dumps(scene,separators=(",", ":")) + "\n")
    manifest = {"seed":args.seed,"blocks_per_side":args.blocks,"blocks":args.blocks**2,
                "district_width_units":args.blocks*PITCH,"total_items":len(items),
                "counts":dict(sorted(counts.items())),"camera":scene["camera"]}
    out.with_suffix(".stats.json").write_text(json.dumps(manifest,indent=2)+"\n")
    print(json.dumps(manifest,indent=2))
    print(f"Run: ./build/release/io --scene {out.relative_to(ROOT) if out.is_relative_to(ROOT) else out}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--blocks",type=int,default=32,help="Blocks per side (1..48)")
    parser.add_argument("--npcs-per-block",type=int,default=8)
    parser.add_argument("--forest-trees",type=int,default=20000)
    parser.add_argument("--seed",type=int,default=7)
    parser.add_argument("--output",type=Path,default=ROOT/"assets/district/huge.json")
    args = parser.parse_args()
    if not 1<=args.blocks<=48 or not 0<=args.npcs_per_block<=64 or not 0<=args.forest_trees<=100000:
        parser.error("blocks must be 1..48, NPCs/block 0..64, forest trees 0..100000")
    generate(args)


if __name__ == "__main__":
    main()
