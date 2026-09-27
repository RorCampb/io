#!/usr/bin/env python3
"""Two independently observing guards; routes are discovered from real course geometry."""
import copy
import json
from pathlib import Path
from build_athletics_demo import scene as course

ROOT = Path(__file__).resolve().parents[1]


def scene():
    result = course()
    source = json.loads((ROOT / "assets/dungeon/cat-mouse.json").read_text())
    items = result["items"]
    items[:] = [i for i in items if i["name"] not in ("viewer-pad", "runner", "tall-obstacle")]

    def block(name, position, size, tint, physical=True):
        half = [v / 2 for v in size]
        item = {"name": name, "asset": "stone", "position": position, "scale": size,
                "tint": tint, "collider": {"shape": {"type": "box", "half_extents": half}, "offset": half}}
        if physical:
            item["physics_body"] = {"type": "static"}
        else:
            item["physics_body"] = {"type": "kinematic"}
        items.append(item)

    # The non-jumper must go north around the gap. Same height, real connected surfaces.
    block("west-bridge-approach", [8, 6, -.5], [4, 4, 1.5], [.55, .7, .4])
    block("east-bridge-approach", [13, 6, -.5], [4, 4, 1.5], [.55, .7, .4])
    block("bridge", [12, 8, -.5], [1, 2, 1.5], [.8, .7, .4])
    block("hide-wall", [24, .5, 0], [.5, 3, 3], [.55, .6, .7])
    block("hide-wall-east", [26, 3.5, 0], [1.5, .5, 3], [.55, .6, .7])
    block("shutter", [12, 8, 5], [1, 2, 2.5], [1, .4, .15], False)
    hero = next(i for i in items if i["name"] == "hero")
    hero["position"] = [25, 2, 0]
    for name, y, tint in [("jumper", 2, [.3, .8, 1]), ("walker", 4.5, [1, .65, .25])]:
        npc = copy.deepcopy(hero)
        npc.update(name=name, position=[2, y, 0], tint=tint)
        items.append(npc)
    nav = result["traversal"]["navigation"]
    nav["domain"]["size"] = [28, 10]
    nav["expansions_per_tick"] = 16
    nav["agents"] = []
    observations = []
    for name, y, jump in [("jumper", 2, 3), ("walker", 4.5, 0)]:
        agent = copy.deepcopy(source["traversal"]["navigation"]["agents"][0])
        agent.update(item=name, goal=[26, y, 0], can_crouch=False,
                     athletics={"step_height": .3, "jump_distance": jump, "max_drop": .6})
        agent["locomotion"]["walk_speed"] = 3.2
        agent["locomotion"]["jump_speed"] = 5
        agent["on_arrival"] = {"type": "patrol", "points": [[2, y, 0], [26, y, 0]]}
        agent["pursuit"]["settings"]["search_seconds"] = 15
        agent["pursuit"]["settings"]["search_radius"] = 1
        nav["agents"].append(agent)
        observation = copy.deepcopy(source["traversal"]["observations"][0])
        observation["observer"] = name
        observation["vision"]["range"] = 14
        observations.append(observation)
    result["traversal"].update(observations=observations, debug_routes=True,
        barrier_cycle={"item": "shutter", "alternate": [12, 8, 1], "interval_seconds": 12})
    result["camera"].update(target=[14, 5, 1], zoom=1.5)
    return result


if __name__ == "__main__":
    output = ROOT / "assets/dungeon/guards.json"
    output.write_text(json.dumps(scene(), indent=2) + "\n")
    print(output)
