//! The relay: where its atoms live, and the step that delivers one entry and records it.

use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ikigai_core::{Capability, Kernel};
use ikigai_events::{Cursor, Refusal};
use ikigai_events_cloudevents::{to_json, Event};

use crate::deliver::{attempt, Attempted, Delivery, Outcome, Policy};
use crate::subscription::Subscription;
use crate::SubscriptionId;

/// Where a relay's atoms live: each subscription's cursor, its delivery records and its dead
/// letters. A host implements it over its own store; [`MemoryStore`] holds them in memory.
#[async_trait]
pub trait Store: Send + Sync {
    /// The subscription's cursor, `None` before its first entry is done with.
    async fn cursor(&self, id: &SubscriptionId) -> Result<Option<Cursor>, ikigai_core::Error>;

    /// The delivery record of the entry at `seq`, when one was written.
    async fn delivery(
        &self,
        id: &SubscriptionId,
        seq: NonZeroU64,
    ) -> Result<Option<Delivery>, ikigai_core::Error>;

    /// Write the delivery record and, when given, the cursor after it, IN ONE UPDATE: a
    /// cursor is never past an entry whose record is not written.
    async fn commit(
        &self,
        delivery: &Delivery,
        cursor: Option<&Cursor>,
    ) -> Result<(), ikigai_core::Error>;

    /// Offer the entry to the relay's dead-letter tuplespace (a layer a person takes from, to
    /// retry or drop it): its record and its JSON event, which is data, never an instruction.
    /// Called before the record and the cursor are committed, so a crash between the two
    /// offers it again: a dead letter is at least once, like every delivery.
    async fn dead_letter(
        &self,
        delivery: &Delivery,
        event: &[u8],
    ) -> Result<(), ikigai_core::Error>;
}

/// A [`Store`] in memory: for tests, and for a host that keeps no relay state across a
/// restart.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<Memory>,
}

#[derive(Debug, Default)]
struct Memory {
    cursors: BTreeMap<SubscriptionId, Cursor>,
    deliveries: BTreeMap<(SubscriptionId, NonZeroU64), Delivery>,
    dead: Vec<(Delivery, Vec<u8>)>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        MemoryStore::default()
    }

    /// Every dead letter offered, in order: its record and its JSON event.
    pub fn dead_letters(&self) -> Vec<(Delivery, Vec<u8>)> {
        self.lock().dead.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Memory> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn cursor(&self, id: &SubscriptionId) -> Result<Option<Cursor>, ikigai_core::Error> {
        Ok(self.lock().cursors.get(id).cloned())
    }

    async fn delivery(
        &self,
        id: &SubscriptionId,
        seq: NonZeroU64,
    ) -> Result<Option<Delivery>, ikigai_core::Error> {
        Ok(self.lock().deliveries.get(&(id.clone(), seq)).cloned())
    }

    async fn commit(
        &self,
        delivery: &Delivery,
        cursor: Option<&Cursor>,
    ) -> Result<(), ikigai_core::Error> {
        let mut memory = self.lock();
        memory.deliveries.insert(
            (delivery.subscription.clone(), delivery.seq),
            delivery.clone(),
        );
        if let Some(cursor) = cursor {
            memory
                .cursors
                .insert(delivery.subscription.clone(), cursor.clone());
        }
        Ok(())
    }

    async fn dead_letter(
        &self,
        delivery: &Delivery,
        event: &[u8],
    ) -> Result<(), ikigai_core::Error> {
        self.lock().dead.push((delivery.clone(), event.to_vec()));
        Ok(())
    }
}

/// Why a step delivered nothing and recorded nothing.
#[derive(Debug)]
#[non_exhaustive]
pub enum StepError {
    /// The event is not the subscription's next entry, or the subscription cannot be
    /// delivered to: one sentence saying what was wrong and what to do.
    Refused(Refusal),
    /// The store could not be read or written.
    Store(ikigai_core::Error),
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::Refused(refusal) => refusal.fmt(f),
            StepError::Store(error) => write!(f, "the relay's store failed: {error}"),
        }
    }
}

impl std::error::Error for StepError {}

impl From<Refusal> for StepError {
    fn from(refusal: Refusal) -> Self {
        StepError::Refused(refusal)
    }
}

impl From<ikigai_core::Error> for StepError {
    fn from(error: ikigai_core::Error) -> Self {
        StepError::Store(error)
    }
}

/// A relay: the kernel it delivers through, its own capability (every subscription's grant
/// is narrowed from it), and its retry policy.
///
/// It reads no feed and keeps no timer. Its host follows each subscription's feed
/// (`source {feed} after={cursor} wait={s}`), lowers each entry to a CloudEvent, and calls
/// [`Relay::step`] for the entry after the cursor; when the step answers
/// [`Outcome::Retry`], the host waits that long and steps the same entry again.
pub struct Relay {
    pub(crate) kernel: Arc<Kernel>,
    pub(crate) capability: Capability,
    pub(crate) policy: Policy,
}

impl Relay {
    /// A relay delivering through `kernel` under `capability`, with the default [`Policy`].
    pub fn new(kernel: Arc<Kernel>, capability: Capability) -> Self {
        Relay {
            kernel,
            capability,
            policy: Policy::default(),
        }
    }

    /// The same relay with `policy`.
    pub fn with_policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    /// Deliver `event`, the subscription's next entry, once, and record what became of it.
    ///
    /// The cursor is read from `store` (a subscription with none starts before the first
    /// entry of the event's epoch). One attempt is made, numbered after the attempts the
    /// entry's record already holds, at `now` (seconds since the Unix epoch). Then:
    ///
    /// - delivered or skipped: the record and the cursor past the entry, in one update;
    /// - dead-lettered: the dead letter first, then the record and the cursor past it;
    /// - retry or stopped: the record alone, so the cursor stays on the owed entry.
    ///
    /// The cursor is written only after the destination acknowledges, so a crash between the
    /// two delivers the entry again; the destination takes it once by its `webhook-id` or
    /// `idempotency-key=`. Delivery is at least once, never exactly once.
    pub async fn step(
        &self,
        store: &dyn Store,
        sub: &Subscription,
        event: &Event,
        now: i64,
    ) -> Result<Delivery, StepError> {
        let cursor = match store.cursor(&sub.id).await? {
            Some(cursor) => cursor,
            None => Cursor::start(event.check()?.advisory.and_then(|c| c.epoch().cloned())),
        };
        let seq = crate::deliver::position(sub, &cursor, event)?;
        let before = store
            .delivery(&sub.id, seq)
            .await?
            .map_or(0, |record| record.attempts);
        let Attempted { delivery, cursor } =
            attempt(self, sub, &cursor, event, before.saturating_add(1), now).await?;
        if delivery.outcome == Outcome::DeadLettered {
            store.dead_letter(&delivery, &to_json(event)).await?;
        }
        store.commit(&delivery, cursor.as_ref()).await?;
        Ok(delivery)
    }
}
