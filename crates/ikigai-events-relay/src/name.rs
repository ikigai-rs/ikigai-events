//! The relay's names (the design note's resource map, section 11): a subscription, its
//! cursor, its deliveries and its status, each partitioned by the subscription's id.

use std::fmt;
use std::num::NonZeroU64;

use ikigai_events::{shown, Refusal};

const SUBSCRIPTION: &str = "urn:relay:subscription:id:";
const CURSOR: &str = "urn:relay:cursor:subscription:";
const DELIVERY: &str = "urn:relay:delivery:subscription:";
const STATUS: &str = "urn:relay:status:subscription:";

/// The longest subscription id.
pub const MAX_SUBSCRIPTION_ID: usize = 64;

/// A subscription's id: 1 to [`MAX_SUBSCRIPTION_ID`] lowercase letters, digits and hyphens,
/// not starting or ending with a hyphen. It holds no `:`, so it is one IRI segment and never
/// spells a boundary; an id outside the alphabet is refused, never percent-encoded.
///
/// ```
/// use ikigai_events_relay::SubscriptionId;
///
/// let id = SubscriptionId::parse("talk-to-ops").unwrap();
/// assert_eq!(id.subscription(), "urn:relay:subscription:id:talk-to-ops");
/// assert_eq!(id.cursor(), "urn:relay:cursor:subscription:talk-to-ops");
/// assert_eq!(id.status(), "urn:relay:status:subscription:talk-to-ops");
/// assert_eq!(
///     id.delivery(std::num::NonZeroU64::new(7).unwrap()),
///     "urn:relay:delivery:subscription:talk-to-ops:seq:7"
/// );
/// assert!(SubscriptionId::parse("a:b").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriptionId(String);

impl SubscriptionId {
    /// Check `s` and keep it, or refuse it with one sentence.
    pub fn parse(s: &str) -> Result<Self, Refusal> {
        let ok = !s.is_empty()
            && s.len() <= MAX_SUBSCRIPTION_ID
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !s.starts_with('-')
            && !s.ends_with('-');
        if ok {
            Ok(SubscriptionId(s.to_string()))
        } else {
            Err(Refusal::new(format!(
                "{} is not a subscription id: use 1 to {MAX_SUBSCRIPTION_ID} lowercase \
                 letters, digits and hyphens, not starting or ending with a hyphen.",
                shown(s)
            )))
        }
    }

    /// Read a subscription's IRI, `urn:relay:subscription:id:{id}`.
    pub fn from_iri(iri: &str) -> Result<Self, Refusal> {
        let id = iri.strip_prefix(SUBSCRIPTION).ok_or_else(|| {
            Refusal::new(format!(
                "{} is not a subscription: a subscription is \
                 `urn:relay:subscription:id:{{id}}`.",
                shown(iri)
            ))
        })?;
        SubscriptionId::parse(id)
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The subscription's atom: what to follow, a filter, a destination, a narrowed grant.
    pub fn subscription(&self) -> String {
        format!("{SUBSCRIPTION}{}", self.0)
    }

    /// The subscription's cursor atom, written after the destination acknowledges.
    pub fn cursor(&self) -> String {
        format!("{CURSOR}{}", self.0)
    }

    /// The delivery record of the entry at feed position `seq`: its attempts and outcome.
    pub fn delivery(&self, seq: NonZeroU64) -> String {
        format!("{DELIVERY}{}:seq:{seq}", self.0)
    }

    /// The subscription's status: lag, last delivery, failures. A composition, uncacheable.
    pub fn status(&self) -> String {
        format!("{STATUS}{}", self.0)
    }
}

impl fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_has_one_spelling() {
        for good in ["a", "talk-to-ops", "0", &"x".repeat(64)] {
            assert!(SubscriptionId::parse(good).is_ok(), "{good}");
        }
        for bad in ["", "-a", "a-", "A", "a:b", "a b", "a_b", &"x".repeat(65)] {
            assert!(SubscriptionId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_subscription_iri_reads_back() {
        let id = SubscriptionId::from_iri("urn:relay:subscription:id:ops").unwrap();
        assert_eq!(id.subscription(), "urn:relay:subscription:id:ops");
        assert!(SubscriptionId::from_iri("urn:relay:subscription:id:ops:x").is_err());
        assert!(SubscriptionId::from_iri("urn:relay:cursor:subscription:ops").is_err());
    }
}
