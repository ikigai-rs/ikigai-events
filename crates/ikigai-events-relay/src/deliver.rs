//! One delivery: an entry, as a CloudEvent, to a subscription's destination, through the
//! kernel, under the subscription's narrowed grant.

use std::num::NonZeroU64;
use std::time::Duration;

use ikigai_core::idempotency::IDEMPOTENCY_KEY_ARG;
use ikigai_core::{
    ArgRef, Capability, Error, FailureClass, IdempotencyKey, Iri, Kernel, Request, Verb,
};
use ikigai_events::{shown, Cursor, Refusal};
use ikigai_events_cloudevents::{to_json, Event, CLOUDEVENTS_JSON};

use crate::subscription::{Destination, Subscription};
use crate::webhook::sign;
use crate::{Relay, SubscriptionId};

/// ikigai-http's POST, through which a webhook is delivered.
pub const HTTP_POST: &str = "urn:http:post";

/// How long a failing delivery is retried before its entry is dead-lettered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// The most attempts an entry gets, the first included.
    pub max_attempts: u32,
    /// The wait after the first failure; each later one doubles it.
    pub first_backoff: Duration,
    /// The longest wait between two attempts.
    pub max_backoff: Duration,
}

impl Default for Policy {
    /// Eight attempts, waiting 5 s, 10 s, 20 s and so on, at most an hour.
    fn default() -> Self {
        Policy {
            max_attempts: 8,
            first_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(3600),
        }
    }
}

impl Policy {
    /// The wait after `attempts` failed attempts (from 1): `first_backoff` doubled each
    /// time, at most `max_backoff`.
    ///
    /// ```
    /// use std::time::Duration;
    /// use ikigai_events_relay::Policy;
    ///
    /// let p = Policy::default();
    /// assert_eq!(p.backoff(1), Duration::from_secs(5));
    /// assert_eq!(p.backoff(3), Duration::from_secs(20));
    /// assert_eq!(p.backoff(40), Duration::from_secs(3600));
    /// ```
    pub fn backoff(&self, attempts: u32) -> Duration {
        let doublings = attempts.saturating_sub(1).min(31);
        self.first_backoff
            .saturating_mul(1u32 << doublings)
            .min(self.max_backoff)
    }
}

/// What became of an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The destination acknowledged it: the cursor moves past it.
    Delivered,
    /// The filter passed over it: the cursor moves past it, nothing was sent.
    Skipped,
    /// The attempt failed in a way that may pass later: try again after the wait. The
    /// cursor stays.
    Retry {
        /// How long to wait: the destination's `Retry-After` when it gave one, else the
        /// policy's backoff.
        after: Duration,
    },
    /// The attempts ran out, or the destination refused it for good: it goes to the
    /// dead-letter tuplespace for a person to take, and the cursor moves past it.
    DeadLettered,
    /// The grant does not cover the delivery (`Denied`): the subscription is stopped and the
    /// cursor stays, so nothing is delivered until the grant covers it again.
    Stopped,
}

/// A delivery record, the content of the atom
/// `urn:relay:delivery:subscription:{id}:seq:{n}`: one entry's attempts and outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    /// The subscription.
    pub subscription: SubscriptionId,
    /// The entry's feed position.
    pub seq: NonZeroU64,
    /// The entry's IRI, the event's `id`.
    pub entry: String,
    /// The event key: the delivery's `webhook-id` and `idempotency-key=`.
    pub key: IdempotencyKey,
    /// Attempts made so far, this one included; 0 when the filter skipped the entry, and a
    /// stopped attempt is not counted.
    pub attempts: u32,
    /// What became of it.
    pub outcome: Outcome,
    /// The last failure, when an attempt failed: its class and its message, escaped.
    pub failure: Option<(FailureClass, String)>,
}

impl Delivery {
    /// The record's atom.
    pub fn iri(&self) -> String {
        self.subscription.delivery(self.seq)
    }
}

/// One attempt's result: the record to write and, when the entry is done with, the cursor to
/// write after it in the same update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempted {
    /// The delivery record.
    pub delivery: Delivery,
    /// The cursor past the entry, for [`Outcome::Delivered`], [`Outcome::Skipped`] and
    /// [`Outcome::DeadLettered`]; `None` while the entry is still owed.
    pub cursor: Option<Cursor>,
}

/// The entry's position, checked against the subscription and its cursor: the event comes
/// from the feed the subscription follows, in that feed's epoch, at the position right after
/// the cursor. Only the feed assigns positions; a relay delivers them in order, one at a time.
pub fn position(sub: &Subscription, cursor: &Cursor, event: &Event) -> Result<NonZeroU64, Refusal> {
    let intake = event.check()?;
    if intake.feed.as_ref() != Some(&sub.follow) {
        return Err(Refusal::new(format!(
            "{} is not the feed `{}` follows: deliver only its own feed's entries.",
            shown(&event.attributes.source),
            sub.id.subscription()
        )));
    }
    let at = intake.advisory.ok_or_else(|| {
        Refusal::new(format!(
            "The event {} carries no `ikseq`: lower the entry with its feed position.",
            shown(&event.attributes.id)
        ))
    })?;
    if at.epoch() != cursor.epoch() {
        return Err(Refusal::new(format!(
            "The event {} is from another epoch of the feed than the cursor `{cursor}`: the \
             feed was restored, so resynchronize the subscription before delivering.",
            shown(&event.attributes.id)
        )));
    }
    match NonZeroU64::new(at.after()) {
        Some(seq) if Some(seq.get()) == cursor.after().checked_add(1) => Ok(seq),
        _ => Err(Refusal::new(format!(
            "The event {} is at position {}, not {}: deliver the feed's entries in order, \
             the one after the cursor `{cursor}` next.",
            shown(&event.attributes.id),
            at.after(),
            cursor.after().saturating_add(1)
        ))),
    }
}

/// Make one attempt to deliver `event` (the next entry after `cursor`) to the subscription's
/// destination, as attempt number `attempt` (from 1), at `now` (seconds since the Unix epoch,
/// the signature's timestamp).
///
/// Every effect is a request through `kernel` under [`Subscription::capability`]: the
/// webhook's secret is read, and the POST is made, under the narrowed grant, so a scope
/// withdrawn since the last attempt is [`Outcome::Stopped`] at this one. Delivery is at least
/// once: a destination that answered but whose answer was lost sees the entry again, under the
/// same `webhook-id` or `idempotency-key=`, and must take it once.
///
/// The event is refused, with nothing sent, when it is not the next entry of the followed
/// feed ([`position`]) or when the destination is [`Destination::Sse`], which is read, not
/// pushed.
pub(crate) async fn attempt(
    relay: &Relay,
    sub: &Subscription,
    cursor: &Cursor,
    event: &Event,
    attempt: u32,
    now: i64,
) -> Result<Attempted, Refusal> {
    let (kernel, policy) = (relay.kernel.as_ref(), &relay.policy);
    let seq = position(sub, cursor, event)?;
    let key = event.check()?.key;
    let record = |attempts, outcome, failure| Delivery {
        subscription: sub.id.clone(),
        seq,
        entry: event.attributes.id.clone(),
        key: key.clone(),
        attempts,
        outcome,
        failure,
    };
    let past = Some(cursor.advanced(seq));
    if !sub.filter.passes(event) {
        return Ok(Attempted {
            delivery: record(0, Outcome::Skipped, None),
            cursor: past,
        });
    }
    let attempt = attempt.max(1);
    let body = to_json(event);
    let cap = sub.capability(&relay.capability);
    let sent = match &sub.destination {
        Destination::Webhook { url, secret } => {
            webhook(kernel, &cap, url, secret, &key, now, body).await?
        }
        Destination::Connector { target } => {
            let request = Request::new(Verb::Sink, iri(target)?)
                .with_arg("content", ArgRef::Inline(body))
                .with_arg(
                    "content_type",
                    ArgRef::Inline(CLOUDEVENTS_JSON.as_bytes().to_vec()),
                )
                .with_arg(
                    IDEMPOTENCY_KEY_ARG,
                    ArgRef::Inline(key.as_str().as_bytes().to_vec()),
                );
            kernel.issue(request, &cap).await.map(drop)
        }
        Destination::Sse => {
            return Err(Refusal::new(format!(
                "`{}` delivers to a server-sent events face, which a door reads: nothing is \
                 pushed to it.",
                sub.id.subscription()
            )))
        }
    };
    Ok(match sent {
        Ok(()) => Attempted {
            delivery: record(attempt, Outcome::Delivered, None),
            cursor: past,
        },
        Err(error) => {
            let class = error.class();
            let failure = Some((class, error.to_string().escape_debug().to_string()));
            let outcome = match class {
                FailureClass::Denied => Outcome::Stopped,
                FailureClass::Transient | FailureClass::RetryAfter | FailureClass::Unknown
                    if attempt < policy.max_attempts =>
                {
                    let after = match &error {
                        Error::RetryAfter {
                            delay: Some(delay), ..
                        } => *delay,
                        _ => policy.backoff(attempt),
                    };
                    Outcome::Retry { after }
                }
                _ => Outcome::DeadLettered,
            };
            let cursor = (outcome == Outcome::DeadLettered).then(|| cursor.advanced(seq));
            // A stop reached no destination, so it spends none of the entry's attempts: a
            // grant restored later gets the policy's full count.
            let attempts = if outcome == Outcome::Stopped {
                attempt - 1
            } else {
                attempt
            };
            Attempted {
                delivery: record(attempts, outcome, failure),
                cursor,
            }
        }
    })
}

fn iri(name: &str) -> Result<Iri, Refusal> {
    Iri::parse(name).map_err(|e| {
        Refusal::new(format!(
            "{} is not an IRI the kernel takes: {}.",
            shown(name),
            e.to_string().escape_debug()
        ))
    })
}

async fn webhook(
    kernel: &Kernel,
    cap: &Capability,
    url: &str,
    secret: &str,
    key: &IdempotencyKey,
    now: i64,
    body: Vec<u8>,
) -> Result<Result<(), Error>, Refusal> {
    let read = Request::new(Verb::Source, iri(secret)?);
    let secret = match kernel.issue(read, cap).await {
        Ok(answer) => answer.bytes,
        Err(error) => return Ok(Err(error)),
    };
    let secret = std::str::from_utf8(&secret).map_err(|_| {
        Refusal::new(
            "The webhook's secret is not UTF-8: store `whsec_` followed by the key in base64.",
        )
    })?;
    let signed = sign(secret, key, now, &body)?;
    let post = Request::new(Verb::Sink, iri(HTTP_POST)?)
        .with_arg("url", ArgRef::Inline(url.as_bytes().to_vec()))
        .with_arg(
            "content_type",
            ArgRef::Inline(CLOUDEVENTS_JSON.as_bytes().to_vec()),
        )
        .with_arg(
            "headers",
            ArgRef::Inline(signed.header_block().into_bytes()),
        )
        .with_arg("content", ArgRef::Inline(body));
    Ok(kernel.issue(post, cap).await.map(drop))
}
