//! Delivery out of ikigai (ledger #4118): a relay follows a feed for each subscription and
//! delivers each entry, as a CloudEvent, to a webhook, to a connector process, or through a
//! server-sent events face.
//!
//! The design is ikigai-core's `docs/design/events-and-resources.md`, section 9 item 7, with
//! sections 5.4 (a subscription is a narrowed grant, rechecked at each delivery), 6.2 (the
//! cursor advances only after the destination acknowledges), 6.3 (Standard Webhooks
//! signatures, retries with backoff, a dead-letter tuplespace) and 6.4 (an SSE `id` is the
//! cursor, and `Last-Event-ID` resumes from it).
//!
//! | name | kind | type |
//! |---|---|---|
//! | `urn:relay:subscription:id:{id}` | ATOM: what to follow, a filter, a destination, a narrowed grant | [`Subscription`] |
//! | `urn:relay:cursor:subscription:{id}` | ATOM: written after the destination acknowledges | [`Cursor`](ikigai_events::Cursor), in a [`Store`] |
//! | `urn:relay:delivery:subscription:{id}:seq:{n}` | ATOM: attempts and outcome | [`Delivery`] |
//! | dead letters | a tuplespace layer per relay | [`Store::dead_letter`] |
//! | `urn:relay:status:subscription:{id}` | composition, uncacheable | named only ([`SubscriptionId::status`]) |
//!
//! Every delivery is a request through the kernel under the subscription's grant: a webhook
//! is a Sink to ikigai-http's `urn:http:post`, a broker is a Sink to the connector process
//! mounted at its name. The relay links no HTTP client and no broker client. Nothing here
//! promises exactly once: an entry is delivered at least once, and its destination takes it
//! once by its event key, which is the `webhook-id` and the `idempotency-key=`.
//!
//! A golden-thread cut is never delivered: a relay follows a feed of entries, and an entry is
//! the only thing it sends. A signature says the delivery came from this relay; it carries
//! no authority, and the receiving door decides what the delivery may do.
//!
//! ```
//! use ikigai_events_relay::{Destination, Filter, Subscription, SubscriptionId};
//! use ikigai_events::Feed;
//!
//! let sub = Subscription {
//!     id: SubscriptionId::parse("talk-to-ops").unwrap(),
//!     follow: Feed::parse("urn:layer:talk").unwrap(),
//!     filter: Filter::types(["say"]),
//!     destination: Destination::webhook("https://hooks.example.com/in", "urn:secret:ops-hook")
//!         .unwrap(),
//!     grant: ["urn:cap:net:hooks.example.com", "urn:cap:kernel:cut", "urn:cap:secret:ops-hook"]
//!         .map(String::from)
//!         .into(),
//! };
//! assert_eq!(sub.id.cursor(), "urn:relay:cursor:subscription:talk-to-ops");
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod deliver;
mod name;
mod relay;
mod sse;
mod subscription;
mod webhook;

pub use deliver::{position, Attempted, Delivery, Outcome, Policy, HTTP_POST};
pub use name::{SubscriptionId, MAX_SUBSCRIPTION_ID};
pub use relay::{MemoryStore, Relay, StepError, Store};
pub use sse::{sse_frame, sse_resume, EVENT_STREAM};
pub use subscription::{Destination, Filter, Subscription};
pub use webhook::{
    sign, Signed, HEADER_WEBHOOK_ID, HEADER_WEBHOOK_SIGNATURE, HEADER_WEBHOOK_TIMESTAMP,
};
