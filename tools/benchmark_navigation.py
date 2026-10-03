#!/usr/bin/env python3
"""Sequential, paced navigation measurements. Build first; do not compile during runs."""
import argparse
import hashlib
import json
import math
import platform
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def percentile(values, p):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * p) - 1)] if values else 0


def activity_summary(data):
    """Sampled simulation-time occupancy, not intent or rendered FPS."""
    states = {}
    for actor in data["actors"]:
        for name, durations in actor["activity"]["seconds_by_status"].items():
            state = states.setdefault(name, {"moving_seconds": 0, "stationary_seconds": 0})
            for key, seconds in durations.items():
                state[key] += seconds
    total = sum(sum(s.values()) for s in states.values())
    return {
        "moving_fraction": sum(s["moving_seconds"] for s in states.values()) / total if total else 0,
        "actor_seconds_by_status": states,
        "end_stationary_over_5s": sum(a["activity"]["current_stationary_seconds"] > 5 for a in data["actors"]),
        "longest_stationary_seconds": max((a["activity"]["longest_stationary_seconds"] for a in data["actors"]), default=0),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--probe", default="target/release/io-worker-probe")
    parser.add_argument("--native", default="build/release/io")
    parser.add_argument("--frames", type=int, default=600)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--gpu", action="store_true")
    parser.add_argument("--inputs", type=Path, help="Replay saved *-input.json scenes (paths based at assets/dungeon)")
    parser.add_argument("--scenes", nargs="+", default=["guards", "athletics", "cat-mouse"])
    args = parser.parse_args()
    if not 1 <= args.frames <= (10000 if args.gpu else 36000) or args.repeats < 1:
        parser.error("frames outside supported range or repeats less than one")
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = {"platform": platform.platform(), "frames": args.frames, "repeats": args.repeats,
                "mode": "gpu-unpaced" if args.gpu else "worker-paced-60hz", "inputs": {}, "runs": []}
    executable = ROOT / (args.native if args.gpu else args.probe)
    manifest["binary_sha256"] = hashlib.sha256(executable.read_bytes()).hexdigest()
    for scene in args.scenes:
        path = ROOT / "assets/dungeon" / f"{scene}.json"
        source = args.inputs / f"{scene}-input.json" if args.inputs else path
        scene_data = json.loads(source.read_text())
        navigation = (scene_data.get("traversal") or {}).get("navigation") or {}
        if args.gpu and navigation.get("planning") == "background":
            print(f"WARNING: {scene}: unpaced background planning may finish while NPCs are still waiting for routes; "
                  "do not treat FPS as an equal-work comparison. Use the paced CPU probe.", flush=True)
        manifest["inputs"][scene] = hashlib.sha256(source.read_bytes()).hexdigest()
        (args.output / f"{scene}-input.json").write_bytes(source.read_bytes())
        if args.inputs:
            data = scene_data
            # The benchmark scenes use builtin assets plus scene-relative packages.
            for package in data.get("packages", []):
                package["file"] = str((path.parent / package["file"]).resolve())
            path = (args.output / f"{scene}-replay.json").resolve()
            path.write_text(json.dumps(data) + "\n")
        for repeat in range(args.repeats):
            output = args.output / f"{scene}-{repeat + 1}.json"
            command = [str(executable)]
            if args.gpu:
                command += ["--scene", str(path), "--benchmark-out", str(output.resolve()),
                            "--benchmark-frames", str(args.frames), "--warmup", "0"]
            else:
                command += [str(path), str(args.frames)]
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=180)
            output.with_suffix(".log").write_text(result.stderr)
            result.check_returncode()
            if not args.gpu:
                output.write_text(result.stdout)
            data = json.loads(output.read_text())
            if args.gpu:
                summary = {k: data["timings_ms"][k] for k in ("update", "frame", "gpu")}
            else:
                samples = [s["simulation_ms"] for s in data["observed_ticks"] if s["tick"]]
                summary = {"sim_p50": percentile(samples, .5), "sim_p95": percentile(samples, .95),
                           "sim_p99": percentile(samples, .99), "sim_max": max(samples, default=0),
                           "ticks": data["completed_tick"], "overruns": data["overruns"],
                           "max_snapshot_age_ms": data["max_snapshot_age_ms"],
                           "prepare_p95_ms": data["prepare_p95_ms"], "actors": data["actors"],
                                   "planning": data.get("planning")}
                summary["discovery"] = data.get("discovery")
                summary["trajectory"] = data.get("trajectory")
                if "movers" in data:
                    distances = [a["travel"]["distance"] for a in data["actors"]]
                    summary.update(npcs_moved=sum(d > 1 for d in distances),
                                   npc_distance_total=sum(distances),
                                   npc_distance_p50=percentile(distances, .5),
                                   npc_distance_min=min(distances, default=0),
                                   movers=data["movers"], navigation_revisions=data["navigation_revisions"],
                                   peak_pending=max((s["pending_jobs"] for s in data["observed_ticks"]), default=0))
                if data["actors"] and "activity" in data["actors"][0]:
                    summary.update(activity_summary(data))
            manifest["runs"].append({"scene": scene, "repeat": repeat + 1, "summary": summary})
            (args.output / "summary.json").write_text(json.dumps(manifest, indent=2) + "\n")
            print(scene, repeat + 1, json.dumps({k: v for k, v in summary.items() if k not in ("actors", "movers")}), flush=True)


if __name__ == "__main__":
    main()
