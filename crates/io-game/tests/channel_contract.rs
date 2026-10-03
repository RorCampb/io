//! A plugin-defined radio feeds a plugin-defined observation stage through public APIs.
use io_game::{channel::*, stage::Stage};
use io_types::{Envelope, Vec3};

#[derive(Clone, Debug)]
enum Radio {
    Sighting { target: u64, position: Vec3 },
    Claim { position: Vec3 },
}

#[derive(Debug, PartialEq)]
enum Knowledge {
    ReportedPosition {
        observer: u64,
        source: u64,
        target: u64,
        position: Vec3,
        tick: u64,
    },
    AllyIntent {
        observer: u64,
        source: u64,
        position: Vec3,
    },
}

struct ReceiveReport;
impl Stage<(u64, &Envelope<Report<Radio>>)> for ReceiveReport {
    type Output = Knowledge;
    type Error = std::convert::Infallible;

    fn run(
        &mut self,
        (observer, envelope): (u64, &Envelope<Report<Radio>>),
    ) -> Result<Knowledge, Self::Error> {
        let report = &envelope.payload;
        Ok(match report.payload {
            Radio::Sighting { target, position } => Knowledge::ReportedPosition {
                observer,
                source: report.sender,
                target,
                position,
                tick: report.sent_tick,
            },
            Radio::Claim { position } => Knowledge::AllyIntent {
                observer,
                source: report.sender,
                position,
            },
        })
    }
}

#[test]
fn radio_reports_enter_observation_stages_without_visual_evidence_or_world_lookups() {
    let mut channel = ItemChannel::new(ChannelId(1), 16, 3).unwrap();
    for item in [10, 20, 30] {
        channel.join(item).unwrap();
    }
    let last_seen = Vec3::new(2., 3., 4.);
    channel
        .send(
            10,
            50,
            Radio::Sighting {
                target: 99,
                position: last_seen,
            },
        )
        .unwrap();
    channel
        .send(
            20,
            51,
            Radio::Claim {
                position: last_seen,
            },
        )
        .unwrap();
    for receiver in [20, 30] {
        let reports = channel.poll(receiver, 16).unwrap();
        assert_eq!(
            ReceiveReport.run((receiver, &reports[0])).unwrap(),
            Knowledge::ReportedPosition {
                observer: receiver,
                source: 10,
                target: 99,
                position: last_seen,
                tick: 50,
            }
        );
        assert_eq!(
            ReceiveReport.run((receiver, &reports[1])).unwrap(),
            Knowledge::AllyIntent {
                observer: receiver,
                source: 20,
                position: last_seen,
            }
        );
        assert!(channel.poll(receiver, 16).unwrap().is_empty());
    }
}

#[test]
fn direct_and_group_channels_are_isolated_even_with_shared_members() {
    let mut direct = ItemChannel::new(ChannelId(1), 4, 2).unwrap();
    let mut squad = ItemChannel::new(ChannelId(2), 4, 384).unwrap();
    for member in [10, 20] {
        direct.join(member).unwrap();
    }
    for member in 1..=384 {
        squad.join(member).unwrap();
    }
    direct
        .send(
            10,
            1,
            Radio::Claim {
                position: Vec3::default(),
            },
        )
        .unwrap();
    squad
        .send(
            10,
            1,
            Radio::Claim {
                position: Vec3::default(),
            },
        )
        .unwrap();
    assert_eq!(direct.poll(30, 1).unwrap_err(), ChannelError::NotMember);
    for member in 1..=384 {
        let reports = squad.poll(member, 1).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].payload.channel, ChannelId(2));
    }
    assert_eq!(squad.buffered(), 0);
    assert_eq!(direct.buffered(), 1);
    assert_eq!(direct.poll(20, 1).unwrap()[0].payload.channel, ChannelId(1));
}
