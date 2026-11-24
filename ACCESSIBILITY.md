# Accessibility

How this repository approaches accessibility: what it emits that people read, the standard we
aim at, the evidence that exists today and what it does not cover, the barriers we know about,
and how to report one. It is a working agreement for contributors, maintainers and coding agents.
It is not a conformance claim, an Accessibility Conformance Report or a VPAT, and it is not legal
advice.

Adapted from the [ACCESSIBILITY.md template](https://github.com/mgifford/ACCESSIBILITY.md)
(MIT).

## Project information

| Field | Value |
|---|---|
| Project | ikigai-events: Rust libraries of identifiers and bridges for ikigai's event-facing modules |
| Published at | not published: the identifiers crate is built, `publish = false` until reviewed |
| Owner | the maintainers of this repository |
| Report a barrier | [a GitHub issue](https://github.com/ikigai-rs/ikigai-events/issues/new) |
| Private channel | [private vulnerability reporting](https://github.com/ikigai-rs/ikigai-events/security/advisories/new), for anything security-sensitive |
| Target | WCAG 2.2 Level AA, for any page or terminal that presents this output |
| Status | Evaluation in progress. No conformance claim. |
| Last reviewed | 2026-11-23 |
| Next review | when a bridge crate first emits a face, terminal text or a delivered event |

## Commitment

People will read what these crates emit in a terminal, through a client, or read aloud by a
screen reader, and an agent will read it as data. An answer that carries its meaning only in
layout, color or a symbol keeps a listener out of what it says. So every sentence these crates
emit will be plain words, the tests will pin those sentences exactly, and this file will say what
has not been tested.

## Scope

### In scope

- **The identifiers crate's refusal sentences** (`crates/ikigai-events`, the `Refusal` type):
  one sentence for each name, token, key or CloudEvent attribute set it refuses. A module puts
  one in front of a person as an `InvalidArgument` detail (`Refusal::into_invalid_argument`),
  so it reaches a terminal, a page or a screen reader. Each sentence begins with the refused
  input in backticks, escaped with Rust's `str::escape_debug` (a control character or an
  escape sequence is spelled, never sent raw) and cut to 64 characters with `…`; an empty
  input is named in words, "An empty value", rather than as an empty pair of backticks. Every
  sentence is pinned byte for byte in `crates/ikigai-events/tests/vectors/identifier_vectors.json`
  (the `refusal` fields), which `tests/vectors.rs` recomputes on every test run, and the escaping
  and cutting are pinned in `src/name.rs`'s unit tests.

As each bridge crate adds output people read (a text face, the text of a delivered or refused
event), it is listed here with its form and its test.

### Not currently in scope

Event payloads themselves: a publisher's payload is carried, never rewritten, so its own
accessibility is its publisher's.

## The rules that output follows

When output arrives, it follows the rules ikigai's other libraries follow: one sentence per
refusal that names what was wrong and what to do; no meaning carried by color, symbols or
alignment alone; no terminal control characters from untrusted input; American English.

## Conformance status

No conformance claim. The only output is the refusal sentences above, which are plain text and
have not been evaluated with assistive technology.

## Evidence

| Check | Result | Evidence |
|---|---|---|
| Refusal sentences pinned by tests | Pass | every refusal in `tests/vectors/identifier_vectors.json`, recomputed by `tests/vectors.rs`; escaping and cutting in `src/name.rs` (`a_refusal_escapes_and_cuts_what_it_quotes`) |
| No control characters from untrusted input | Pass | the same unit test sends an escape sequence and reads back `\u{1b}`; the vectors carry a layer name with ESC and one with a trailing newline |
| Refusal sentences read by a screen reader | Blocked | no screen-reader session has been run |
| Text faces read by a screen reader | Blocked | no face exists yet |

## Known limitations

- A refusal quotes the input it refused inside backticks, which a screen reader may read as
  "backtick" or skip, depending on its punctuation setting. Untested.
- A refusal is in English only.

## Contributor requirements

A pull request that adds output people read lists it under "In scope" above, pins it in a test,
and fills the pull request's `## Accessibility` section with a result for every check: Pass, Fail
or Blocked, with its evidence.

## Reporting a barrier

[Open an issue](https://github.com/ikigai-rs/ikigai-events/issues/new) and put "accessibility"
in its title. Useful details: what you were trying to do, what happened and what you expected,
whether you found a way around it, and your tools and any assistive technology, if you are
comfortable sharing them. You never need to disclose a disability.

Anything security-sensitive, or anything you would rather not post publicly, goes through
[private vulnerability reporting](https://github.com/ikigai-rs/ikigai-events/security/advisories/new).
If neither route works for you, say so in whichever you can reach and we will find another.

## Ownership and review

The maintainers of this repository own this file and review it whenever a crate first emits
output people read.

### Change history

- 2026-11-23: first version, for the scaffold.
- 2026-11-23: the identifiers crate's refusal sentences enter scope (ledger #4126).

## References

- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)
- [ikigai-cli's ACCESSIBILITY.md](https://github.com/ikigai-rs/ikigai-cli/blob/main/ACCESSIBILITY.md), the model this file follows
