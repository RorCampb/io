//! Bounded, typed group communication owned by the authoritative game plugin.
//! This is not the lossy presentation event stream or a cross-thread transport.
use io_types::{Envelope, MessageId};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

/// Plugin-assigned identity, unique within its session. Do not reuse for a new channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(pub u64);

/// Reported information, not direct sensory evidence. Payload semantics belong to plugins.
#[derive(Clone, Debug, PartialEq)]
pub struct Report<P> {
    pub channel: ChannelId,
    pub sender: u64,
    /// Simulation tick of publication, supplied by the owning plugin.
    pub sent_tick: u64,
    pub payload: P,
}

pub type SharedReport<P> = Arc<Envelope<Report<P>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelError {
    InvalidCapacity,
    MemberLimit,
    NotMember,
    Full,
    SequenceExhausted,
}

/// A rejected send retains the payload so the plugin can retry, coalesce or discard it.
#[derive(Debug)]
pub struct SendError<P> {
    pub reason: ChannelError,
    pub payload: P,
}

/// One shared log with an independent cursor per Item ID, including the sender.
///
/// New members receive only future messages. Polling acknowledges delivery, not successful
/// gameplay execution. Unread messages are never overwritten: a slow member can fill the
/// channel. The owner must poll it, remove it, or handle `Full` explicitly.
///
/// Membership is a plugin contract, not authentication. The owner validates Item existence
/// and removes despawned members. Channels use no world queries and grant no world knowledge.
/// Clones have independent membership/cursors and share only immutable message allocations;
/// snapshots must not be used as a second authoritative consumer.
#[derive(Clone, Debug)]
pub struct ItemChannel<P> {
    id: ChannelId,
    capacity: usize,
    member_limit: usize,
    members: BTreeMap<u64, u64>,
    messages: VecDeque<SharedReport<P>>,
    sequence: u64,
}

impl<P> ItemChannel<P> {
    pub fn new(id: ChannelId, capacity: usize, member_limit: usize) -> Result<Self, ChannelError> {
        if capacity == 0 || member_limit == 0 {
            return Err(ChannelError::InvalidCapacity);
        }
        Ok(Self {
            id,
            capacity,
            member_limit,
            members: BTreeMap::new(),
            messages: VecDeque::new(),
            sequence: 0,
        })
    }

    pub fn id(&self) -> ChannelId {
        self.id
    }

    pub fn members(&self) -> impl Iterator<Item = u64> + '_ {
        self.members.keys().copied()
    }

    pub fn buffered(&self) -> usize {
        self.messages.len()
    }

    /// Returns false for an existing member, preserving its unread messages.
    pub fn join(&mut self, item: u64) -> Result<bool, ChannelError> {
        if self.members.contains_key(&item) {
            return Ok(false);
        }
        if self.members.len() == self.member_limit {
            return Err(ChannelError::MemberLimit);
        }
        self.members.insert(item, self.sequence);
        Ok(true)
    }

    /// Leaving discards this member's unread delivery obligations, not other members'.
    pub fn leave(&mut self, item: u64) -> bool {
        let removed = self.members.remove(&item).is_some();
        self.reclaim();
        removed
    }

    /// Broadcast to current members. A two-member channel is a direct conversation.
    /// IDs are monotonic within this channel; identify reports by (ChannelId, MessageId).
    /// Retrying an already successful send publishes a NEW message, not an idempotent retry.
    pub fn send(
        &mut self,
        sender: u64,
        sent_tick: u64,
        payload: P,
    ) -> Result<MessageId, SendError<P>> {
        let reason = if !self.members.contains_key(&sender) {
            Some(ChannelError::NotMember)
        } else if self.messages.len() == self.capacity {
            Some(ChannelError::Full)
        } else if self.sequence == u64::MAX {
            Some(ChannelError::SequenceExhausted)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(SendError { reason, payload });
        }
        self.sequence += 1;
        let id = MessageId(self.sequence);
        self.messages.push_back(Arc::new(Envelope::new(
            id,
            Report {
                channel: self.id,
                sender,
                sent_tick,
                payload,
            },
        )));
        Ok(id)
    }

    /// Deliver at most `limit` reports in publication order; zero is a no-op.
    /// Payloads are not copied and another member's cursor is never advanced.
    pub fn poll(
        &mut self,
        member: u64,
        limit: usize,
    ) -> Result<Vec<SharedReport<P>>, ChannelError> {
        let cursor = self
            .members
            .get_mut(&member)
            .ok_or(ChannelError::NotMember)?;
        // Retained message IDs are contiguous; skip directly to this member's unread range.
        let skip = self
            .messages
            .front()
            .map_or(0, |first| cursor.saturating_sub(first.id.0 - 1) as usize);
        let reports: Vec<_> = self
            .messages
            .iter()
            .skip(skip)
            .take(limit)
            .cloned()
            .collect();
        if let Some(last) = reports.last() {
            *cursor = last.id.0;
            self.reclaim();
        }
        Ok(reports)
    }

    fn reclaim(&mut self) {
        let consumed = self
            .members
            .values()
            .copied()
            .min()
            .unwrap_or(self.sequence);
        while self.messages.front().is_some_and(|m| m.id.0 <= consumed) {
            self.messages.pop_front();
        }
    }
}

#[cfg(test)]
mod tests;
