use super::*;

fn channel(capacity: usize) -> ItemChannel<u32> {
    let mut c = ItemChannel::new(ChannelId(7), capacity, 3).unwrap();
    c.join(10).unwrap();
    c.join(20).unwrap();
    c
}

#[test]
fn group_members_read_the_same_allocation_independently_and_in_order() {
    let mut c = channel(4);
    c.join(30).unwrap();
    assert_eq!(c.send(10, 80, 100).unwrap(), MessageId(1));
    c.send(20, 81, 200).unwrap();
    let a = c.poll(10, 1).unwrap();
    let b = c.poll(20, 4).unwrap();
    let d = c.poll(30, 4).unwrap();
    assert!(Arc::ptr_eq(&a[0], &b[0]));
    assert!(Arc::ptr_eq(&b[0], &d[0]));
    assert_eq!(b.iter().map(|m| m.id.0).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(a[0].payload.sender, 10);
    assert_eq!(a[0].payload.sent_tick, 80);
    assert_eq!(a[0].payload.channel, ChannelId(7));
    assert_eq!(c.buffered(), 1);
    assert!(c.poll(20, 4).unwrap().is_empty());
    assert_eq!(c.poll(10, 4).unwrap()[0].payload.payload, 200);
    assert_eq!(c.buffered(), 0);
}

#[test]
fn full_buffer_rejects_without_losing_payload_or_advancing_sequence() {
    let mut c = channel(1);
    c.send(10, 1, 100).unwrap();
    c.poll(10, 1).unwrap();
    let rejected = c.send(10, 2, 200).unwrap_err();
    assert_eq!(rejected.reason, ChannelError::Full);
    assert_eq!(rejected.payload, 200);
    assert!(c.poll(20, 0).unwrap().is_empty());
    assert_eq!(c.buffered(), 1);
    assert_eq!(c.poll(20, 1).unwrap()[0].payload.payload, 100);
    assert_eq!(c.send(10, 2, rejected.payload).unwrap(), MessageId(2));
}

#[test]
fn membership_controls_access_and_join_never_replays_old_history() {
    let mut c = channel(4);
    c.send(10, 1, 100).unwrap();
    assert_eq!(c.join(10), Ok(false));
    assert_eq!(c.join(30), Ok(true));
    assert_eq!(c.join(40), Err(ChannelError::MemberLimit));
    assert!(c.poll(30, 4).unwrap().is_empty());
    assert_eq!(
        c.send(40, 1, 100).unwrap_err().reason,
        ChannelError::NotMember
    );
    assert_eq!(c.poll(40, 1).unwrap_err(), ChannelError::NotMember);
    assert_eq!(c.poll(10, 1).unwrap().len(), 1);
    assert!(c.leave(20));
    assert_eq!(c.buffered(), 0);
    assert!(!c.leave(20));
    c.join(20).unwrap();
    assert!(c.poll(20, 4).unwrap().is_empty());
    c.send(30, 2, 200).unwrap();
    assert_eq!(c.poll(20, 4).unwrap()[0].payload.payload, 200);
    for id in [10, 20, 30] {
        c.leave(id);
    }
    assert_eq!(c.buffered(), 0);
    assert_eq!(c.members().count(), 0);
}

#[test]
fn snapshots_do_not_consume_live_messages_or_change_membership() {
    let mut c = channel(4);
    c.send(10, 1, 100).unwrap();
    let mut snapshot = c.clone();
    let old = snapshot.poll(20, 1).unwrap();
    snapshot.leave(10);
    assert_eq!(snapshot.buffered(), 0);
    assert_eq!(c.buffered(), 1);
    assert_eq!(c.members().count(), 2);
    let live = c.poll(20, 1).unwrap();
    assert!(Arc::ptr_eq(&old[0], &live[0]));
    c.send(10, 2, 200).unwrap();
    assert!(snapshot.poll(20, 4).unwrap().is_empty());
}

#[test]
fn configuration_and_sequence_exhaustion_are_checked() {
    for (capacity, members) in [(0, 1), (1, 0)] {
        assert_eq!(
            ItemChannel::<u32>::new(ChannelId(1), capacity, members).unwrap_err(),
            ChannelError::InvalidCapacity
        );
    }
    let mut c = channel(4);
    c.sequence = u64::MAX - 1;
    c.members.values_mut().for_each(|v| *v = u64::MAX - 1);
    assert_eq!(c.send(10, 1, 100).unwrap(), MessageId(u64::MAX));
    assert_eq!(c.poll(10, 4).unwrap().len(), 1);
    assert_eq!(c.poll(20, 4).unwrap().len(), 1);
    assert_eq!(c.buffered(), 0);
    assert_eq!(
        c.send(10, 1, 200).unwrap_err().reason,
        ChannelError::SequenceExhausted
    );
}

#[test]
fn polling_does_not_require_cloneable_payloads() {
    #[derive(Debug)]
    struct Payload;
    let mut c = ItemChannel::new(ChannelId(1), 1, 1).unwrap();
    c.join(10).unwrap();
    c.send(10, 1, Payload).unwrap();
    assert_eq!(c.poll(10, 1).unwrap().len(), 1);
}
