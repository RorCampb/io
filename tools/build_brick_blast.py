"""Generate loose bricks launched together using ordinary scene velocities."""
import argparse
import json
import math
from pathlib import Path
import random


def build(seed=7, strength=1.0, count=1000):
    if isinstance(count, bool) or not isinstance(count, int) or not 1 <= count <= 100000:
        raise ValueError("count must be an integer within 1..100000")
    if not math.isfinite(strength) or not 0 <= strength <= 2:
        raise ValueError("strength must be finite and within 0..2")
    rng = random.Random(seed)
    size = [1.2, 0.6, 0.45]
    half = [v / 2 for v in size]
    tiles = math.ceil(count / 1000)
    tile_columns = math.ceil(math.sqrt(tiles))
    columns = 30 * tile_columns
    rows = 30 * math.ceil(tiles / tile_columns)
    xs = [i * 1.28 + (i // 3) * 0.22 for i in range(columns)]
    ys = [i * 0.70 + (i // 5) * 0.25 for i in range(rows)]
    ground_x = max(70, xs[-1] / 2 + 50)
    ground_y = max(70, ys[-1] / 2 + 50)
    blast_radius = max(25, math.hypot(xs[-1] / 2, ys[-1] / 2) * 1.1)
    items = [{
        "name": "ground", "asset": "box", "position": [-ground_x, -ground_y, -1],
        "scale": [2 * ground_x, 2 * ground_y, 1], "tint": [0.12, 0.15, 0.18],
        "physics_body": {"type": "static"},
        "collider": {"shape": {"type": "box", "half_extents": [ground_x, ground_y, 0.5]},
                     "offset": [ground_x, ground_y, 0.5], "friction": 0.7},
    }]
    positions = []
    for y in range(rows):
        for x in range(columns):
            if len(positions) == count:
                break
            center_x = xs[x] - xs[-1] / 2 + rng.uniform(-0.006, 0.006)
            center_y = ys[y] - ys[-1] / 2 + rng.uniform(-0.006, 0.006)
            positions.append((center_x, center_y, 0.002))
            # Repeat the original four low patches per 30x30 tile.
            tx, ty = x % 30, y % 30
            if len(positions) < count and (5 <= tx < 10 or 20 <= tx < 25) and (5 <= ty < 10 or 20 <= ty < 25):
                positions.append((center_x, center_y, size[2] + 0.004))
        if len(positions) == count:
            break

    for i, (x, y, z) in enumerate(positions):
        distance = math.hypot(x, y)
        falloff = max(0, 1 - distance / blast_radius)
        outward = (3 + 6 * falloff + rng.uniform(-0.5, 0.5)) * strength
        lift = (8 + 5 * falloff + rng.uniform(-1, 1)) * strength
        direction = (x / distance, y / distance) if distance else (1, 0)
        items.append({
            "name": f"brick-{i}", "asset": "box",
            "position": [x - half[0], y - half[1], z], "scale": list(size),
            "tint": [0.78 + rng.uniform(-0.08, 0.08), 0.38 + rng.uniform(-0.06, 0.12), 0.15],
            "physics_body": {"type": "dynamic", "mass": 2,
                             "velocity": [outward * direction[0], outward * direction[1], lift],
                             "angular_velocity": [rng.uniform(-3, 3) * strength for _ in range(3)]},
            "collider": {"shape": {"type": "box", "half_extents": list(half)},
                         "offset": list(half), "friction": 0.65, "restitution": 0.15},
        })
    camera = {"target": [0, 0, 4], "zoom": 0.7, "render_distance": 150}
    if count > 1000:
        # Allow room around the footprint for flight, not just the initial layout.
        view_x, view_y = xs[-1] + 80, ys[-1] + 80
        projected_width = (view_x + view_y) / math.sqrt(2)
        projected_height = (view_x + view_y) / math.sqrt(6) + 20 * math.sqrt(2 / 3)
        camera["zoom"] = max(0.025, min(0.7, 1024 / (16 * projected_width), 640 / (16 * projected_height)))
        camera["render_distance"] = max(150, math.hypot(view_x / 2, view_y / 2) + 30)
    return {
        "version": 1, "dimensions": [1000, 1000, 1000], "origin": [100, 100, 0],
        "camera": camera,
        "physics": {"solver": "pgs", "gravity": [0, 0, -9.81], "substeps": 4,
                    "iterations": 12, "warm_start": True, "sleep": {"enabled": True}},
        "assets": [{"name": "box", "builtin": "box"}], "items": items,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--count", type=int, default=1000, help="individual bricks, 1..100000")
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--strength", type=float, default=1, help="launch multiplier, 0..2; zero disables the blast")
    parser.add_argument("--output", type=Path, help="default name includes non-default counts")
    args = parser.parse_args()
    try:
        scene = build(args.seed, args.strength, args.count)
    except ValueError as error:
        parser.error(str(error))
    output = args.output or Path("assets/physics") / (
        "brick-blast.json" if args.count == 1000 else f"brick-blast-{args.count}.json")
    output.parent.mkdir(parents=True, exist_ok=True)
    # Keep large generated scenes compact and avoid a second full-size JSON string.
    with output.open("w") as stream:
        json.dump(scene, stream, indent=2 if args.count <= 1000 else None)
        stream.write("\n")
    upper = sum(item["position"][2] > 0.1 for item in scene["items"][1:])
    print(f"Wrote {output}: {args.count} bricks, {args.count - upper} at ground level, {upper} in low piles")


if __name__ == "__main__":
    main()
