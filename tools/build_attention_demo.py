#!/usr/bin/env python3
"""Build a looping visibility exercise using existing assets and scene contracts."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def box(template, name, position, size, tint):
    item = copy.deepcopy(template)
    item.update(name=name, position=position, scale=size, tint=tint)
    half = [value / 2 for value in size]
    item["collider"]["shape"]["half_extents"] = half
    item["collider"]["offset"] = half
    return item


def main():
    source = ROOT / "assets/dungeon/entry.json"
    scene = json.loads(source.read_text())
    hero = copy.deepcopy(next(i for i in scene["items"] if i["name"] == "hero"))
    target = copy.deepcopy(hero)
    target.update(name="runner", position=[45, 50, 0], tint=[0.6, 0.9, 1.0])
    target["animation"] = {"clip": "running_in_place", "speed": 0.65}
    scene["items"] = [
        box(scene["items"][0], "floor", [40, 44, -0.3], [20, 20, 0.3], [0.7, 0.85, 0.8]),
        box(scene["items"][0], "barrier", [49, 54, 0], [2, 0.6, 2.6], [1.0, 0.8, 0.5]),
        hero, target,
    ]
    scene["interiors"] = []
    scene["portals"] = []
    scene["camera"].pop("rigs", None)
    scene["camera"]["target"] = [50, 58, 1.33]
    scene["camera"]["zoom"] = 1.8
    locomotion = {k: copy.deepcopy(v) for k, v in scene["traversal"].items() if k not in ("player", "navigation", "observations")}
    locomotion["walk_speed"] = 1.6
    locomotion["clips"]["walk"]["speed"] = 0.65
    scene["traversal"]["navigation"] = {
        "domain": {"origin": [40, 44, 0], "size": [20, 20], "cell_size": 0.5},
        "expansions_per_tick": 32,
        "agents": [{
            "item": "runner", "goal": [55, 50, 0], "locomotion": locomotion,
            "on_arrival": {"type": "patrol", "points": [[55, 60, 0], [45, 60, 0], [45, 50, 0], [55, 50, 0]]},
        }],
    }
    scene["traversal"]["observations"] = [{
        "observer": "runner", "target": "hero",
        "vision": {"range": 24, "fov_degrees": 120, "gain_per_second": 2.4, "decay_per_second": 0.35},
    }]
    output = source.with_name("attention.json")
    output.write_text(json.dumps(scene, indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    main()
