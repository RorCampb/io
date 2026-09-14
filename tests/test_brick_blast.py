import contextlib
import io
import json
import math
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools import build_brick_blast as blast


class BrickBlastTests(unittest.TestCase):
    def test_large_count_scales_footprint_floor_and_camera_not_body_sizes(self):
        scene = blast.build(count=100000)
        bricks = scene["items"][1:]
        self.assertEqual(len(bricks), 100000)
        self.assertEqual(len({item["name"] for item in bricks}), 100000)
        self.assertEqual(len({tuple(item["position"]) for item in bricks}), 100000)
        self.assertEqual(sum(item["position"][2] > 0.1 for item in bricks), 10000)
        ground = scene["items"][0]
        for item in bricks:
            self.assertEqual(item["scale"], [1.2, 0.6, 0.45])
            self.assertGreater(item["physics_body"]["velocity"][2], 0)
            for axis in range(2):
                self.assertGreaterEqual(item["position"][axis], ground["position"][axis])
                self.assertLessEqual(item["position"][axis] + item["scale"][axis],
                                     ground["position"][axis] + ground["scale"][axis])
            self.assertLess(math.hypot(*item["position"][:2]), scene["camera"]["render_distance"])
        self.assertLess(scene["camera"]["zoom"], 0.7)
        self.assertGreaterEqual(scene["camera"]["zoom"], 0.025)
        self.assertEqual(scene["physics"], blast.build()["physics"])

    def test_partial_counts_and_existing_default_scene(self):
        for count in [1, 17, 999, 1001, 2345]:
            self.assertEqual(len(blast.build(count=count)["items"]), count + 1)
        for count in [0, -1, 100001, 1.5, True]:
            with self.assertRaises(ValueError):
                blast.build(count=count)
        path = Path(__file__).resolve().parent.parent / "assets/physics/brick-blast.json"
        self.assertEqual(json.loads(path.read_text()), blast.build())

    def test_count_low_piles_and_nonoverlapping_colliders(self):
        scene = blast.build()
        bricks = scene["items"][1:]
        self.assertEqual(len(bricks), 1000)
        self.assertEqual(len({item["name"] for item in bricks}), 1000)
        self.assertEqual(sum(item["position"][2] < 0.1 for item in bricks), 900)
        self.assertEqual(sum(item["position"][2] > 0.1 for item in bricks), 100)
        self.assertTrue(all(item["physics_body"]["type"] == "dynamic" for item in bricks))
        self.assertEqual(scene["assets"], [{"name": "box", "builtin": "box"}])
        ordered = sorted(bricks, key=lambda item: item["position"][0])
        for index, a in enumerate(ordered):
            for b in ordered[index + 1:]:
                if b["position"][0] >= a["position"][0] + a["scale"][0]:
                    break
                self.assertTrue(any(
                    a["position"][axis] + a["scale"][axis] <= b["position"][axis]
                    or b["position"][axis] + b["scale"][axis] <= a["position"][axis]
                    for axis in range(3)), (a["name"], b["name"]))

    def test_simultaneous_outward_launch_is_seeded_and_configurable(self):
        scene = blast.build()
        self.assertEqual(scene, blast.build())
        self.assertNotEqual(scene, blast.build(seed=8))
        for item in scene["items"][1:]:
            velocity = item["physics_body"]["velocity"]
            self.assertGreater(velocity[2], 0)
            center = [p + s / 2 for p, s in zip(item["position"], item["scale"])]
            self.assertGreater(center[0] * velocity[0] + center[1] * velocity[1], 0)
            self.assertTrue(all(math.isfinite(v) for v in velocity))
        for normal, doubled, off in zip(scene["items"][1:], blast.build(strength=2)["items"][1:],
                                        blast.build(strength=0)["items"][1:]):
            self.assertEqual(normal["position"], doubled["position"])
            for key in ["velocity", "angular_velocity"]:
                self.assertEqual(doubled["physics_body"][key], [2 * v for v in normal["physics_body"][key]])
                self.assertEqual(off["physics_body"][key], [0, 0, 0])

    def test_cli_rejects_invalid_strength_without_overwriting(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "blast.json"
            with patch("sys.argv", ["blast", "--output", str(output)]), contextlib.redirect_stdout(io.StringIO()):
                blast.main()
            saved = output.read_bytes()
            self.assertEqual(json.loads(saved), blast.build())
            for option, value in [("--strength", "nan"), ("--strength", "inf"),
                                  ("--strength", "-1"), ("--strength", "3"),
                                  ("--count", "0"), ("--count", "100001"), ("--count", "1.5")]:
                with patch("sys.argv", ["blast", "--output", str(output), option, value]), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as stopped:
                        blast.main()
                self.assertEqual(stopped.exception.code, 2)
                self.assertEqual(output.read_bytes(), saved)


if __name__ == "__main__":
    unittest.main()
