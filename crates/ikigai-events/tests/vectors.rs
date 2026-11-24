//! The shared identifier vectors: every name, key and mapping this crate computes, written to
//! `tests/vectors/identifier_vectors.json` for the faces that mirror it by hand (Python,
//! TypeScript, .NET). The test below recomputes every row from
//! `tests/vectors/identifier_cases.json` and fails if the committed file differs by a byte.
//!
//! To add a case, edit `identifier_cases.json`, then regenerate with
//!
//! ```text
//! cargo test -p ikigai-events --test vectors -- --ignored
//! ```
//!
//! and commit both files.

use std::num::NonZeroU64;

use ikigai_events::cloudevents::{self, Attributes, EntryFacts, MAPPING};
use ikigai_events::{
    event_key, Cursor, Entry, EntryId, Feed, FeedEpoch, FenceKey, ItemRef, LayerName, LedgerName,
    Listener, ListenerId, Refusal, Step, EVENT_KEY_DOMAIN, EVENT_KEY_PREFIX, FENCE_KEY_PREFIX,
};
use serde_json::{json, Value};

const CASES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vectors/identifier_cases.json"
);
const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vectors/identifier_vectors.json"
);

const ABOUT: &str = "Identifier vectors (ledger #4126) computed from \
     tests/vectors/identifier_cases.json by crates/ikigai-events/tests/vectors.rs, which fails if \
     this file differs from what it computes. Every 64-bit number is a STRING of decimal digits. \
     Where the crate refuses, the value is null and `refusal` holds its one sentence; a face \
     must refuse exactly there, and matching the sentence byte for byte is recommended. \
     `tokens`: per token kind, whether each input is accepted. `names`: each IRI read as an \
     entry, a feed and a listener. `cursors`: a cursor's text read and written back. \
     `event_keys`: event_key(source, id), BLAKE3 over the domain, source and id, each framed \
     by its byte length as a little-endian u64, written `event:b3:{hex}`. `fence_keys` and \
     `fence_texts`: a fencing key built and read. `lower` and `lift`: the CloudEvents \
     attribute mapping, each way. `mapping` and `constants`: the mapping as data.";

fn refusal(r: &Refusal) -> Value {
    Value::String(r.sentence().to_string())
}

/// A result as `{field: value, refusal: null}` or `{field: null, refusal: sentence}`.
fn outcome<T>(field: &str, result: Result<T, Refusal>, show: impl Fn(T) -> Value) -> Value {
    match result {
        Ok(v) => json!({ field: show(v), "refusal": null }),
        Err(r) => json!({ field: null, "refusal": refusal(&r) }),
    }
}

fn s(v: &Value, field: &str) -> String {
    v[field]
        .as_str()
        .unwrap_or_else(|| panic!("`{field}` is a string in {v}"))
        .to_string()
}

fn opt(v: &Value, field: &str) -> Option<String> {
    v.get(field).and_then(Value::as_str).map(str::to_string)
}

fn u64_of(text: &str) -> u64 {
    text.parse().unwrap_or_else(|_| panic!("`{text}` is a u64"))
}

fn token_rows(inputs: &Value, parse: impl Fn(&str) -> Result<String, Refusal>) -> Value {
    Value::Array(
        inputs
            .as_array()
            .expect("a list")
            .iter()
            .map(|input| {
                let input = input.as_str().expect("a string");
                let mut row = outcome("token", parse(input), Value::String);
                row["input"] = json!(input);
                row
            })
            .collect(),
    )
}

fn entry_json(e: Entry) -> Value {
    let mut v = match &e {
        Entry::Layer { layer, id } => {
            json!({"family": "layer", "layer": layer.as_str(), "id": id.as_str()})
        }
        Entry::Transition { ledger, seq } => {
            json!({"family": "transition", "ledger": ledger.as_str(), "seq": seq.to_string()})
        }
        _ => unreachable!("a family this test does not know"),
    };
    v["iri"] = json!(e.iri());
    v["feed_source"] = json!(e.feed().source());
    v["feed"] = json!(e.feed().feed());
    v
}

fn feed_json(f: Feed) -> Value {
    let family = match &f {
        Feed::Layer(_) => "layer",
        Feed::Transition(_) => "transition",
        _ => unreachable!("a family this test does not know"),
    };
    json!({"family": family, "source": f.source(), "feed": f.feed()})
}

fn cursor_json(c: Cursor) -> Value {
    json!({
        "epoch": c.epoch().map(|e| e.as_str().to_string()),
        "after": c.after().to_string(),
        "text": c.to_string(),
    })
}

fn fence_json(k: FenceKey) -> Value {
    json!({
        "ledger": k.ledger().as_str(),
        "item": k.item().as_str(),
        "epoch": k.epoch().to_string(),
        "step": k.step().as_str(),
        "key": k.to_string(),
    })
}

fn attributes_json(a: Attributes) -> Value {
    json!({
        "specversion": a.specversion,
        "id": a.id,
        "source": a.source,
        "type": a.kind,
        "subject": a.subject,
        "time": a.time,
        "datacontenttype": a.datacontenttype,
        "ikseq": a.ikseq,
        "ikepoch": a.ikepoch,
        "traceparent": a.traceparent,
    })
}

fn compute() -> String {
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(CASES).expect("the cases")).expect("JSON");
    let t = &cases["tokens"];

    let tokens = json!({
        "layer": token_rows(&t["layer"], |s| LayerName::parse(s).map(|x| x.to_string())),
        "ledger": token_rows(&t["ledger"], |s| LedgerName::parse(s).map(|x| x.to_string())),
        "entry_id": token_rows(&t["entry_id"], |s| EntryId::parse(s).map(|x| x.to_string())),
        "epoch": token_rows(&t["epoch"], |s| FeedEpoch::parse(s).map(|x| x.to_string())),
        "listener_id": token_rows(&t["listener_id"], |s| ListenerId::parse(s).map(|x| x.to_string())),
        "item": token_rows(&t["item"], |s| ItemRef::parse(s).map(|x| x.to_string())),
        "step": token_rows(&t["step"], |s| Step::parse(s).map(|x| x.to_string())),
    });

    let names: Vec<Value> = cases["names"]
        .as_array()
        .expect("names")
        .iter()
        .map(|iri| {
            let iri = iri.as_str().expect("an IRI");
            let entry = outcome("entry", Entry::parse(iri), entry_json);
            let feed = outcome("feed", Feed::parse(iri), feed_json);
            let listener = outcome(
                "listener",
                Listener::parse(iri),
                |l| json!({"id": l.id().as_str(), "iri": l.iri()}),
            );
            json!({
                "iri": iri,
                "entry": entry["entry"], "entry_refusal": entry["refusal"],
                "feed": feed["feed"], "feed_refusal": feed["refusal"],
                "listener": listener["listener"], "listener_refusal": listener["refusal"],
            })
        })
        .collect();

    let cursors: Vec<Value> = cases["cursors"]
        .as_array()
        .expect("cursors")
        .iter()
        .map(|text| {
            let text = text.as_str().expect("a string");
            let mut row = outcome("cursor", Cursor::parse(text), cursor_json);
            row["text"] = json!(text);
            row
        })
        .collect();

    let event_keys: Vec<Value> = cases["event_keys"]
        .as_array()
        .expect("event_keys")
        .iter()
        .map(|c| {
            let (source, id) = (s(c, "source"), s(c, "id"));
            let mut row = outcome("key", event_key(&source, &id), |k| json!(k.as_str()));
            row["source"] = json!(source);
            row["id"] = json!(id);
            row
        })
        .collect();

    let fence_keys: Vec<Value> = cases["fence_keys"]
        .as_array()
        .expect("fence_keys")
        .iter()
        .map(|c| {
            let built = FenceKey::new(
                &s(c, "ledger"),
                &s(c, "item"),
                u64_of(&s(c, "epoch")),
                &s(c, "step"),
            );
            let mut row = outcome("key", built, |k| json!(k.to_string()));
            for f in ["ledger", "item", "epoch", "step"] {
                row[f] = c[f].clone();
            }
            row
        })
        .collect();

    let fence_texts: Vec<Value> = cases["fence_texts"]
        .as_array()
        .expect("fence_texts")
        .iter()
        .map(|text| {
            let text = text.as_str().expect("a string");
            let mut row = outcome("fence", FenceKey::parse(text), fence_json);
            row["text"] = json!(text);
            row
        })
        .collect();

    let lower: Vec<Value> = cases["lower"]
        .as_array()
        .expect("lower")
        .iter()
        .map(|c| {
            let facts = EntryFacts {
                entry: Entry::parse(&s(c, "entry")).expect("a case's entry parses"),
                seq: NonZeroU64::new(u64_of(&s(c, "seq"))).expect("a case's seq is not 0"),
                epoch: opt(c, "epoch").map(|e| FeedEpoch::parse(&e).expect("an epoch")),
                kind: s(c, "kind"),
                subject: opt(c, "subject"),
                created: opt(c, "created"),
                content_type: opt(c, "content_type"),
                traceparent: opt(c, "traceparent"),
            };
            let mut row = outcome("attributes", cloudevents::lower(&facts), attributes_json);
            row["name"] = c["name"].clone();
            row
        })
        .collect();

    let lift: Vec<Value> = cases["lift"]
        .as_array()
        .expect("lift")
        .iter()
        .map(|c| {
            let a = &c["attributes"];
            let attrs = Attributes {
                specversion: s(a, "specversion"),
                id: s(a, "id"),
                source: s(a, "source"),
                kind: s(a, "type"),
                subject: opt(a, "subject"),
                time: opt(a, "time"),
                datacontenttype: opt(a, "datacontenttype"),
                ikseq: opt(a, "ikseq"),
                ikepoch: opt(a, "ikepoch"),
                traceparent: opt(a, "traceparent"),
            };
            let mut row = outcome("intake", cloudevents::lift(&attrs), |i| {
                json!({
                    "key": i.key.as_str(),
                    "authored_time": i.authored_time,
                    "advisory": i.advisory.map(|c| c.to_string()),
                    "entry": i.entry.map(|e| e.iri()),
                    "feed": i.feed.map(|f| f.source()),
                })
            });
            row["name"] = c["name"].clone();
            row
        })
        .collect();

    let mapping: Vec<Value> = MAPPING
        .iter()
        .map(|r| json!({"attribute": r.attribute, "required": r.required, "ikigai": r.ikigai}))
        .collect();

    let file = json!({
        "about": ABOUT,
        "constants": {
            "specversion": cloudevents::SPECVERSION,
            "ext_seq": cloudevents::EXT_SEQ,
            "ext_epoch": cloudevents::EXT_EPOCH,
            "ext_traceparent": cloudevents::EXT_TRACEPARENT,
            "event_key_domain": EVENT_KEY_DOMAIN,
            "event_key_prefix": EVENT_KEY_PREFIX,
            "fence_key_prefix": FENCE_KEY_PREFIX,
        },
        "tokens": tokens,
        "names": names,
        "cursors": cursors,
        "event_keys": event_keys,
        "fence_keys": fence_keys,
        "fence_texts": fence_texts,
        "lower": lower,
        "lift": lift,
        "mapping": mapping,
    });
    // serde_json's map is ordered by key, so the bytes depend on the cases alone.
    let mut text = serde_json::to_string_pretty(&file).expect("serializable");
    text.push('\n');
    text
}

#[test]
fn the_committed_vectors_are_what_this_crate_computes() {
    let committed = std::fs::read_to_string(VECTORS).expect(
        "tests/vectors/identifier_vectors.json exists; regenerate it with \
         `cargo test -p ikigai-events --test vectors -- --ignored`",
    );
    assert!(
        committed == compute(),
        "tests/vectors/identifier_vectors.json differs from what this crate computes; \
         regenerate it with `cargo test -p ikigai-events --test vectors -- --ignored` and review \
         the diff"
    );
}

#[test]
#[ignore = "writes the vectors file; run it after editing the cases"]
fn regenerate() {
    std::fs::write(VECTORS, compute()).expect("written");
}
