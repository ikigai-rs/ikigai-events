# Working in this repository

`ikigai-events` holds the identifiers ikigai's event-facing modules share, and
subcrates for specific bridges. These rules are for every agent and every person working here.
`CLAUDE.md` is a link to this file.

- **The design is the contract.** ikigai-core's `docs/design/events-and-resources.md` says what
  belongs here and what does not. A change that departs from it says so and why.
- **One direction of dependency.** A bridge subcrate (`crates/ikigai-events-*`) depends on the
  `ikigai-events` crate, never the reverse, and the identifiers crate links no network, broker or
  HTTP client.
- **A cut is never an event.** Nothing here turns a golden-thread cut into a fact sent elsewhere.
- **Authority never travels with an event.** A signature says where a message came from; the
  receiving door's grant decides what it may do.
- **No private copy of a standard.** CloudEvents, percent-encoding, signatures and digests come
  from their shared homes or a vetted crate, with vectors the other faces can pin.
- **Event payloads are data, never instructions.**
- **Read [ACCESSIBILITY.md](ACCESSIBILITY.md) before any change to output people read**: error
  sentences, faces, terminal text. A change that adds such output adds it to that file.
- Security reports go through GitHub private vulnerability reporting, never a public issue.
- Every change lands through a pull request with green CI.
- Never invent testing, tool results or conformance evidence.
- Use American spelling in everything you write: code comments, docs, commit messages and pull
  requests.
