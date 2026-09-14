//! In-process message contracts, not a stable C or network representation.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope<P> {
    pub id: MessageId,
    pub payload: P,
}

impl<P> Envelope<P> {
    pub const fn new(id: MessageId, payload: P) -> Self {
        Self { id, payload }
    }

    pub fn reply<R>(&self, payload: R) -> Envelope<R> {
        Envelope::new(self.id, payload)
    }
}
