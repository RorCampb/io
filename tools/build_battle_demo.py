#!/usr/bin/env python3
"""Build the dungeon squad exercise from the existing dungeon assets/contracts."""
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def scene():
    result = json.loads((ROOT / "assets/dungeon/entry.json").read_text())
    hero = copy.deepcopy(next(i for i in result["items"] if i["name"] == "hero"))
    hero.update(position=[45, 69, 0], tint=[.35, .75, 1.])
    items = []

    def block(name, p, size, tint):
        half = [v / 2 for v in size]
        items.append(dict(name=name, asset="stone", position=p, scale=size, tint=tint,
                          physics_body={"type": "static"},
                          collider={"shape": {"type": "box", "half_extents": half}, "offset": half}))

    stone = [.32, .37, .40]
    block("courtyard", [26, 18, -.4], [48, 58, .4], [.21, .26, .23])
    # A 28m x 24m hall, a covered entrance passage, and an alternate side entrance.
    for name, p, size in [
        ("west-wall", [35.2, 23.2, 0], [.8, 25.6, 5.5]),
        ("north-wall", [36, 23.2, 0], [28, .8, 5.5]),
        ("east-north", [64, 23.2, 0], [.8, 9.8, 5.5]),
        ("east-south", [64, 37, 0], [.8, 11.8, 5.5]),
        ("east-lintel", [64, 33, 3.2], [.8, 4, 2.3]),
        ("south-west", [36, 48, 0], [12, .8, 5.5]),
        ("south-east", [52, 48, 0], [12, .8, 5.5]),
        ("passage-west", [47.2, 48, 0], [.8, 6, 3.2]),
        ("passage-east", [52, 48, 0], [.8, 6, 3.2]),
        ("passage-roof", [48, 48, 2.6], [4, 6, .6]),
        ("hall-roof", [36, 24, 5.5], [28, 24, .4]),
        ("partition-west", [36, 35.5, 0], [10, .7, 3.1]),
        ("partition-east", [54, 35.5, 0], [10, .7, 3.1]),
    ]:
        block(name, p, size, stone)
    for x in [39, 60]:
        for y in [29, 43]:
            block(f"pillar-{x}-{y}", [x, y, 0], [1, 1, 5.5], [.45, .43, .36])
    for name, p, size in [
        ("approach-hide", [43, 63, 0], [4, 1, 2.5]),
        ("approach-low", [49, 58, 0], [3, .9, 1.3]),
        ("west-ruin", [32, 54, 0], [5, 1, 2.8]),
        ("east-ruin", [60, 58, 0], [4, 1, 2.6]),
        ("side-hide", [68, 39, 0], [1, 4, 2.7]),
        ("hall-cover-west", [41, 40, 0], [2.5, .8, 2.1]),
        ("hall-cover-east", [56.5, 40, 0], [2.5, .8, 2.1]),
        ("rear-altar", [48, 27, 0], [4, 2, 1.3]),
    ]:
        block(name, p, size, [.49, .43, .32])
    # Entrances stay flush with the courtyard: decorative thresholds must not block walking.
    items.append(hero)
    traversal = result["traversal"]
    locomotion = {k: copy.deepcopy(v) for k, v in traversal.items() if k != "player"}
    locomotion["walk_speed"] = 2.8
    agents, observations, members = [], [], []
    for name, p, role in [
        ("blade-1", [46, 44, 0], "melee"), ("blade-2", [54, 44, 0], "melee"),
        ("blade-3", [43, 30, 0], "melee"), ("blade-4", [57, 30, 0], "melee"),
        ("bow-1", [42, 38.8, 0], "ranged"), ("bow-2", [58, 38.8, 0], "ranged"),
    ]:
        npc = copy.deepcopy(hero)
        npc.update(name=name, position=p, rotation_xyzw=[0, 0, 1, 0],
                   tint=[.95, .42, .27] if role == "melee" else [.85, .72, .25])
        items.append(npc)
        agents.append(dict(item=name, goal=p, locomotion_profile="dungeon_guard", can_crouch=True,
                           steering={"look_ahead": 3, "acceleration": 7, "braking": 12, "arrival_response": .25}))
        observations.append(dict(observer=name, target="hero", notice_attention=.35,
                                 vision={"range": 26, "fov_degrees": 240, "gain_per_second": 1.6,
                                         "decay_per_second": .6}))
        members.append(dict(item=name, role=role))
    traversal.update(
        locomotion_profiles={"dungeon_guard": locomotion},
        navigation={"domain": {"origin": [26, 18, 0], "size": [48, 58], "cell_size": .6},
                    "planning": "background", "expansions_per_tick": 16, "agents": agents},
        observations=observations, debug_routes=True,
        battle={"members": members, "preparation_seconds": 1.2, "memory_seconds": 12,
                "decision_seconds": .75, "report_seconds": .6})
    result["items"] = items
    result["interiors"] = [
        {"name": "Gate Passage", "min": [48, 48, 0], "max": [52, 54, 2.6], "ceiling": 2.6, "entry_direction": [0, -1, 0]},
        {"name": "Guard Hall", "min": [36, 24, 0], "max": [64, 48, 5.5], "ceiling": 5.5, "entry_direction": [0, -1, 0]},
    ]
    result["portals"] = [
        {"name": "Outer Gate", "from": {"type": "exterior"}, "to": {"type": "interior", "name": "Gate Passage"},
         "center": [50, 54, 1.3], "normal": [0, -1, 0], "width": 4, "height": 2.6},
        {"name": "Hall Gate", "from": {"type": "interior", "name": "Gate Passage"}, "to": {"type": "interior", "name": "Guard Hall"},
         "center": [50, 48, 1.3], "normal": [0, -1, 0], "width": 4, "height": 2.6},
        {"name": "Side Door", "from": {"type": "exterior"}, "to": {"type": "interior", "name": "Guard Hall"},
         "center": [64, 35, 1.6], "normal": [-1, 0, 0], "width": 4, "height": 3.2},
    ]
    result["camera"].pop("rigs", None)
    result["camera"].update(target=[45, 69, 1.33], zoom=3)
    result["simulation"] = {"tick_hz": 60}
    return result


if __name__ == "__main__":
    path = ROOT / "assets/dungeon/battle.json"
    path.write_text(json.dumps(scene(), indent=2) + "\n")
    print(path)
