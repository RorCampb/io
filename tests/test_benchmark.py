import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from tools import benchmark_engine as bench


class BenchmarkTests(unittest.TestCase):
    def test_oversized_layout_fails_before_build_or_scene_allocation(self):
        with patch("sys.argv", ["benchmark", "--cases", "animated", "--counts", "1000000"]), \
                patch.object(bench.subprocess, "run") as run, contextlib.redirect_stderr(io.StringIO()) as error:
            with self.assertRaises(SystemExit) as stopped:
                bench.main()
        self.assertEqual(stopped.exception.code, 2)
        run.assert_not_called()
        self.assertIn("--spacing", error.getvalue())
        self.assertNotIn("Traceback", error.getvalue())
        self.assertGreaterEqual(bench.view_zoom("animated", 1000000, 1.5), 0.025)

    def test_completed_report_survives_escape_during_inspection(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "results"

            def native(command, **kwargs):
                if command[0] == "make":
                    return
                path = Path(command[command.index("--benchmark-out") + 1])
                timings = {name: {"p95": 1., "mean": 1.} for name in
                           ("frame", "gpu", "update", "prepare", "submit")}
                path.write_text(json.dumps({
                    "samples": [{"visible": 1, "upload_bytes": 992}], "timings_ms": timings,
                    "throughput_fps": 100., "peak_process_rss_bytes": 1000,
                    "dynamic_capacity_bytes": 1000, "measured_capacity_growths": 0,
                    "drawable_size": [2560, 1600], "renderer": "test", "gl_version": "4.1", "msaa": 4,
                }))
                raise subprocess.CalledProcessError(130, command)

            with patch("sys.argv", ["benchmark", "--cases", "animated", "--counts", "1", "--watch",
                                    "--output", str(output)]), \
                    patch.object(bench.subprocess, "run", side_effect=native), \
                    patch.object(bench, "git", return_value="test"), \
                    patch.object(bench.platform, "platform", return_value="test"), \
                    patch.object(bench, "sha256", return_value="test"), contextlib.redirect_stdout(io.StringIO()):
                bench.main()
            report = json.loads((output / "summary.json").read_text())
            self.assertEqual(len(report["results"]), 1)
            self.assertTrue(report["results"][0]["within_budget"])
            self.assertIn("Cancelled by viewer", report["stops"]["animated"]["reason"])


if __name__ == "__main__":
    unittest.main()
