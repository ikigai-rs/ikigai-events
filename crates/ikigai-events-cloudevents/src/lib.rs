//! CloudEvents for ikigai: `application/cloudevents+json` and its batch form lifted to Turtle and
//! lowered back, as a transreptor pair, and the keyed Sink an intake writes (ledger #4117).
//!
//! The design is ikigai-core's `docs/design/events-and-resources.md`, section 6.1: a CloudEvent
//! is a representation of an entry. Its attribute mapping, its idempotency key and its
//! ikigai names are the identifiers crate's ([`ikigai_events::cloudevents`]); this crate adds
//! the formats: the JSON event format and its batch form, read and written with `serde_json`,
//! and a Turtle form, with the checks those formats define done by vetted parsers.
//!
//! | name | what it answers |
//! |---|---|
//! | `urn:iki:cloudevent:lift` | `content` (a CloudEvent, or a batch) as Turtle: the [form](#the-turtle-form) |
//! | `urn:iki:cloudevent:lower` | `content` (Turtle in the form) as `application/cloudevents+json` or `application/cloudevents-batch+json` |
//!
//! Both are transreptors, so a host that binds [`space`] converts between the three media types
//! by `as=`. An intake (a door route that takes events in) uses [`lift_event`] and
//! [`sink_request`]: each event becomes one Sink of its Turtle keyed with `idempotency-key=` set
//! to its event key, so a redelivery answers the first answer and writes nothing.
//!
//! # The Turtle form
//!
//! Every attribute `{name}` of an event, extensions included, is the property `ce:{name}`
//! (`ce:` is [`NS`]), and the event's data is `ce:data` or `ce:data_base64`. The event is the
//! node [`event_node`]: `urn:iki:event:key:b3:{hex}`, named by its event key, never by its `id`,
//! so a foreign event that spells an ikigai entry's IRI never states anything about that entry.
//! Nothing in the form is a term the receiving log stamps: `time` is `ce:time` (authored time),
//! never `dcterms:created`, and `ikseq` is `ce:ikseq` (advisory), never `ik:seq`.
//!
//! ```
//! use ikigai_events_cloudevents::{from_turtle, parse_event, to_json, to_turtle};
//!
//! let json = r#"{"specversion":"1.0","id":"A234-1234-1234",
//!     "source":"https://example.com/sensors/tn-1234567","type":"com.example.reading",
//!     "time":"2026-11-23T20:00:00Z","data":{"celsius":21}}"#;
//! let event = parse_event(json.as_bytes()).unwrap();
//! let turtle = String::from_utf8(to_turtle(&[event.clone()]).unwrap()).unwrap();
//! assert!(turtle.contains("<urn:iki:event:key:b3:"));
//! assert!(turtle.contains(r#"ce:time "2026-11-23T20:00:00Z"^^xsd:dateTime"#));
//! assert!(turtle.contains(r#"ce:data "{\"celsius\":21}"^^rdf:JSON"#));
//!
//! let back = from_turtle(turtle.as_bytes()).unwrap();
//! assert_eq!(back, vec![event.clone()]);
//! assert_eq!(
//!     String::from_utf8(to_json(&event)).unwrap(),
//!     r#"{"specversion":"1.0","id":"A234-1234-1234","source":"https://example.com/sensors/tn-1234567","type":"com.example.reading","time":"2026-11-23T20:00:00Z","data":{"celsius":21}}"#
//! );
//! ```
//!
//! # Bounds
//!
//! Every input is bounded before it is parsed: [`MAX_EVENT_BYTES`] for one event,
//! [`MAX_BATCH_BYTES`] and [`MAX_BATCH_EVENTS`] for a batch or a Turtle form, and
//! [`MAX_NESTING`] JSON levels, counted by an iterative pre-scan
//! ([`ikigai_rdf::check_json_nesting`]) before `serde_json` recurses. Turtle is pre-scanned by
//! [`ikigai_rdf::check_turtle_nesting`].
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod check;
mod endpoint;
mod json;
mod rdf;

use std::collections::BTreeMap;

use ikigai_events::cloudevents::{self, Attributes, Intake};
use ikigai_events::{shown, Refusal};

pub use endpoint::{lift_batch, lift_event, sink_request, space, Lifted, LIFT, LOWER, SPACE_ID};
pub use json::{batch_to_json, parse_batch, parse_event, to_json};
pub use rdf::{event_node, from_turtle, to_turtle, NODE_PREFIX};

/// The media type of one event in the JSON event format.
pub const CLOUDEVENTS_JSON: &str = "application/cloudevents+json";
/// The media type of a batch of events in the JSON batch format.
pub const CLOUDEVENTS_BATCH_JSON: &str = "application/cloudevents-batch+json";
/// The media type of the Turtle form.
pub const TURTLE: &str = "text/turtle";

/// The namespace of the Turtle form: `ce:{name}` for every attribute `{name}`.
pub const NS: &str = "https://ikigai-rs.dev/ns/cloudevents#";

/// The form's vocabulary: the class, the specification's attributes, ikigai's three
/// extensions and the two data properties, as Turtle.
pub const VOCABULARY: &str = include_str!("vocabulary.ttl");

/// The largest single event taken, in bytes: 1 MiB. The CloudEvents specification asks
/// intermediaries to forward events of 64 KB or less, so this is generous.
pub const MAX_EVENT_BYTES: usize = 1 << 20;
/// The largest batch, or Turtle form, taken, in bytes: 8 MiB.
pub const MAX_BATCH_BYTES: usize = 8 << 20;
/// The most events in one batch or one Turtle form.
pub const MAX_BATCH_EVENTS: usize = 1000;
/// How deep JSON may nest objects and arrays, counted together outside strings, the envelope
/// included: the ecosystem's one bound on caller nesting.
pub const MAX_NESTING: usize = ikigai_rdf::MAX_JSON_NESTING;

/// The attribute names the specification and ikigai define, which an extension never takes.
const CONTEXT: [&str; 11] = [
    "specversion",
    "id",
    "source",
    "type",
    "subject",
    "time",
    "datacontenttype",
    "dataschema",
    cloudevents::EXT_SEQ,
    cloudevents::EXT_EPOCH,
    cloudevents::EXT_TRACEPARENT,
];

/// One CloudEvent: its context attributes, its extensions and its data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Event {
    /// The attributes the identifiers crate maps: the required four, `subject`, `time`,
    /// `datacontenttype`, and ikigai's `ikseq`, `ikepoch` and `traceparent`.
    pub attributes: Attributes,
    /// `dataschema`: an absolute URI.
    pub dataschema: Option<String>,
    /// Every other extension attribute, by name.
    pub extensions: BTreeMap<String, Extension>,
    /// The event's data, if it carries any.
    pub data: Option<Data>,
}

/// An extension attribute's value, in one of the three types the JSON format carries as
/// itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Extension {
    /// A CloudEvents `String` (and `URI`, `URI-reference`, `Timestamp`, `Binary`, which the
    /// JSON format writes as strings).
    String(String),
    /// A CloudEvents `Integer`: signed 32-bit.
    Integer(i32),
    /// A CloudEvents `Boolean`.
    Boolean(bool),
}

/// An event's data, as the JSON format carries it.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    /// `data` that is a JSON string: text, such as Turtle when `datacontenttype` says so.
    Text(String),
    /// `data` that is any other JSON value. Never a string: that is [`Data::Text`].
    Json(serde_json::Value),
    /// `data_base64`: binary data as RFC 4648 base64, kept as it was written.
    Base64(String),
}

impl Event {
    /// Check the event as the formats define it, and say what it gives an intake: its key,
    /// its authored time, its advisory position and the ikigai names it carries.
    ///
    /// The mapping's own checks come first ([`cloudevents::lift`]: `specversion` is `1.0`,
    /// `type`, `source` and `id` are not empty, `ikseq` and `ikepoch` are well formed); then
    /// `source` is a URI-reference, `time` RFC 3339, `dataschema` an absolute URI,
    /// `traceparent` a W3C trace context, every extension name lowercase letters and digits
    /// and none a context attribute's, and `data_base64` base64.
    ///
    /// ```
    /// use ikigai_events_cloudevents::Event;
    /// use ikigai_events::cloudevents::Attributes;
    ///
    /// let mut event = Event {
    ///     attributes: Attributes {
    ///         specversion: "1.0".into(),
    ///         id: "1".into(),
    ///         source: "urn:x".into(),
    ///         kind: "k".into(),
    ///         time: Some("yesterday".into()),
    ///         ..Attributes::default()
    ///     },
    ///     ..Event::default()
    /// };
    /// assert_eq!(
    ///     event.check().unwrap_err().to_string(),
    ///     "`yesterday` is not an RFC 3339 time: send `time` as a date-time with an offset, \
    ///      such as `2026-11-23T20:00:00Z`."
    /// );
    /// event.attributes.time = None;
    /// assert!(event.check().is_ok());
    /// ```
    pub fn check(&self) -> Result<Intake, Refusal> {
        let a = &self.attributes;
        let intake = cloudevents::lift(a)?;
        check::source(&a.source)?;
        if let Some(time) = &a.time {
            check::rfc3339(time)?;
        }
        for (name, value) in [
            ("subject", &a.subject),
            ("datacontenttype", &a.datacontenttype),
        ] {
            if value.as_deref() == Some("") {
                return Err(Refusal::new(format!(
                    "An event's `{name}` is empty: leave it out, or give it a value."
                )));
            }
        }
        if let Some(schema) = &self.dataschema {
            check::dataschema(schema)?;
        }
        if let Some(trace) = &a.traceparent {
            check::traceparent(trace)?;
        }
        for name in self.extensions.keys() {
            check::attribute_name(name)?;
            if CONTEXT.contains(&name.as_str()) || name == "data" {
                return Err(Refusal::new(format!(
                    "{} is a context attribute's name, not an extension's: give it in its \
                     own field.",
                    shown(name)
                )));
            }
        }
        match &self.data {
            Some(Data::Base64(text)) => check::data_base64(text)?,
            Some(Data::Json(serde_json::Value::String(_))) => {
                return Err(Refusal::new(
                    "An event's JSON data is a string: give it as text data instead.",
                ))
            }
            Some(Data::Json(serde_json::Value::Null)) => {
                return Err(Refusal::new(
                    "An event's JSON data is null, which the format reads as no data: leave \
                     it out.",
                ))
            }
            _ => {}
        }
        Ok(intake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxrdf::NamedNode;

    /// The vocabulary parses, declares every attribute the form writes by name, and says the
    /// crate's version.
    #[test]
    fn the_vocabulary_declares_every_named_term() {
        let triples: Vec<_> = oxttl::TurtleParser::new()
            .for_slice(VOCABULARY.as_bytes())
            .collect::<Result<_, _>>()
            .expect("the vocabulary is Turtle");
        let declared = |term: &str| {
            let iri = NamedNode::new_unchecked(format!("{NS}{term}"));
            triples.iter().any(|t| {
                t.subject == iri.clone().into()
                    && t.predicate.as_str() == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
            })
        };
        for term in CONTEXT.iter().chain(&["Event", "data", "data_base64"]) {
            assert!(declared(term), "ce:{term} is declared");
        }
        assert!(VOCABULARY.contains(&format!(
            "owl:versionInfo \"{}\"",
            env!("CARGO_PKG_VERSION")
        )));
    }
}
