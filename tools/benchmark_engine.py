"""Repeatable native-engine load sweeps. Uses only Python's standard library.

python3 tools/benchmark_engine.py --cases animated moving --target-fps 60
"""
import argparse
import datetime
import hashlib
import json
import math
import platform
from pathlib import Path
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LADDERS = {
    "static": [1000, 10000, 50000, 100000],
    "animated": [25, 100, 250, 500, 1000, 2000, 5000],
    "moving": [25, 100, 250, 500, 1000, 2000, 5000],
    "camera": [1000, 10000, 50000, 100000],
}


def view_zoom(case, count, spacing):
    side = math.ceil(math.sqrt(count))
    span = side * spacing
    # Fit the entire static/crowd workload even at diagonal camera angles.
    zoom = min(16, 0.85 * min(40 / (span / math.sqrt(2) + 3),
                              25 / (span / math.sqrt(6) + 3)))
    if case == "camera":
        zoom = 2.2
    if zoom < 0.025:
        maximum_spacing = min((0.85 * 40 / 0.025 - 3) * math.sqrt(2),
                              (0.85 * 25 / 0.025 - 3) * math.sqrt(6)) / side
        raise ValueError(f"{case} with {count} items at spacing {spacing:g} exceeds the camera zoom range. "
                         f"Use --spacing below {maximum_spacing:.3f} or a smaller count. "
                         "This is a benchmark layout limit, not a measured engine capacity limit.")
    return zoom


def scene_for(case, count, spacing=3.0):
    side = math.ceil(math.sqrt(count))
    zoom = view_zoom(case, count, spacing)
    animated = case in ("animated", "moving")
    asset = {"name": "model", "file": str(ROOT / "assets/street-kit/runner.glb")} if animated else {
        "name": "model", "builtin": "box"}
    items = []
    for i in range(count):
        x = (i % side - (side - 1) / 2) * spacing
        y = (i // side - (side - 1) / 2) * spacing
        item = {"name": f"item-{i}", "asset": "model", "position": [x, y, 0]}
        if animated:
            item["animation"] = {"clip": "Run", "speed": 0.8 + (i % 17) / 40}
        if case == "moving":
            item["motion"] = {"route": [[x, y, 0], [x + 1, y, 0],
                                         [x + 1, y + 1, 0], [x, y + 1, 0]],
                              "speed": 1.2, "start_distance": (i % 17) / 17 * 4}
        items.append(item)
    return {"version": 1, "dimensions": [20000, 20000, 256], "origin": [10000, 10000, 0],
            "camera": {"target": [0, 0, 0.8], "zoom": zoom}, "assets": [asset], "items": items}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def summarize(result, case, count, target):
    samples = result["samples"]
    timings = result["timings_ms"]
    budget = 1000 / target
    visible_min = min(s["visible"] for s in samples)
    valid = case == "camera" or visible_min == count
    within_budget = result["throughput_fps"] >= target and max(
        timings["frame"]["p95"], timings["gpu"]["p95"]) <= budget
    return {"case": case, "items": count, "valid_workload": valid,
            "within_budget": valid and within_budget,
            "fps": result["throughput_fps"], "frame_p95_ms": timings["frame"]["p95"],
            "gpu_p95_ms": timings["gpu"]["p95"], "update_mean_ms": timings["update"]["mean"],
            "prepare_mean_ms": timings["prepare"]["mean"], "submit_mean_ms": timings["submit"]["mean"],
            "visible_min": visible_min, "visible_max": max(s["visible"] for s in samples),
            "upload_mean_bytes": statistics.mean(s["upload_bytes"] for s in samples),
            "peak_process_rss_bytes": result["peak_process_rss_bytes"],
            "dynamic_capacity_bytes": result["dynamic_capacity_bytes"],
            "measured_capacity_growths": result["measured_capacity_growths"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", nargs="+", choices=LADDERS, default=list(LADDERS))
    parser.add_argument("--counts", nargs="+", type=int, help="Override each case's load ladder")
    parser.add_argument("--spacing", type=float, default=3.0, help="World units between items; lower values fit denser populations")
    parser.add_argument("--frames", type=int, default=300)
    parser.add_argument("--warmup", type=int, default=120)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--target-fps", type=float, default=60)
    parser.add_argument("--timeout", type=float, default=120, help="Maximum seconds per native process")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--continue-past-budget", action="store_true")
    parser.add_argument("--watch", action="store_true", help="Preview each scene and wait for Enter afterward; Esc stops")
    args = parser.parse_args()
    if not (1 <= args.frames <= 10000 and 0 <= args.warmup <= 10000 and 1 <= args.repeats <= 20):
        parser.error("frames: 1..10000; warmup: 0..10000; repeats: 1..20")
    if not math.isfinite(args.target_fps) or args.target_fps <= 0 or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("target-fps and timeout must be positive and finite")
    if args.counts and any(n < 1 or n > 1_000_000 for n in args.counts):
        parser.error("counts must be between 1 and 1000000")
    if not math.isfinite(args.spacing) or args.spacing <= 0:
        parser.error("spacing must be positive and finite")
    try:
        for case in args.cases:
            for count in args.counts or LADDERS[case]:
                view_zoom(case, count, args.spacing)
    except ValueError as error:
        parser.error(str(error))
    subprocess.run(["make", "release"], cwd=ROOT, check=True)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    output = (args.output or ROOT / "build/benchmarks" / stamp).resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = ROOT / "build/release/io"
    manifest = {"schema": 1, "time_utc": stamp, "commit": git("rev-parse", "HEAD"),
                "worktree_status": git("status", "--short"), "machine": platform.platform(),
                "binary_sha256": sha256(binary), "runner_sha256": sha256(ROOT / "assets/street-kit/runner.glb"),
                "settings": {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
                "results": [], "stops": {}}

    def save():
        (output / "summary.json").write_text(json.dumps(manifest, indent=2) + "\n")

    save()
    print(f"Results: {output}", flush=True)
    for case in args.cases:
        for count in sorted(set(args.counts or LADDERS[case])):
            scene = output / f"{case}-{count}.json"
            scene.write_text(json.dumps(scene_for(case, count, args.spacing)) + "\n")
            runs = []
            failed = False
            for repeat in range(args.repeats):
                name = f"{case}-{count}-run{repeat + 1}"
                report = output / f"{name}.json"
                command = [str(binary), "--scene", str(scene), "--benchmark-out", str(report),
                           "--benchmark-frames", str(args.frames), "--warmup", str(args.warmup)]
                if case == "camera":
                    command.append("--benchmark-orbit")
                if args.watch:
                    command.append("--benchmark-watch")
                def collect_report():
                    result = json.loads(report.read_text())
                    row = summarize(result, case, count, args.target_fps)
                    row.update(report=report.name, scene_sha256=sha256(scene), repeat=repeat + 1,
                               drawable_size=result["drawable_size"], renderer=result["renderer"],
                               gl_version=result["gl_version"], msaa=result["msaa"])
                    runs.append(row)
                    manifest["results"].append(row)
                    print(f"{case:8} {count:7} run {repeat + 1}: {row['fps']:7.1f} fps | "
                          f"p95 frame {row['frame_p95_ms']:6.2f} ms GPU {row['gpu_p95_ms']:6.2f} ms | "
                          f"visible {row['visible_min']}..{row['visible_max']}", flush=True)
                    save()
                try:
                    with (output / f"{name}.log").open("w") as log:
                        subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                                       timeout=None if args.watch else args.timeout, check=True)
                    collect_report()
                except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
                    manifest["stops"][case] = {"items": count, "reason": str(error), "log": f"{name}.log"}
                    if isinstance(error, subprocess.CalledProcessError) and error.returncode == 130:
                        if report.exists():
                            collect_report()
                        manifest["stops"][case]["reason"] = "Cancelled by viewer; completed measurements are retained"
                        save()
                        print(f"Stopped by viewer. Results retained in {output}", flush=True)
                        return
                    failed = True
                    save()
                    tail = (output / f"{name}.log").read_text().splitlines()[-12:]
                    print(f"{case}: stopped; {name}.log:\n" + "\n".join(tail), flush=True)
                    break
            if failed:
                break
            if any(not r["valid_workload"] for r in runs):
                manifest["stops"][case] = {"items": count, "reason": "Not all requested objects remained visible"}
                save()
                break
            if all(not r["within_budget"] for r in runs) and not args.continue_past_budget:
                manifest["stops"][case] = {"items": count, "reason": "All repeats exceeded target budget"}
                save()
                break
        else:
            manifest["stops"][case] = {"reason": "Configured ladder exhausted; no higher load tested"}
            save()
    print(f"Saved {len(manifest['results'])} runs to {output / 'summary.json'}", flush=True)


if __name__ == "__main__":
    main()
