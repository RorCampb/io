# Shared Observation Intake

`io_perception::intake` defines the engine contract for multiple sources feeding
one Item's observation memory. It does not define enemies, doors, cover, or tactics.
The plugin owns an `ObservationMemory<K, P>` instance per observer and chooses
its fact key `K`, payload `P`, retention times, and decision policy.

```text
Existing visual pipeline -> plugin-approved visual fact --+
ItemChannel report ------> plugin payload adapter --------+-> ObservationMemory<K, P>
Other direct producer ----> plugin-defined fact -----------+       |
                                                        plugin decision stage
                                                                 |
                                                     existing navigation requests
```

## Contracts

- `ObservationRecord<P>` has checked, read-only metadata: observer Item ID,
  subject Item ID, source, sequence ID, original observation time, receipt time.
- `ObservationSource::Direct { producer }` distinguishes plugin-defined producers
  such as vision and hearing. `Reported { sender, channel }` explicitly marks
  communicated knowledge. Report sequence/provenance comes from the channel envelope.
- `Remember<K, P>` supplies a fact key and absolute expiry in simulation seconds.
  `ObservationMemory` implements the existing `Stage<Remember<K, P>>` interface.
- Memory keeps the latest fact per **(subject, fact key, source)**. Two observers'
  reports and the recipient's own sighting remain separate. The engine does not
  decide which source is trustworthy or merge their confidence/attention scores.

This is shared **state memory**, not a second event bus. `ItemChannel` still owns
delivery and reader cursors; memory accepts adapted facts after delivery. Nothing
polls automatically, reads hidden world state, sends navigation commands, or raises
visual attention on receiving a report.

## Plugin Use

```rust
use io_game::stage::Stage;
use io_perception::intake::{
    ObservationMemory, ObservationRecord, ProducerId, Remember,
};
use io_types::{MessageId, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FactKey { Position }

let mut knowledge = ObservationMemory::<FactKey, Vec3>::new(10, 64, 16).unwrap();

// Existing vision/attention stages provide the contact. Plugin policy decides
// whether it is meaningful enough to remember. No world lookup is done here.
let contact_position = Vec3::new(2., 3., 0.);
let record = ObservationRecord::direct(
    10, 99, ProducerId(1), MessageId(1), (2.0, 2.0), contact_position,
).unwrap();
knowledge.run(Remember {
    key: FactKey::Position,
    record,
    expires_at: 14.0,
}).unwrap();

// A channel consumer constructs ObservationRecord::reported(observer, subject,
// &envelope, (original_observation_seconds, receipt_seconds), mapped_payload)
// and submits the SAME Remember request to the SAME memory.

knowledge.advance(3.0).unwrap();
for (key, observation) in knowledge.records() {
    // Plugin decides how source(), observed_at(), and payload() affect behavior.
    assert_eq!(observation.observer(), 10);
    assert_eq!(observation.subject(), 99);
}
```

The executable source-combination examples/tests are in
`crates/io-perception/src/intake/tests.rs`. The battle module in `io-playground`
uses this path for direct sightings, radio sightings, position claims, and releases.
Its policy selects the freshest known position, preferring direct sight at equal
timestamps. That ranking is **plugin code**, not an engine rule.

## Ordering and Limits

All times use the same monotonic simulation clock in seconds. Creation rejects
non-finite/negative times and observations dated after their receipt. Intake rejects
foreign observers and receipts older than its current clock. Delayed data should
retain its original observation time but receive the **current** local receipt time.
Channel `sent_tick` is publication time, not proof of when the underlying event
was observed; preserve original time in the report's payload.

Direct producers allocate monotonic sequence IDs across all their subjects and
fact keys. Each radio envelope maps to one fact; the channel's message ID supplies
the sequence. An already accepted or earlier sequence from the same source returns
`Duplicate`. A newer message describing an older observation cannot replace a newer
fact from that source (`Stale`). Already expired observations return `Expired`.
There is no automatic fan-out of one envelope into several independently sequenced
facts; use separate messages or a plugin payload containing the combined fact.

Call `advance(now)` every update even when no input arrives. Expired facts are
removed. Sequence watermarks survive expiry and `forget_subject`, so replaying an
old message cannot revive that fact. Slots for distinct sources persist for the
memory's lifetime; use stable producer/channel identities.

Record and source counts are bounded independently. Capacity exhaustion returns
`RecordsFull` or `SourcesFull` rather than dropping unrelated knowledge. A capacity
rejection does not consume the sequence, so a caller retaining the request can retry
with a current receipt time. Intake may advance its clock/reclaim expired entries
before returning a rejection; it is not a multi-stage transaction. Payload bytes are
not bounded by these counts. Plugins should use compact enums and handle errors.

Memory clones are independent snapshot state. Like the rest of the plugin API,
this is trusted in-process Rust, not authentication or proof that a sensor was honest.

## Scope

Existing visual evidence/attention integration and existing navigation are unchanged.
Older playground pursuits and obstacle discovery still use their established visual
contracts; they have not all been migrated just to use this new type. The unfinished
battle policy now uses the shared intake instead of private sighting/claim stores.
The authored cover list was removed. Dynamic cover evaluation and interactive
dungeon battle validation remain separate work; this is not a completed combat system.
