# ikigai-events-relay

Delivery out of [ikigai](https://github.com/ikigai-rs) (ledger #4118): for each subscription, a
relay delivers its feed's entries, as CloudEvents, to a webhook, to a connector process, or
through a server-sent events face, and records what became of each one.

The design is ikigai-core's `docs/design/events-and-resources.md`, section 9 item 7, with
sections 5.4, 6.2, 6.3 and 6.4. It builds on the `ikigai-events` identifiers crate (feeds,
cursors, event keys, the refusal type) and on `ikigai-events-cloudevents` (the JSON event
format and an event's checks), never the reverse.

Status: built, unpublished (`publish = false` until reviewed).

## The resource map

| set | name | kind | type |
|---|---|---|---|
| a subscription | `urn:relay:subscription:id:{id}` | **ATOM** | `Subscription`: what to follow (a `Feed`), a filter (by `type`), a destination, a narrowed grant |
| its cursor | `urn:relay:cursor:subscription:{id}` | **ATOM** | a `Cursor`, written by `Store::commit` after the destination acknowledges |
| a delivery | `urn:relay:delivery:subscription:{id}:seq:{n}` | **ATOM** | `Delivery`: attempts, outcome, the last failure's class |
| dead letters | a tuplespace layer per relay | **ATOM** (a layer) | `Store::dead_letter`: the record and the JSON event, for a person to take |
| a relay's status | `urn:relay:status:subscription:{id}` | composition | named only (`SubscriptionId::status`); not built yet |

The relay holds no atoms itself: a host implements `Store` over its own store (the record and
the cursor in ONE update), and `MemoryStore` keeps them in memory.

## One delivery

`Relay::step` takes the subscription's next entry, lowered to a CloudEvent by its host, and:

1. checks it is the followed feed's, in the cursor's epoch, at the position right after the
   cursor, and refuses it with one sentence otherwise (nothing is sent);
2. skips it when the filter passes over it (the cursor moves past it, nothing is sent);
3. delivers it through the kernel, under the relay's capability attenuated to the
   subscription's grant, so a scope withdrawn from either stops delivery at the next attempt:
   - **webhook**: reads the signing secret from its `urn:` name (an ikigai-secret one), then
     issues `sink urn:http:post` (ikigai-http) with `content_type=application/cloudevents+json`
     and the Standard Webhooks headers: `webhook-id` is the event key (`event:b3:{hex}`, the
     digest of the feed's source and the entry IRI), the same on every attempt;
   - **connector**: issues a Sink of the JSON event to the connector process mounted at its
     `urn:` name, with `idempotency-key=` the event key;
   - **SSE**: nothing is pushed; a door writes `sse_frame` per entry and resumes from
     `sse_resume(Last-Event-ID)`.
4. decides by the failure's `FailureClass`: acknowledged → `Delivered`; transient, retry-after
   or unknown → `Retry` after the destination's `Retry-After` or the policy's doubling
   backoff (5 s, at most an hour, eight attempts by default); `Denied` → `Stopped`, the cursor
   stays; anything else, or the last attempt → `DeadLettered`, and the cursor moves on.

The relay links no HTTP client and no broker client, reads no feed and keeps no timer: its
host follows the feed with `after=` and `wait=`, and waits out a `Retry`. Delivery is at
least once, never exactly once; the destination takes an entry once by its event key.

A signature says the delivery came from this relay; it carries no authority, and the
receiving door decides what it may do. Signatures come from `standardwebhooks`, the
specification's reference implementation, never a private HMAC; `tests/signature.rs` pins it
against the specification's published example and against an HMAC computed outside Rust.

## Not built yet

- the atoms' Source faces and the status composition (`urn:relay:status:subscription:{id}`);
- the feed follower (reading `source {feed} after= wait=` and lowering each entry), which
  needs the layer feed's epoch (item 3) to land;
- the dead-letter tuplespace's own layer: `Store::dead_letter` is the seam a host fills by
  offering a tuple to it.

## License

MIT OR Apache-2.0.
