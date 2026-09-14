"""Generate a repeatable rigid-body wall or brick volume without modifying models."""
import argparse
import json
import math
import os
from pathlib import Path


def build(count, columns, rows, speed, gravity, mass, seed, depth=1, brick_size=(1.3, 1.2, 0.8), sleep=True, warm_start=True, iterations=None, solver="pgs", substeps=None):
    defaults = {"pgs": (4, 12), "tgs": (8, 4)}[solver]
    substeps = defaults[0] if substeps is None else substeps
    iterations = defaults[1] if iterations is None else iterations
    import random
    rng = random.Random(seed)
    items = []

    def box(name, position, size, fixed=False):
        items.append({"name": name, "asset": "box", "position": position, "scale": size,
                      "tint": [0.16, 0.19, 0.23] if fixed else [0.72, 0.46 + rng.random()*0.15, 0.2],
                      "physics_body": {"type": "static"} if fixed else {"type": "dynamic", "mass": 25},
                      "collider": {"shape": {"type": "box", "half_extents": [v/2 for v in size]},
                                   "offset": [v/2 for v in size], "friction": 0.7}})

    sx, sy, sz = brick_size
    span_x, span_y, span_z = [n * (v + 0.002) - 0.002 for n, v in zip((columns, depth, rows), brick_size)]
    ground_x = max(50, span_x / 2 + 20)
    ground_y = max(40, span_y + 20)
    box("ground", [-ground_x, -40, -1], [ground_x * 2, ground_y + 40, 1], True)
    for z in range(rows):
        for y in range(depth):
            for x in range(columns):
                name = f"block-{z}-{x}" if depth == 1 else f"block-{z}-{y}-{x}"
                box(name, [x*(sx+0.002)-columns*(sx+0.002)/2, y*(sy+0.002), z*(sz+0.002)], list(brick_size))
    width = max(1, min(columns, math.ceil(math.sqrt(count))))
    for i in range(count):
        items.append({"name": f"projectile-{i}", "asset": "runner",
                      "position": [(i % width-(width-1)/2)*1.5, -12-(i//width)*2.5, 1],
                      "yaw_degrees": 180, "tint": [0.85, 0.94, 1],
                      "physics_body": {"type": "dynamic", "mass": mass,
                                       "velocity": [rng.uniform(-0.3, 0.3), speed, 4],
                                       "angular_velocity": [0.4, 0.2, 0.1]},
                      "collider": {"shape": {"type": "box", "half_extents": [0.4, 0.3, 0.9]},
                                   "offset": [0, 0, 0.9], "friction": 0.5, "restitution": 0.1}})
    camera = {"target": [0, -2, 2], "zoom": 1.5, "render_distance": 120}
    if depth > 1:
        front = -14 - math.ceil(count / width) * 2.5 if count else 0
        # Fit a conservative isometric projection in the default 1280x800 window.
        view_y = span_y - front
        projected_width = (span_x + view_y) / math.sqrt(2)
        projected_height = (span_x + view_y) / math.sqrt(6) + span_z * math.sqrt(2/3)
        camera = {"target": [0, (span_y + front)/2, span_z/2],
                  "zoom": max(0.025, min(1.5, 1024/(16*projected_width), 640/(16*projected_height))),
                  "render_distance": max(120, math.sqrt(span_x**2 + view_y**2 + span_z**2))}
    return {"version": 1, "dimensions": [1000, 1000, 1000], "origin": [100, 100, 0],
            "camera": camera,
            "physics": {"solver": solver, "gravity": [0, 0, gravity], "substeps": substeps, "iterations": iterations,
                        "warm_start": warm_start, "sleep": {"enabled": sleep}},
            "assets": [{"name": "box", "builtin": "box"}, {"name": "runner", "file": ""}],
            "items": items}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--count", type=int, default=16)
    p.add_argument("--columns", type=int, default=12)
    p.add_argument("--rows", type=int, default=6)
    p.add_argument("--depth", type=int, default=1, help="brick layers front to back")
    p.add_argument("--brick-size", type=float, nargs=3, default=[1.3, 1.2, 0.8], metavar=("X", "Y", "Z"))
    p.add_argument("--sleep", action=argparse.BooleanOptionalAction, default=True)
    p.add_argument("--warm-start", action=argparse.BooleanOptionalAction, default=True)
    p.add_argument("--solver", choices=["pgs", "tgs"], default="pgs")
    p.add_argument("--substeps", type=int, help="default: PGS 4, TGS 8")
    p.add_argument("--iterations", type=int, help="default: PGS 12, TGS 4")
    p.add_argument("--speed", type=float, default=16)
    p.add_argument("--gravity", type=float, default=-9.81)
    p.add_argument("--mass", type=float, default=70)
    p.add_argument("--seed", type=int, default=7)
    p.add_argument("--output", type=Path, default=Path("assets/physics/impact.json"))
    args = p.parse_args()
    defaults = {"pgs": (4, 12), "tgs": (8, 4)}[args.solver]
    args.substeps = defaults[0] if args.substeps is None else args.substeps
    args.iterations = defaults[1] if args.iterations is None else args.iterations
    if not 1 <= args.substeps <= 32:
        p.error("substeps must be 1..32")
    if not 1 <= args.iterations <= 64:
        p.error("iterations must be 1..64")
    if not 0 <= args.count <= 10000 or any(not 1 <= n <= 100 for n in [args.columns, args.rows, args.depth]):
        p.error("count must be 0..10000; columns, rows, and depth must be 1..100")
    if args.columns * args.rows * args.depth > 100000:
        p.error("brick volumes are limited to 100000 bodies; this is not a million-body solver")
    if any(not math.isfinite(v) or not 0.1 <= v <= 5 for v in args.brick_size):
        p.error("brick-size components must be 0.1..5")
    if not all(math.isfinite(v) for v in [args.speed, args.gravity, args.mass]) or not 0 <= args.speed <= 50 or not -100 <= args.gravity <= 100 or not 0.01 <= args.mass <= 10000:
        p.error("expected speed 0..50, gravity -100..100, mass 0.01..10000")
    scene = build(args.count, args.columns, args.rows, args.speed, args.gravity, args.mass, args.seed, args.depth, args.brick_size, args.sleep, args.warm_start, args.iterations, args.solver, args.substeps)
    asset = Path(__file__).resolve().parent.parent / "assets/street-kit/runner.glb"
    scene["assets"][1]["file"] = os.path.relpath(asset, args.output.resolve().parent)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(scene, indent=2) + "\n")
    print(f"Wrote {args.output}: {args.rows*args.columns*args.depth} blocks, {args.count} launched bodies")


if __name__ == "__main__":
    main()
