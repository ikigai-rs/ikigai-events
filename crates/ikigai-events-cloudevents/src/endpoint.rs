//! The transreptor pair, and the keyed Sink an intake writes.

use async_trait::async_trait;
use ikigai_core::idempotency::IDEMPOTENCY_KEY_ARG;
use ikigai_core::{
    space_iri, ArgRef, ArgSpec, Description, Endpoint, EndpointSpace, Error, Exact, FailureClass,
    Invocation, Iri, ReprType, Representation, Request, Result, Verb,
};
use ikigai_events::cloudevents::Intake;
use ikigai_events::Refusal;

use crate::{
    batch_to_json, event_node, from_turtle, parse_batch, parse_event, to_json, to_turtle, Event,
    CLOUDEVENTS_BATCH_JSON, CLOUDEVENTS_JSON, MAX_BATCH_BYTES, TURTLE,
};

/// The lift: a CloudEvent, or a batch, as the Turtle form.
pub const LIFT: &str = "urn:iki:cloudevent:lift";
/// The lower: the Turtle form as a CloudEvent, or a batch.
pub const LOWER: &str = "urn:iki:cloudevent:lower";
/// The identity [`space`] claims.
pub const SPACE_ID: &str = "urn:iki:space:cloudevent";

const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const EVENT_TYPES: [&str; 2] = [CLOUDEVENTS_JSON, CLOUDEVENTS_BATCH_JSON];

/// The pair, bound at [`LIFT`] and [`LOWER`].
///
/// ```
/// use futures::executor::block_on;
/// use ikigai_core::{ArgRef, Capability, Iri, Kernel, Request, Verb};
/// use std::sync::Arc;
///
/// let kernel = Kernel::new(Arc::new(ikigai_events_cloudevents::space()));
/// let event = br#"{"specversion":"1.0","id":"1","source":"urn:x","type":"k"}"#;
/// let lift = Request::new(Verb::Source, Iri::parse("urn:iki:cloudevent:lift").unwrap())
///     .with_arg("content", ArgRef::Inline(event.to_vec()));
/// let turtle = block_on(kernel.issue(lift, &Capability::root())).unwrap();
/// let lower = Request::new(Verb::Source, Iri::parse("urn:iki:cloudevent:lower").unwrap())
///     .with_arg("content", ArgRef::Inline(turtle.bytes.to_vec()));
/// let back = block_on(kernel.issue(lower, &Capability::root())).unwrap();
/// assert_eq!(back.repr_type.media_type, "application/cloudevents+json");
/// assert_eq!(back.bytes, &event[..]);
/// ```
pub fn space() -> EndpointSpace {
    EndpointSpace::new()
        .bind(Exact::new(LIFT), Lift)
        .bind(Exact::new(LOWER), Lower)
        .named(space_iri("cloudevent"))
}

fn optional<'a>(inv: &'a Invocation<'_>, name: &str) -> Result<Option<&'a str>> {
    match inv.inline_str(name) {
        Ok(value) => Ok(Some(value)),
        Err(Error::MissingArgument(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `content`, collected under a pool one byte past [`MAX_BATCH_BYTES`], so a body past it is
/// refused as it arrives rather than read whole.
async fn content(inv: &Invocation<'_>) -> Result<Vec<u8>> {
    let body = inv.content_body("content").await?;
    let bytes = inv
        .with_memory_limit(MAX_BATCH_BYTES as u64 + 1)
        .collect(body)
        .await?;
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(Error::OverBudget {
            detail: format!("content exceeds {MAX_BATCH_BYTES} bytes"),
        });
    }
    Ok(bytes.to_vec())
}

fn refused(refusal: Refusal) -> Error {
    refusal.into_invalid_argument("content")
}

fn content_input(summary: &str) -> ArgSpec {
    ArgSpec::new("content")
        .summary(summary)
        .class(XSD_STRING)
        .reads_body()
}

struct Lift;

#[async_trait]
impl Endpoint for Lift {
    async fn invoke(&self, inv: &Invocation<'_>) -> Result<Representation> {
        let face = optional(inv, "as")?.unwrap_or(TURTLE);
        if face != TURTLE {
            return Err(Error::InvalidArgument {
                name: "as".into(),
                detail: format!("the lift answers `{TURTLE}` only, not `{face}`"),
            });
        }
        let bytes = content(inv).await?;
        // One event is a JSON object, a batch a JSON array: a transreption plan hands the
        // bytes without their media type, so the first token decides.
        let first = bytes.iter().find(|b| !b.is_ascii_whitespace());
        let events = if first == Some(&b'[') {
            parse_batch(&bytes)
        } else {
            parse_event(&bytes).map(|event| vec![event])
        }
        .map_err(refused)?;
        let turtle = to_turtle(&events).map_err(refused)?;
        Ok(
            Representation::new(ReprType::new(TURTLE).with_param("charset", "utf-8"), turtle)
                .cacheable(),
        )
    }

    fn name(&self) -> &str {
        "cloudevent-lift"
    }

    fn describe(&self) -> Description {
        Description::new("cloudevent-lift")
            .title("CloudEvent lift")
            .summary(
                "A CloudEvent in the JSON event format, or a JSON batch of them, as Turtle: one \
                 node per event, named by its event key, with a ce: property per attribute",
            )
            .verb(Verb::Source)
            .verb(Verb::Meta)
            .input(content_input(
                "the event (a JSON object) or the batch (a JSON array of events)",
            ))
            .input(
                ArgSpec::new("as")
                    .summary("the face: Turtle")
                    .class(XSD_STRING)
                    .optional()
                    .one_of([TURTLE])
                    .default_value(TURTLE),
            )
            .transreptor(EVENT_TYPES, [TURTLE])
            .fails(FailureClass::OverBudget)
            .output(TURTLE)
    }
}

struct Lower;

#[async_trait]
impl Endpoint for Lower {
    async fn invoke(&self, inv: &Invocation<'_>) -> Result<Representation> {
        let face = optional(inv, "as")?.unwrap_or(CLOUDEVENTS_JSON);
        if !EVENT_TYPES.contains(&face) {
            return Err(Error::InvalidArgument {
                name: "as".into(),
                detail: format!(
                    "the lower answers `{CLOUDEVENTS_JSON}` or `{CLOUDEVENTS_BATCH_JSON}`, not \
                     `{face}`"
                ),
            });
        }
        let events = from_turtle(&content(inv).await?).map_err(refused)?;
        let bytes = if face == CLOUDEVENTS_BATCH_JSON {
            batch_to_json(&events)
        } else {
            match events.as_slice() {
                [event] => to_json(event),
                _ => {
                    return Err(refused(Refusal::new(format!(
                        "The form holds {} events, not one: ask for `as={CLOUDEVENTS_BATCH_JSON}`.",
                        events.len()
                    ))))
                }
            }
        };
        Ok(Representation::new(ReprType::new(face), bytes).cacheable())
    }

    fn name(&self) -> &str {
        "cloudevent-lower"
    }

    fn describe(&self) -> Description {
        Description::new("cloudevent-lower")
            .title("CloudEvent lower")
            .summary(
                "Turtle in the CloudEvent lift's form as a CloudEvent in the JSON event format, \
                 or as a JSON batch",
            )
            .verb(Verb::Source)
            .verb(Verb::Meta)
            .input(content_input("Turtle in the lift's form"))
            .input(
                ArgSpec::new("as")
                    .summary("the face: one event, or a batch")
                    .class(XSD_STRING)
                    .optional()
                    .one_of(EVENT_TYPES)
                    .default_value(CLOUDEVENTS_JSON),
            )
            .transreptor([TURTLE], EVENT_TYPES)
            .fails(FailureClass::OverBudget)
            .output(CLOUDEVENTS_JSON)
            .output(CLOUDEVENTS_BATCH_JSON)
    }
}

/// One event lifted for an intake: what it gives the intake, its node, and its Turtle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lifted {
    /// The event's key, authored time, advisory position and ikigai names
    /// ([`Event::check`]).
    pub intake: Intake,
    /// Its node in the form ([`event_node`]).
    pub node: String,
    /// The form, holding this event alone.
    pub turtle: Vec<u8>,
}

/// Lift one event for an intake.
pub fn lift_event(event: &Event) -> std::result::Result<Lifted, Refusal> {
    let intake = event.check()?;
    Ok(Lifted {
        node: event_node(&intake.key),
        turtle: to_turtle(std::slice::from_ref(event))?,
        intake,
    })
}

/// Lift each event of a batch for an intake, one [`Lifted`] each, so each is its own keyed
/// Sink.
pub fn lift_batch(events: &[Event]) -> std::result::Result<Vec<Lifted>, Refusal> {
    events.iter().map(lift_event).collect()
}

/// The Sink an intake issues for one lifted event: its Turtle as `content`, keyed with
/// `idempotency-key=` set to the event key ([`ikigai_events::cloudevents::lift`]'s key).
///
/// A redelivery lifts to the same bytes under the same key, so a target that honors the key
/// answers the first answer and writes nothing. A different event under the same `source`
/// and `id` is a different request under the same key, which the target refuses.
///
/// ```
/// use ikigai_core::{idempotency::IdempotencyKey, Iri};
/// use ikigai_events_cloudevents::{lift_event, parse_event, sink_request};
///
/// let event = parse_event(br#"{"specversion":"1.0","id":"1","source":"urn:x","type":"k"}"#)
///     .unwrap();
/// let lifted = lift_event(&event).unwrap();
/// let sink = sink_request(Iri::parse("urn:layer:inbox").unwrap(), &lifted);
/// let key = IdempotencyKey::from_request(&sink, "idempotency-key").unwrap().unwrap();
/// assert_eq!(key, lifted.intake.key);
/// ```
pub fn sink_request(target: Iri, lifted: &Lifted) -> Request {
    Request::new(Verb::Sink, target)
        .with_arg("content", ArgRef::Inline(lifted.turtle.clone()))
        .with_arg(
            IDEMPOTENCY_KEY_ARG,
            ArgRef::Inline(lifted.intake.key.as_str().as_bytes().to_vec()),
        )
}
