#!/usr/bin/env python3
"""Three independent crossing lanes: 1, 3 and 6 m/s observed barriers."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def scene():
    source = json.loads((ROOT / "assets/dungeon/cat-mouse.json").read_text())
    traversal = copy.deepcopy(source["traversal"])
    traversal.pop("observations")
    traversal["debug_routes"] = True
    traversal["obstacle_observations"] = {
        "reaction": {}, "interval_seconds": 0.05, "observers_per_tick": 3,
        "tracks_per_observer": 4, "notice_attention": 0.15,
        "vision": {"range": 18, "fov_degrees": 240,
                   "gain_per_second": 6, "decay_per_second": 2},
    }
    result = {"version": 1, "origin": [0, 0, 0], "dimensions": [100, 100, 30],
              "simulation": {"tick_hz": 144},
              "camera": {"target": [30, 54, 1.33], "zoom": 0.8, "render_distance": 150},
              "assets": source["assets"], "packages": source["packages"],
              "items": [], "traversal": traversal}
    items = result["items"]

    def box(name, position, size, tint, body="static"):
        half = [s / 2 for s in size]
        items.append({"name": name, "asset": "stone", "position": position,
                      "scale": size, "tint": tint, "physics_body": {"type": body},
                      "collider": {"shape": {"type": "box", "half_extents": half}, "offset": half}})

    box("floor", [15, 32, -0.3], [32, 44, 0.3], [.3, .42, .36])
    hero = copy.deepcopy(next(i for i in source["items"] if i["name"] == "hero"))
    hero["position"] = [18, 34, 0]
    items.append(hero)
    agents, cycles = [], []
    for lane, speed in enumerate([1, 3, 6]):
        y = 40 + 14 * lane
        npc = copy.deepcopy(next(i for i in source["items"] if i["name"] == "runner"))
        npc.update(name=f"npc-{speed}", position=[20, y, 0])
        items.append(npc)
        agent = copy.deepcopy(source["traversal"]["navigation"]["agents"][0])
        agent.pop("pursuit")
        agent.update(item=npc["name"], goal=[40, y, 0],
                     on_arrival={"type": "patrol", "points": [[20, y, 0], [40, y, 0]]})
        agent["steering"]["trajectory"] = {}
        agents.append(agent)
        box(f"crossing-{speed}", [30, y-6, 0], [1.5, 1.5, 2.2],
            [1, .75-.08*lane, .2], "kinematic")
        cycles.append({"item": f"crossing-{speed}", "alternate": [30, y+6, 0],
                       "interval_seconds": 1.7, "travel_seconds": 12/speed})
    traversal["barrier_cycles"] = cycles
    traversal["navigation"] = {"domain": {"origin": [15, 32, 0], "size": [32, 44], "cell_size": .5},
                               "planning": "background", "expansions_per_tick": 32, "agents": agents}
    return result


if __name__ == "__main__":
    path = ROOT / "assets/dungeon/reactions.json"
    path.write_text(json.dumps(scene(), indent=2) + "\n")
    print(path)
