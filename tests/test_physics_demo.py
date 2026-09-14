import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools import build_physics_demo as demo


class PhysicsDemoTests(unittest.TestCase):
    def test_ten_thousand_block_scene_has_no_projectiles(self):
        scene = demo.build(0, 20, 25, 16, -9.81, 70, 7, depth=20, brick_size=(1, 1, 1))
        bricks = scene["items"][1:]
        self.assertEqual(len(bricks), 10000)
        self.assertEqual(scene["items"][0]["physics_body"]["type"], "static")
        self.assertTrue(all(item["asset"] == "box" and item["physics_body"]["type"] == "dynamic"
                            for item in bricks))
        self.assertEqual(len({item["name"] for item in bricks}), 10000)
        self.assertEqual(len({tuple(item["position"]) for item in bricks}), 10000)
        self.assertEqual(scene["physics"]["solver"], "pgs")

    def test_solver_presets_and_overrides(self):
        for solver, expected in [("pgs", (4, 12)), ("tgs", (8, 4))]:
            scene = demo.build(0, 1, 1, 16, -9.81, 70, 7, solver=solver)
            physics = scene["physics"]
            self.assertEqual(physics["solver"], solver)
            self.assertEqual((physics["substeps"], physics["iterations"]), expected)
            scene = demo.build(0, 1, 1, 16, -9.81, 70, 7, solver=solver, substeps=16, iterations=3)
            self.assertEqual((scene["physics"]["substeps"], scene["physics"]["iterations"]), (16, 3))

    def test_cube_has_unique_nonoverlapping_individual_colliders(self):
        scene = demo.build(0, 10, 10, 16, -9.81, 70, 7, depth=10, brick_size=(1, 1, 1), sleep=False)
        bricks = scene["items"][1:]
        self.assertEqual(len(bricks), 1000)
        self.assertEqual(len({b["name"] for b in bricks}), 1000)
        self.assertEqual(len({tuple(b["position"]) for b in bricks}), 1000)
        for axis in range(3):
            positions = sorted({b["position"][axis] for b in bricks})
            self.assertEqual(len(positions), 10)
            self.assertTrue(all(b-a > 1 for a, b in zip(positions, positions[1:])))
        self.assertTrue(all(b["collider"]["shape"]["half_extents"] == [0.5]*3 for b in bricks))
        self.assertFalse(scene["physics"]["sleep"]["enabled"])
        self.assertGreater(scene["camera"]["target"][2], 0)

    def test_warm_start_and_iteration_controls_are_independent(self):
        scene = demo.build(0, 3, 6, 16, -9.81, 70, 7, sleep=False, warm_start=False, iterations=4)
        self.assertFalse(scene["physics"]["warm_start"])
        self.assertFalse(scene["physics"]["sleep"]["enabled"])
        self.assertEqual(scene["physics"]["iterations"], 4)

    def test_seeded_scene_has_independent_bodies_and_configurable_physics(self):
        scene = demo.build(3, 4, 2, 12, -4, 120, 7)
        self.assertEqual(scene, demo.build(3, 4, 2, 12, -4, 120, 7))
        self.assertEqual(len(scene["items"]), 12)
        self.assertEqual(scene["physics"]["gravity"], [0, 0, -4])
        self.assertEqual(scene["items"][0]["physics_body"], {"type": "static"})
        for item in scene["items"][-3:]:
            self.assertEqual(item["physics_body"]["mass"], 120)
            self.assertEqual(item["physics_body"]["velocity"][1], 12)
            self.assertIn("collider", item)
        self.assertEqual(len({i["name"] for i in scene["items"]}), 12)

    def test_custom_output_resolves_asset_and_invalid_arguments_do_not_write(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "scene.json"
            with patch("sys.argv", ["demo", "--output", str(output)]), contextlib.redirect_stdout(io.StringIO()):
                demo.main()
            scene = json.loads(output.read_text())
            self.assertTrue((output.parent / scene["assets"][1]["file"]).is_file())
            saved = output.read_bytes()
            with patch("sys.argv", ["demo", "--output", str(output), "--columns", "100", "--rows", "100", "--depth", "100"]), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as stopped:
                    demo.main()
            self.assertEqual(stopped.exception.code, 2)
            self.assertEqual(output.read_bytes(), saved)
            for option, value in [("--count", "-1"), ("--mass", "nan"), ("--gravity", "inf"), ("--depth", "0"), ("--iterations", "0"), ("--substeps", "33"), ("--solver", "automatic")]:
                with patch("sys.argv", ["demo", "--output", str(output), option, value]), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as stopped:
                        demo.main()
                self.assertEqual(stopped.exception.code, 2)
                self.assertEqual(output.read_bytes(), saved)


if __name__ == "__main__":
    unittest.main()
