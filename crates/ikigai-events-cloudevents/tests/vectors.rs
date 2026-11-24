//! The CloudEvents vectors: every event, batch and Turtle form in
//! `tests/vectors/cloudevents_cases.json`, read, written and lifted by this crate, written to
//! `tests/vectors/cloudevents_vectors.json` for the faces that mirror it by hand (Python,
//! TypeScript, .NET). The test below recomputes every row and fails if the committed file
//! differs by a byte.
//!
//! To add a case, edit `cloudevents_cases.json`, then regenerate with
//!
//! ```text
//! cargo test -p ikigai-events-cloudevents --test vectors -- --ignored
//! ```
//!
//! and commit both files.

use ikigai_events::cloudevents::Intake;
use ikigai_events::Refusal;
use ikigai_events_cloudevents::{
    batch_to_json, event_node, from_turtle, parse_batch, parse_event, to_json, to_turtle,
    CLOUDEVENTS_BATCH_JSON, CLOUDEVENTS_JSON, MAX_BATCH_BYTES, MAX_BATCH_EVENTS, MAX_EVENT_BYTES,
    MAX_NESTING, NODE_PREFIX, NS, TURTLE,
};
use serde_json::{json, Value};

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vectors/cloudevents_cases.json"
);
const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vectors/cloudevents_vectors.json"
);

const ABOUT: &str = "CloudEvents vectors (ledger #4117) computed from \
     tests/vectors/cloudevents_cases.json by crates/ikigai-events-cloudevents/tests/vectors.rs, \
     which fails if this file differs from what it computes. Every input is TEXT, its bytes the \
     input. Where the crate refuses, the outputs are null and `refusal` holds its one sentence; \
     a face must refuse exactly there, and matching the sentence byte for byte is recommended. \
     `events`: one event read, written back (`json`), keyed (`key`, the identifiers crate's \
     event key), named (`node`) and lifted (`turtle`), with what it gives an intake. \
     `batches`: a batch read, written back and lifted. `turtle`: a form lowered to a batch. \
     Every `turtle` an event or batch row holds lowers back to that row's `json`.";

fn sentence(r: &Refusal) -> Value {
    json!(r.sentence())
}

fn s(v: &Value, field: &str) -> String {
    v[field]
        .as_str()
        .unwrap_or_else(|| panic!("`{field}` is a string in {v}"))
        .to_string()
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("UTF-8")
}

fn intake_json(i: &Intake) -> Value {
    json!({
        "key": i.key.as_str(),
        "authored_time": i.authored_time,
        "advisory": i.advisory.as_ref().map(|c| c.to_string()),
        "entry": i.entry.as_ref().map(|e| e.iri()),
        "feed": i.feed.as_ref().map(|f| f.source()),
    })
}

fn rows(cases: &Value, section: &str) -> Vec<(String, String)> {
    let field = if section == "turtle" {
        "turtle"
    } else {
        "json"
    };
    cases[section]
        .as_array()
        .unwrap_or_else(|| panic!("{section}"))
        .iter()
        .map(|c| (s(c, "name"), s(c, field)))
        .collect()
}

fn event_row(name: &str, input: &str) -> Value {
    let lifted = parse_event(input.as_bytes()).and_then(|event| {
        let intake = event.check()?;
        let turtle = to_turtle(std::slice::from_ref(&event))?;
        Ok((event, intake, turtle))
    });
    match lifted {
        Ok((event, intake, turtle)) => json!({
            "name": name,
            "input": input,
            "json": text(to_json(&event)),
            "node": event_node(&intake.key),
            "intake": intake_json(&intake),
            "turtle": text(turtle),
            "refusal": null,
        }),
        Err(r) => json!({
            "name": name, "input": input,
            "json": null, "node": null, "intake": null, "turtle": null,
            "refusal": sentence(&r),
        }),
    }
}

fn batch_row(name: &str, input: &str) -> Value {
    let lifted = parse_batch(input.as_bytes()).and_then(|events| {
        let turtle = to_turtle(&events)?;
        Ok((events, turtle))
    });
    match lifted {
        Ok((events, turtle)) => json!({
            "name": name,
            "input": input,
            "json": text(batch_to_json(&events)),
            "turtle": text(turtle),
            "refusal": null,
        }),
        Err(r) => json!({
            "name": name, "input": input, "json": null, "turtle": null,
            "refusal": sentence(&r),
        }),
    }
}

fn turtle_row(name: &str, input: &str) -> Value {
    match from_turtle(input.as_bytes()) {
        Ok(events) => json!({
            "name": name, "input": input,
            "json": text(batch_to_json(&events)), "refusal": null,
        }),
        Err(r) => json!({"name": name, "input": input, "json": null, "refusal": sentence(&r)}),
    }
}

fn compute() -> String {
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(CASES).expect("the cases")).expect("JSON");
    let events: Vec<Value> = rows(&cases, "events")
        .iter()
        .map(|(n, i)| event_row(n, i))
        .collect();
    let batches: Vec<Value> = rows(&cases, "batches")
        .iter()
        .map(|(n, i)| batch_row(n, i))
        .collect();
    let turtle: Vec<Value> = rows(&cases, "turtle")
        .iter()
        .map(|(n, i)| turtle_row(n, i))
        .collect();
    let file = json!({
        "about": ABOUT,
        "constants": {
            "cloudevents_json": CLOUDEVENTS_JSON,
            "cloudevents_batch_json": CLOUDEVENTS_BATCH_JSON,
            "turtle": TURTLE,
            "namespace": NS,
            "node_prefix": NODE_PREFIX,
            "max_event_bytes": MAX_EVENT_BYTES.to_string(),
            "max_batch_bytes": MAX_BATCH_BYTES.to_string(),
            "max_batch_events": MAX_BATCH_EVENTS.to_string(),
            "max_nesting": MAX_NESTING.to_string(),
        },
        "events": events,
        "batches": batches,
        "turtle": turtle,
    });
    // serde_json's map is ordered by key, so the bytes depend on the cases alone.
    let mut text = serde_json::to_string_pretty(&file).expect("serializable");
    text.push('\n');
    text
}

#[test]
fn the_committed_vectors_are_what_this_crate_computes() {
    let committed = std::fs::read_to_string(VECTORS).expect(
        "tests/vectors/cloudevents_vectors.json exists; regenerate it with \
         `cargo test -p ikigai-events-cloudevents --test vectors -- --ignored`",
    );
    assert!(
        committed == compute(),
        "tests/vectors/cloudevents_vectors.json differs from what this crate computes; \
         regenerate it with `cargo test -p ikigai-events-cloudevents --test vectors -- --ignored` \
         and review the diff"
    );
}

/// Every accepted event and batch round-trips: its JSON reads back to itself, and its Turtle
/// lowers back to it.
#[test]
fn every_lift_lowers_back_to_what_was_lifted() {
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(CASES).expect("the cases")).expect("JSON");
    for (name, input) in rows(&cases, "events") {
        let Ok(event) = parse_event(input.as_bytes()) else {
            continue;
        };
        let written = to_json(&event);
        assert_eq!(parse_event(&written).unwrap(), event, "{name}: JSON");
        let turtle = to_turtle(std::slice::from_ref(&event)).unwrap();
        assert_eq!(from_turtle(&turtle).unwrap(), vec![event], "{name}: Turtle");
    }
    for (name, input) in rows(&cases, "batches") {
        let Ok(events) = parse_batch(input.as_bytes()) else {
            continue;
        };
        let Ok(turtle) = to_turtle(&events) else {
            continue;
        };
        let back = from_turtle(&turtle).unwrap();
        // A redelivery inside one batch is one node, so it lowers once.
        let mut expected = events.clone();
        expected.dedup();
        assert_eq!(back, expected, "{name}");
    }
}

#[test]
#[ignore = "writes the vectors file; run it after editing the cases"]
fn regenerate() {
    std::fs::write(VECTORS, compute()).expect("written");
}
