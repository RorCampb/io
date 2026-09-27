#!/usr/bin/env python3
"""Deterministic district workload. Patrol destinations are not precomputed routes."""
import argparse
import copy
import json
import math
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def scene(count=128, moving="timed", long_routes=False, overview=False, tick_hz=144, debug_routes=False):
    if not 1 <= count <= 256 or moving not in ("timed", "continuous", "static"):
        raise ValueError("use 1..256 NPCs and timed/continuous/static moving geometry")
    if type(tick_hz) is not int or not 4 <= tick_hz <= 1000:
        raise ValueError("tick_hz must be an integer between 4 and 1000")
    source = json.loads((ROOT / "assets/dungeon/cat-mouse.json").read_text())
    locomotion = copy.deepcopy(source["traversal"])
    locomotion.pop("observations")
    locomotion["obstacle_observations"] = {
        "vision": {"range": 18, "fov_degrees": 200, "gain_per_second": 6, "decay_per_second": 2},
        "notice_attention": 0.15, "interval_seconds": 0.1, "retention_seconds": 2,
        "observers_per_tick": 8, "tracks_per_observer": 6,
    }
    locomotion.pop("navigation")
    locomotion["walk_speed"] = 3.2
    result = {"version": 1, "origin": [0, 0, 0], "dimensions": [400, 400, 40],
              "simulation": {"tick_hz": tick_hz},
              "camera": {"target": [150, 150, 1.33], "follow": "hero", "zoom": .7,
                         "render_distance": 650},
              "assets": [{"name": "stone", "builtin": "box"}],
              "packages": source["packages"], "items": [], "traversal": locomotion}
    if overview:
        result["camera"] = {"target": [200, 200, 0], "zoom": .07, "render_distance": 650}
    items = result["items"]

    def box(name, p, size, tint, body="static", rotation=None):
        item = {"name": name, "asset": "stone", "position": p, "scale": size, "tint": tint}
        if body:
            half = [v / 2 for v in size]
            item.update(physics_body={"type": body},
                        collider={"shape": {"type": "box", "half_extents": half}, "offset": half})
        if rotation:
            item["rotation_xyzw"] = rotation
        items.append(item)

    for y in range(20):
        for x in range(20):
            box(f"terrain-{x}-{y}", [20*x, 20*y, -.5], [20, 20, .5],
                [.26 + .012*((x+y) % 3), .39 + .012*(x % 3), .32])
    centers = [(x, y) for y in (150, 250, 50, 350) for x in (150, 250, 50, 350)]
    cycles = []
    for district, (x, y) in enumerate(centers):
        prefix = f"district-{district}"
        for dx, dy, sx, sy in [(-34, -32, 68, 16), (-34, 18, 68, 16),
                               (-34, -18, 16, 36), (18, -18, 16, 36)]:
            box(f"{prefix}-paving-{dx}-{dy}", [x+dx, y+dy, .01], [sx, sy, .01],
                [.54, .58, .48], None)
        for h, (dx, dy) in enumerate([(-43, -42), (-10, -43), (25, -42),
                                     (-43, 34), (-10, 35), (25, 34)]):
            tint = [.62 + .04*(h % 2), .54, .40]
            box(f"{prefix}-house-{h}", [x+dx, y+dy, 0], [14, 10, 6], tint)
            box(f"{prefix}-roof-{h}", [x+dx-.5, y+dy-.5, 6], [15, 11, 1], [.4, .23, .18])
            box(f"{prefix}-door-{h}", [x+dx+6, y+dy-.03, 0], [2, .05, 2.8], [.2, .16, .12], None)
            for window in (2, 10):
                box(f"{prefix}-window-{h}-{window}", [x+dx+window, y+dy-.04, 3],
                    [2, .06, 1.5], [.65, .8, .82], None)
        for k in range(6):
            box(f"{prefix}-crate-{k}", [x-9+3*k, y-25, 0], [2, 2, 1.4], [.66, .45, .22])
            box(f"{prefix}-lamp-{k}", [x-15+6*k, y+32, 0], [.4, .4, 4], [.24, .27, .25])
            box(f"{prefix}-light-{k}", [x-15+6*k-.3, y+31.7, 4], [1, 1, .4], [1, .85, .38], None)
        box(f"{prefix}-cover-west", [x-14, y+11, 0], [8, 1, 2.5], [.43, .48, .56])
        box(f"{prefix}-cover-east", [x+6, y+10, 0], [1, 6, 2.5], [.43, .48, .56])
        # The raised market crossing is physical geometry, not an authored nav link.
        angle = math.atan2(2, 12)
        # Extend below the floor so feet-center support exists before the body's
        # leading edge contacts the incline (the support contract is center-based).
        length = math.hypot(13, 13/6)
        box(f"{prefix}-ramp-up", [x-19+.4*math.sin(angle), y-3, -1/6-.4*math.cos(angle)],
            [length, 6, .4], [.57, .62, .65], rotation=[0, -math.sin(angle/2), 0, math.cos(angle/2)])
        box(f"{prefix}-deck", [x-6, y-3, 0], [12, 6, 2], [.60, .65, .68])
        box(f"{prefix}-ramp-down", [x+6-.4*math.sin(angle), y-3, 2-.4*math.cos(angle)],
            [length, 6, .4], [.57, .62, .65], rotation=[0, math.sin(angle/2), 0, math.cos(angle/2)])
        if district < 8:
            name = f"moving-load-{district}"
            box(name, [x-5, y+20, 0], [3, 3, 2], [1, .48, .12], "kinematic")
            if moving != "static":
                cycles.append({"item": name, "alternate": [x+5, y+20, 0],
                               "interval_seconds": 12 if moving == "timed" else 0,
                               "travel_seconds": 3})
    hero = copy.deepcopy(next(i for i in source["items"] if i["name"] == "hero"))
    hero["position"] = [150, 142, 0]
    hero["tint"] = [1, .9, .35]
    items.append(hero)
    agents = []
    colors = [[.25, .8, 1], [1, .5, .22], [.7, .95, .35], [.95, .7, .5]]
    for i in range(count):
        district, slot = i % 16, i // 16
        x, y = centers[district]
        if slot == 0:
            points = [[x-24, y, 0], [x, y, 2], [x+24, y, 0], [x+24, y+18, 0], [x-24, y+18, 0]]
        else:
            # Distinct lanes/spawns. The provider remains responsible for detours.
            lane = (slot-1)//4
            radius = 20 + 2*lane
            points = [[x-radius, y-radius, 0], [x+radius, y-radius, 0],
                      [x+radius, y+radius, 0], [x-radius, y+radius, 0]]
            shift = (slot-1) % 4
            points = points[shift:] + points[:shift]
        npc = copy.deepcopy(hero)
        # Start partway along a leg, not on another agent's first destination.
        spawn = points[0] if slot == 0 else [a + .15*(b-a) for a, b in zip(points[0], points[1])]
        npc.update(name=f"npc-{i:03}", position=spawn, tint=colors[slot % 4])
        items.append(npc)
        patrol = points[2:] + points[:2]
        goal = points[1]
        if long_routes and slot == 1:
            other_x, other_y = centers[(district+5) % len(centers)]
            goal = [other_x-20, other_y-20, 0]
            patrol = [points[0], goal]
        agents.append({"item": npc["name"], "goal": goal, "can_crouch": True,
                       "on_arrival": {"type": "patrol", "points": patrol}})
    locomotion.update(barrier_cycles=cycles, debug_routes=debug_routes,
        navigation={"planning": "background", "domain": {"origin": [0, 0, 0],
                    "size": [400, 400], "cell_size": 2},
                    "expansions_per_tick": 16, "agents": agents})
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--npcs", type=int, default=128)
    p.add_argument("--moving", choices=["timed", "continuous", "static"], default="timed")
    p.add_argument("--long-routes", action="store_true")
    p.add_argument("--overview", action="store_true")
    p.add_argument("--tick-hz", type=int, default=144)
    p.add_argument("--debug-routes", action="store_true", help="draw accepted routes and waypoint crosses")
    p.add_argument("--output", type=Path)
    a = p.parse_args()
    if not 1 <= a.npcs <= 256:
        p.error("this layout supports 1..256 nonoverlapping NPC spawns")
    if not 4 <= a.tick_hz <= 1000:
        p.error("--tick-hz must be between 4 and 1000")
    result = scene(a.npcs, a.moving, a.long_routes, a.overview, a.tick_hz, a.debug_routes)
    output = a.output or ROOT / "assets/dungeon" / f"stress-{a.npcs}.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    for package in result["packages"]:
        package["file"] = os.path.relpath(
            (ROOT / "assets/dungeon" / package["file"]).resolve(), output.resolve().parent)
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(f"{output}: 400x400m, {a.npcs} NPCs, {len(result['items'])} Items, "
          f"{sum('collider' in i for i in result['items'])} colliders, {a.moving} movers")


if __name__ == "__main__":
    main()
