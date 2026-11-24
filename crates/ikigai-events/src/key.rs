//! The two keys that make an effect safe to repeat, instead of a promise of exactly-once
//! delivery: the idempotency key of an event, and the fencing key of a leased effect.
//!
//! Both are [`IdempotencyKey`]s, core's token for `idempotency-key=`, so a keyed Sink takes
//! either one as it is.

use std::fmt;

use ikigai_core::{ContentHasher, IdempotencyKey};

use crate::name::{parse_u64, shown, ItemRef, LedgerName, Refusal, Step};

/// The domain an event key's digest is taken in, so no other digest of the same fields can
/// equal it.
pub const EVENT_KEY_DOMAIN: &str = "ikigai-events:event-key:v1";

/// The prefix of every event key.
pub const EVENT_KEY_PREFIX: &str = "event:";

/// The prefix of every fencing key.
pub const FENCE_KEY_PREFIX: &str = "fence:";

/// Feed one field to the digest: its length in bytes as a little-endian `u64`, then its
/// bytes, so two field sequences can never read as one (the framing core's own identity
/// digests use).
fn feed(h: &mut ContentHasher, field: &[u8]) {
    h.update(&(field.len() as u64).to_le_bytes());
    h.update(field);
}

/// The idempotency key of an event, from its `source` and `id` (a CloudEvent's two
/// identifying attributes, unique together): `event:` and core's content id of the three
/// framed fields [`EVENT_KEY_DOMAIN`], `source` and `id`.
///
/// A redelivered event derives the same key, so a keyed Sink answers it with its first answer
/// and writes nothing. The same key is what a receiver of an ikigai entry derives from the
/// CloudEvent the entry becomes (`source` the feed's, `id` the entry's IRI).
///
/// The digest is BLAKE3 over, in order, for each of the three fields: its length in bytes as
/// a little-endian 64-bit integer, then its UTF-8 bytes. The key is `event:b3:` and the 64
/// lowercase hex digits of the digest.
///
/// ```
/// use ikigai_events::event_key;
///
/// let key = event_key("urn:layer:talk", "urn:layer:talk:entry:1").unwrap();
/// assert!(key.as_str().starts_with("event:b3:"));
/// assert_eq!(key.as_str().len(), "event:b3:".len() + 64);
/// assert_ne!(key, event_key("urn:layer:tal", "kurn:layer:talk:entry:1").unwrap());
/// assert!(event_key("", "1").is_err());
/// ```
pub fn event_key(source: &str, id: &str) -> Result<IdempotencyKey, Refusal> {
    if source.is_empty() {
        return Err(Refusal::new(
            "An event's `source` is empty: name where the event came from.",
        ));
    }
    if id.is_empty() {
        return Err(Refusal::new(
            "An event's `id` is empty: name the event within its source.",
        ));
    }
    let mut h = ContentHasher::new();
    feed(&mut h, EVENT_KEY_DOMAIN.as_bytes());
    feed(&mut h, source.as_bytes());
    feed(&mut h, id.as_bytes());
    let text = format!("{EVENT_KEY_PREFIX}{}", h.finish());
    // Always fits: 73 characters of core's key alphabet.
    IdempotencyKey::parse(&text).map_err(|e| Refusal::new(e.to_string()))
}

/// The fencing key of one step of an effect done under a ledger claim:
/// `fence:{ledger}:{item}:{epoch}:{step}`, `epoch` the claim's `ledger:claimEpoch`.
///
/// A stale holder's retry and the current holder's attempt carry different epochs, so a
/// receiver that keeps keys can never take one for the other; and whoever accepts the effect
/// later (a merge, a close) reads the epoch back from the key and checks it is still current.
/// The key is legible on purpose: it is recorded in what the effect leaves behind.
///
/// ```
/// use ikigai_events::FenceKey;
///
/// let key = FenceKey::new("default", "4126", 3, "open-pr").unwrap();
/// assert_eq!(key.to_string(), "fence:default:4126:3:open-pr");
/// assert_eq!(FenceKey::parse("fence:default:4126:3:open-pr").unwrap(), key);
/// assert_eq!(key.idempotency_key().as_str(), "fence:default:4126:3:open-pr");
/// assert!(FenceKey::parse("default:4126:3:open-pr").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FenceKey {
    ledger: LedgerName,
    item: ItemRef,
    epoch: u64,
    step: Step,
}

impl FenceKey {
    /// The key of `step` under the claim on `item` in `ledger` at `epoch`.
    pub fn new(ledger: &str, item: &str, epoch: u64, step: &str) -> Result<FenceKey, Refusal> {
        Ok(FenceKey {
            ledger: LedgerName::parse(ledger)?,
            item: ItemRef::parse(item)?,
            epoch,
            step: Step::parse(step)?,
        })
    }

    /// The ledger.
    pub fn ledger(&self) -> &LedgerName {
        &self.ledger
    }

    /// The item.
    pub fn item(&self) -> &ItemRef {
        &self.item
    }

    /// The claim's epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The step.
    pub fn step(&self) -> &Step {
        &self.step
    }

    /// The key as core's idempotency token.
    pub fn idempotency_key(&self) -> IdempotencyKey {
        // Always fits: at most 221 characters, every one in core's key alphabet.
        IdempotencyKey::parse(&self.to_string())
            .expect("a fencing key is built from tokens inside the idempotency-key alphabet")
    }

    /// Read a fencing key's text.
    pub fn parse(text: &str) -> Result<FenceKey, Refusal> {
        let refuse = || {
            Refusal::new(format!(
                "{} is not a fencing key: a fencing key is \
                 `fence:{{ledger}}:{{item}}:{{epoch}}:{{step}}`.",
                shown(text)
            ))
        };
        let rest = text.strip_prefix(FENCE_KEY_PREFIX).ok_or_else(refuse)?;
        let parts: Vec<&str> = rest.split(':').collect();
        let [ledger, item, epoch, step] = parts[..] else {
            return Err(refuse());
        };
        Ok(FenceKey {
            ledger: LedgerName::parse(ledger)?,
            item: ItemRef::parse(item)?,
            epoch: parse_u64(epoch, "a claim epoch")?,
            step: Step::parse(step)?,
        })
    }
}

impl fmt::Display for FenceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{FENCE_KEY_PREFIX}{}:{}:{}:{}",
            self.ledger, self.item, self.epoch, self.step
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::{MAX_ITEM, MAX_LEDGER_NAME, MAX_STEP};

    #[test]
    fn the_longest_fencing_key_is_an_idempotency_key() {
        let key = FenceKey::new(
            &"a".repeat(MAX_LEDGER_NAME),
            &"1".repeat(MAX_ITEM),
            u64::MAX,
            &"~".repeat(MAX_STEP),
        )
        .unwrap();
        assert!(key.to_string().len() <= ikigai_core::idempotency::MAX_LEN);
        assert_eq!(key.idempotency_key().as_str(), key.to_string());
    }

    #[test]
    fn an_event_key_frames_its_fields() {
        let a = event_key("ab", "c").unwrap();
        let b = event_key("a", "bc").unwrap();
        assert_ne!(a, b);
        assert_eq!(a, event_key("ab", "c").unwrap());
    }
}
