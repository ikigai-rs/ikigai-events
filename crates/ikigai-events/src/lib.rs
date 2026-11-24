//! The identifiers ikigai's event-facing modules share.
//!
//! The design is ikigai-core's `docs/design/events-and-resources.md`: state is a resource, a
//! fact about a change is an immutable ENTRY, and a log is the set of those entries read in
//! order by a cursor. A golden-thread cut is a hint to re-read, never an event, so nothing here
//! names one. Ordering belongs to the log that owns the entries, and effects are made safe by
//! idempotency and fencing keys, never by a promise of exactly-once delivery.
//!
//! This crate holds only what every publisher and consumer must spell the same way:
//!
//! | what | name or form | type |
//! |---|---|---|
//! | a feed | source `urn:layer:{layer}`, read at `urn:layer:{layer}:feed`; `urn:iki:ledger:{ledger}:transition` | [`Feed`] |
//! | an entry | `urn:layer:{layer}:entry:{id}`; `urn:iki:ledger:{ledger}:transition:seq:{n}` | [`Entry`] |
//! | a feed epoch | `[a-z0-9-]{1,64}` | [`FeedEpoch`] |
//! | a cursor's value | `{epoch}:{after}`, or `{after}` | [`Cursor`] |
//! | a listener | `urn:kernel:listener:id:{id}` | [`Listener`] |
//! | an event's idempotency key | `event:b3:{hex}`, from (source, id) | [`event_key`] |
//! | a fencing key | `fence:{ledger}:{item}:{epoch}:{step}` | [`FenceKey`] |
//! | the CloudEvents mapping | attribute ↔ ikigai term | [`cloudevents`] |
//!
//! Every constructor and parser refuses a malformed name with one sentence ([`Refusal`]); no
//! name is percent-encoded, so each has one spelling. The shared vectors in `tests/vectors/`
//! pin every output for the Python, TypeScript and .NET faces.
//!
//! ```
//! use ikigai_events::{event_key, Entry};
//!
//! let entry = Entry::parse("urn:layer:talk:entry:01m84z0jyt0000000000001").unwrap();
//! let key = event_key(&entry.feed().source(), &entry.iri()).unwrap();
//! assert!(key.as_str().starts_with("event:b3:"));
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cloudevents;
mod feed;
mod key;
mod name;

pub use feed::{Cursor, Entry, Feed, Listener};
pub use key::{event_key, FenceKey, EVENT_KEY_DOMAIN, EVENT_KEY_PREFIX, FENCE_KEY_PREFIX};
pub use name::{
    EntryId, FeedEpoch, ItemRef, LayerName, LedgerName, ListenerId, Refusal, Step, MAX_ENTRY_ID,
    MAX_EPOCH, MAX_ITEM, MAX_LAYER_NAME, MAX_LEDGER_NAME, MAX_LISTENER_ID, MAX_STEP, SHOWN,
};
