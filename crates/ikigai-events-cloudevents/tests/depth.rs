//! Deep input is refused before it is parsed, through both doors, and nothing aborts.
//!
//! A door serves each request on a worker with a 2 MiB stack, and a stack overflow aborts the
//! WHOLE process. So every probe runs in a CHILD PROCESS (this test binary re-executed with one
//! probe named in its environment) on a 2 MiB thread: an abort kills the child and reads here
//! as a failure with the signal named, never as a crashed test runner.

use futures::executor::block_on;
use ikigai_core::{ArgRef, Capability, Iri, Kernel, Request, Verb};
use std::process::Command;
use std::sync::Arc;

const PROBE: &str = "IKIGAI_CLOUDEVENTS_DEPTH_PROBE";
/// The bound, restated so these tests read the same against code that predates it.
const BOUND: usize = 64;
/// Far past any bound: deep enough to overflow a 2 MiB stack in any recursive parser.
const DEEP: usize = 100_000;

/// An event whose `data` nests `n` arrays, so the document nests `n + 1` levels.
fn event(n: usize) -> String {
    format!(
        r#"{{"specversion":"1.0","id":"d","source":"urn:x","type":"k","data":{}1{}}}"#,
        "[".repeat(n),
        "]".repeat(n)
    )
}

/// A probe's door and content.
fn input(name: &str, n: usize) -> (&'static str, String) {
    let lift = "urn:iki:cloudevent:lift";
    let lower = "urn:iki:cloudevent:lower";
    match name {
        "event" => (lift, event(n)),
        "batch" => (lift, format!("[{}]", event(n))),
        // A form whose `rdf:JSON` data nests deep: the Turtle is flat, the literal is not.
        "json-literal" => (
            lower,
            format!(
                "<urn:iki:event:key:b3:00> a <https://ikigai-rs.dev/ns/cloudevents#Event> ; \
                 <https://ikigai-rs.dev/ns/cloudevents#data> \"{}1{}\"^^<http://www.w3.org/1999/02/22-rdf-syntax-ns#JSON> .",
                "[".repeat(n),
                "]".repeat(n)
            ),
        ),
        // Turtle's own nesting: blank node property lists, collections, triple terms.
        "blank-nodes" => (
            lower,
            format!(
                "<urn:iki:event:key:b3:00> <urn:p> {}<urn:o>{} .",
                "[ <urn:p> ".repeat(n),
                " ]".repeat(n)
            ),
        ),
        "collections" => (
            lower,
            format!(
                "<urn:iki:event:key:b3:00> <urn:p> {}<urn:o>{} .",
                "( ".repeat(n),
                " )".repeat(n)
            ),
        ),
        "triple-terms" => (
            lower,
            format!(
                "<urn:iki:event:key:b3:00> <urn:p> {}<urn:o>{} .",
                "<<( <urn:s> <urn:p> ".repeat(n),
                " )>>".repeat(n)
            ),
        ),
        other => panic!("no probe named {other}"),
    }
}

fn issue(name: &str, n: usize) -> ikigai_core::Result<String> {
    let (door, content) = input(name, n);
    let kernel = Kernel::new(Arc::new(ikigai_events_cloudevents::space()));
    let request = Request::new(Verb::Source, Iri::parse(door).unwrap())
        .with_arg("content", ArgRef::Inline(content.into_bytes()));
    block_on(kernel.issue(request, &Capability::root()))
        .map(|rep| String::from_utf8_lossy(&rep.bytes).into_owned())
}

/// The child's half: inert unless a parent named a probe. Runs it on a 2 MiB thread, prints
/// the outcome, and exits before the harness can.
#[test]
fn probe_child() {
    let Ok(spec) = std::env::var(PROBE) else {
        return;
    };
    let (name, n) = spec.split_once(':').unwrap();
    let (name, n) = (name.to_string(), n.parse::<usize>().unwrap());
    let outcome = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || match issue(&name, n) {
            Ok(text) => format!("ok {}", text.chars().take(200).collect::<String>()),
            Err(e) => format!("err {}", e.to_string().replace('\n', " ")),
        })
        .unwrap()
        .join()
        .unwrap();
    println!("\nOUTCOME {}", outcome.replace('\n', " "));
    std::process::exit(0);
}

/// The parent's half: run one probe in a child and return what it said, or panic naming how
/// it died. An abort is the defect. `output()` waits for the child, so its status is final.
fn probe(name: &str, n: usize) -> String {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "probe_child", "--nocapture", "--test-threads=1"])
        .env(PROBE, format!("{name}:{n}"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{name} nested {n} deep did not survive a 2 MiB thread: {} — {}",
        out.status,
        stderr
            .lines()
            .find(|l| l.contains("overflow"))
            .unwrap_or(&stderr)
    );
    let at = stdout
        .find("\nOUTCOME ")
        .unwrap_or_else(|| panic!("the {name} probe reported nothing: {stdout}"));
    stdout[at + "\nOUTCOME ".len()..]
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

#[test]
fn deep_json_is_refused_at_both_doors_before_it_is_parsed() {
    for name in ["event", "batch"] {
        let outcome = probe(name, DEEP);
        assert!(
            outcome.starts_with("err invalid argument `content`")
                && outcome.contains("deeper than 64"),
            "{name}: {outcome}"
        );
    }
    let outcome = probe("json-literal", DEEP);
    assert!(
        outcome.starts_with("err invalid argument `content`") && outcome.contains("deeper than 64"),
        "json-literal: {outcome}"
    );
}

#[test]
fn deep_turtle_is_refused_and_aborts_nothing() {
    for name in ["blank-nodes", "collections", "triple-terms"] {
        let outcome = probe(name, DEEP);
        assert!(
            outcome.starts_with("err invalid argument `content`"),
            "{name}: {outcome}"
        );
    }
}

#[test]
fn an_event_at_the_bound_is_lifted_on_a_small_thread_and_one_past_is_refused() {
    // `event(n)` nests `n + 1` levels.
    let ok = probe("event", BOUND - 1);
    assert!(ok.starts_with("ok "), "at the bound: {ok}");
    let past = probe("event", BOUND);
    assert!(past.contains("deeper than 64"), "one past: {past}");
}
