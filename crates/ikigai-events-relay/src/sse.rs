//! The server-sent events face: one entry as one event, its `id` the cursor past it, and a
//! reconnect's `Last-Event-ID` read back as the cursor to resume from.

use ikigai_events::{shown, Cursor, FeedEpoch, Refusal};
use ikigai_events_cloudevents::{to_json, Event};

/// The media type of the face.
pub const EVENT_STREAM: &str = "text/event-stream";

/// One entry as one server-sent event: `id:` the cursor past it (`{epoch}:{seq}`, or
/// `{seq}`), and `data:` its CloudEvent in the JSON event format, on one line.
///
/// The JSON is written compact, and JSON escapes every line break inside a string, so the
/// event's text can never begin another field: a payload is data, never a field of the
/// stream. There is no `event:` field, because a CloudEvent's `type` is the sender's text;
/// a page reads it from the data.
///
/// ```
/// use ikigai_events::Cursor;
/// use ikigai_events_cloudevents::parse_event;
/// use ikigai_events_relay::sse_frame;
///
/// let event = parse_event(br#"{"specversion":"1.0","id":"urn:layer:talk:entry:7",
///     "source":"urn:layer:talk","type":"say","ikseq":"7","data":"a\nb"}"#).unwrap();
/// assert_eq!(
///     sse_frame(&Cursor::parse("e1:7").unwrap(), &event),
///     "id: e1:7\ndata: {\"specversion\":\"1.0\",\"id\":\"urn:layer:talk:entry:7\",\
///      \"source\":\"urn:layer:talk\",\"type\":\"say\",\"ikseq\":\"7\",\"data\":\"a\\nb\"}\n\n"
/// );
/// ```
pub fn sse_frame(cursor: &Cursor, event: &Event) -> String {
    let json = String::from_utf8_lossy(&to_json(event)).into_owned();
    debug_assert!(
        !json.contains(['\n', '\r']),
        "compact JSON holds no line break"
    );
    format!("id: {cursor}\ndata: {json}\n\n")
}

/// The cursor a reconnect resumes from: its `Last-Event-ID` read as a cursor, checked
/// against the feed's current epoch. A cursor from another epoch is refused, as the feed
/// itself refuses it, because its positions belong to a numbering the feed no longer has.
///
/// ```
/// use ikigai_events::FeedEpoch;
/// use ikigai_events_relay::sse_resume;
///
/// let e1 = FeedEpoch::parse("e1").unwrap();
/// assert_eq!(sse_resume("e1:7", Some(&e1)).unwrap().after(), 7);
/// assert_eq!(
///     sse_resume("e0:7", Some(&e1)).unwrap_err().to_string(),
///     "`e0:7` is a cursor from another epoch of the feed: reconnect without \
///      `Last-Event-ID` and read the feed again from the start."
/// );
/// ```
pub fn sse_resume(last_event_id: &str, epoch: Option<&FeedEpoch>) -> Result<Cursor, Refusal> {
    let cursor = Cursor::parse(last_event_id.trim())?;
    if cursor.epoch() != epoch {
        return Err(Refusal::new(format!(
            "{} is a cursor from another epoch of the feed: reconnect without \
             `Last-Event-ID` and read the feed again from the start.",
            shown(last_event_id)
        )));
    }
    Ok(cursor)
}
