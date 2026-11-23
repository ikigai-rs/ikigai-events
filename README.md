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

Status: scaffold. Nothing is published.

## License

MIT OR Apache-2.0.
