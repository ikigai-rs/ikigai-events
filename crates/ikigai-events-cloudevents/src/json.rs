//! The JSON event format (`application/cloudevents+json`) and its batch form
//! (`application/cloudevents-batch+json`), read and written with `serde_json`.

use ikigai_events::{shown, Refusal};
use serde_json::{Map, Value};

use crate::MAX_NESTING;
use crate::{check, Data, Event, Extension, MAX_BATCH_BYTES, MAX_BATCH_EVENTS, MAX_EVENT_BYTES};

/// Read one event in the JSON event format, and [check](Event::check) it.
///
/// A `null` attribute is an absent one, and a `null` `data` is no data. `ikseq` is a string
/// of decimal digits, never a number.
///
/// ```
/// use ikigai_events_cloudevents::parse_event;
///
/// let event = parse_event(
///     br#"{"specversion":"1.0","id":"1","source":"urn:x","type":"k","flag":true,"n":-7}"#,
/// )
/// .unwrap();
/// assert_eq!(event.extensions.len(), 2);
/// assert_eq!(
///     parse_event(br#"{"specversion":"1.0","id":"1","source":"urn:x"}"#)
///         .unwrap_err()
///         .to_string(),
///     "An event has no `type`: every CloudEvent carries `specversion`, `id`, `source` and \
///      `type`."
/// );
/// ```
pub fn parse_event(bytes: &[u8]) -> Result<Event, Refusal> {
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(Refusal::new(format!(
            "An event of {} bytes is over the {MAX_EVENT_BYTES} one event may be: send a \
             smaller one.",
            bytes.len()
        )));
    }
    match parse_json(bytes)? {
        Value::Object(map) => from_object(map),
        other => Err(Refusal::new(format!(
            "The content is {}, not a CloudEvent: the JSON event format is one JSON object.",
            kind(&other)
        ))),
    }
}

/// Read a batch in the JSON batch format: a JSON array of events, each read as
/// [`parse_event`] reads one.
///
/// ```
/// use ikigai_events_cloudevents::parse_batch;
///
/// let batch = parse_batch(
///     br#"[{"specversion":"1.0","id":"1","source":"urn:x","type":"k"},
///          {"specversion":"1.0","id":"2","source":"urn:x","type":"k"}]"#,
/// )
/// .unwrap();
/// assert_eq!(batch.len(), 2);
/// assert!(parse_batch(b"[]").unwrap().is_empty());
/// ```
pub fn parse_batch(bytes: &[u8]) -> Result<Vec<Event>, Refusal> {
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(Refusal::new(format!(
            "A batch of {} bytes is over the {MAX_BATCH_BYTES} one batch may be: send it in \
             smaller batches.",
            bytes.len()
        )));
    }
    let items = match parse_json(bytes)? {
        Value::Array(items) => items,
        other => {
            return Err(Refusal::new(format!(
                "The content is {}, not a batch: the JSON batch format is one JSON array of \
                 events.",
                kind(&other)
            )))
        }
    };
    if items.len() > MAX_BATCH_EVENTS {
        return Err(Refusal::new(format!(
            "A batch of {} events is over the {MAX_BATCH_EVENTS} one batch may hold: send it \
             in smaller batches.",
            items.len()
        )));
    }
    items
        .into_iter()
        .map(|item| match item {
            Value::Object(map) => from_object(map),
            other => Err(Refusal::new(format!(
                "A batch holds {}, not an event: every member of a batch is one JSON object.",
                kind(&other)
            ))),
        })
        .collect()
}

/// Bound the nesting, then parse.
fn parse_json(bytes: &[u8]) -> Result<Value, Refusal> {
    // The shared iterative scan, so `serde_json` never recurses past the bound. Its own
    // refusal speaks of JSON-LD; this one speaks of events.
    if ikigai_rdf::check_json_nesting(bytes, "content").is_err() {
        return Err(Refusal::new(format!(
            "The content nests JSON objects and arrays deeper than {MAX_NESTING}: flatten the \
             event's data, or send it as `data_base64`."
        )));
    }
    serde_json::from_slice(bytes).map_err(|e| {
        Refusal::new(format!(
            "The content is not JSON: {}.",
            e.to_string().escape_debug()
        ))
    })
}

/// How a JSON value reads in a sentence.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// An optional string attribute: absent when `null`.
fn string(name: &str, value: Value) -> Result<Option<String>, Refusal> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Number(_) if name == ikigai_events::cloudevents::EXT_SEQ => Err(Refusal::new(
            "An event's `ikseq` is a number: send it as a string of decimal digits, because a \
             CloudEvents Integer is 32-bit and a feed position is 64-bit.",
        )),
        other => Err(Refusal::new(format!(
            "An event's `{name}` is {}, not a string: send it as a JSON string.",
            kind(&other)
        ))),
    }
}

fn from_object(map: Map<String, Value>) -> Result<Event, Refusal> {
    let mut event = Event::default();
    let mut required = [false; 4];
    let mut data = None;
    let mut base64 = None;
    for (name, value) in map {
        let a = &mut event.attributes;
        match name.as_str() {
            "specversion" | "id" | "source" | "type" => {
                let Some(text) = string(&name, value)? else {
                    continue;
                };
                let (slot, at) = match name.as_str() {
                    "specversion" => (&mut a.specversion, 0),
                    "id" => (&mut a.id, 1),
                    "source" => (&mut a.source, 2),
                    _ => (&mut a.kind, 3),
                };
                *slot = text;
                required[at] = true;
            }
            "subject" => a.subject = string(&name, value)?,
            "time" => a.time = string(&name, value)?,
            "datacontenttype" => a.datacontenttype = string(&name, value)?,
            "dataschema" => event.dataschema = string(&name, value)?,
            "ikseq" => a.ikseq = string(&name, value)?,
            "ikepoch" => a.ikepoch = string(&name, value)?,
            "traceparent" => a.traceparent = string(&name, value)?,
            "data" => {
                data = match value {
                    Value::Null => None,
                    Value::String(text) => Some(Data::Text(text)),
                    other => Some(Data::Json(other)),
                }
            }
            "data_base64" => base64 = string(&name, value)?.map(Data::Base64),
            _ => {
                check::attribute_name(&name)?;
                let extension = match value {
                    Value::Null => continue,
                    Value::String(s) => Extension::String(s),
                    Value::Bool(b) => Extension::Boolean(b),
                    Value::Number(n) => match n.as_i64().and_then(|n| i32::try_from(n).ok()) {
                        Some(n) => Extension::Integer(n),
                        None => {
                            return Err(Refusal::new(format!(
                                "An event's `{name}` is {n}, which is not a CloudEvents \
                                 Integer: send a whole number from -2147483648 to 2147483647, \
                                 or a string."
                            )))
                        }
                    },
                    other => {
                        return Err(Refusal::new(format!(
                            "An event's extension {} is {}: send a string, a boolean or an \
                             integer.",
                            shown(&name),
                            kind(&other)
                        )))
                    }
                };
                event.extensions.insert(name, extension);
            }
        }
    }
    if let Some(at) = required.iter().position(|seen| !seen) {
        let name = ["specversion", "id", "source", "type"][at];
        return Err(Refusal::new(format!(
            "An event has no `{name}`: every CloudEvent carries `specversion`, `id`, `source` \
             and `type`."
        )));
    }
    if data.is_some() && base64.is_some() {
        return Err(Refusal::new(
            "An event carries both `data` and `data_base64`: send one of them.",
        ));
    }
    event.data = data.or(base64);
    event.check()?;
    Ok(event)
}

/// A string as a JSON string, escaped by `serde_json`.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serializes")
}

fn write_event(out: &mut String, event: &Event) {
    let a = &event.attributes;
    let mut fields: Vec<(&str, String)> = vec![
        ("specversion", quoted(&a.specversion)),
        ("id", quoted(&a.id)),
        ("source", quoted(&a.source)),
        ("type", quoted(&a.kind)),
    ];
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
            fields.push((name, quoted(value)));
        }
    }
    for (name, value) in &event.extensions {
        let text = match value {
            Extension::String(s) => quoted(s),
            Extension::Integer(n) => n.to_string(),
            Extension::Boolean(b) => b.to_string(),
        };
        fields.push((name, text));
    }
    match &event.data {
        Some(Data::Text(text)) => fields.push(("data", quoted(text))),
        Some(Data::Json(value)) => fields.push((
            "data",
            serde_json::to_string(value).expect("a JSON value always serializes"),
        )),
        Some(Data::Base64(text)) => fields.push(("data_base64", quoted(text))),
        None => {}
    }
    out.push('{');
    for (i, (name, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&quoted(name));
        out.push(':');
        out.push_str(value);
    }
    out.push('}');
}

/// Write one event in the JSON event format ([`CLOUDEVENTS_JSON`](crate::CLOUDEVENTS_JSON)), compact: the context
/// attributes in the specification's order, then the extensions by name, then the data. It
/// writes the event as given; [check](Event::check) one built by hand first.
pub fn to_json(event: &Event) -> Vec<u8> {
    let mut out = String::new();
    write_event(&mut out, event);
    out.into_bytes()
}

/// Write events in the JSON batch format: a JSON array of [`to_json`]'s objects, in order.
///
/// ```
/// use ikigai_events_cloudevents::batch_to_json;
///
/// assert_eq!(batch_to_json(&[]), b"[]");
/// ```
pub fn batch_to_json(events: &[crate::Event]) -> Vec<u8> {
    let mut out = String::from("[");
    for (i, event) in events.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_event(&mut out, event);
    }
    out.push(']');
    out.into_bytes()
}
