# Shared identifier vectors

`identifier_vectors.json` is what the `ikigai-events` crate computes for every name, key and
CloudEvents mapping it owns. It is the one copy every face that mirrors those functions by hand
(Python, TypeScript, .NET) pins itself against.

It cannot drift from the crate: `tests/vectors.rs` recomputes every output from
`identifier_cases.json` and fails if the committed file differs by a byte. To add a case, edit
`identifier_cases.json` and regenerate:

```text
cargo test -p ikigai-events --test vectors -- --ignored
```

## Consuming it from another face

Copy `identifier_vectors.json` **verbatim** into the face's tests (do not reformat it) and
assert against it. Every 64-bit number (a sequence number, a cursor position, a claim epoch) is
a **string** of decimal digits, so a face whose numbers are doubles reads it exactly. Where the
crate refuses, the value is `null` and `refusal` holds the sentence; a face must refuse exactly
there, and matching the sentence byte for byte is recommended.

| section | input | what the face must compute |
|---|---|---|
| `tokens.{kind}[]` | `input` | `token` (the input, accepted) or `refusal`, for `layer`, `ledger`, `entry_id`, `epoch`, `listener_id`, `item` and `step` |
| `names[]` | `iri` | read as an entry (`entry`: `family`, its fields, canonical `iri`, `feed_source`, `feed`), as a feed (`feed`: `family`, `source`, `feed`) and as a listener (`listener`: `id`, `iri`), each with its `…_refusal` |
| `cursors[]` | `text` | `cursor` (`epoch` or null, `after`, the canonical `text`) or `refusal` |
| `event_keys[]` | `source`, `id` | `key`: `event:b3:` and the hex BLAKE3 digest of three fields, `constants.event_key_domain`, `source`, `id`, each fed as its UTF-8 byte length (a little-endian unsigned 64-bit integer) then its bytes |
| `fence_keys[]` | `ledger`, `item`, `epoch`, `step` | `key`: `fence:{ledger}:{item}:{epoch}:{step}`, or `refusal` |
| `fence_texts[]` | `text` | `fence` (its four fields and the canonical `key`) or `refusal` |
| `lower[]` | the entry facts in `identifier_cases.json` | `attributes`: the CloudEvent's context attributes, or `refusal` |
| `lift[]` | `attributes` in `identifier_cases.json` | `intake`: `key`, `authored_time`, `advisory` (a cursor's text), `entry` and `feed` (the ikigai names it carries, or null), or `refusal` |
| `mapping[]` | none | the attribute mapping, as data |
| `constants` | none | the CloudEvents version, extension names, and key prefixes and domain |

The `ikigai-python` virtual environment's `blake3` reproduced every `event_keys` row when the
file was first written (2026-11-23), with nothing but the framing above.
