#!/usr/bin/env python3
"""Compose an autonomous pursuit playground from the existing attention assets."""
import json
from pathlib import Path

from build_attention_demo import box

ROOT = Path(__file__).resolve().parents[1]


def main():
    source = ROOT / "assets/dungeon/attention.json"
    scene = json.loads(source.read_text())
    scene["traversal"]["observations"][0]["vision"]["fov_degrees"] = 240
    floor = scene["items"][0]
    for name, position, size, tint in [
        ("north-wall", [40, 44, 0], [20, .4, 2.6], [.65, .75, .8]),
        ("south-wall", [40, 63.6, 0], [20, .4, 2.6], [.65, .75, .8]),
        ("west-wall", [40, 44.4, 0], [.4, 19.2, 2.6], [.65, .75, .8]),
        ("east-wall", [59.6, 44.4, 0], [.4, 19.2, 2.6], [.65, .75, .8]),
        ("corner-long", [44, 53, 0], [3, .6, 2.6], [.7, .85, .95]),
        ("corner-short", [44, 53.6, 0], [.6, 2.4, 2.6], [.7, .85, .95]),
        ("low-cover", [54, 53, 0], [2, 1, 1.25], [.95, .75, .45]),
        ("north-stack", [49, 47, 0], [1.5, 1.5, 2.], [.9, .65, .4]),
        ("south-stack", [47, 60, 0], [1.2, 1.2, 1.3], [.9, .65, .4]),
    ]:
        scene["items"].append(box(floor, name, position, size, tint))
    npc = scene["traversal"]["navigation"]["agents"][0]
    npc["locomotion"]["walk_speed"] = 2.7
    npc["locomotion"]["clips"]["walk"]["speed"] = .9
    npc["steering"] = {"look_ahead": 3, "acceleration": 9, "braking": 14, "arrival_response": .25}
    npc["pursuit"] = {
        "target": "hero",
        "settings": {
            "notice_attention": .18,
            "minimum_focus": .03,
            "lost_sight_seconds": .75,
            "search_seconds": 12,
            "search_radius": 3,
            "repath_seconds": .5,
            "tag_distance": 1.25,
            "grace_seconds": 3,
        },
    }
    output = source.with_name("cat-mouse.json")
    output.write_text(json.dumps(scene, indent=2) + "\n")
    print(output)


if __name__ == "__main__":
    main()
