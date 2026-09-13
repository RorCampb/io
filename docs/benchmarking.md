# Benchmarking

From the project root, watch a small animated crowd:

```sh
python3 tools/benchmark_engine.py --cases animated --counts 25 100 250 --watch
```

Each run previews the scene for three seconds, measures it uncapped, then holds
the final scene until you press Enter. Mouse controls work during preview and
inspection. Press Esc or close the window to stop the entire sweep. Preview uses
a separate world, so inspecting it cannot change the measured simulation. The
measurement starts from the configured scene and runs its warm-up normally.
The time waiting for Enter is not included in results. Native-process timeouts
are disabled in watch mode so inspection is not cut short.

For unattended measurements:

```sh
python3 tools/benchmark_engine.py --cases static animated moving camera
```

The script builds release, generates scene JSON, and starts a fresh native process
for each repeat. Defaults are 120 warm-up frames, 300 measured frames, three repeats,
and a 60 FPS target. Simulation advances by 1/60 second per rendered frame, with
the engine's existing fixed simulation tick. Vsync and the normal idle delay are
disabled during measurement. Esc cancels measurement. Resizing or minimizing the
window invalidates the run. Native GUI access is required.

Override the load ladder to explore a specific boundary:

```sh
python3 tools/benchmark_engine.py --cases animated moving --counts 100 250 500 1000 2000 --frames 600
```

The static case measures repeated boxes with a cached frame packet. Animated
crowds share runner geometry but evaluate independent animation players. Moving
crowds also follow short routes, exercising spatial updates. The camera case
orbits a cropped view of boxes to exercise changing visibility and frame packets.
Static and crowd views zoom out to fit the requested population; these tests
do not maintain a constant model size on screen. `--spacing` sets the distance
between items (default 3 world units). Camera-fit errors are checked before building
or allocating the scene and suggest a smaller spacing. Reducing spacing can cause
models and routes to overlap, so record it when comparing workloads. A layout
limit is not evidence of an engine capacity limit. A larger count therefore does
not represent the same workload as detailed characters viewed close up.

The script stops increasing a case when every repeat exceeds the target budget,
or when the intended full population is not visible. Passing requires throughput
at or above target and both p95 frame time and p95 GPU time within the corresponding
budget. `--continue-past-budget` disables the budget stop. Exhausting the configured
ladder means only that no larger load was tested, not that the engine has no limit.
Use `--counts` to refine the interval between passing and failing loads.

## Measurements

Each run writes raw samples and summaries under a new `build/benchmarks/<timestamp>`
directory. `summary.json` records repeats, scene and executable hashes, commit,
dirty worktree status, platform, resolution, driver, and stopping reasons. Native
stderr goes to a corresponding `.log`. A completed report is aggregated into the
summary even if you press Esc during the subsequent inspection. Failed runs print
the log tail in the terminal; resizing/minimizing invalidates timing rather than
being classified as a performance limit.

Times are milliseconds; summaries contain mean, p50, p95, p99, and maximum:

| Measurement | Includes |
| --- | --- |
| `update` | Rust world simulation and scripted camera action |
| `prepare` | Rust visibility, pose sampling, and frame-packet construction |
| `submit` | C buffer uploads and OpenGL draw submission, including driver waits |
| `frame` | Update, preparation, submission, and swap on the CPU |
| `gpu` | GPU elapsed query around renderer upload/draw commands |

GPU results are collected after the measured sequence, with no per-frame forced
GPU finish. Warm-up work is drained once before timing. Throughput includes final
GPU completion and result collection. CPU and GPU work overlap, so their times
must not be added together. Reported uncapped throughput is not a promise of
displayed FPS; compositor and driver behavior can still affect it.

Samples also record visible items, spatial candidates, joint matrices, and upload
bytes. Reports include dynamic buffer capacity, capacity growths during measurement,
and peak process RSS (macOS bytes). Buffer capacity is logical storage, not measured
physical GPU memory. RSS includes startup and warm-up; watch mode's preview can
raise its peak. Compare memory results from unattended runs.

Repeat comparisons on the same machine, power conditions, framebuffer resolution,
MSAA, scene, warm-up length, and build profile. Keep other heavy workloads closed.
Use the same watch setting for comparisons. Investigate consistent regressions
across repeats rather than treating a single noisy frame as a capacity limit.

The native measurement mode also accepts existing scenes directly:

```sh
make release
./build/release/io --scene assets/street-kit/demo.json --benchmark-out build/street-benchmark.json --benchmark-frames 600 --warmup 120 --benchmark-watch
```

Benchmark mode sets render/simulation distance to 20,000 units. The scene camera
still controls viewport culling. This stresses more world activity than the normal
default distance. Remove `--benchmark-watch` for automated measurement.
