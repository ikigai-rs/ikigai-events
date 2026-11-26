//! A subscription: what to follow, a filter, a destination and a narrowed grant.

use std::collections::BTreeSet;

use ikigai_core::Capability;
use ikigai_events::{shown, Feed, Refusal};
use ikigai_events_cloudevents::Event;

use crate::SubscriptionId;

/// One subscription, the content of its atom `urn:relay:subscription:id:{id}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    /// Its id, which names its cursor, its deliveries and its status.
    pub id: SubscriptionId,
    /// The feed it follows.
    pub follow: Feed,
    /// Which of the feed's entries it delivers.
    pub filter: Filter,
    /// Where it delivers them.
    pub destination: Destination,
    /// The scopes it is granted. Every delivery runs under the relay's own capability
    /// attenuated to these ([`Subscription::capability`]), so a subscription can never act
    /// with more than both allow, and a scope withdrawn from either stops its delivery at
    /// the next attempt.
    pub grant: BTreeSet<String>,
}

impl Subscription {
    /// The capability a delivery runs under: `relay`, attenuated to the grant. Never wider
    /// than `relay`: there is no operation that widens a capability.
    ///
    /// ```
    /// use ikigai_core::Capability;
    /// use ikigai_events::Feed;
    /// use ikigai_events_relay::{Destination, Filter, Subscription, SubscriptionId};
    ///
    /// let sub = Subscription {
    ///     id: SubscriptionId::parse("ops").unwrap(),
    ///     follow: Feed::parse("urn:layer:talk").unwrap(),
    ///     filter: Filter::all(),
    ///     destination: Destination::Sse,
    ///     grant: ["urn:cap:net:hooks.example.com".to_string()].into(),
    /// };
    /// let cap = sub.capability(&Capability::scoped(["urn:cap:net:hooks.example.com"]));
    /// assert!(cap.allows("urn:cap:net:hooks.example.com"));
    /// assert!(!sub.capability(&Capability::scoped(["urn:cap:other"])).allows("urn:cap:net:hooks.example.com"));
    /// ```
    pub fn capability(&self, relay: &Capability) -> Capability {
        relay.attenuate(self.grant.iter().cloned())
    }
}

/// Which entries a subscription delivers: by the CloudEvent `type` each carries.
///
/// The filter is a set of exact values; an empty set delivers every entry. An entry the
/// filter passes over is still acknowledged: the cursor moves past it, with a delivery record
/// that says it was skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// The `type` values delivered; empty for all.
    pub types: BTreeSet<String>,
}

impl Filter {
    /// A filter that delivers every entry.
    pub fn all() -> Filter {
        Filter::default()
    }

    /// A filter that delivers the entries whose `type` is one of `types`.
    pub fn types<I, S>(types: I) -> Filter
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Filter {
            types: types.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether the filter delivers `event`.
    pub fn passes(&self, event: &Event) -> bool {
        self.types.is_empty() || self.types.contains(&event.attributes.kind)
    }
}

/// Where a subscription delivers.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Destination {
    /// A webhook: each entry is POSTed as `application/cloudevents+json` through the
    /// kernel's `urn:http:post` (ikigai-http), signed with the Standard Webhooks headers.
    Webhook {
        /// The URL posted to, `https://` or `http://`. ikigai-http's `urn:cap:net:` scope
        /// for its host must be in the grant.
        url: String,
        /// The resource that answers the signing secret (`whsec_` and base64), read through
        /// the kernel under the grant at each delivery, so the relay holds no key material:
        /// a `urn:` name, such as an ikigai-secret one. Never a network fetch.
        secret: String,
    },
    /// A connector process mounted at a name (a broker producer, one per system): each
    /// entry is a Sink of its JSON event to that name, keyed with `idempotency-key=` set to
    /// its event key. The relay links no broker client; the connector holds the network
    /// grant and acknowledges by answering.
    Connector {
        /// The connector's name, a `urn:`.
        target: String,
    },
    /// A server-sent events face: nothing is pushed. A door reads the feed and writes each
    /// entry as [`crate::sse_frame`], and a reconnect resumes from [`crate::sse_resume`].
    Sse,
}

impl Destination {
    /// A webhook destination, checked.
    ///
    /// ```
    /// use ikigai_events_relay::Destination;
    ///
    /// assert!(Destination::webhook("https://hooks.example.com/in", "urn:secret:hook").is_ok());
    /// assert_eq!(
    ///     Destination::webhook("ftp://x", "urn:secret:hook").unwrap_err().to_string(),
    ///     "`ftp://x` is not a webhook URL: give an `https://` or `http://` URL."
    /// );
    /// ```
    pub fn webhook(url: &str, secret: &str) -> Result<Destination, Refusal> {
        let scheme = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"));
        if !scheme.is_some_and(|rest| {
            !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
        }) {
            return Err(Refusal::new(format!(
                "{} is not a webhook URL: give an `https://` or `http://` URL.",
                shown(url)
            )));
        }
        Ok(Destination::Webhook {
            url: url.to_string(),
            secret: local_name(secret, "a webhook's secret")?,
        })
    }

    /// A connector destination, checked.
    pub fn connector(target: &str) -> Result<Destination, Refusal> {
        Ok(Destination::Connector {
            target: local_name(target, "a connector")?,
        })
    }
}

fn local_name(iri: &str, what: &str) -> Result<String, Refusal> {
    let ok = iri.strip_prefix("urn:").is_some_and(|rest| {
        !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
    });
    if ok {
        Ok(iri.to_string())
    } else {
        Err(Refusal::new(format!(
            "{} is not a name for {what}: give a `urn:` name the kernel resolves, never a \
             network address.",
            shown(iri)
        )))
    }
}
