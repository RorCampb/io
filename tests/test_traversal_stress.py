import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from build_traversal_stress import scene
from benchmark_navigation import activity_summary


class TraversalStress(unittest.TestCase):
    def test_route_guides_are_an_optional_presentation_setting(self):
        plain, traced = scene(), scene(debug_routes=True)
        self.assertFalse(plain["traversal"].pop("debug_routes"))
        self.assertTrue(traced["traversal"].pop("debug_routes"))
        self.assertEqual(plain, traced)

    def test_obstacle_discovery_is_enabled_without_all_pairs_bindings(self):
        traversal = scene()["traversal"]
        self.assertNotIn("observations", traversal)
        settings = traversal["obstacle_observations"]
        self.assertEqual(settings["observers_per_tick"], 8)
        self.assertEqual(settings["tracks_per_observer"], 6)
        self.assertGreater(settings["notice_attention"], 0)

    def test_simulation_rate_is_configurable_without_changing_workload(self):
        fast = scene()
        baseline = scene(tick_hz=60)
        self.assertEqual(fast.pop("simulation"), {"tick_hz": 144})
        self.assertEqual(baseline.pop("simulation"), {"tick_hz": 60})
        self.assertEqual(fast, baseline)
        for rate in (3, 1001, 144.5, True):
            with self.assertRaises(ValueError):
                scene(tick_hz=rate)

    def test_activity_summary_counts_motion_not_intent(self):
        result = activity_summary({"actors": [{"activity": {
            "seconds_by_status": {
                "Following": {"moving_seconds": 2, "stationary_seconds": 3},
                "Planning": {"moving_seconds": 0, "stationary_seconds": 5}},
            "current_stationary_seconds": 6,
            "longest_stationary_seconds": 8,
        }}]})
        self.assertEqual(result["moving_fraction"], .2)
        self.assertEqual(result["end_stationary_over_5s"], 1)
        self.assertEqual(result["longest_stationary_seconds"], 8)
        self.assertEqual(activity_summary({"actors": []})["moving_fraction"], 0)

    def test_reproducible_and_population_is_not_queue_capacity(self):
        for count in (16, 64, 128, 256):
            s = scene(count)
            self.assertEqual(s, scene(count))
            self.assertEqual(len(s["traversal"]["navigation"]["agents"]), count)
            self.assertGreater(len(s["items"]), 1300)
            names = [i["name"] for i in s["items"]]
            self.assertEqual(len(names), len(set(names)))
            spawns = [tuple(i["position"]) for i in s["items"] if i["name"].startswith("npc-")]
            self.assertEqual(len(spawns), len(set(spawns)))
            for agent in s["traversal"]["navigation"]["agents"]:
                self.assertNotIn(tuple(agent["goal"]), spawns)
            self.assertEqual(s["dimensions"][:2], [400, 400])
            self.assertLessEqual(201**2, 65536)

    def test_controls_change_only_the_intended_workload(self):
        timed, continuous, static = (scene(64, mode) for mode in ("timed", "continuous", "static"))
        self.assertEqual(timed["items"], continuous["items"])
        self.assertEqual(timed["items"], static["items"])
        self.assertEqual(len(timed["traversal"]["barrier_cycles"]), 8)
        self.assertTrue(all(c["interval_seconds"] == 0 for c in continuous["traversal"]["barrier_cycles"]))
        self.assertEqual(static["traversal"]["barrier_cycles"], [])
        self.assertNotEqual(scene(64, long_routes=True)["traversal"]["navigation"],
                            timed["traversal"]["navigation"])

    def test_malformed_generator_inputs_rejected(self):
        for count in (0, 257):
            with self.assertRaises(ValueError):
                scene(count)
        with self.assertRaises(ValueError):
            scene(moving="typo")
