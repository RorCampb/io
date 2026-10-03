//! Source-neutral per-Item observation memory. Payload meaning and trust are plugin policy.
use io_game::{
    channel::{ChannelId, Report},
    stage::Stage,
};
use io_types::{Envelope, MessageId};
use std::collections::BTreeMap;

/// Plugin-assigned sensor/producer identity, stable within one observer's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProducerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObservationSource {
    /// Vision, hearing, or another directly sampled plugin-defined source.
    Direct { producer: ProducerId },
    /// Information delivered by a channel, not a visual contact.
    Reported { sender: u64, channel: ChannelId },
}

#[derive(Clone, Debug)]
pub struct ObservationRecord<P> {
    observer: u64,
    subject: u64,
    source: ObservationSource,
    sequence: MessageId,
    observed_at: f64,
    received_at: f64,
    payload: P,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntakeError {
    InvalidTime,
    WrongObserver,
    TimeReversed,
    InvalidCapacity,
    RecordsFull,
    SourcesFull,
}

impl<P> ObservationRecord<P> {
    /// The producer allocates monotonic IDs across ALL its subjects and fact keys.
    pub fn direct(
        observer: u64,
        subject: u64,
        producer: ProducerId,
        sequence: MessageId,
        times: (f64, f64),
        payload: P,
    ) -> Result<Self, IntakeError> {
        Self::new(
            observer,
            subject,
            ObservationSource::Direct { producer },
            sequence,
            times,
            payload,
        )
    }

    /// Preserve channel provenance/sequence from the actual delivered envelope.
    /// `times` is (original observation time, local receipt time) in simulation seconds.
    /// Channel sent_tick is NOT the observation time; forwarding must preserve that time
    /// in its payload. Mapping reported content into P remains a trusted plugin operation.
    pub fn reported<Q>(
        observer: u64,
        subject: u64,
        report: &Envelope<Report<Q>>,
        times: (f64, f64),
        payload: P,
    ) -> Result<Self, IntakeError> {
        Self::new(
            observer,
            subject,
            ObservationSource::Reported {
                sender: report.payload.sender,
                channel: report.payload.channel,
            },
            report.id,
            times,
            payload,
        )
    }

    fn new(
        observer: u64,
        subject: u64,
        source: ObservationSource,
        sequence: MessageId,
        (observed_at, received_at): (f64, f64),
        payload: P,
    ) -> Result<Self, IntakeError> {
        if !observed_at.is_finite()
            || !received_at.is_finite()
            || observed_at < 0.
            || received_at < observed_at
        {
            return Err(IntakeError::InvalidTime);
        }
        Ok(Self {
            observer,
            subject,
            source,
            sequence,
            observed_at,
            received_at,
            payload,
        })
    }
    pub fn observer(&self) -> u64 {
        self.observer
    }
    pub fn subject(&self) -> u64 {
        self.subject
    }
    pub fn source(&self) -> ObservationSource {
        self.source
    }
    pub fn sequence(&self) -> MessageId {
        self.sequence
    }
    pub fn observed_at(&self) -> f64 {
        self.observed_at
    }
    pub fn received_at(&self) -> f64 {
        self.received_at
    }
    pub fn payload(&self) -> &P {
        &self.payload
    }
}

/// Plugins choose the fact key and an absolute expiry, not an engine-assigned attention score.
pub struct Remember<K, P> {
    pub key: K,
    pub record: ObservationRecord<P>,
    pub expires_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntakeOutcome {
    Stored,
    Duplicate,
    Stale,
    Expired,
}

#[derive(Clone, Debug)]
struct Entry<P> {
    record: ObservationRecord<P>,
    expires_at: f64,
}

/// Latest fact per (subject, plugin key, source). Conflicting sources remain separate.
///
/// Sequence watermarks survive expiry/forgetting, so replay cannot resurrect old knowledge.
/// Source slots persist for this memory's lifetime and are explicitly bounded. Use stable
/// producer/channel identities; constructing new channels every tick will exhaust this limit.
/// This is state memory, NOT a reliable action/event queue. Use ItemChannel for delivery.
/// No world lookup, attention mutation, trust ranking, navigation, or automatic eviction occurs.
#[derive(Clone, Debug)]
pub struct ObservationMemory<K, P> {
    observer: u64,
    record_limit: usize,
    source_limit: usize,
    now: f64,
    records: BTreeMap<(u64, K, ObservationSource), Entry<P>>,
    watermarks: BTreeMap<ObservationSource, u64>,
}
impl<K: Ord, P> ObservationMemory<K, P> {
    pub fn new(
        observer: u64,
        record_limit: usize,
        source_limit: usize,
    ) -> Result<Self, IntakeError> {
        if record_limit == 0 || source_limit == 0 {
            return Err(IntakeError::InvalidCapacity);
        }
        Ok(Self {
            observer,
            record_limit,
            source_limit,
            now: 0.,
            records: BTreeMap::new(),
            watermarks: BTreeMap::new(),
        })
    }
    pub fn observer(&self) -> u64 {
        self.observer
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn records(&self) -> impl Iterator<Item = (&K, &ObservationRecord<P>)> {
        self.records
            .iter()
            .map(|((_, key, _), entry)| (key, &entry.record))
    }
    /// Call each decision/update phase even when no producers have supplied new records.
    pub fn advance(&mut self, now: f64) -> Result<(), IntakeError> {
        if !now.is_finite() || now < 0. {
            return Err(IntakeError::InvalidTime);
        }
        if now < self.now {
            return Err(IntakeError::TimeReversed);
        }
        self.now = now;
        self.records.retain(|_, e| e.expires_at > now);
        Ok(())
    }
    pub fn forget_subject(&mut self, subject: u64) {
        self.records.retain(|(id, _, _), _| *id != subject);
    }

    pub fn ingest(&mut self, request: Remember<K, P>) -> Result<IntakeOutcome, IntakeError> {
        let Remember {
            key,
            record,
            expires_at,
        } = request;
        if record.observer != self.observer {
            return Err(IntakeError::WrongObserver);
        }
        if !expires_at.is_finite() || expires_at < record.observed_at {
            return Err(IntakeError::InvalidTime);
        }
        self.advance(record.received_at)?;
        let source = record.source;
        let sequence = record.sequence.0;
        match self.watermarks.get(&source) {
            Some(&last) if sequence <= last => return Ok(IntakeOutcome::Duplicate),
            None if self.watermarks.len() == self.source_limit => {
                return Err(IntakeError::SourcesFull)
            }
            _ => {}
        }
        let key = (record.subject, key, source);
        let outcome = if expires_at <= self.now {
            IntakeOutcome::Expired
        } else if self
            .records
            .get(&key)
            .is_some_and(|e| e.record.observed_at > record.observed_at)
        {
            IntakeOutcome::Stale
        } else {
            if !self.records.contains_key(&key) && self.records.len() == self.record_limit {
                return Err(IntakeError::RecordsFull);
            }
            self.records.insert(key, Entry { record, expires_at });
            IntakeOutcome::Stored
        };
        self.watermarks.insert(source, sequence);
        Ok(outcome)
    }
}

impl<K: Ord, P> Stage<Remember<K, P>> for ObservationMemory<K, P> {
    type Output = IntakeOutcome;
    type Error = IntakeError;
    fn run(&mut self, request: Remember<K, P>) -> Result<Self::Output, Self::Error> {
        self.ingest(request)
    }
}

#[cfg(test)]
mod tests;
