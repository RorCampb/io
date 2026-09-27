#!/usr/bin/env python3
"""Derive a data-only navigation demo without changing the camera regression scene."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    source = ROOT / "assets/dungeon/entry.json"
    scene = json.loads(source.read_text())
    hero = next(item for item in scene["items"] if item["name"] == "hero")
    npc = copy.deepcopy(hero)
    npc.update(name="scout", position=[51, 59, 0])
    scene["items"].append(npc)
    scene["traversal"]["navigation"] = {
        "domain": {"origin": [40, 30, 0], "size": [20, 36], "cell_size": 0.5},
        "expansions_per_tick": 12,
        "agents": [{
            "item": "scout",
            "goal": [50, 37, 0],
            "can_crouch": True,
            "familiar_points": [[51, 59, 0], [51, 58, 0]],
        }],
    }
    output = source.with_name("navigation.json")
    output.write_text(json.dumps(scene, indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    main()
