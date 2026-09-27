# Movement Plugins

## Ownership

- `io-game`: existing `GamePlugin`/`Session` lifecycle and checked world access.
- `io-traversal`: generic route planning, Item bindings, objective tickets and execution contracts.
- `plugins/io-locomotion`: optional upright-body movement, surface/jump/step providers and animation.
- `io-world`: authoritative Items, geometry queries and physics.
- Your plugin: capabilities, action meanings, observations and gameplay reactions.

These are extension traits in existing crates, not a new plugin loader or crate
per ability. Plugins are trusted, statically linked Rust modules or crates. One
`Session` hosts a top-level `GamePlugin`, which composes movement services.
The native host still explicitly wires input, scene configuration and presentation.

## Code Map

There is one generic route/execution crate. The old `io-navigation` crate has
been merged into `io-traversal::navigation`; there is no humanoid feature in core.

| Location | Responsibility |
| --- | --- |
| `crates/io-traversal/src/navigation/` | `Waypoint<A>`, `Agent<P>`, route storage/progress, search, `RouteProvider` |
| `crates/io-traversal/src/coordinator.rs` | Internal `RouteCoordinator<D>`: one Item's goal, ticket, retries and action lifecycle |
| `crates/io-traversal/src/planning.rs` | Bounded background search jobs and checked completions, not another executor |
| `crates/io-traversal/src/executor.rs` | `TraversalExecutor` trait: the plugin contract, not an extra runtime object |
| `crates/io-traversal/src/lib.rs` | `Movement<D>`: bindings, shared planning budget and direct results |
| `plugins/io-locomotion/src/motor.rs` | One optional `Motor` for physical movement, turning, gravity and support |
| `plugins/io-locomotion/src/animation.rs` | Shared animation driver; cannot change physical poses |
| `plugins/io-locomotion/src/paths.rs` | Shared walking/step/jump feasibility and planned motion sampling |
| `plugins/io-locomotion/src/executor.rs` | `SurfaceExecutor`: steering/route adapter to that motor |
| `plugins/io-locomotion/src/athletics.rs` | Additional stair/jump route provider and adapter to the same motor |
| `crates/io-playground/src/movement.rs` | Application selection of a plugin configuration, not movement mechanics |

`Controller<D>` was renamed `RouteCoordinator<D>`, not replaced with another
layer. `CharacterMotor` moved out of core and became `io_locomotion::Motor`.
Player input and both NPC executors share that motor; the athletics adapter no
longer implements its own pose updates, gravity, turning or animation transitions.
`Agent::route()` exposes read-only waypoints for diagnostics, not mutable route state.

```text
Core RouteCoordinator -> plugin executor -> plugin Motor -> checked world queries/mutations
Player input ----------------------------> same Motor
Motor execution state -------------------> AnimationDriver
```

## Define Traversal for an Item

Implement two contracts in your plugin:

1. `io_traversal::navigation::RouteProvider` defines its own configuration, node identity,
   capability profile and compact action type. It supplies eligible connections,
   positive costs, live clearance and optional shortcut checks. The configuration
   need not be a grid: nodes can identify authored links, surfaces or other locations.
2. `io_traversal::TraversalExecutor` binds one existing Item ID. It validates the
   required world components, exposes its capability profile and executes the
   provider's actions. It owns typed feedback/events and returns `Running`,
   `Complete`, `Blocked` or `Failed`.

The executor's associated `Routes` type links these contracts at compile time.
Core traversal does not require a character body, humanoid action enum, model,
skeleton or animation. An action can carry a compact ID into richer plugin-owned
state. The plugin decides what an Item is; the engine does not need a door enum.

Construct `Movement<D>` with the provider configuration, shared search budget
and `ActorBinding<D>` values containing an executor, familiar points and
look-ahead distance. Duplicate Item bindings are rejected. Population is not capped
by the planner queue. Inline mode accepts 1-128 search expansions per planning turn.
Background mode admits at most 32 outstanding jobs, independently of population.

One service has one provider/executor Rust type. Compose separate services or
your own typed dispatch implementation for different implementations. The host
must enforce a single movement authority per Item across services; registration
inside one service cannot detect another service controlling the same Item.

The complete public-API example is
`crates/io-traversal/tests/item_plugin.rs`: a model-less kinematic sphere uses a
vertical route provider with `Config = ()` and `Calibrate | Translate` actions.
It uses physics targets and live sphere casts, not io_locomotion::Motor or placement.
Calibration completes explicitly, even though the Item already occupies its
waypoint. This test does not add a new windowed scene.

Executors may compose typed `io_game::stage::Stage<Input>` steps without adding
another movement authority. The supplied locomotion implementation passes a supported
trajectory to actor-only validation instead of walking the same geometry twice.
See [Movement stages](movement-stages.md) for the contracts, public developer example,
scratch/provenance rules and remaining work that is deliberately not shared.

## Requests and Configuration

```rust
use io_traversal::{NavigationGoal, NavigationRequest};
use io_types::{Envelope, MessageId, Vec3};

// Inside your plugin, after constructing movement and resolving actor:
let request = Envelope::new(MessageId(1), NavigationRequest::Start {
    actor,
    goal: NavigationGoal::Position(Vec3::new(12.0, 8.0, 0.0)),
});
let reply = movement.request(request);
// Handle reply.payload explicitly; reply.id correlates with request.id.
```

`Start` replaces the current objective and returns a
`NavigationTicket { actor, revision }`, not an arrival notification.
`Replace` and `Cancel` require the current ticket. Unknown actors, invalid goals,
stale tickets and revision exhaustion leave the accepted objective unchanged.
Tickets belong to this service/world lifetime, not portable save-game identities.
`Reconsider { ticket }` requests another evaluation of the same objective without
incrementing its ticket. Repeated requests coalesce rather than cancelling queued
work. Safe accepted movement continues while a replacement is searched. A developer
can issue this from an attention/observation stage; core traversal has no dependency
on perception or a particular enemy/object interpretation.

`Position` requests a location. Optional `Approach` chooses a destination on an
XY ring using the provider's `offset_destination` support; providers without
that support cannot use it. The caller supplies observed or remembered positions.
Neither request secretly reads a target Item's live transform.

`replace_executor(ticket, replacement, world)` validates a new executor for the
same Item, cancels held intent, preserves the objective and replans under a new
ticket revision. Invalid or stale replacements are rejected without replacing
the existing executor. Use this for changed per-Item traversal configuration;
it is not runtime code loading or replacement with a different Rust type.

## Lifecycle and Feedback

Forward these from your top-level `GamePlugin`:

- `active_items`: include `movement.active_items()` for off-camera simulation.
- `before_step`: call `movement.before_step(world, tick.seconds())` before physics.
- `update`: call `movement.update(world, tick.seconds())` after physics.

`Session` integrates the world exactly once. Executors may stage physical targets
in `execute` and apply them in `before_step`; cancelling must clear staged intent
synchronously and apply any required physical stop before the next integration.
The service checks capability/revision changes before handing over that step.
Providers default to `RouteValidity::RevisionBound`. The optional
`RouteValidity::LiveChecked` contract permits retaining continuous routes across
geometry revisions only when provider and executor enforce live segment/support
safety. It does not suppress cache invalidation or certify future segments forever.
The supplied surface provider opts in; discrete athletics and cargo examples do not.
Continuous worker results also require a valid connector from the current pose,
while discrete results keep exact-start/revision validation.

`goal_approach_radius(profile)` defaults to zero. An opt-in provider may defer
transient occupancy tests near a geometrically valid destination; its executor must
still enforce actual collision safety. SurfaceRoutes uses body size and grid spacing,
not a scene-specific NPC count or destination list. Existing shared scenery tests
still apply and actor checks elsewhere in the search remain active.

An executor receives `begin` when the active target/action changes, then
`execute` on updates, including `None` when no route action is available.
`Complete` acknowledges the current route step. Position alone does not complete
an action unless its provider explicitly opts into `automatic_completion`.
Look-ahead cannot skip discrete actions. `Blocked` uses the common bounded-backoff
retry policy; `Failed` reports execution/provider failure rather than pretending
that no geometric route exists. This is synchronous completion, not an async
completion message that can be delivered later without a ticket.

`MovementFrame<F, E>` returns authoritative results directly:
`ActorFeedback<F>.execution` is the executor's own feedback type, and
`MovementEvent<E>::Execution` wraps its events. Feedback also includes status,
known-node count, update tick, objective ticket and `execution_ticket`.
The objective ticket can differ from `execution_ticket` while a replacement is
pending. Background planning may continue a safe old route during that interval;
the tickets match when the new route is installed.

## Background Search and Refinement

Select this before registering the plugin session:

```rust
let movement = movement.with_planning(io_traversal::PlanningMode::Background);
```

The supplied playground accepts `navigation.planning: "background"`; the guards
and athletics scenes enable it. The default remains `"inline"` for compatibility
and deterministic accelerated tests. These are scheduling modes of the same
provider, route and executor, not separate navigation implementations.

1. The coordinator captures an immutable `WorldSnapshot` and submits the objective,
   actor ticket, planning-attempt ID, capability profile and geometry revision.
2. One lazily started worker per Movement service runs search, then the typed
   `RouteRefinement` stage on the same snapshot. It round-robins expansion/refinement
   slices; each refinement invocation makes at most one provider geometry call.
   Approach destination selection and familiarization retain their existing bounds.
   At most 32 objectives are outstanding, including refinement.
3. Simulation polls completions without waiting. Only matching attempts can be
   accepted. The coordinator checks the ticket; `RouteHandoff` checks profile,
   geometry revision and the live connector before installing the existing `Agent` route.
4. The plugin executor still runs on the simulation tick. Live collision checks,
   physics and world mutation never move to the planning worker. C continues to
   consume the existing published render state; no second renderer buffer is added.

Cancellation/replacement discards superseded work. A full queue defers submission;
it never blocks the tick, and capacity is checked before copying the world snapshot.
Admission uses the existing actor round-robin: at 60 Hz, 256 bindings can take
about 4.3 seconds to receive their first submission opportunity, even before search
latency or queue saturation. This is not a guarantee of prompt routes at any scale.
A job is limited to 65,536 search/refinement slices. Providers
remain trusted bounded Rust code: cancellation is checked between calls, not inside
an arbitrary provider call. Shutdown joins the worker; published clones contain no
worker or shared queue. A clone that is subsequently simulated starts independently.

Discrete-action handoff is deliberately conservative: the actor must remain within
1 cm of the job's start, with a matching geometry revision. Live-checked continuous
routes may join from the current pose through a bounded, validated connector;
unrelated geometry changes do not cancel them. An old valid route continues while
a replacement is searched/refined, including when a replacement search fails.
A new actor with no route waits for preparation. Initial raw paths are not separately
published ahead of refinement. This is not arbitrary splicing onto jump takeoffs.

`planning_stats()` exposes submissions, completions, accept/reject/cancel counts,
queue saturation, pending count and job wall latency. Snapshot creation and live
safety checks still cost simulation time. Background planning isolates expensive
search and repeated shortcut preparation; it does not make individual geometry
queries cheaper or guarantee unlimited agents. `navigation_stats()` on the playground
and `Movement::stats()` distinguish `refinement_queries` from live `steering_queries`.
See [Navigation benchmarks](navigation-benchmarks.md) for repeatable measurements.
See [Traversal stress district](traversal-stress.md) for the larger population,
moving-obstacle fixtures and per-NPC progress measurements.

Navigation statuses are factual: `Idle`, `Planning`, `Following`, `Arrived`,
`Blocked`, `Unreachable`, `Failed`. Your plugin decides whether arrival means
talk, attack or do nothing. Read `Movement::actor` or the returned frame for
essential decisions. Optional `world.emit` history is bounded/lossy presentation
feedback, not a reliable completion transport.

## Provider and Safety Rules

Connections need finite destinations and positive integer costs; at most 64
outgoing connections are accepted per expansion. Invalid output fails the route.
A heuristic must be admissible for the supplied costs; the default zero heuristic
is safe. Provider calls must themselves perform bounded work: an expansion budget
cannot sandbox arbitrary plugin code or guarantee a millisecond deadline.

Node IDs must be stable within a provider revision. Capability profiles and cache
keys must include relevant traversal differences. `revision(world)` must change
when cached geometry or plugin-owned route inputs become stale; its default uses
the world's navigation revision. The service invalidates routes and held execution
on relevant changes. This is coarse invalidation, not per-edge observation memory.
Live physical checks remain necessary between planning and execution.

Provider/executor clones must not share mutable gameplay state. Use immutable
shared data or copy-on-write state, deterministic simulation inputs and no blocking
I/O. The service is not a scripting sandbox. `PluginWorld` operations are checked
individually, not atomic transactions: errors do not roll back earlier mutations.
`place` is caller-resolved, not collision-aware. Your executor must use appropriate
physics commands or world queries to make its movement safe.

## Optional Locomotion Plugin

The engine never depends on `io-locomotion`. The application or its top-level
game plugin explicitly adds it as a normal Rust dependency:

```toml
io-traversal = { path = "../../crates/io-traversal" }
io-locomotion = { path = "../../plugins/io-locomotion" }
```

`io_locomotion::surface::SurfaceRoutes` supplies the cached layered walking/
crouching provider. `SurfaceExecutor` translates its actions into steering and
shared `Motor` commands. `Movement` and `ActorBinding` are convenience types
for selecting this supplied implementation, not alternative generic contracts.

`io_locomotion::athletics` supplies an additional geometry-derived step/jump
provider and a thin executor that delegates to the same motor. It has no cache,
crouching or pursuit implementation. Its capabilities and action semantics belong
to this plugin, not engine enums. See [Athletics Course](athletics.md).

The motor receives either direct input or a checked planned action. Both use the
same pose, gravity/support and animation pipeline. Planning and planned execution
share feasibility math in `paths.rs`. Animation transitions are isolated in
`animation.rs`; they do not determine physical displacement. The current body is
an upright box, with kinematic character movement, not a dynamic rigid-body solver.

`SteeringSettings` and the optional `Steering` trait belong to this plugin.
For runtime response changes, import `SteeringControl` and call
`movement.set_steering(actor, response, world)`. It validates and replaces the
executor through the core contract, preserving the objective and returning a new
`NavigationTicket`. Store that returned ticket for later Replace/Cancel requests.

`Item.character_body` remains optional world-owned body data used by the supplied
plugin and older game examples. This refactor does not implement an arbitrary
runtime component registry or migrate every legacy game movement rule. Generic
traversal does not require that component, a model or animation.

## Verification

```sh
# Generic physics Item, without the humanoid implementation:
cargo test -p io-traversal --test item_plugin
# Existing humanoid consumer and support/motion regressions:
cargo test -p io-locomotion
cargo test -p io-playground
./build/release/io --scene assets/dungeon/cat-mouse.json
```

The generic test covers off-camera execution at 30/60/144 Hz, explicit completion,
collision rejection, invalid provider output, capability replacement, stale tickets,
cancellation, clone independence, geometry changes before integration and executor
failure. Humanoid tests retain shared-cache planning, steering, support-following,
target reversal, invalid requests and direct feedback despite event-history overflow.
