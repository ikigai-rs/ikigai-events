# CloudEvents vectors

`cloudevents_vectors.json` is what the `ikigai-events-cloudevents` crate computes for every
event, batch and Turtle form in `cloudevents_cases.json`. It is the one copy every face that
mirrors the CloudEvents formats by hand (Python, TypeScript, .NET) pins itself against, beside
the identifiers crate's
[`identifier_vectors.json`](../../../ikigai-events/tests/vectors/README.md), whose `lower` and
`lift` rows cover the attribute mapping itself.

It cannot drift from the crate: `tests/vectors.rs` recomputes every row from
`cloudevents_cases.json` and fails if the committed file differs by a byte. To add a case, edit
`cloudevents_cases.json` and regenerate:

```text
cargo test -p ikigai-events-cloudevents --test vectors -- --ignored
```

## Consuming it from another face

Copy `cloudevents_vectors.json` **verbatim** into the face's tests (do not reformat it) and
assert against it. Every input is TEXT (`input`), so its bytes are the input. Where the crate
refuses, the outputs are `null` and `refusal` holds the sentence; a face must refuse exactly
there, and matching the sentence byte for byte is recommended. Every bound is a string of
decimal digits under `constants`.

| section | input | what the face must compute |
|---|---|---|
| `events[]` | `input`, one event in the JSON event format | `json`: the event written back, compact, byte for byte (the four required attributes, then `subject`, `time`, `datacontenttype`, `dataschema`, `ikseq`, `ikepoch`, `traceparent`, then other extensions by name, then `data` or `data_base64`; a JSON `data` value with its object keys sorted); `node`; `intake` (`key`, `authored_time`, `advisory`, `entry`, `feed`, as the identifiers crate's `lift`); `turtle`, the Turtle form |
| `batches[]` | `input`, a batch in the JSON batch format | `json`, the batch written back, and `turtle` |
| `turtle[]` | `input`, Turtle | `json`: the form lowered to a batch, or `refusal` |

**Compare `turtle` as a graph, not as bytes.** It is what this crate's serializer (oxttl) writes:
the same triples in another serializer's layout are the same form. Every `turtle` an event or
batch row holds lowers back to that row's `json`, so a face can use those as further `turtle[]`
inputs.

## The form, in one paragraph

One node per event, `urn:iki:event:key:` and the event key without its `event:` prefix (so
`urn:iki:event:key:b3:{hex}`), typed `ce:Event` (`ce:` is
`https://ikigai-rs.dev/ns/cloudevents#`). Every attribute `{name}` is `ce:{name}`: `source` and
`dataschema` typed `xsd:anyURI`, `time` typed `xsd:dateTime` with its text as sent, an integer
extension `xsd:int`, a boolean extension `xsd:boolean`, every other attribute a plain string.
String data is `ce:data` as a plain string, any other JSON data `ce:data` typed `rdf:JSON` (its
compact text, object keys sorted), binary data `ce:data_base64` typed `xsd:base64Binary`. The
form is closed: anything else is refused.
