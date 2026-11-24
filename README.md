# ikigai-events

Events, streams and resources for [ikigai](https://github.com/ikigai-rs): the identifiers every
event-facing module shares, and subcrates for specific bridges.

ikigai models state as resources, a fact about a change as an immutable entry, and a stream as a
representation body. Event-driven systems describe the same world from the other end. This
repository holds the pieces both sides need to agree on, so a module that publishes or consumes
events names them the same way:

- the names of entries, feeds, cursors, feed epochs and transitions;
- how an event's source and id become an idempotency key, and how a lease epoch is carried into a
  side effect (a fencing key);
- the mapping between an ikigai entry and a CloudEvent's attributes.

Specific bridges (a CloudEvents face, delivery out of ikigai, intake from webhooks and brokers)
live here as subcrates, each depending on the shared identifiers crate.

The design is ikigai-core's
[`docs/design/events-and-resources.md`](https://github.com/ikigai-rs/ikigai-core/blob/main/docs/design/events-and-resources.md).
Its rules hold here: a golden-thread cut is a hint to re-read, never an event; ordering and replay
belong to the log that owns the entries; effects are made safe by idempotency keys and fencing,
never by an exactly-once promise; and authority never travels with an event.

Status: built and unpublished (`publish = false` until reviewed): the identifiers crate, and
the first bridge, [`ikigai-events-cloudevents`](crates/ikigai-events-cloudevents/README.md).

## The resource map

Names are singular, the type first, and every partition is by a named key. The identifiers
crate holds NO state and serves no endpoint: it is the one place these names, keys and the
mapping are spelled, so the modules that own the atoms (the layer host, the ledger, the
kernel, a relay) and every consumer agree. Each row is a type in `crates/ikigai-events`.

| set | name or form | kind | owner of the atom | type |
|---|---|---|---|---|
| a layer feed | source `urn:layer:{layer}`; read at `urn:layer:{layer}:feed` (`after=`, `wait=`, `epoch=`) | composition | ikigai-layers | `Feed::Layer` |
| a layer entry | `urn:layer:{layer}:entry:{id}` | **ATOM**, immutable | ikigai-layers | `Entry::Layer` |
| a ledger's transitions | `urn:iki:ledger:{ledger}:transition` (source and feed are one name) | composition | ikigai-ledger (ledger #4116) | `Feed::Transition` |
| a ledger transition | `urn:iki:ledger:{ledger}:transition:seq:{n}`, `n` from 1 | **ATOM**, immutable | ikigai-ledger (ledger #4116) | `Entry::Transition` |
| a feed epoch | a token, `[a-z0-9-]{1,64}` | value | the door that owns the feed | `FeedEpoch` |
| a cursor's value | `{epoch}:{after}`, or `{after}` for a feed with no epoch; also a server-sent event's `id` | value | the consumer | `Cursor` |
| a listener | `urn:kernel:listener:id:{id}` | **ATOM** (session state) | ikigai-core (ledger #4112) | `Listener` |
| an event's idempotency key | `event:b3:{hex}`, from (source, id) | value | whoever takes the event in | `event_key` |
| a fencing key | `fence:{ledger}:{item}:{epoch}:{step}` | value | whoever performs the effect | `FenceKey` |
| the CloudEvents mapping | attribute ↔ ikigai term | data and two functions | this crate | `cloudevents::{MAPPING, lower, lift}` |
| the CloudEvents lift and lower | `urn:iki:cloudevent:lift`, `urn:iki:cloudevent:lower` | transreptor pair | `ikigai-events-cloudevents` | `space()` |
| an event, as RDF | `urn:iki:event:key:b3:{hex}`, named by its event key | node in a representation | `ikigai-events-cloudevents` | `event_node` |

Three decisions the map carries:

- **A cursor's NAME belongs to its consumer.** A cursor is an atom in the consumer's own space,
  written after the effect is recorded (a relay's is its subscription's). What every consumer
  shares is the cursor's value and its text, so that is what this crate names.
- **One spelling per name.** No name is percent-encoded: a token outside its alphabet is
  refused with one sentence. A ledger's transitions are always named with the ledger,
  `default` included, because an idempotency key derived from a `source` needs one spelling.
- **Keys are core's.** Both keys are `ikigai_core::IdempotencyKey`s, so a keyed Sink takes
  either as `idempotency-key=` unchanged, and the event key's digest is core's `b3:` content id.

The shared vectors, which the Python, TypeScript and .NET faces copy verbatim, are
[`crates/ikigai-events/tests/vectors/`](crates/ikigai-events/tests/vectors/README.md) and, for
the CloudEvents formats,
[`crates/ikigai-events-cloudevents/tests/vectors/`](crates/ikigai-events-cloudevents/tests/vectors/README.md).

## License

MIT OR Apache-2.0.
