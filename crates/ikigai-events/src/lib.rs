//! The identifiers ikigai's event-facing modules share.
//!
//! A scaffold. The design is ikigai-core's `docs/design/events-and-resources.md`: state is a
//! resource, a fact about a change is an immutable entry, and a stream is a representation body.
//! This crate will hold what every publisher and consumer must name the same way (entries,
//! feeds, cursors, feed epochs, transitions, idempotency and fencing keys, and the mapping to a
//! CloudEvent's attributes). Specific bridges live beside it as subcrates that depend on it.
#![forbid(unsafe_code)]
