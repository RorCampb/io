"""Author a small collider-backed entrance scene using the standard asset contract."""
import json
from pathlib import Path


def rig(yaw, pitch, zoom, height, yaw_limit, min_zoom, max_zoom, fov=45):
    return {"yaw_degrees": yaw, "pitch_degrees": pitch, "zoom": zoom, "fov_degrees": fov,
            "target_height": height, "yaw_limit_degrees": yaw_limit,
            "min_zoom": min_zoom, "max_zoom": max_zoom,
            "response_seconds": 0.25, "approach": 4}


def scene():
    items = []

    def box(name, position, size, color):
        items.append({"name": name, "asset": "stone", "position": position,
                      "scale": size, "tint": color,
                      "physics_body": {"type": "static"},
                      "collider": {"shape": {"type": "box", "half_extents": [s / 2 for s in size]},
                                   "offset": [s / 2 for s in size]}})

    stone = [0.26, 0.31, 0.35]
    trim = [0.40, 0.36, 0.27]
    box("courtyard", [30, 20, -0.3], [40, 50, 0.3], [0.16, 0.22, 0.18])
    box("front-left", [42, 49, 0], [6.5, 0.8, 5], stone)
    box("front-right", [51.5, 49, 0], [6.5, 0.8, 5], stone)
    box("lintel", [48.5, 49, 1.5], [3, 0.8, 3.5], trim)
    box("passage-left", [47.7, 43, 0], [0.8, 6, 3], stone)
    box("passage-right", [51.5, 43, 0], [0.8, 6, 3], stone)
    box("passage-ceiling", [48.5, 43, 1.5], [3, 6, 0.3], stone)
    box("room-left", [43.2, 30.2, 0], [0.8, 13.6, 4], stone)
    box("room-right", [56, 30.2, 0], [0.8, 13.6, 4], stone)
    box("room-back", [44, 30.2, 0], [12, 0.8, 4], stone)
    box("room-front-left", [44, 43, 0], [3.7, 0.8, 4], stone)
    box("room-front-right", [52.3, 43, 0], [3.7, 0.8, 4], stone)
    box("room-roof", [44, 31, 4], [12, 12, 0.3], stone)
    box("jump-plinth", [53, 55, 0], [2, 2, 0.55], trim)
    box("room-plinth", [48.5, 33, 0], [3, 2, 0.45], trim)
    for side in [46, 53]:
        for y in [53, 62]:
            box(f"approach-pillar-{side}-{y}", [side, y, 0], [0.7, 0.7, 2.4], trim)
    items.append({"name": "hero", "appearance": "mixamo/x_bot", "position": [50, 58, 0],
                  "character_body": {"radius": 0.35, "height": 1.9, "max_slope": 0.8},
                  "animation": {"clip": "idle", "speed": 1}})

    def clip(name, speed=1, looping=True, fixed=False):
        return {"clip": name, "speed": speed, "looping": looping, "fixed_root_height": fixed}

    return {
        "version": 1, "origin": [0, 0, 0], "dimensions": [100, 100, 30],
        "simulation": {"tick_hz": 60},
        "camera": {"target": [50, 58, 1.33], "follow": "hero", "zoom": 3,
                   "render_distance": 120,
                   "orbit": {"target_height_fraction": 0.7, "min_distance": 0.4, "max_distance": 35, "clearance": 0.12,
                             "response_seconds": 0.18, "lookahead_seconds": 0},
                   "projection": {"type": "zoom_perspective", "start_zoom": 1.5,
                                  "end_zoom": 5, "vertical_fov_degrees": 45,
                                  "near_clip": 0.05, "smoothing_seconds": 0.16},
                   "rigs": {"motion": {"type": "interior_envelope", "clearance": 0.12,
                                       "close_distance": 0.6, "max_retreat": 8,
                                       "max_rise": 3, "max_fov_degrees": 95},
                            "exterior": rig(45, 35.26439, 3, 1, 180, 0.5, 16),
                            "interiors": {"Low Passage": rig(0, 5, 16, 0.7, 180, 0.5, 16, 50),
                                          "Entry Chamber": rig(0, 15, 10, 1, 180, 0.5, 16, 68)}}},
        "portals": [
            {"name": "Courtyard Entrance", "from": {"type": "exterior"},
             "to": {"type": "interior", "name": "Low Passage"}, "center": [50, 49.8, 0.75],
             "normal": [0, -1, 0], "width": 3, "height": 1.5},
            {"name": "Chamber Entrance", "from": {"type": "interior", "name": "Low Passage"},
             "to": {"type": "interior", "name": "Entry Chamber"}, "center": [50, 43, 0.75],
             "normal": [0, -1, 0], "width": 3, "height": 1.5},
        ],
        "interiors": [
            {"name": "Low Passage", "min": [48.5, 43, 0], "max": [51.5, 49.8, 1.5],
             "ceiling": 1.5, "entry_direction": [0, -1, 0]},
            {"name": "Entry Chamber", "min": [44, 31, 0], "max": [56, 43, 4],
             "ceiling": 4, "entry_direction": [0, -1, 0]},
        ],
        "assets": [{"name": "stone", "builtin": "box"}],
        "packages": [{"namespace": "mixamo", "file": "../action-adventure/package.json"}],
        "items": items,
        "traversal": {"player": "hero", "walk_speed": 3.4, "crouch_speed": 0.95,
                      "jump_speed": 4.5, "gravity": 9.81, "standing_height": 1.9,
                      "crouch_height": 1.15, "turn_speed": 8, "blend_seconds": 0.12,
                      "clips": {"idle": clip("idle"), "walk": clip("running_in_place", 0.85),
                                "crouch_idle": clip("crouched_sneaking_left", 0),
                                "crouch_walk": clip("crouched_sneaking_left", 0.9),
                                "jump": clip("jumping_up", looping=False, fixed=True),
                                "fall": clip("falling_idle", fixed=True),
                                "land": clip("hard_landing", looping=False)}},
    }


if __name__ == "__main__":
    path = Path(__file__).resolve().parents[1] / "assets/dungeon/entry.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(scene(), indent=2) + "\n")
    print(path)
