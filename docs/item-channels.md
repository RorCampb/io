# Item Communication Channels

`io_game::channel::ItemChannel<P>` is a bounded shared log connecting existing
`Item.id` values. The owning game plugin chooses a typed payload enum `P` and
stores channels as part of its authoritative state. No new crate, NPC identity,
world component, thread, or navigation controller is introduced.

## Basic Use

```rust
use io_game::channel::{ChannelId, ItemChannel};
use io_types::Vec3;

#[derive(Clone, Debug)]
enum Radio {
    Sighting { target: u64, position: Vec3 },
    Claim { position: Vec3 },
}

let mut squad = ItemChannel::new(ChannelId(1), 64, 8).unwrap();
for item in [10, 20, 30] {
    squad.join(item).unwrap();
}
squad.send(10, 120, Radio::Sighting {
    target: 99,
    position: Vec3::new(4., 8., 0.),
}).unwrap();

// Each member has its own cursor, including the sender.
for member in [10, 20, 30] {
    for envelope in squad.poll(member, 8).unwrap() {
        let report = &envelope.payload;
        // Feed report.sender, report.sent_tick and report.payload to YOUR
        // observation/behavior stage. Receiving is not an automatic action.
        assert_eq!(report.sender, 10);
    }
}
assert_eq!(squad.buffered(), 0);
```

A two-member channel is a direct conversation. A channel with more members is a
group. An Item can belong to multiple independent channels. Allocate unique
channel IDs within the session and do not reuse them for replacement channels.
Channel-local message IDs start at one and never wrap; the full report identity
is `(ChannelId, MessageId)`. Sending the same payload again is a new message,
not automatic deduplication.

## Delivery Contract

- Only members can send or poll. Members are Item IDs, not indices into world storage.
- New and rejoining members receive future messages only. Repeated `join` on an
  existing member preserves its unread messages.
- All current members, including the sender, receive messages in publication order.
  A plugin can ignore its own reports after polling them.
- Polling advances only that reader's cursor. It acknowledges delivery, not
  successful behavior execution. Stage failures/retries are the plugin's responsibility.
- Payloads are stored once behind `Arc`; readers obtain shared immutable handles
  without cloning the payload. Fully consumed messages are reclaimed.
- Message count and membership count are bounded. `Full` rejects a send and returns
  its original payload; unread reports are never overwritten. A slow reader can
  block publication. Poll it, remove it, or apply an explicit plugin retry/coalescing
  policy. Do not retry in a busy loop inside a simulation tick.
- Leaving releases that member's delivery obligations. The plugin validates Item
  existence and removes despawned members. The channel does not scan the world.
- Counts are bounded, not arbitrary payload bytes or handles retained by consumers.
  Prefer compact enum payloads and bounded plugin-owned inboxes.
- Cloning channel state copies cursors and membership independently, sharing only
  immutable messages. Renderer snapshots must not mutate authoritative channels.

## Observations and Scheduling

The existing `io-perception` visual pipeline models visual evidence. Its shared
[observation intake](observation-intake.md) now accepts facts from visual, radio,
and other producers into an engine-defined per-Item memory. A radio report must
not be inserted as a fake visual contact, raise visual attention automatically,
or read a hidden target's current position. Its sender, publication tick, and
reported payload are available to a plugin-defined observation stage. If the
report describes an older sighting, include that observation's original time in
the payload too. Trust, expiry, range, recipients, tactical response, and knowledge
retention belong to the plugin.

`crates/io-game/tests/channel_contract.rs` is an executable public-API example:
custom radio enums become reported-position and ally-intent knowledge using the
existing `Stage` trait, without world lookups or engine observation enum changes.
It also checks isolated direct/group channels and delivery to 384 members.

Operations are synchronous and nonblocking on the simulation owner. There is no
automatic background delivery or extra async runtime. For next-tick squad reactions,
poll all members first, evaluate observations/decisions, then publish outgoing
reports. Do not interleave one actor's send and another actor's poll unless that
same-tick ordering is intentional. The host/plugin defines this phase ordering.

Ordinary playground scenes do not enable squad communication. The unfinished
opt-in battle policy now sends sightings/claims and adapts them through the shared
observation intake before decisions. That integration is covered by Rust tests;
the dungeon battle exercise and dynamic cover behavior are not yet a completed feature.

Run `cargo test -p io-game` to check the contract.
