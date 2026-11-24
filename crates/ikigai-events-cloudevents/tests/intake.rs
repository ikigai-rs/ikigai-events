//! What an intake does with the pair: a redelivered event is the same keyed Sink, byte for
//! byte, so a target that honors `idempotency-key=` answers the first answer and writes
//! nothing; and a host plans conversions through the pair by media type.

use ikigai_core::idempotency::{IdempotencyKey, Replay, ReplayScope, IDEMPOTENCY_KEY_ARG};
use ikigai_core::{select_transreptor, Iri, ReprType, Representation, Time};
use ikigai_events::event_key;
use ikigai_events_cloudevents::{
    lift_batch, lift_event, parse_batch, parse_event, sink_request, space, CLOUDEVENTS_BATCH_JSON,
    CLOUDEVENTS_JSON, LIFT, LOWER, TURTLE,
};

const FIRST: &str = r#"{"specversion":"1.0","id":"A234-1234-1234","source":"https://example.com/sensors/tn-1234567","type":"com.example.reading","time":"2026-11-23T20:00:00Z","data":{"celsius":21,"unit":"C"}}"#;
/// The same event redelivered by another sender's encoder: keys reordered, spaced out.
const REDELIVERED: &str = r#"{ "data": { "unit": "C", "celsius": 21 }, "type": "com.example.reading",
  "time": "2026-11-23T20:00:00Z", "source": "https://example.com/sensors/tn-1234567",
  "id": "A234-1234-1234", "specversion": "1.0" }"#;
/// A different event under the same `source` and `id`.
const IMPOSTOR: &str = r#"{"specversion":"1.0","id":"A234-1234-1234","source":"https://example.com/sensors/tn-1234567","type":"com.example.reading","time":"2026-11-23T20:00:00Z","data":{"celsius":99}}"#;

fn inbox() -> Iri {
    Iri::parse("urn:layer:inbox").unwrap()
}

#[test]
fn a_redelivery_is_the_same_keyed_sink_and_replays_the_first_answer() {
    let first = lift_event(&parse_event(FIRST.as_bytes()).unwrap()).unwrap();
    let again = lift_event(&parse_event(REDELIVERED.as_bytes()).unwrap()).unwrap();
    assert_eq!(
        first, again,
        "one event lifts to one form, whatever its encoding"
    );
    assert_eq!(
        first.intake.key,
        event_key("https://example.com/sensors/tn-1234567", "A234-1234-1234").unwrap()
    );

    let (one, two) = (sink_request(inbox(), &first), sink_request(inbox(), &again));
    let key = IdempotencyKey::from_request(&one, IDEMPOTENCY_KEY_ARG)
        .unwrap()
        .expect("the Sink is keyed");
    assert_eq!(key, first.intake.key);

    // The record a target keeps beside its write, as core's toolkit defines it.
    let answer = Representation::new(ReprType::new("text/plain"), b"seq 12".to_vec());
    let record = Replay::success(
        ReplayScope::new("urn:layer:inbox", None, key),
        one.fingerprint_without(IDEMPOTENCY_KEY_ARG),
        &answer,
        Time::from_millis(1_000),
    );
    assert!(
        record.matches(two.fingerprint_without(IDEMPOTENCY_KEY_ARG)),
        "the redelivery matches: the target answers the first answer and writes nothing"
    );

    let impostor = lift_event(&parse_event(IMPOSTOR.as_bytes()).unwrap()).unwrap();
    assert_eq!(
        impostor.intake.key, first.intake.key,
        "same source and id, same key"
    );
    assert!(
        !record.matches(sink_request(inbox(), &impostor).fingerprint_without(IDEMPOTENCY_KEY_ARG)),
        "a different event under the same key is a different request, which the target refuses"
    );
}

#[test]
fn a_batch_is_one_keyed_sink_per_event() {
    let second = FIRST.replace("A234-1234-1234", "B1");
    let batch = parse_batch(format!("[{FIRST},{second}]").as_bytes()).unwrap();
    let lifted = lift_batch(&batch).unwrap();
    assert_eq!(lifted.len(), 2);
    assert_ne!(lifted[0].intake.key, lifted[1].intake.key);
    for one in &lifted {
        let sink = sink_request(inbox(), one);
        assert_eq!(
            IdempotencyKey::from_request(&sink, IDEMPOTENCY_KEY_ARG)
                .unwrap()
                .unwrap(),
            one.intake.key
        );
    }
}

#[test]
fn a_host_plans_through_the_pair_by_media_type() {
    let space = space();
    let steps = |from, to| {
        select_transreptor(&space, from, to)
            .unwrap_or_else(|| panic!("a plan from {from} to {to}"))
            .into_iter()
            .map(|s| s.endpoint)
            .collect::<Vec<_>>()
    };
    assert_eq!(steps(CLOUDEVENTS_JSON, TURTLE), [LIFT]);
    assert_eq!(steps(CLOUDEVENTS_BATCH_JSON, TURTLE), [LIFT]);
    assert_eq!(steps(TURTLE, CLOUDEVENTS_JSON), [LOWER]);
    // One event to the batch form pivots through Turtle, the canonical hub.
    assert_eq!(
        steps(CLOUDEVENTS_JSON, CLOUDEVENTS_BATCH_JSON),
        [LIFT, LOWER]
    );
}
