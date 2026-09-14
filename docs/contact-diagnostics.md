# Contact Work Diagnostics

This measures repeated solver work. It does not skip contacts, change precision,
reduce iterations, alter sleeping, or implement an adaptive scheduler.

## Run

From the repository root, with `build/` present:

```sh
cargo run --release --offline --bin io-physics-bench -- assets/physics/cube.json 90 --contact-diagnostics > build/contact-work-cube.json
```

The trailing flag is optional. Without it the benchmark uses the normal solver,
does not allocate the diagnostic buffers, and omits `contact_diagnostics` from
JSON. The existing `benchmark_physics` API remains uninstrumented. Native scene
rendering is unchanged; this CLI is headless.

The Rust interface is `World::set_contact_diagnostics(true)` and
`World::contact_diagnostics()`. Enabling resets the report; disabling releases
it. Reports aggregate substeps within the latest attempted physics tick, and can
be partial on a solver error. The benchmark rejects failed simulations rather
than presenting partial measurements as successful runs.

## Meaning of Small

For one complete normal-plus-friction contact visit, the observer measures each
body's change in linear and angular velocity. Its metric is:

```text
max(length(delta_velocity_A) + radius_A * length(delta_angular_velocity_A),
    length(delta_velocity_B) + radius_B * length(delta_angular_velocity_B))
```

The radius encloses the collider around its center of mass: sphere radius or
box half-diagonal. This bounds the induced velocity change at any point on the
collider, in world units/second. It accounts for mass and rotational response,
unlike an absolute impulse threshold. It is not positional error, kinetic energy,
an exact complementarity residual, or a bound on future accumulated drift.

The reporting threshold is `0.001` units/second; it never controls the solver.
Histograms have six bins: exactly zero, `(0, 1e-5]`, `(1e-5, 1e-4]`,
`(1e-4, 1e-3]`, `(1e-3, 1e-2]`, and `>1e-2`.

- `visits` counts actual solver contact visits for that iteration across all
  substeps/ticks. The same contact is counted again on the next iteration.
- `correction_histogram` measures changes when each contact actually runs.
- `end_pass_histogram` measures another hypothetical visit from the pass's final
  state, using cloned bodies and impulses. No replay result enters the world.
- `small_but_unsettled` counts visits that were small but whose end-pass replay
  exceeds the threshold. Later contacts have made them significant again.
- `small_then_large` counts a small visit followed by a large visit on the next
  actual iteration of the same substep. It is zero on iteration one.
- `supporting_small` counts small visits with positive accumulated normal
  impulse. Small correction does not mean the contact carries no support.
- `invalid_samples` counts nonfinite actual/replay measurements; these are not
  silently categorized as small. Each histogram sums to visits when this is zero.

JSON includes per-iteration totals and separate 30-tick windows (one simulated
second at 30 Hz), with a partial final window if needed. Sleeping contacts do not
enter these denominators. Contact IDs are not tracked between substeps, and this
report does not establish a spatial cutoff or measure an impact's propagation
distance.

## Measurements

Development Mac, 2026-09-13, release, four substeps and 12 iterations:

| Workload | Ticks | Small visits on pass 12 | Small but large at pass end, as % of all visits |
| --- | --- | --- | --- |
| 4,096-brick cube + 16 projectiles | 90 | 31.66% | 12.71% |
| 384-brick structure + 1 projectile | 180 | 65.78% | 5.03% |
| 256 floor bricks, sleeping disabled | 120 | 100% | 0% |

In the cube, 40.16% of visits categorized as small are significant again at the
end of that same pass. Only 11.34% of pass-12 visits make exactly zero change.
At the looser `0.01` reporting threshold 89.40% are small, but that is not a safe
skipping threshold or a demonstrated speedup. Omitting small corrections can
accumulate error and change every subsequent contact decision.

The single-projectile structure has 79.90% small pass-12 visits in its final
30-tick window, versus 65.78% across startup and impact together. It ends with
347 awake and 38 sleeping dynamic bodies. This is a localized-input workload,
not proof that all remaining work is spatially confined to the impact region.

The floor is a positive control: repeated corrections become negligible. All
its small pass-12 visits still carry positive normal support impulses. Sleeping
was deliberately disabled; the engine already eliminates settled work through
sleeping when enabled.

The cube performed 95,043,480 real contact visits across all 12 passes. The
observer additionally replays each contact, so its timings are deliberately
inflated: 247.60 ms/tick observed versus 150.82 ms/tick unobserved in these runs.
All reported non-timing cube metrics match between the two runs, and regression
tests check exact body states and impulses with observation on/off. No invalid
samples occurred. Do not compare instrumented time to ordinary gameplay FPS.

Reproduce the two smaller inputs without changing engine code:

```sh
python3 tools/build_physics_demo.py --columns 24 --rows 4 --depth 4 --brick-size 1 1 1 --count 1 --output build/contact-localized.json
target/release/io-physics-bench build/contact-localized.json 180 --contact-diagnostics > build/contact-work-localized.json
python3 tools/build_physics_demo.py --columns 16 --rows 1 --depth 16 --brick-size 1 1 1 --count 0 --no-sleep --output build/contact-quiet.json
target/release/io-physics-bench build/contact-quiet.json 120 --contact-diagnostics > build/contact-work-quiet.json
```

These results justify investigating error-driven work scheduling with neighbor
invalidation, not removing contacts after one small update. The diagnostic replay
itself is too expensive to use as that scheduler. Any implementation needs a
cheap dirty-contact mechanism, periodic/global validation, and measured drift,
penetration, missed-support and runtime comparisons against the full solver.
