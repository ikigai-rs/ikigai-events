//! The module recipe as one test: `ikigai-conformance` walks both doors with no opt-outs.
//!
//! - Both are `pure` and `cacheable`: a function of the bytes they are handed.
//! - Each needs a [`Fixture`]: the walk's minimal `x` is not an event, nor the form, and is
//!   refused as `InvalidArgument`, as it should be.
//! - `content` reads a body, so BODY-INPUTS hands it a stored body (`body_input_store`).
//! - VOCABULARY holds the form's terms to [`ikigai_events_cloudevents::VOCABULARY`].
//! - `space()` reads nothing while it is built, so it claims `urn:iki:space:cloudevent`.

use ikigai_conformance::{Fixture, Suite};
use ikigai_core::{Kernel, MemoryContentStore, Verb};
use ikigai_events_cloudevents::{parse_event, to_turtle};
use std::sync::Arc;

/// One event with every attribute the vocabulary declares, so VOCABULARY sees each term.
const EVENT: &str = r#"{"specversion":"1.0","id":"urn:layer:talk:entry:7","source":"urn:layer:talk","type":"say","subject":"urn:x","time":"2026-11-23T20:00:00Z","datacontenttype":"text/plain","dataschema":"https://example.com/s","ikseq":"7","ikepoch":"e-1","traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01","data":"hello"}"#;

#[test]
fn conforms() {
    let form =
        String::from_utf8(to_turtle(&[parse_event(EVENT.as_bytes()).unwrap()]).unwrap()).unwrap();
    let space = Arc::new(ikigai_events_cloudevents::space());
    let store = Arc::new(MemoryContentStore::new(1024 * 1024));
    let kernel = Kernel::new(space.clone()).with_content_store(store.clone());
    let report = Suite::new()
        .own("cloudevent-*")
        .vocabulary(ikigai_events_cloudevents::VOCABULARY)
        .pure("cloudevent-lift")
        .pure("cloudevent-lower")
        .cacheable("cloudevent-lift")
        .cacheable("cloudevent-lower")
        .fixture(Fixture::new("cloudevent-lift", Verb::Source).arg("content", EVENT))
        .fixture(Fixture::new("cloudevent-lower", Verb::Source).arg("content", &form))
        .self_named_space("cloudevent", ikigai_events_cloudevents::space)
        .body_space(space)
        .body_input_store(store)
        .run_blocking(&kernel);
    // Printed even when clean (`--nocapture`): the report is the record.
    eprintln!("{report}");
    assert!(report.is_clean(), "{report}");
    assert!(report.unprobed.is_empty(), "every check probes: {report}");
    assert_eq!(report.endpoints, 2, "{report}");
    assert_eq!(
        report.checks.skipped().count(),
        0,
        "every check runs: {report}"
    );
    assert_eq!(
        ikigai_core::space_iri("cloudevent").as_str(),
        ikigai_events_cloudevents::SPACE_ID
    );
}
