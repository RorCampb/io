use super::*;
use io_game::channel::ItemChannel;

fn direct(
    subject: u64,
    id: u64,
    observed: f64,
    received: f64,
    value: i32,
) -> ObservationRecord<i32> {
    ObservationRecord::direct(
        10,
        subject,
        ProducerId(1),
        MessageId(id),
        (observed, received),
        value,
    )
    .unwrap()
}
fn remember(record: ObservationRecord<i32>, expires_at: f64) -> Remember<u8, i32> {
    Remember {
        key: 0,
        record,
        expires_at,
    }
}

#[test]
fn multiple_sources_keep_distinct_provenance_and_do_not_overwrite_direct_evidence() {
    let mut memory = ObservationMemory::new(10, 8, 4).unwrap();
    memory
        .run(remember(direct(99, 1, 2., 2., 20), 10.))
        .unwrap();
    let mut radio = ItemChannel::new(ChannelId(7), 8, 3).unwrap();
    for id in [10, 11, 12] {
        radio.join(id).unwrap();
    }
    radio.send(11, 120, 15).unwrap();
    radio.send(12, 120, 17).unwrap();
    for envelope in radio.poll(10, 8).unwrap() {
        let record =
            ObservationRecord::reported(10, 99, &envelope, (1., 3.), envelope.payload.payload)
                .unwrap();
        assert_eq!(memory.run(remember(record, 8.)), Ok(IntakeOutcome::Stored));
    }
    assert_eq!(memory.len(), 3);
    let records = memory.records().map(|(_, r)| r).collect::<Vec<_>>();
    assert!(records.iter().any(|r| r.source()
        == ObservationSource::Direct {
            producer: ProducerId(1)
        }
        && *r.payload() == 20
        && r.observed_at() == 2.));
    for r in records
        .into_iter()
        .filter(|r| matches!(r.source(), ObservationSource::Reported { .. }))
    {
        assert_eq!(r.observer(), 10);
        assert_eq!(r.subject(), 99);
        assert_eq!(r.observed_at(), 1.);
        assert_eq!(r.received_at(), 3.);
    }
}

#[test]
fn stale_delayed_data_duplicates_and_expiry_do_not_refresh_knowledge() {
    let mut m = ObservationMemory::new(10, 4, 2).unwrap();
    assert_eq!(
        m.ingest(remember(direct(99, 1, 5., 5., 50), 10.)),
        Ok(IntakeOutcome::Stored)
    );
    assert_eq!(
        m.ingest(remember(direct(99, 2, 4., 6., 40), 20.)),
        Ok(IntakeOutcome::Stale)
    );
    assert_eq!(
        m.ingest(remember(direct(99, 2, 7., 7., 70), 20.)),
        Ok(IntakeOutcome::Duplicate)
    );
    assert_eq!(*m.records().next().unwrap().1.payload(), 50);
    m.advance(10.).unwrap();
    assert!(m.is_empty());
    assert_eq!(
        m.ingest(remember(direct(99, 1, 5., 10., 50), 20.)),
        Ok(IntakeOutcome::Duplicate)
    );
    assert_eq!(
        m.ingest(remember(direct(99, 3, 6., 10., 60), 9.)),
        Ok(IntakeOutcome::Expired)
    );
    assert!(m.is_empty());
    assert_eq!(
        m.ingest(remember(direct(99, 4, 10., 10., 100), 20.)),
        Ok(IntakeOutcome::Stored)
    );
    m.forget_subject(99);
    assert_eq!(
        m.ingest(remember(direct(99, 4, 10., 10., 100), 20.)),
        Ok(IntakeOutcome::Duplicate)
    );
}

#[test]
fn invalid_times_observers_and_reversed_receipts_are_rejected() {
    for times in [(f64::NAN, 1.), (0., f64::INFINITY), (-1., 1.), (2., 1.)] {
        assert_eq!(
            ObservationRecord::direct(10, 99, ProducerId(1), MessageId(1), times, 1).unwrap_err(),
            IntakeError::InvalidTime
        );
    }
    let mut m = ObservationMemory::new(10, 2, 2).unwrap();
    let foreign =
        ObservationRecord::direct(11, 99, ProducerId(1), MessageId(1), (1., 1.), 1).unwrap();
    assert_eq!(
        m.ingest(remember(foreign, 2.)),
        Err(IntakeError::WrongObserver)
    );
    assert_eq!(
        m.ingest(remember(direct(99, 1, 2., 2., 1), 1.)),
        Err(IntakeError::InvalidTime)
    );
    m.advance(3.).unwrap();
    assert_eq!(
        m.ingest(remember(direct(99, 1, 2., 2., 1), 4.)),
        Err(IntakeError::TimeReversed)
    );
    assert!(m.is_empty());
}

#[test]
fn limits_are_explicit_and_failed_capacity_admission_can_be_retried() {
    assert_eq!(
        ObservationMemory::<u8, i32>::new(10, 0, 1).unwrap_err(),
        IntakeError::InvalidCapacity
    );
    let mut m = ObservationMemory::new(10, 1, 1).unwrap();
    m.ingest(remember(direct(99, 1, 1., 1., 1), 3.)).unwrap();
    assert_eq!(
        m.ingest(remember(direct(98, 2, 2., 2., 2), 6.)),
        Err(IntakeError::RecordsFull)
    );
    m.advance(3.).unwrap();
    assert_eq!(
        m.ingest(remember(direct(98, 2, 2., 3., 2), 6.)),
        Ok(IntakeOutcome::Stored)
    );
    let other =
        ObservationRecord::direct(10, 98, ProducerId(2), MessageId(1), (3., 3.), 3).unwrap();
    assert_eq!(m.ingest(remember(other, 6.)), Err(IntakeError::SourcesFull));
    assert_eq!(*m.records().next().unwrap().1.payload(), 2);
}

#[test]
fn plugin_fact_keys_and_subjects_are_separate_and_snapshot_progress_is_independent() {
    let mut m = ObservationMemory::new(10, 8, 2).unwrap();
    for (id, subject, key) in [(1, 99, 0), (2, 99, 1), (3, 98, 0)] {
        m.ingest(Remember {
            key,
            record: direct(subject, id, 1., 1., id as i32),
            expires_at: 5.,
        })
        .unwrap();
    }
    let snapshot = m.clone();
    m.forget_subject(99);
    assert_eq!(m.len(), 1);
    assert_eq!(snapshot.len(), 3);
    m.advance(5.).unwrap();
    assert!(m.is_empty());
    assert_eq!(snapshot.len(), 3);
}
