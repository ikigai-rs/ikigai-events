//! The Turtle form: one node per event, named by its event key, and one `ce:{name}` property
//! per attribute.

use std::collections::{BTreeMap, HashMap};

use ikigai_core::IdempotencyKey;
use ikigai_events::{shown, Refusal, EVENT_KEY_PREFIX};
use oxrdf::vocab::{rdf, xsd};
use oxrdf::{Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, Term, Triple};
use oxttl::{TurtleParser, TurtleSerializer};

use crate::{check, Data, Event, Extension, MAX_BATCH_BYTES, MAX_BATCH_EVENTS, NS};

/// `rdf:JSON` (JSON-LD 1.1 and RDF 1.2), named here because oxrdf gates its constant behind
/// `rdf-12`, which this crate does not need.
const RDF_JSON: NamedNodeRef<'static> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#JSON");

/// The prefix of every event's node: `urn:iki:event:key:`, then its event key's digest.
pub const NODE_PREFIX: &str = "urn:iki:event:key:";

/// The node an event is in the Turtle form: [`NODE_PREFIX`] and its event key without the
/// key's `event:` prefix, so `urn:iki:event:key:b3:{hex}`.
///
/// ```
/// use ikigai_events::event_key;
/// use ikigai_events_cloudevents::event_node;
///
/// let key = event_key("urn:layer:talk", "urn:layer:talk:entry:1").unwrap();
/// let node = event_node(&key);
/// assert_eq!(node, format!("urn:iki:event:key:{}", &key.as_str()["event:".len()..]));
/// assert!(node.starts_with("urn:iki:event:key:b3:"));
/// ```
pub fn event_node(key: &IdempotencyKey) -> String {
    let text = key.as_str();
    format!(
        "{NODE_PREFIX}{}",
        text.strip_prefix(EVENT_KEY_PREFIX).unwrap_or(text)
    )
}

fn ce(name: &str) -> NamedNode {
    NamedNode::new_unchecked(format!("{NS}{name}"))
}

fn typed(value: &str, datatype: NamedNodeRef<'_>) -> Term {
    Literal::new_typed_literal(value, datatype).into()
}

fn plain(value: &str) -> Term {
    Literal::new_simple_literal(value).into()
}

/// The attribute's datatype in the form, for the attributes that are not `xsd:string`.
fn datatype_of(name: &str) -> NamedNodeRef<'static> {
    match name {
        "source" | "dataschema" => xsd::ANY_URI,
        "time" => xsd::DATE_TIME,
        _ => xsd::STRING,
    }
}

fn triples_of(event: &Event, node: &NamedNode, out: &mut Vec<Triple>) {
    let a = &event.attributes;
    let mut push = |name: &str, object: Term| {
        out.push(Triple::new(node.clone(), ce(name), object));
    };
    push("specversion", plain(&a.specversion));
    push("id", plain(&a.id));
    push("source", typed(&a.source, xsd::ANY_URI));
    push("type", plain(&a.kind));
    for (name, value) in [
        ("subject", &a.subject),
        ("time", &a.time),
        ("datacontenttype", &a.datacontenttype),
        ("dataschema", &event.dataschema),
        ("ikseq", &a.ikseq),
        ("ikepoch", &a.ikepoch),
        ("traceparent", &a.traceparent),
    ] {
        if let Some(value) = value {
            push(name, typed(value, datatype_of(name)));
        }
    }
    for (name, value) in &event.extensions {
        let object = match value {
            Extension::String(s) => plain(s),
            Extension::Integer(n) => typed(&n.to_string(), xsd::INT),
            Extension::Boolean(b) => typed(&b.to_string(), xsd::BOOLEAN),
        };
        push(name, object);
    }
    match &event.data {
        Some(Data::Text(text)) => push("data", plain(text)),
        Some(Data::Json(value)) => push(
            "data",
            typed(
                &serde_json::to_string(value).expect("a JSON value always serializes"),
                RDF_JSON,
            ),
        ),
        Some(Data::Base64(text)) => push("data_base64", typed(text, xsd::BASE_64_BINARY)),
        None => {}
    }
}

/// Lift events to the Turtle form, after [checking](Event::check) each.
///
/// The events are written in order. An event that appears twice (the same `source` and `id`)
/// is written once; two that share `source` and `id` but differ are refused, since they would
/// be one node with two values for an attribute.
///
/// `time` is written as it was sent, typed `xsd:dateTime`. RFC 3339 is nearly a subset of
/// `xsd:dateTime`; a lowercase `t` or `z`, a space between date and time, an offset past 14
/// hours or a leap second is valid RFC 3339 that an `xsd:dateTime` reader may call ill-typed,
/// and is kept rather than rewritten, so the event lowers back as it was sent.
pub fn to_turtle(events: &[Event]) -> Result<Vec<u8>, Refusal> {
    if events.len() > MAX_BATCH_EVENTS {
        return Err(Refusal::new(format!(
            "A batch of {} events is over the {MAX_BATCH_EVENTS} one batch may hold: send it in \
             smaller batches.",
            events.len()
        )));
    }
    let mut seen: HashMap<String, &Event> = HashMap::new();
    let mut triples = Vec::new();
    for event in events {
        let intake = event.check()?;
        let node = event_node(&intake.key);
        if let Some(first) = seen.get(&node) {
            if *first != event {
                return Err(Refusal::new(format!(
                    "Two events share the `source` {} and the `id` {} but differ: an event's \
                     `source` and `id` name it, so send one of them.",
                    shown(&event.attributes.source),
                    shown(&event.attributes.id)
                )));
            }
            continue;
        }
        let iri = NamedNode::new_unchecked(node.clone());
        triples.push(Triple::new(
            iri.clone(),
            rdf::TYPE,
            NamedNode::new_unchecked(format!("{NS}Event")),
        ));
        triples_of(event, &iri, &mut triples);
        seen.insert(node, event);
    }
    let failed = |e: std::io::Error| Refusal::new(format!("The form could not be written: {e}."));
    let mut writer = TurtleSerializer::new()
        .with_prefix("ce", NS)
        .and_then(|s| s.with_prefix("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
        .and_then(|s| s.with_prefix("xsd", "http://www.w3.org/2001/XMLSchema#"))
        .expect("the three prefixes are IRIs")
        .for_writer(Vec::new());
    for triple in &triples {
        writer.serialize_triple(triple).map_err(failed)?;
    }
    writer.finish().map_err(failed)
}

/// One node's properties, as read.
#[derive(Default)]
struct Node {
    typed: bool,
    values: BTreeMap<String, Literal>,
}

/// Lower the Turtle form to events, in the order their nodes first appear.
///
/// The form is closed: a blank node, a property outside `ce:` (other than `rdf:type
/// ce:Event`), a property given twice, a value that is not a literal of the attribute's
/// datatype, or a node whose name is not its event's [`event_node`] is refused, so every graph
/// this reads is one [`to_turtle`] writes.
///
/// ```
/// use ikigai_events_cloudevents::from_turtle;
///
/// let refusal = from_turtle(b"<urn:x> <urn:p> 1 .").unwrap_err();
/// assert_eq!(
///     refusal.to_string(),
///     "`urn:x` is not an event's node: the form names each event \
///      `urn:iki:event:key:b3:{hex}`, from its `source` and `id`."
/// );
/// ```
pub fn from_turtle(bytes: &[u8]) -> Result<Vec<Event>, Refusal> {
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(Refusal::new(format!(
            "A form of {} bytes is over the {MAX_BATCH_BYTES} one batch may be: send it in \
             smaller batches.",
            bytes.len()
        )));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Refusal::new("The content is not UTF-8, so it is not Turtle."))?;
    // The shared scan for RDF 1.2's nested triple terms, before the parser sees them.
    if ikigai_rdf::check_turtle_nesting(text, "content").is_err() {
        return Err(Refusal::new(format!(
            "The content nests triple terms deeper than {}: the form never nests them.",
            ikigai_rdf::MAX_TURTLE_NESTING
        )));
    }
    let event_class = format!("{NS}Event");
    let mut order: Vec<String> = Vec::new();
    let mut nodes: HashMap<String, Node> = HashMap::new();
    for triple in TurtleParser::new().for_slice(bytes) {
        let triple = triple.map_err(|e| {
            Refusal::new(format!(
                "The content is not Turtle: {}.",
                e.to_string().escape_debug()
            ))
        })?;
        let subject = match triple.subject {
            NamedOrBlankNode::NamedNode(n) => n.into_string(),
            _ => {
                return Err(Refusal::new(
                    "The content has a blank node: the form names every event, so send it \
                     without one.",
                ))
            }
        };
        if !subject.starts_with(NODE_PREFIX) {
            return Err(Refusal::new(format!(
                "{} is not an event's node: the form names each event \
                 `{NODE_PREFIX}b3:{{hex}}`, from its `source` and `id`.",
                shown(&subject)
            )));
        }
        if !nodes.contains_key(&subject) {
            if order.len() == MAX_BATCH_EVENTS {
                return Err(Refusal::new(format!(
                    "The form holds more than the {MAX_BATCH_EVENTS} events one batch may \
                     hold: send it in smaller batches."
                )));
            }
            order.push(subject.clone());
        }
        let node = nodes.entry(subject.clone()).or_default();
        let predicate = triple.predicate.as_str();
        if triple.predicate.as_ref() == rdf::TYPE {
            match &triple.object {
                Term::NamedNode(class) if class.as_str() == event_class && !node.typed => {
                    node.typed = true;
                    continue;
                }
                _ => {
                    return Err(Refusal::new(format!(
                        "{} has a type other than `ce:Event`: the form types each event once, \
                         as `ce:Event`.",
                        shown(&subject)
                    )))
                }
            }
        }
        let Some(name) = predicate.strip_prefix(NS) else {
            return Err(Refusal::new(format!(
                "{} is not a property of the form: every property is `ce:` and an attribute's \
                 name.",
                shown(predicate)
            )));
        };
        if name != "data" && name != "data_base64" {
            check::attribute_name(name)?;
        }
        let Term::Literal(literal) = triple.object else {
            return Err(Refusal::new(format!(
                "`ce:{name}` of {} is not a literal: every attribute's value is one.",
                shown(&subject)
            )));
        };
        if node.values.insert(name.to_string(), literal).is_some() {
            return Err(Refusal::new(format!(
                "{} has two values for `ce:{name}`: an attribute has one value.",
                shown(&subject)
            )));
        }
    }
    order
        .into_iter()
        .map(|subject| {
            let node = nodes.remove(&subject).expect("every ordered node was read");
            if !node.typed {
                return Err(Refusal::new(format!(
                    "{} is not typed `ce:Event`: the form types each event.",
                    shown(&subject)
                )));
            }
            let event = event_of(&subject, node.values)?;
            let intake = event.check()?;
            if event_node(&intake.key) != subject {
                return Err(Refusal::new(format!(
                    "{} is not the node of the event it describes: the form names each event \
                     from its `source` and `id`.",
                    shown(&subject)
                )));
            }
            Ok(event)
        })
        .collect()
}

/// A literal's text, when its datatype is `datatype` (and it has no language tag).
fn text_of(
    subject: &str,
    name: &str,
    literal: &Literal,
    datatype: NamedNodeRef<'_>,
) -> Result<String, Refusal> {
    if literal.language().is_none() && literal.datatype() == datatype {
        return Ok(literal.value().to_string());
    }
    Err(wrong_type(subject, name))
}

fn wrong_type(subject: &str, name: &str) -> Refusal {
    Refusal::new(format!(
        "`ce:{name}` of {} is not a literal of the attribute's datatype: see the form's \
         vocabulary.",
        shown(subject)
    ))
}

fn event_of(subject: &str, values: BTreeMap<String, Literal>) -> Result<Event, Refusal> {
    let mut event = Event::default();
    for (name, literal) in values {
        let a = &mut event.attributes;
        let text = |datatype| text_of(subject, &name, &literal, datatype);
        match name.as_str() {
            "specversion" => a.specversion = text(xsd::STRING)?,
            "id" => a.id = text(xsd::STRING)?,
            "source" => a.source = text(xsd::ANY_URI)?,
            "type" => a.kind = text(xsd::STRING)?,
            "subject" => a.subject = Some(text(xsd::STRING)?),
            "time" => a.time = Some(text(xsd::DATE_TIME)?),
            "datacontenttype" => a.datacontenttype = Some(text(xsd::STRING)?),
            "dataschema" => event.dataschema = Some(text(xsd::ANY_URI)?),
            "ikseq" => a.ikseq = Some(text(xsd::STRING)?),
            "ikepoch" => a.ikepoch = Some(text(xsd::STRING)?),
            "traceparent" => a.traceparent = Some(text(xsd::STRING)?),
            "data_base64" => {
                // Read after `data` (the map is in name order), so both are seen here.
                if event.data.is_some() {
                    return Err(Refusal::new(format!(
                        "{} has both `ce:data` and `ce:data_base64`: an event carries one.",
                        shown(subject)
                    )));
                }
                event.data = Some(Data::Base64(text(xsd::BASE_64_BINARY)?))
            }
            "data" => {
                event.data = Some(if literal.datatype() == RDF_JSON {
                    Data::Json(json_of(subject, literal.value())?)
                } else {
                    Data::Text(text(xsd::STRING)?)
                })
            }
            _ => {
                let extension = if literal.language().is_some() {
                    return Err(wrong_type(subject, &name));
                } else if literal.datatype() == xsd::STRING {
                    Extension::String(literal.value().to_string())
                } else if literal.datatype() == xsd::INT {
                    Extension::Integer(
                        literal
                            .value()
                            .parse()
                            .map_err(|_| wrong_type(subject, &name))?,
                    )
                } else if literal.datatype() == xsd::BOOLEAN {
                    Extension::Boolean(match literal.value() {
                        "true" | "1" => true,
                        "false" | "0" => false,
                        _ => return Err(wrong_type(subject, &name)),
                    })
                } else {
                    return Err(wrong_type(subject, &name));
                };
                event.extensions.insert(name, extension);
            }
        }
    }
    for (name, value) in [
        ("specversion", &event.attributes.specversion),
        ("id", &event.attributes.id),
        ("source", &event.attributes.source),
        ("type", &event.attributes.kind),
    ] {
        if value.is_empty() {
            return Err(Refusal::new(format!(
                "{} has no `ce:{name}`: every event carries `specversion`, `id`, `source` and \
                 `type`.",
                shown(subject)
            )));
        }
    }
    Ok(event)
}

/// An `rdf:JSON` literal's value, bounded before it is parsed, as the JSON doors are.
fn json_of(subject: &str, text: &str) -> Result<serde_json::Value, Refusal> {
    if ikigai_rdf::check_json_nesting(text.as_bytes(), "content").is_err() {
        return Err(Refusal::new(format!(
            "The data of {} nests JSON objects and arrays deeper than {}: flatten it.",
            shown(subject),
            crate::MAX_NESTING
        )));
    }
    serde_json::from_str(text).map_err(|e| {
        Refusal::new(format!(
            "The data of {} is typed `rdf:JSON` but is not JSON: {}.",
            shown(subject),
            e.to_string().escape_debug()
        ))
    })
}
