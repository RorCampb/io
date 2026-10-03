# NPC Performance Audit: 2026-10-02

## Method

Three sequential 60-second release CPU benchmarks of the saved 384-NPC scene,
followed by a separate 60-second run sampled with macOS `sample` during seconds
20-40 (1 ms requested interval). No other engine process, builds or tests ran
during these measurements. Normal desktop background processes were not disabled.
No engine code, scene settings, collision fidelity or NPC counts were changed.

The executable and source scene hashes exactly match the post-height-fix artifacts:

- Binary: `cf90733f9d3914887b07f26df5839ebc42a66677b6c9d84e22c12644582e6e57`
- Scene: `c0f27a6436eb36653b572720a8d5a5328b8c18da21f1f21b9712487039957a84`

Raw runs, input, summary, executable and tracked-source diff are in
`build/npc-audit-20261002/`. Profile stacks and its separate probe output are in
`build/npc-profile-20261002/`. Timings below exclude the profiled run.

```sh
cargo build --release --bin io-worker-probe
python3 tools/benchmark_navigation.py build/npc-audit-20261002 \
  --inputs build/height-after --frames 3600 --repeats 3 --scenes stress-384
bash tools/profile_simulation.sh assets/dungeon/stress-384.json build/npc-profile-20261002
```

Use new output directory names to repeat. The CPU probe polls at 60 Hz, records
observed unique ticks, and prepares render frames without OpenGL. It does not
measure GPU/display FPS or capture every simulation tick. SIM Hz below is actual
completed ticks divided by wall time, not the configured 144 Hz.

## Results

| Metric | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Actual SIM Hz | 78.23 | 76.50 | 76.96 |
| Tick p50 / p95 / p99 (ms) | 12.79 / 14.24 / 14.97 | 13.15 / 14.75 / 15.47 | 12.96 / 14.72 / 15.64 |
| Completed ticks / overruns | 4694 / 4694 | 4590 / 4590 | 4618 / 4618 |
| Simulated seconds in 60 wall seconds | 32.60 | 31.88 | 32.07 |
| Snapshot copy p95 (ms) | 0.361 | 0.349 | 0.347 |
| CPU frame preparation p95 (ms) | 0.833 | 0.828 | 0.832 |
| NPCs moving over 1 metre | 312 | 318 | 316 |
| Total travel (m) | 11296 | 12583 | 12300 |
| Moving actor-time | 31.0% | 35.6% | 34.6% |
| Planning actor-time | 68.3% | 63.9% | 64.9% |
| End stationary over 5 simulated seconds | 195 | 156 | 177 |
| Submitted / completed / accepted route jobs | 517 / 485 / 451 | 527 / 495 / 484 | 504 / 472 / 461 |
| Maximum completed job latency (s) | 9.14 | 8.36 | 10.23 |
| Samples with all 32 job slots occupied | 77.8% | 78.0% | 80.2% |

All ticks overrun the 6.94 ms target. Existing fixed-step overrun policy explains
the slow-motion effect; smoother renderer refresh does not restore elapsed game
time. Most stationary actor-time is Planning, not Following. Queued job latency
is not the same as the full NPC wait: `Work.started` begins at submission and
includes time between worker slices, but excludes pre-admission waiting. The
current counters do not split admission, service CPU and refinement latency per
request, or provide an uncensored distribution for unfinished jobs.

## Profile Findings

Percentages are approximate shares of each thread's 16,689 stack samples,
including descendants. They are not additive across threads or overlapping
parent/child functions, and are not precise per-job CPU timers.

- Simulation: traversal service about 76%; within this, motor about 49%, immediate
  steering validation about 16%, and trajectory work about 8% of the whole thread.
  Motor's supported-walk pipeline alone accounts for about 37%, and its preceding
  support probe about 11%. Discovery/perception accounts for about 17%.
- Planning: neighbor connection generation about 63%, per-edge `live_clear`
  about 31%, route refinement about 2%, and waiting for work about 4%.
  Thus the worker is largely doing geometry work, not mostly blocked on its queue.
- In the `live_clear` branch, about 4,922 samples are in terrain/scenery support
  tracing versus 219 in the following actor-clearance stage. These are nested
  within the 31% branch, not additional costs.

The code explains the repeated work. `plugins/io-locomotion/src/surface.rs:156`
builds connections by walking the geometry for Walk and Crouch capabilities.
`crates/io-traversal/src/navigation/mod.rs:516` then calls `live_clear` on eligible
connections. The supplied provider implements it through another full supported
walk before checking actors (`surface.rs:414`, `pipeline.rs:86`). The existing
pipeline shares results between its own two stages, but not across connection
generation and that second call.

On the simulation side, `plugins/io-locomotion/src/motor.rs:297` probes support,
then calls the walking pipeline, whose `crates/io-world/src/support.rs:188`
probes starting support again. This also runs when the requested planar movement
is zero. Idle does not mean zero motor cost. Immediate steering checks are another
separate walk before the motor (`executor.rs:283`). These operations have different
world/filter/body semantics; removing checks without preserving those contracts
would be unsafe.

Reported navigation counters also show 3,245-7,053 invalidations and only roughly
7-19% edge-cache hits among reported hit/miss counts. The shared planner switches
between job snapshots; `Navigation::synchronize` clears the provider cache when
the revision differs. This is a cache-reuse concern to investigate, not proof of
the precise time lost, and not the same as attention-triggered route cancellation.
These counters are aggregated through completed results, not a complete worker
event trace.

## Recommended Next Step

Prioritize reusing proven support/connector geometry for actor checks, and then
sharing the current-step support result through steering/motor execution where
the world, body and requested path match. Preserve live collision safety and
explicit invalidation. Audit idle support reuse and snapshot cache identity next.
A larger queue alone does not reduce geometry cost. Before scheduling changes,
add separate admission/service/refinement timings rather than interpreting
`max_job_ms` as pure search compute. No optimization was implemented in this audit.
