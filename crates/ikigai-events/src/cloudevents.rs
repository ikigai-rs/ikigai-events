//! The mapping between an ikigai entry and a CloudEvent's attributes (the events design note,
//! section 6.1), as data and functions.
//!
//! Attributes only: no JSON, no HTTP and no broker. The `application/cloudevents+json`
//! transreptor and the delivery relay are their own subcrates, and they carry these values in
//! the formats their bindings define. Validation that a format defines (an RFC 3339 `time`, a
//! URI-reference `source`, a W3C `traceparent`) belongs to the crate that writes or reads that
//! format, with a vetted parser; here those values pass through unchanged.

use std::num::NonZeroU64;

use ikigai_core::IdempotencyKey;

use crate::feed::{Cursor, Entry, Feed};
use crate::key::event_key;
use crate::name::{parse_seq, shown, FeedEpoch, Refusal};

/// The CloudEvents specification version these attributes follow.
pub const SPECVERSION: &str = "1.0";

/// The extension attribute that carries an entry's feed position.
///
/// Its value is a STRING of decimal digits, never a CloudEvents `Integer`: that type is a
/// signed 32-bit number and a feed position is a 64-bit one.
pub const EXT_SEQ: &str = "ikseq";

/// The extension attribute that carries the feed's epoch.
pub const EXT_EPOCH: &str = "ikepoch";

/// The extension attribute that carries the run's W3C trace context.
pub const EXT_TRACEPARENT: &str = "traceparent";

/// One row of the mapping: a CloudEvent attribute and where its value comes from in ikigai.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// The CloudEvent attribute's name.
    pub attribute: &'static str,
    /// Whether every CloudEvent carries it.
    pub required: bool,
    /// Where its value comes from in ikigai, and when it is present.
    pub ikigai: &'static str,
}

/// The mapping, one row per attribute, in the order the design note gives it.
///
/// ```
/// use ikigai_events::cloudevents::MAPPING;
///
/// let id = MAPPING.iter().find(|r| r.attribute == "id").unwrap();
/// assert!(id.required);
/// assert!(id.ikigai.starts_with("the entry's IRI"));
/// ```
pub const MAPPING: &[Rule] = &[
    Rule {
        attribute: "specversion",
        required: true,
        ikigai: "always `1.0`",
    },
    Rule {
        attribute: "id",
        required: true,
        ikigai: "the entry's IRI (`urn:layer:{layer}:entry:{id}`, \
                 `urn:iki:ledger:{ledger}:transition:seq:{n}`), unique within its source",
    },
    Rule {
        attribute: "source",
        required: true,
        ikigai: "the feed's source (`urn:layer:{layer}`, \
                 `urn:iki:ledger:{ledger}:transition`)",
    },
    Rule {
        attribute: "type",
        required: true,
        ikigai: "the entry's `rdf:type` as an IRI, or the feed event's kind \
                 (`say`, `out`, `taken`, `returned`, `sweep`)",
    },
    Rule {
        attribute: "subject",
        required: false,
        ikigai: "the resource the entry is about (a tuple's subject, an item's IRI), \
                 when it names one",
    },
    Rule {
        attribute: "time",
        required: false,
        ikigai: "`dcterms:created`, the host clock's stamp; taken in as AUTHORED time, \
                 never as commit order",
    },
    Rule {
        attribute: "datacontenttype",
        required: false,
        ikigai: "the media type of the entry's representation \
                 (`text/turtle`, `application/ld+json`)",
    },
    Rule {
        attribute: EXT_SEQ,
        required: false,
        ikigai: "the entry's feed position (`ik:seq`) as decimal digits in a string; \
                 advisory when taken in, since only the receiving log assigns positions",
    },
    Rule {
        attribute: EXT_EPOCH,
        required: false,
        ikigai: "the feed's epoch, when the feed names one",
    },
    Rule {
        attribute: EXT_TRACEPARENT,
        required: false,
        ikigai: "the run's W3C trace context, when it has one",
    },
];

/// A CloudEvent's context attributes, each as the string its bindings carry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attributes {
    /// `specversion`.
    pub specversion: String,
    /// `id`.
    pub id: String,
    /// `source`.
    pub source: String,
    /// `type`.
    pub kind: String,
    /// `subject`.
    pub subject: Option<String>,
    /// `time`.
    pub time: Option<String>,
    /// `datacontenttype`.
    pub datacontenttype: Option<String>,
    /// The [`EXT_SEQ`] extension.
    pub ikseq: Option<String>,
    /// The [`EXT_EPOCH`] extension.
    pub ikepoch: Option<String>,
    /// The [`EXT_TRACEPARENT`] extension.
    pub traceparent: Option<String>,
}

/// What an ikigai entry offers a CloudEvent: the entry, its place in its feed, and the facts
/// its host stamped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryFacts {
    /// The entry.
    pub entry: Entry,
    /// Its feed position. For a transition it is the `n` in its name.
    pub seq: NonZeroU64,
    /// The feed's epoch, when the feed names one.
    pub epoch: Option<FeedEpoch>,
    /// Its `rdf:type` IRI, or the feed event's kind.
    pub kind: String,
    /// The resource it is about, when it names one.
    pub subject: Option<String>,
    /// Its `dcterms:created` stamp.
    pub created: Option<String>,
    /// The media type of its representation.
    pub content_type: Option<String>,
    /// The run's trace context.
    pub traceparent: Option<String>,
}

/// The CloudEvent attributes of an ikigai entry.
///
/// ```
/// use std::num::NonZeroU64;
/// use ikigai_events::cloudevents::{lower, EntryFacts};
/// use ikigai_events::{Entry, FeedEpoch};
///
/// let attrs = lower(&EntryFacts {
///     entry: Entry::layer("talk", "01m84z0jyt0000000000007").unwrap(),
///     seq: NonZeroU64::new(7).unwrap(),
///     epoch: Some(FeedEpoch::parse("5f0c9e3a1b2d4c6e8f0a1b2c3d4e5f60").unwrap()),
///     kind: "say".into(),
///     subject: None,
///     created: Some("2026-11-23T20:00:52.197Z".into()),
///     content_type: Some("text/turtle".into()),
///     traceparent: None,
/// })
/// .unwrap();
/// assert_eq!(attrs.id, "urn:layer:talk:entry:01m84z0jyt0000000000007");
/// assert_eq!(attrs.source, "urn:layer:talk");
/// assert_eq!(attrs.ikseq.as_deref(), Some("7"));
/// ```
pub fn lower(facts: &EntryFacts) -> Result<Attributes, Refusal> {
    if let Some(named) = facts.entry.seq() {
        if named != facts.seq {
            return Err(Refusal::new(format!(
                "`{}` is at position {named}, not {}: give the transition its own position.",
                facts.entry, facts.seq
            )));
        }
    }
    if facts.kind.is_empty() {
        return Err(Refusal::new(
            "An entry's type is empty: give its `rdf:type` IRI or its feed event's kind.",
        ));
    }
    Ok(Attributes {
        specversion: SPECVERSION.to_string(),
        id: facts.entry.iri(),
        source: facts.entry.feed().source(),
        kind: facts.kind.clone(),
        subject: facts.subject.clone(),
        time: facts.created.clone(),
        datacontenttype: facts.content_type.clone(),
        ikseq: Some(facts.seq.to_string()),
        ikepoch: facts.epoch.as_ref().map(|e| e.to_string()),
        traceparent: facts.traceparent.clone(),
    })
}

/// What an incoming CloudEvent gives the Sink that takes it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intake {
    /// The idempotency key of the event, from its `source` and `id`.
    pub key: IdempotencyKey,
    /// The event's `time`, kept as AUTHORED time: when its author says it happened, never
    /// the receiving log's order.
    pub authored_time: Option<String>,
    /// The position its sender says it had, when it carries [`EXT_SEQ`]. Advisory: only the
    /// receiving log assigns positions.
    pub advisory: Option<Cursor>,
    /// The ikigai entry its `id` names, when it names one AND its `source` is that entry's
    /// feed. Still a claim: the door that took the event in decides who sent it.
    pub entry: Option<Entry>,
    /// The ikigai feed its `source` names, when it names one.
    pub feed: Option<Feed>,
}

/// Take a CloudEvent in: its key, its authored time, and what it says about ikigai names.
///
/// ```
/// use ikigai_events::cloudevents::{lift, Attributes};
/// use ikigai_events::event_key;
///
/// let attrs = Attributes {
///     specversion: "1.0".into(),
///     id: "A234-1234-1234".into(),
///     source: "https://example.com/sensors/tn-1234567".into(),
///     kind: "com.example.reading".into(),
///     time: Some("2026-11-23T20:00:00Z".into()),
///     ..Attributes::default()
/// };
/// let intake = lift(&attrs).unwrap();
/// assert_eq!(intake.key, event_key(&attrs.source, &attrs.id).unwrap());
/// assert_eq!(intake.entry, None);
/// ```
pub fn lift(attrs: &Attributes) -> Result<Intake, Refusal> {
    if attrs.specversion != SPECVERSION {
        return Err(Refusal::new(format!(
            "{} is not a CloudEvents version these attributes follow: send `specversion` \
             `{SPECVERSION}`.",
            shown(&attrs.specversion)
        )));
    }
    if attrs.kind.is_empty() {
        return Err(Refusal::new(
            "An event's `type` is empty: name what kind of event it is.",
        ));
    }
    let key = event_key(&attrs.source, &attrs.id)?;
    let epoch = attrs.ikepoch.as_deref().map(FeedEpoch::parse).transpose()?;
    let advisory = match attrs.ikseq.as_deref() {
        Some(seq) => Some(Cursor::new(epoch, parse_seq(seq)?.get())),
        None if epoch.is_some() => {
            return Err(Refusal::new(format!(
                "An event names an `{EXT_EPOCH}` with no `{EXT_SEQ}`: send both, or neither."
            )))
        }
        None => None,
    };
    let feed = Feed::parse(&attrs.source).ok();
    // An `id` names an ikigai entry only from that entry's own feed: an event from anywhere
    // else that spells an entry IRI is still just a foreign id.
    let entry = Entry::parse(&attrs.id)
        .ok()
        .filter(|e| feed.as_ref() == Some(&e.feed()));
    Ok(Intake {
        key,
        authored_time: attrs.time.clone(),
        advisory,
        entry,
        feed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lowered_entry_lifts_to_its_own_key() {
        let facts = EntryFacts {
            entry: Entry::transition("default", 9).unwrap(),
            seq: NonZeroU64::new(9).unwrap(),
            epoch: None,
            kind: "claimed".into(),
            subject: Some("urn:iki:ledger:default:item:4126".into()),
            created: None,
            content_type: None,
            traceparent: None,
        };
        let intake = lift(&lower(&facts).unwrap()).unwrap();
        assert_eq!(intake.entry, Some(facts.entry.clone()));
        assert_eq!(intake.feed, Some(facts.entry.feed()));
        assert_eq!(intake.advisory, Some(Cursor::new(None, 9)));
        assert_eq!(
            intake.key,
            event_key(
                "urn:iki:ledger:default:transition",
                "urn:iki:ledger:default:transition:seq:9"
            )
            .unwrap()
        );
    }

    #[test]
    fn a_transition_keeps_the_position_its_name_holds() {
        let facts = EntryFacts {
            entry: Entry::transition("default", 9).unwrap(),
            seq: NonZeroU64::new(8).unwrap(),
            epoch: None,
            kind: "claimed".into(),
            subject: None,
            created: None,
            content_type: None,
            traceparent: None,
        };
        assert!(lower(&facts).is_err());
    }

    #[test]
    fn every_extension_name_is_a_valid_cloudevents_attribute_name() {
        // CloudEvents 1.0: lowercase a-z and 0-9, at most 20 characters.
        for rule in MAPPING {
            let a = rule.attribute;
            assert!(
                a.len() <= 20
                    && a.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            );
        }
    }
}
