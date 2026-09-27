# Navigation benchmarks

Build before measuring; do not compile or run other benchmarks concurrently.

```sh
cargo build --release --bin io-worker-probe
make PROFILE=release
python3 tools/benchmark_navigation.py build/navigation-results/cpu
python3 tools/benchmark_navigation.py build/navigation-results/gpu --gpu
```

Each command runs guards, athletics and cat-mouse sequentially, three repeats of
600 frames each. Use a fresh output directory. Guards and athletics enable background
planning; cat-mouse is an unchanged inline control. `--scenes`, `--frames` and
`--repeats` select other run lengths. The GPU command opens native OpenGL windows.

## Meaning of the metrics

- **Paced CPU probe:** samples published simulation snapshots and prepares rendering
  at 60 Hz for about ten wall-clock seconds. Reports unique observed tick p50/p95/p99/
  maximum, completed ticks, overruns, snapshot age, frame preparation, final actor
  positions/statuses and planning counters. Tick distributions are sampled, not an
  exhaustive trace: publication can skip ticks. Completed ticks and overrun counters
  are authoritative accumulated worker values.
- **Native GPU benchmark:** unpaced fixed-step simulation and OpenGL draw timing,
  including every benchmark frame's update/frame/GPU distribution. It is useful for
  spike isolation but **not** an equal-work throughput comparison after offloading:
  a fast unpaced main loop can advance simulated time before background routes finish.
  Use paced progress and the behavioral regression alongside it.
- **Behavioral verification:** `cargo test --release background_guards` verifies the
  real background service traverses stairs, jumps the gap, takes the alternate bridge
  for the non-jumper, and reaches destinations without collider penetration.

Reports retain raw JSON/stderr, scene input copies and executable/scene SHA256 hashes.
The initial preserved binaries are `build/navigation-before/io-worker-probe` and
`build/navigation-before/io`. Initial reports are under `build/navigation-before/`;
post-change reports are under `build/navigation-after/`. These generated artifacts
are local, not committed assets. Scene dependencies still refer to the local asset
packages; this is not a portable archive of the meshes.

Replay a saved baseline with its original binary (the harness rebases package paths):

```sh
python3 tools/benchmark_navigation.py build/navigation-replay \
  --probe build/navigation-before/io-worker-probe \
  --inputs build/navigation-before/cpu
```

GPU settings for this comparison: native 1280x800 window, Retina framebuffer,
4x MSAA, same models/camera/scene geometry, no benchmark warmup. Keeping startup
planning inside the measurement is intentional. The only scene change for guards
and athletics is the planning scheduling setting. The result is a latency isolation
measurement, not evidence that search became cheaper or that large crowds scale.

## 2026-09-27 Results

Paced release probe, 600 frames/run, three repeats. Entries are the median of
per-run statistics, not pooled percentiles. Times are milliseconds.

| Scene / scheduling | Tick p95 | Tick p99 | Tick max | Completed ticks | Overruns |
| --- | ---: | ---: | ---: | ---: | ---: |
| Guards / before inline | 3.359 | 36.578 | 56.180 | 499 | 66 |
| Guards / background | 0.602 | 0.791 | 0.965 | 598 | 0 |
| Athletics / before inline | 20.211 | 23.099 | 25.560 | 585 | 57 |
| Athletics / background | 0.301 | 0.540 | 0.612 | 598 | 0 |
| Cat-mouse / before inline | 1.236 | 1.554 | 28.766 | 596 | 4 |
| Cat-mouse / after inline control | 1.172 | 1.340 | 4.842 | 596 | 4 |

All six background runs had zero overruns. Guards accepted both initial plans and
both actors advanced beyond their baseline ten-second positions; athletics accepted
its plan and traversed the course. Median first observed Following time decreased
from 2.272 to 1.570 seconds for guards (first of two actors), and from 2.005 to
1.470 seconds for athletics. The longest initial guard plan still takes about
2.45-2.50 seconds; athletics takes 1.42-1.44 seconds. This remains noticeable initial
AI latency, despite the smooth simulation tick.

Frame preparation p95 stayed roughly unchanged: guards 0.345 -> 0.360 ms,
athletics 0.273 -> 0.273 ms. The unmodified cat-mouse control still has four
accumulated overruns per median run; do not interpret its sampled maximum variation
as an optimization. This change targets search stalls in the enabled scenes.

An additional 30-second guards run (`--frames 1800 --repeats 1`) includes later
pursuit and barrier cycles. Before: 1,708 ticks, 71 overruns, 57.691 ms sampled
maximum. After: 1,798 ticks, zero overruns, 0.974 ms maximum. Both guards ended Idle
at the same positions, approximately (25, 3.2, 0) and (26, 2.733, 0).

The longer background run also exposes a remaining inefficiency: 56 submitted/
completed jobs, three accepted, 52 rejected and one cancelled, no pending work.
Moving-start rejection preserves safety but can repeatedly spend planning work
while the old route continues. This is not solved by a worker queue. Safe generic
route rebasing or coalesced retries is a follow-up, alongside athletics feasibility
caching. Results are in `build/navigation-{before,after}/extended/`.

**Native after-results are not an FPS improvement claim.** The 600-frame background
guards/athletics runs finish in roughly one second, before their initial searches
finish. The renderer still draws every frame, but the NPC workload is predominantly
waiting for routes rather than traversing the course. Its smaller GPU/update timings are
therefore not comparable to the moving inline baseline. Raw results are retained
for diagnosis, but excluded from the performance conclusion above; the runner warns
about this combination. A future paced native benchmark should measure real gameplay
presentation, rather than advancing ten simulated seconds in one wall-clock second.

Validation: full `make test`, Clippy with warnings denied, formatting checks and all
eight native OpenGL regression suites passed. The real background course test passed
in debug and release. No renderer/FFI buffer change or new crate was required.

## Route Refinement Pipeline Comparison

Fresh baselines recorded before implementation on 2026-09-27: 128-NPC moving-load
and stationary-load districts, three sequential 60-second runs each. The final
pipeline repeats the same six runs, replaying the saved inputs. Scene hashes match;
no asset, speed, goal, population, tick-rate or collision setting was changed.
No builds or other benchmark runs were started concurrently with these measurements.

The original binary is retained as `build/refinement-before-probe`; the final binary
is `build/refinement-after-probe`. Raw reports, scene copies and SHA-256 manifests
are in `build/refinement-before/` and `build/refinement-after/`. Commands:

```sh
python3 tools/benchmark_navigation.py build/refinement-before \
  --probe build/refinement-before-probe --frames 3600 --repeats 3 \
  --scenes stress-128 stress-static
python3 tools/benchmark_navigation.py build/refinement-after \
  --inputs build/refinement-before --frames 3600 --repeats 3 \
  --scenes stress-128 stress-static
```

Medians of per-run statistics (milliseconds, not pooled percentiles):

| Workload | Tick p50 | Tick p95 | Tick p99 | Completed ticks | Overruns |
| --- | ---: | ---: | ---: | ---: | ---: |
| Moving loads, before | 15.788 | 20.292 | 21.494 | 3424 | 1419 |
| Moving loads, refined | 4.190 | 9.214 | 11.752 | 3595 | 16 |
| Stationary loads, before | 17.340 | 21.324 | 22.376 | 3323 | 1923 |
| Stationary loads, refined | 4.257 | 10.230 | 12.079 | 3595 | 16 |

P95 falls 54.6% and 52.0%, respectively. The simulation remains configured at 60 Hz.
This is a CPU simulation measurement, not a GPU/render-FPS claim. Rare overruns
remain: final per-run maxima are approximately 20-27 ms. The median maximum did
not improve materially. One baseline moving-load run had a 270 ms outlier; it is
retained in the raw data and not used to inflate a headline speedup.

Progress is checked alongside timing. All 128 NPCs moved over one metre in every
baseline and final run. Median total travel is 15,873 -> 15,546 m with moving loads
(-2.1%), and 17,281 -> 17,168 m with stationary loads (-0.7%). Moving actor-time
increases from 73.3% -> 74.9% and 82.1% -> 83.2%. Every final run ends with zero
actors stationary for more than five seconds. Trajectories differ because refinement
and real-time simulation progress change arrival/traffic timings; this is an
equal-input comparison, not an identical sequence of physical interactions.

The first trial (`build/refinement-trial/`) was not accepted as the final result:
it lowered tick time but added too much waypoint braking and reduced travel.
The final refinement phase validates and merges straight runs, and a new regression
requires sustained straight-line travel at 30/60/144 Hz. Intermediate trial directories
are retained for diagnosis, not substituted for the six final runs above.

The new `refinement_queries` counter records only about 1,100-1,200 completed-job
preparation checks in each final moving-load run, separate from roughly 347,000-
354,000 live handoff/corner/steering checks. A deterministic stage test confirms
that repeated following of an unchanged prepared route performs no new refinement.
This is shared traversal infrastructure, not a stress-scene fast path.

Residual costs/limits: CPU frame-preparation p95 increased from roughly 0.37 ms
to 0.88 ms (moving) / 0.95 ms (stationary), still below 1 ms. This was not separately
profiled. Initial route admission can still take about nine seconds for the last
actors; background queue/caches are not unlimited. Immediate steering and motor
checks remain separate; unchanged-height mutation and repeated spatial candidate
collection were deliberately not optimized in this change. Tight-corner braking,
general crowd deadlocks, and spline/turn-radius motion are not solved by refinement.

Verification: 365 Rust tests including doctests passed, with three existing ignored
tests. Clippy with warnings denied, formatting/diff checks, and all eight native
OpenGL regression suites passed. Tests cover worker-only/cancellable refinement,
atomic commit, no repeated refinement queries, bounded candidate work, stale
profile/revision handoffs, action boundaries, body-safe diagonals, ramps, stairs,
jumps, live/unobserved walls and preserved execution during replacement work.
The unobserved-wall test still checks nonpenetration and eventual arrival; its
time allowance is now eight seconds because detection occurs at immediate motion
range rather than a repeated distant look-ahead scan.

The native release executable is rebuilt. To inspect the unchanged scene:

```sh
./build/release/io --scene assets/dungeon/stress-128.json
```

## Dynamic Observation Discovery (2026-09-27)

After the user raised this scene to 144 Hz, two sequential thirty-second runs
were recorded before discovery, and two after the final plugin changes. No
compilation or other benchmark ran concurrently. Both use the same 128-NPC
geometry, goals, movement speeds and 144 Hz target; the only scene difference is
the new `traversal.obstacle_observations` block. These CPU probes poll at 60 Hz;
tick-time percentiles are sampled snapshots, not a complete 144 Hz trace or GPU FPS.

| Metric | Discovery off, before (two runs) | Discovery on, final (two runs) |
| --- | --- | --- |
| Simulation p50 (ms) | 4.021 / 4.038 | 4.275 / 4.285 |
| Simulation p95 (ms) | 9.319 / 8.716 | 6.764 / 6.585 |
| Simulation p99 (ms) | 12.457 / 12.746 | 7.664 / 7.914 |
| Completed ticks | 4188 / 4204 | 4292 / 4290 |
| Overruns | 313 / 270 | 193 / 186 |
| NPC distance total (m) | 6970 / 7130 | 6758 / 6769 |
| Moving actor-time | 69.8% / 70.7% | 66.1% / 65.8% |
| Actors moving over 1m | 128 / 128 | 128 / 128 |
| End stationary over 5s | 0 / 0 | 0 / 0 |
| Route jobs submitted | 280 / 280 | 369 / 361 |

This is a feature-cost comparison, **not an equal-work speedup claim**. Median
tick cost increased about 0.25 ms (6%). Total sampled travel decreased about 4%,
and more actor-time was spent planning. The extra observation-triggered route
work changes the workload and explains why lower tail tick times alone should
not be called an optimization. Initial queue latency still reached roughly ten
seconds; occasional ticks exceeded the 6.94 ms budget (maximum 24.61 / 15.75 ms).

Final discovery counters show about 34,300 observer scans, 159,000 track samples,
568 / 576 retained tracks and 199 / 145 reconsideration submissions. Qualified
notices numbered 22,423 / 21,951, of which 21,594 / 21,229 described unchanged
noticed geometry after resynchronization and were suppressed for routing. The
bounded journal can overflow between sparse samples in this busy scene; the
plugin resamples evidence rather than assuming an unseen object did not change.

Raw inputs, hashes and runs are in `build/discovery-before` and
`build/discovery-final`. The baseline executable is the identical hashed
`build/refinement-after-probe`; the final executable is `build/discovery-final-probe`.
Intermediate runs in `build/discovery-trial` and `build/discovery-after` exposed
repeated resynchronization replans and premature initial-search reconsideration;
they are retained, not reported as the finished result.

Verification: 373 Rust tests including doctests pass (three existing ignored),
strict workspace Clippy passes, six stress-generator Python tests pass, and
format/diff checks pass. The native release executable is rebuilt. This change
does not alter OpenGL code; native GPU regression suites were not rerun here.
Dynamic discovery and route reconsideration are enabled in `stress-128.json`;
other existing saved stress scenes retain their earlier settings. Short-term
attention is not knowledge-restricted path search, and this does not solve
acceleration-aware corner smoothing or general crowd traffic.
