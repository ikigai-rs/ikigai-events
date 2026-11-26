//! The relay against a kernel: stand-ins for ikigai-http's `urn:http:post`, a secret and a
//! connector, each requiring its own scope as the real ones do, so the kernel's capability
//! check, not the relay, decides what a subscription may reach.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::executor::block_on;
use ikigai_core::{
    ArgSpec, Capability, Description, Endpoint, EndpointSpace, Error, Exact, Invocation, Kernel,
    ReprType, Representation, Result, Verb,
};
use ikigai_events::{event_key, Cursor, Feed};
use ikigai_events_cloudevents::{parse_event, to_json, Event};
use ikigai_events_relay::{
    Destination, Filter, MemoryStore, Outcome, Policy, Relay, StepError, Store, Subscription,
    SubscriptionId,
};

const NET: &str = "urn:cap:net:hooks.example.com";
const SECRET_CAP: &str = "urn:cap:secret:ops-hook";
const CONNECTOR_CAP: &str = "urn:cap:connector:broker";
const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";

/// One request a stand-in received: its arguments, as text.
type Seen = Vec<(String, String)>;

#[derive(Clone, Default)]
struct Recorder {
    seen: Arc<Mutex<Vec<Seen>>>,
    script: Arc<Mutex<VecDeque<Error>>>,
}

impl Recorder {
    fn fail_next(&self, error: Error) {
        self.script.lock().unwrap().push_back(error);
    }
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

struct Sink {
    name: &'static str,
    requires: &'static str,
    args: &'static [&'static str],
    recorder: Recorder,
}

#[async_trait]
impl Endpoint for Sink {
    async fn invoke(&self, inv: &Invocation<'_>) -> Result<Representation> {
        let mut seen = Vec::new();
        for name in self.args {
            if let Ok(value) = inv.inline_str(name) {
                seen.push((name.to_string(), value.to_string()));
            }
        }
        self.recorder.seen.lock().unwrap().push(seen);
        if let Some(error) = self.recorder.script.lock().unwrap().pop_front() {
            return Err(error);
        }
        Ok(Representation::new(
            ReprType::new("text/plain"),
            b"ok".to_vec(),
        ))
    }

    fn name(&self) -> &str {
        self.name
    }

    fn describe(&self) -> Description {
        let mut d = Description::new(self.name)
            .verb(Verb::Sink)
            .requires(self.requires);
        for name in self.args {
            d = d.input(ArgSpec::new(*name).class(XSD_STRING).optional());
        }
        d
    }
}

struct Secret;

#[async_trait]
impl Endpoint for Secret {
    async fn invoke(&self, _inv: &Invocation<'_>) -> Result<Representation> {
        Ok(Representation::new(
            ReprType::new("text/plain"),
            SECRET.as_bytes().to_vec(),
        ))
    }

    fn name(&self) -> &str {
        "secret"
    }

    fn describe(&self) -> Description {
        Description::new("secret")
            .verb(Verb::Source)
            .requires(SECRET_CAP)
    }
}

struct World {
    post: Recorder,
    connector: Recorder,
    kernel: Arc<Kernel>,
}

fn world() -> World {
    let post = Recorder::default();
    let connector = Recorder::default();
    let space = EndpointSpace::new()
        .bind(
            Exact::new("urn:http:post"),
            Sink {
                name: "http-post",
                requires: NET,
                args: &["url", "content_type", "headers", "content"],
                recorder: post.clone(),
            },
        )
        .bind(Exact::new("urn:secret:ops-hook"), Secret)
        .bind(
            Exact::new("urn:connector:broker"),
            Sink {
                name: "broker",
                requires: CONNECTOR_CAP,
                args: &["content_type", "content", "idempotency-key"],
                recorder: connector.clone(),
            },
        );
    World {
        post,
        connector,
        kernel: Arc::new(Kernel::new(Arc::new(space))),
    }
}

fn granted() -> Capability {
    Capability::scoped([NET, SECRET_CAP, CONNECTOR_CAP, "urn:cap:kernel:cut"])
}

fn subscription(destination: Destination) -> Subscription {
    Subscription {
        id: SubscriptionId::parse("talk-to-ops").unwrap(),
        follow: Feed::parse("urn:layer:talk").unwrap(),
        filter: Filter::types(["say"]),
        destination,
        grant: [NET, SECRET_CAP, CONNECTOR_CAP, "urn:cap:kernel:cut"]
            .map(String::from)
            .into(),
    }
}

fn webhook() -> Subscription {
    subscription(
        Destination::webhook("https://hooks.example.com/in", "urn:secret:ops-hook").unwrap(),
    )
}

fn entry(seq: u64, kind: &str) -> Event {
    let json = format!(
        r#"{{"specversion":"1.0","id":"urn:layer:talk:entry:{seq}","source":"urn:layer:talk","type":"{kind}","ikseq":"{seq}","ikepoch":"e1","data":"hello"}}"#
    );
    parse_event(json.as_bytes()).unwrap()
}

fn arg<'a>(seen: &'a Seen, name: &str) -> &'a str {
    &seen.iter().find(|(n, _)| n == name).unwrap().1
}

const NOW: i64 = 1_800_000_000;

#[test]
fn a_webhook_delivery_is_signed_and_moves_the_cursor_after_it_is_acknowledged() {
    let w = world();
    let relay = Relay::new(w.kernel.clone(), granted());
    let store = MemoryStore::new();
    let sub = webhook();
    let event = entry(1, "say");

    let delivery = block_on(relay.step(&store, &sub, &event, NOW)).unwrap();
    assert_eq!(delivery.outcome, Outcome::Delivered);
    assert_eq!(delivery.attempts, 1);
    assert_eq!(
        delivery.iri(),
        "urn:relay:delivery:subscription:talk-to-ops:seq:1"
    );
    let key = event_key("urn:layer:talk", "urn:layer:talk:entry:1").unwrap();
    assert_eq!(delivery.key, key);
    assert_eq!(
        block_on(store.cursor(&sub.id)).unwrap(),
        Some(Cursor::parse("e1:1").unwrap())
    );

    let posts = w.post.seen();
    assert_eq!(posts.len(), 1);
    let post = &posts[0];
    assert_eq!(arg(post, "url"), "https://hooks.example.com/in");
    assert_eq!(arg(post, "content_type"), "application/cloudevents+json");
    assert_eq!(arg(post, "content").as_bytes(), to_json(&event));

    // The headers verify under the specification's reference verifier, with the same key.
    let mut headers = http::HeaderMap::new();
    for line in arg(post, "headers").lines() {
        let (name, value) = line.split_once(": ").unwrap();
        headers.insert(
            http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            http::HeaderValue::from_str(value).unwrap(),
        );
    }
    assert_eq!(headers["webhook-id"], key.as_str());
    assert_eq!(headers["webhook-timestamp"], NOW.to_string().as_str());
    standardwebhooks::Webhook::new(SECRET)
        .unwrap()
        .verify_with_timestamp(arg(post, "content").as_bytes(), &headers, NOW)
        .expect("the reference verifier accepts the delivery");
}

#[test]
fn a_transient_failure_is_retried_with_backoff_and_the_cursor_waits() {
    let w = world();
    let relay = Relay::new(w.kernel.clone(), granted());
    let store = MemoryStore::new();
    let sub = webhook();
    let event = entry(1, "say");

    w.post
        .fail_next(Error::Unavailable("connection reset".into()));
    let first = block_on(relay.step(&store, &sub, &event, NOW)).unwrap();
    assert_eq!(
        first.outcome,
        Outcome::Retry {
            after: Duration::from_secs(5)
        }
    );
    assert_eq!(
        first.failure.as_ref().unwrap().0,
        ikigai_core::FailureClass::Transient
    );
    assert_eq!(block_on(store.cursor(&sub.id)).unwrap(), None);

    w.post.fail_next(Error::RetryAfter {
        delay: Some(Duration::from_secs(90)),
        detail: "rate-limited".into(),
    });
    let second = block_on(relay.step(&store, &sub, &event, NOW + 5)).unwrap();
    assert_eq!(
        second.outcome,
        Outcome::Retry {
            after: Duration::from_secs(90)
        }
    );
    assert_eq!(second.attempts, 2);

    let third = block_on(relay.step(&store, &sub, &event, NOW + 95)).unwrap();
    assert_eq!(third.outcome, Outcome::Delivered);
    assert_eq!(third.attempts, 3);
    assert_eq!(
        block_on(store.cursor(&sub.id)).unwrap(),
        Some(Cursor::parse("e1:1").unwrap())
    );

    // Every attempt carried the same webhook-id, so the receiver takes the entry once.
    let ids: Vec<String> = w
        .post
        .seen()
        .iter()
        .map(|p| arg(p, "headers").lines().next().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| id == &ids[0]));
}

#[test]
fn after_its_last_attempt_an_entry_is_dead_lettered_and_the_cursor_moves_on() {
    let w = world();
    let policy = Policy {
        max_attempts: 2,
        ..Policy::default()
    };
    let relay = Relay::new(w.kernel.clone(), granted()).with_policy(policy);
    let store = MemoryStore::new();
    let sub = webhook();
    let event = entry(1, "say");

    w.post.fail_next(Error::Unavailable("down".into()));
    w.post.fail_next(Error::Unavailable("still down".into()));
    let first = block_on(relay.step(&store, &sub, &event, NOW)).unwrap();
    assert!(matches!(first.outcome, Outcome::Retry { .. }));
    let last = block_on(relay.step(&store, &sub, &event, NOW + 5)).unwrap();
    assert_eq!(last.outcome, Outcome::DeadLettered);
    assert_eq!(last.failure.as_ref().unwrap().1, "unavailable: still down");

    let dead = store.dead_letters();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].0, last);
    assert_eq!(dead[0].1, to_json(&event));
    assert_eq!(
        block_on(store.cursor(&sub.id)).unwrap(),
        Some(Cursor::parse("e1:1").unwrap())
    );
    assert_eq!(
        block_on(store.delivery(&sub.id, last.seq)).unwrap(),
        Some(last)
    );
}

#[test]
fn a_permanent_refusal_is_dead_lettered_at_once() {
    let w = world();
    let relay = Relay::new(w.kernel.clone(), granted());
    let store = MemoryStore::new();
    w.post.fail_next(Error::Conflict("HTTP 409".into()));
    let d = block_on(relay.step(&store, &webhook(), &entry(1, "say"), NOW)).unwrap();
    assert_eq!(d.outcome, Outcome::DeadLettered);
    assert_eq!(d.attempts, 1);
    assert_eq!(store.dead_letters().len(), 1);
}

#[test]
fn a_withdrawn_grant_stops_delivery_and_keeps_the_cursor() {
    let w = world();
    let store = MemoryStore::new();
    let sub = webhook();
    // The relay's own capability no longer holds the network scope: the subscription's
    // grant narrows from it, so it cannot reach the POST either.
    let relay = Relay::new(
        w.kernel.clone(),
        Capability::scoped([SECRET_CAP, "urn:cap:kernel:cut"]),
    );
    let d = block_on(relay.step(&store, &sub, &entry(1, "say"), NOW)).unwrap();
    assert_eq!(d.outcome, Outcome::Stopped);
    assert_eq!(d.attempts, 0, "a stop spends no attempt");
    assert_eq!(
        d.failure.as_ref().unwrap().0,
        ikigai_core::FailureClass::Denied
    );
    assert!(w.post.seen().is_empty(), "nothing was posted");
    assert_eq!(block_on(store.cursor(&sub.id)).unwrap(), None);
    assert!(store.dead_letters().is_empty());

    // A grant the subscription itself no longer holds stops it the same way, even under a
    // relay that holds everything.
    let mut narrowed = webhook();
    narrowed.grant.remove(NET);
    let relay = Relay::new(w.kernel.clone(), Capability::root());
    let d = block_on(relay.step(&store, &narrowed, &entry(1, "say"), NOW)).unwrap();
    assert_eq!(d.outcome, Outcome::Stopped);
    assert!(w.post.seen().is_empty());
}

#[test]
fn an_entry_the_filter_passes_over_is_skipped_and_acknowledged() {
    let w = world();
    let relay = Relay::new(w.kernel.clone(), granted());
    let store = MemoryStore::new();
    let sub = webhook();
    let d = block_on(relay.step(&store, &sub, &entry(1, "out"), NOW)).unwrap();
    assert_eq!(d.outcome, Outcome::Skipped);
    assert_eq!(d.attempts, 0);
    assert!(w.post.seen().is_empty());
    assert_eq!(
        block_on(store.cursor(&sub.id)).unwrap(),
        Some(Cursor::parse("e1:1").unwrap())
    );
    let next = block_on(relay.step(&store, &sub, &entry(2, "say"), NOW)).unwrap();
    assert_eq!(next.outcome, Outcome::Delivered);
}

#[test]
fn a_connector_takes_the_event_keyed_by_its_event_key() {
    let w = world();
    let relay = Relay::new(w.kernel.clone(), granted());
    let store = MemoryStore::new();
    let sub = subscription(Destination::connector("urn:connector:broker").unwrap());
    let event = entry(1, "say");
    let d = block_on(relay.step(&store, &sub, &event, NOW)).unwrap();
    assert_eq!(d.outcome, Outcome::Delivered);
    let seen = w.connector.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(arg(&seen[0], "idempotency-key"), d.key.as_str());
    assert_eq!(arg(&seen[0], "content").as_bytes(), to_json(&event));
    assert!(w.post.seen().is_empty());
}

fn refused(result: std::result::Result<ikigai_events_relay::Delivery, StepError>) -> String {
    match result {
        Err(StepError::Refused(refusal)) => refusal.to_string(),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn an_entry_out_of_order_or_from_elsewhere_is_refused_with_nothing_sent() {
    let w = world();
    let relay = Relay::new(w.kernel.clone(), granted());
    let store = MemoryStore::new();
    let sub = webhook();

    assert_eq!(
        refused(block_on(relay.step(&store, &sub, &entry(2, "say"), NOW))),
        "The event `urn:layer:talk:entry:2` is at position 2, not 1: deliver the feed's \
         entries in order, the one after the cursor `e1:0` next."
    );

    block_on(relay.step(&store, &sub, &entry(1, "say"), NOW)).unwrap();
    let other_epoch = parse_event(
        br#"{"specversion":"1.0","id":"urn:layer:talk:entry:2","source":"urn:layer:talk","type":"say","ikseq":"2","ikepoch":"e2"}"#,
    )
    .unwrap();
    assert_eq!(
        refused(block_on(relay.step(&store, &sub, &other_epoch, NOW))),
        "The event `urn:layer:talk:entry:2` is from another epoch of the feed than the cursor \
         `e1:1`: the feed was restored, so resynchronize the subscription before delivering."
    );

    let other_feed = parse_event(
        br#"{"specversion":"1.0","id":"urn:layer:notes:entry:2","source":"urn:layer:notes","type":"say","ikseq":"2","ikepoch":"e1"}"#,
    )
    .unwrap();
    assert_eq!(
        refused(block_on(relay.step(&store, &sub, &other_feed, NOW))),
        "`urn:layer:notes` is not the feed `urn:relay:subscription:id:talk-to-ops` follows: \
         deliver only its own feed's entries."
    );

    let unnumbered = parse_event(
        br#"{"specversion":"1.0","id":"urn:layer:talk:entry:2","source":"urn:layer:talk","type":"say"}"#,
    )
    .unwrap();
    assert_eq!(
        refused(block_on(relay.step(&store, &sub, &unnumbered, NOW))),
        "The event `urn:layer:talk:entry:2` carries no `ikseq`: lower the entry with its feed \
         position."
    );

    let sse = subscription(Destination::Sse);
    assert_eq!(
        refused(block_on(relay.step(
            &MemoryStore::new(),
            &sse,
            &entry(1, "say"),
            NOW
        ))),
        "`urn:relay:subscription:id:talk-to-ops` delivers to a server-sent events face, which \
         a door reads: nothing is pushed to it."
    );

    assert_eq!(
        w.post.seen().len(),
        1,
        "only the one in-order entry was posted"
    );
}

#[test]
fn a_bad_destination_is_refused_in_one_sentence() {
    assert_eq!(
        Destination::webhook(
            "https://hooks.example.com/in",
            "https://secrets.example.com/k"
        )
        .unwrap_err()
        .to_string(),
        "`https://secrets.example.com/k` is not a name for a webhook's secret: give a `urn:` \
         name the kernel resolves, never a network address."
    );
    assert_eq!(
        Destination::connector("kafka").unwrap_err().to_string(),
        "`kafka` is not a name for a connector: give a `urn:` name the kernel resolves, never \
         a network address."
    );
    assert_eq!(
        SubscriptionId::parse("Ops").unwrap_err().to_string(),
        "`Ops` is not a subscription id: use 1 to 64 lowercase letters, digits and hyphens, \
         not starting or ending with a hyphen."
    );
}
