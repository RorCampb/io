#!/usr/bin/env python3
"""Geometry-only course; the plugin discovers connections, not an authored action list."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def scene():
    source = json.loads((ROOT / "assets/dungeon/cat-mouse.json").read_text())
    locomotion = copy.deepcopy(source["traversal"])
    locomotion.pop("observations")
    locomotion.pop("navigation")
    locomotion["walk_speed"] = 3.2
    locomotion["jump_speed"] = 5.0
    result = {
        "version": 1, "origin": [0, 0, 0], "dimensions": [40, 20, 20],
        "simulation": {"tick_hz": 60},
        "camera": {"target": [14, 3, 1], "zoom": 0.8, "render_distance": 100},
        "assets": [{"name": "stone", "builtin": "box"}],
        "packages": source["packages"], "items": [], "traversal": locomotion,
    }

    def block(name, position, size, tint):
        half = [v / 2 for v in size]
        result["items"].append({
            "name": name, "asset": "stone", "position": position, "scale": size,
            "tint": tint, "physics_body": {"type": "static"},
            "collider": {"shape": {"type": "box", "half_extents": half}, "offset": half},
        })

    # Real risers and treads. No invisible sloped collider.
    block("west", [0, 0, -0.5], [4, 6, 0.5], [0.45, 0.65, 0.66])
    for i in range(4):
        block(f"up-{i}", [4 + i, 0, -0.5], [1, 6, 0.75 + 0.25 * i],
              [0.5 + i * 0.06, 0.7, 0.75])
    block("takeoff", [8, 0, -0.5], [4, 6, 1.5], [0.6, 0.78, 0.8])
    block("landing", [13, 0, -0.5], [4, 6, 1.5], [0.65, 0.78, 0.55])
    for i in range(3):
        block(f"down-{i}", [17 + i, 0, -0.5], [1, 6, 1.25 - i * 0.25],
              [0.7, 0.78 - i * 0.06, 0.5])
    block("east", [20, 0, -0.5], [8, 6, 0.5], [0.68, 0.68, 0.45])
    block("tall-obstacle", [22, 1, 0], [1, 3, 3], [0.7, 0.35, 0.25])
    block("deck-obstacle", [9, 0, 1], [1, 3, 3], [0.7, 0.35, 0.25])
    # Catch floor is below the allowed drop and cannot provide a shortcut.
    block("pit", [0, 0, -5], [28, 6, 0.3], [0.2, 0.25, 0.3])
    block("viewer-pad", [11, 8, -0.5], [6, 4, 0.5], [0.45, 0.5, 0.6])
    hero = copy.deepcopy(next(i for i in source["items"] if i["name"] == "hero"))
    hero["position"] = [14, 10, 0]
    result["items"].append(hero)
    runner = copy.deepcopy(hero)
    runner.update(name="runner", position=[2, 3.5, 0], tint=[0.45, 0.85, 1])
    result["items"].append(runner)
    locomotion["navigation"] = {
        "planning": "background",
        "domain": {"origin": [0, 0, 0], "size": [28, 6], "cell_size": 0.5},
        "athletics": {"step_height": 0.3, "jump_distance": 3, "max_drop": 0.6},
        "expansions_per_tick": 8,
        "agents": [{
            "item": "runner", "goal": [26, 3.5, 0], "can_crouch": False,
            "on_arrival": {"type": "patrol", "points": [[2, 3.5, 0], [26, 3.5, 0]]},
        }],
    }
    return result


if __name__ == "__main__":
    output = ROOT / "assets/dungeon/athletics.json"
    output.write_text(json.dumps(scene(), indent=2) + "\n")
    print(output)
