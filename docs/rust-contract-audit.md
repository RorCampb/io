# Rust Contract Audit

Audit scope: every Rust production module and its tests in `io-types`, `io-world`,
`io-assets`, and the root application, including the validation CLI. This is an
invariant audit, not a mechanical conversion of `if` statements to `match`.

Follow-up: [Item Components](item-components.md) separates placement, optional
rendering/durability, and explicit depletion responses. References below to item
visual domains now refer to the optional Renderable component; baseline items no
longer intrinsically have health, character classification, or appearance IDs.

## Enforced Boundaries

| Boundary | Enforcement |
| --- | --- |
| Input -> catalog | Strict serde fields/enums, source exclusivity, numeric validation, resolved names, versioned package capabilities, actual mesh/clip checks |
| Import -> shared models | Geometry, clips, and bounds are read-only outside `io-assets`; validated constructors/importers own creation |
| Construction -> world | `World::try_new` rejects duplicate/zero IDs, invalid poses/bounds/colors, and out-of-domain visual states before indexing |
| Gameplay -> retained state | World exposes shared item references, not mutable items; checked pose/state methods preserve state on rejected input |
| Animation -> simulation | Private playback fields; checked speed/seek/event methods; validated immutable route data |
| Camera -> projection | Private viewport state, checked numeric updates, rejected overflowing pan and nonfinite interpolation |
| World -> render packet | Unresolved visible appearances/states/clips return errors; checked joint offsets; failed frames are not published through the app/FFI |
| Rust -> C | Explicit pointer/lifetime requirements, cleared failure outputs, immutable model arrays, borrowed frame arrays |

## Findings Addressed

- Scene validation previously depended on reading JSON from disk. In-memory
  catalog construction now executes the same numeric validation.
- Asset source and capability strings now resolve into explicit enums. A package
  cannot claim a skeleton or clips that disagree with the imported model.
- Imported model geometry and animation clips could be mutated across crate
  boundaries after validation. Public slice/value getters replace that access.
- Animation time, speed, playback, and event bindings could be assigned directly.
  They are now private. Speed is finite within 0..64; seek is finite/nonnegative;
  once playback clamps to its duration and rejects loop-event tracks.
- Invalid item data and duplicate IDs could reach world construction/indexing.
  Fallible constructors validate before indexing; the production scene path uses
  them. Pose updates validate transformed bounds before committing index changes.
- Visual state mutations previously relied entirely on the caller. Each item now
  carries its resolved state count, and world mutations reject out-of-domain IDs.
- Spatial cell-span arithmetic could overflow before widening. Calculations now
  widen first and route oversized bounds to overflow storage without multiplying
  unchecked spans. Bounds helpers reject invalid clamp inputs.
- Projection silently skipped unresolved appearances/states and fell back from
  invalid clip references. It now returns a failure instead of accepting a partial
  frame. Deliberate below-threshold LOD omission remains a valid outcome.
- Material texture channels beyond base color could be ignored. The importer now
  rejects those unsupported channels and invalid color/emission values.
- Input dispatch used a nested wildcard fallback, weakening exhaustiveness when
  adding actions. It now explicitly handles every action and shares cache
  invalidation through a helper. A rejected pan no longer detaches following.

## Why Conditions Remain

Use exhaustive `match` for closed alternatives: axes, input actions, effects,
playback modes, asset sources, mesh kinds, and supported package capabilities.
Adding an enum variant then exposes missing dispatch handling at compile time.
Avoid wildcard fallback in dispatch where a new variant needs explicit behavior.

Use `if`, guards, and `Result` for value predicates: finite numbers, range checks,
unique names, ordered thresholds, compatible rigs, and optional components.
Exhaustive matching cannot prove that an imported float is finite or that a file's
joint names agree with another file. Replacing these conditions with matches
would not strengthen their contracts.

Input descriptions deliberately remain editable structs. `Item` is also editable
before world insertion, while `World` owns and protects accepted items. This is
not a claim that every possible invalid value is unrepresentable in Rust.

## Remaining Limits

- Typed appearance/state IDs prevent mixing their Rust types, not forged numbers
  or IDs from another catalog. Scene resolution supplies the real state domain;
  the mesh-independent world checks its supplied count. Projection checks visible
  references against the actual catalog. A future public spawning API should
  resolve appearances before handing items to the world, as scene loading does.
- Catalog/appearance fields are internal to the root application's private
  modules, not an immutable public registry API. If these modules become public
  crates, encapsulate them before exposing mutation or hot reload.
- `Space::new` and `World::new` remain trusted convenience constructors that panic
  on invalid programmer input; content loading uses `try_new`. Remaining internal
  `unwrap`/`unreachable` paths include private camera/index/route invariants and
  tests, not a replacement for input validation.
- Runtime mutation APIs generally return `bool`: invalid, missing, and unchanged
  operations can share `false`. They preserve state, but richer error enums would
  be useful for editor diagnostics and a future public scripting API.
- Events are validated semantically, but total event work, asset sizes, scene
  sizes, and allocation failure are not comprehensively budgeted. Extremely short
  loop durations or dense event tracks can generate excessive work. Untrusted
  mod ingestion needs explicit resource budgets, not just these type contracts.
- Low-level asset palette sampling still permits a rest-pose fallback; the frame
  builder resolves and checks an item's clip before invoking it. World animation
  indices intentionally do not depend on `io-assets`.
- FFI cannot verify arbitrary nonnull pointer liveness, allocation extent, or C
  aliasing. `unsafe extern` marks those caller obligations; it does not disable
  type safety in the safe world, camera, and asset modules.

## Regression Checks

```sh
make test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --bin io-validate -- package assets/street-kit/package.json
cargo run --bin io-validate -- scene assets/street-kit/package-demo.json
make gpu-test
make release
```

Tests exercise rejected mutations without revision/index changes, invalid package
declarations and dependency escapes, namespace reuse, actual Blender package
equivalence, numeric overflow, and explicit frame rejection. Existing coverage
continues to check huge-world loading, LOD selection, animation/effects, camera
independence, C ABI layout, and GPU buffer reuse. These checks do not establish a
performance improvement or certify arbitrary assets as visually correct.
