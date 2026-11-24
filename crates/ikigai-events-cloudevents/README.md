# ikigai-events-cloudevents

CloudEvents for [ikigai](https://github.com/ikigai-rs): an event in the JSON event format
(`application/cloudevents+json`), or a batch of them (`application/cloudevents-batch+json`),
lifted to Turtle and lowered back, as a transreptor pair; and the keyed Sink an intake writes for
each event, so a redelivery writes nothing.

It builds on the `ikigai-events` identifiers crate: the attribute mapping, the event key and the
ikigai names an event carries are that crate's. This one adds the formats, and the checks they
define, each through a vetted parser:

| what | checked by |
|---|---|
| the JSON formats | `serde_json`, after an iterative nesting scan (`ikigai_rdf::check_json_nesting`) |
| `time`, RFC 3339 | the `time` crate's well-known RFC 3339 parser |
| `source` (URI-reference), `dataschema` (URI) | `oxiri`, ASCII only, since RFC 3987 adds only non-ASCII to RFC 3986's grammar |
| `data_base64`, RFC 4648 | the `base64` crate's standard engine |
| `traceparent`, W3C Trace Context | this crate, by the specification's rules: no vetted parser exists to reuse (see `src/check.rs`) |
| Turtle | `oxttl`, after `ikigai_rdf::check_turtle_nesting` |

Status: built, unpublished (`publish = false` until reviewed, like the identifiers crate).

## The resource map

| set | name | kind | how |
|---|---|---|---|
| the lift | `urn:iki:cloudevent:lift` | transreptor | `content` (one event, or a batch) → `text/turtle`; pure, cacheable |
| the lower | `urn:iki:cloudevent:lower` | transreptor | `content` (the Turtle form) → `as=application/cloudevents+json` (default) or `…-batch+json`; pure, cacheable |
| an event, as RDF | `urn:iki:event:key:b3:{hex}` | node in a representation | named by its event key, never by its `id` |
| an intake's write | a Sink of one event's form to the intake's target, `idempotency-key=` its event key | composition (`lift_event`, `sink_request`) | the target, not this crate, keeps the replay record |

It holds no state: no atom, no store, no clock. The two transreptors are the code that cannot be
a composition (a format mapping, written once, with vectors); everything else an intake or a
relay does composes them. A host binds `space()`, and its transreption planner then converts
between the three media types by `as=`, one event to the batch form through Turtle, the
canonical hub.

## The Turtle form

```turtle
@prefix ce: <https://ikigai-rs.dev/ns/cloudevents#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
<urn:iki:event:key:b3:f8b0072e431ab22c4eef2621e4cb08104de988e9c9dedac339b8bbbf536c376d> a ce:Event ;
	ce:specversion "1.0" ;
	ce:id "urn:layer:talk:entry:01m84z0jyt0000000000007" ;
	ce:source "urn:layer:talk"^^xsd:anyURI ;
	ce:type "say" ;
	ce:time "2026-11-23T20:00:52.197Z"^^xsd:dateTime ;
	ce:datacontenttype "text/turtle" ;
	ce:ikseq "7" ;
	ce:ikepoch "5f0c9e3a1b2d4c6e8f0a1b2c3d4e5f60" ;
	ce:data "<urn:layer:talk:entry:01m84z0jyt0000000000007> <https://ikigai-rs.dev/ns#body> \"hello\" ." .
```

Every attribute `{name}`, extensions included, is `ce:{name}`; the data is `ce:data` (a string,
or any other JSON value typed `rdf:JSON`) or `ce:data_base64`. The vocabulary is
[`src/vocabulary.ttl`](src/vocabulary.ttl). Four decisions the form carries:

- **The node is the event key's, never the `id`.** A foreign event can spell an ikigai entry's
  IRI as its `id`; naming the node by the key means it still states nothing about that entry.
- **Nothing in the form is a term the receiving log stamps.** `time` is `ce:time`, authored
  time, never `dcterms:created`; `ikseq` is `ce:ikseq`, advisory, never `ik:seq`. A layer host
  refuses an entry that sends its own stamps, and an intake must not look like one.
- **The form is closed.** The lower refuses any triple the lift would not write, so the pair
  loses nothing in either direction (it is declared lossless). A batch's members are carried
  as a set: the lower writes them in the order their nodes first appear.
- **`time` is kept as it was sent**, typed `xsd:dateTime`, even where RFC 3339 and
  `xsd:dateTime` disagree (a lowercase `t`, a space separator, an offset past 14 hours).

## Bounds

One event at most 1 MiB, a batch or a form at most 8 MiB and 1,000 events, and JSON at most 64
levels deep, envelope included, refused by a scan before any parse. `tests/depth.rs` runs every
deep case in a child process on a 2 MiB thread, through both doors.

## Vectors

[`tests/vectors/`](tests/vectors/README.md): every event, batch and form the tests read, with
what this crate writes for it and every refusal sentence, for the faces to copy verbatim.

## License

MIT OR Apache-2.0.
