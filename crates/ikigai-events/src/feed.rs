//! Feeds, entries, cursors and listeners: the names a publisher and a consumer must spell the
//! same way.

use std::fmt;
use std::num::NonZeroU64;

use crate::name::{
    parse_seq, parse_u64, shown, EntryId, FeedEpoch, LayerName, LedgerName, ListenerId, Refusal,
};

const LAYER: &str = "urn:layer:";
const LEDGER: &str = "urn:iki:ledger:";
const LISTENER: &str = "urn:kernel:listener:id:";

/// A feed: a set of entries in one gapless order that its owning log assigns.
///
/// Two names belong to each feed. Its [`source`](Feed::source) names the set the entries
/// belong to, and is a CloudEvent's `source`; its [`feed`](Feed::feed) is the resource a
/// follower reads with `after=` and `wait=`. For a ledger's transitions they are one name.
///
/// ```
/// use ikigai_events::Feed;
///
/// let talk = Feed::parse("urn:layer:talk:feed").unwrap();
/// assert_eq!(talk, Feed::parse("urn:layer:talk").unwrap());
/// assert_eq!(talk.source(), "urn:layer:talk");
/// assert_eq!(talk.feed(), "urn:layer:talk:feed");
///
/// let ledger = Feed::parse("urn:iki:ledger:default:transition").unwrap();
/// assert_eq!(ledger.source(), "urn:iki:ledger:default:transition");
/// assert_eq!(ledger.feed(), ledger.source());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Feed {
    /// A layer's feed: source `urn:layer:{layer}`, feed `urn:layer:{layer}:feed`.
    Layer(LayerName),
    /// A ledger's transitions: `urn:iki:ledger:{ledger}:transition`.
    Transition(LedgerName),
}

impl Feed {
    /// The name of the set the entries belong to: a CloudEvent's `source`.
    pub fn source(&self) -> String {
        match self {
            Feed::Layer(layer) => format!("{LAYER}{layer}"),
            Feed::Transition(ledger) => format!("{LEDGER}{ledger}:transition"),
        }
    }

    /// The resource a follower reads with `after=` and `wait=`.
    pub fn feed(&self) -> String {
        match self {
            Feed::Layer(layer) => format!("{LAYER}{layer}:feed"),
            Feed::Transition(_) => self.source(),
        }
    }

    /// Read either of a feed's names. A ledger's transitions are always named with the
    /// ledger, `default` included: the bare `urn:iki:ledger:transition` is not an identifier,
    /// because an idempotency key derived from a `source` needs one spelling.
    pub fn parse(iri: &str) -> Result<Feed, Refusal> {
        let refuse = || {
            Refusal::new(format!(
                "{} is not a feed: a feed is `urn:layer:{{layer}}`, `urn:layer:{{layer}}:feed` \
                 or `urn:iki:ledger:{{ledger}}:transition`.",
                shown(iri)
            ))
        };
        if let Some(rest) = iri.strip_prefix(LAYER) {
            let layer = rest.strip_suffix(":feed").unwrap_or(rest);
            if layer.contains(':') {
                return Err(refuse());
            }
            return Ok(Feed::Layer(LayerName::parse(layer)?));
        }
        if let Some(rest) = iri.strip_prefix(LEDGER) {
            if let Some(ledger) = rest.strip_suffix(":transition") {
                if !ledger.contains(':') {
                    return Ok(Feed::Transition(LedgerName::parse(ledger)?));
                }
            }
        }
        Err(refuse())
    }
}

impl fmt::Display for Feed {
    /// The feed's [`source`](Feed::source).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source())
    }
}

/// An entry: one immutable fact in a feed, an atom with its own name that never changes.
///
/// Its IRI is a CloudEvent's `id`. A layer entry's position is not in its name; a ledger
/// transition's is.
///
/// ```
/// use ikigai_events::{Entry, Feed};
///
/// let said = Entry::parse("urn:layer:talk:entry:01m84z0jyt0000000000001").unwrap();
/// assert_eq!(said.feed(), Feed::parse("urn:layer:talk").unwrap());
/// assert_eq!(said.seq(), None);
///
/// let claimed = Entry::transition("default", 42).unwrap();
/// assert_eq!(claimed.iri(), "urn:iki:ledger:default:transition:seq:42");
/// assert_eq!(claimed.seq().map(u64::from), Some(42));
/// assert!(Entry::parse("urn:iki:ledger:default:transition:seq:042").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Entry {
    /// `urn:layer:{layer}:entry:{id}`.
    Layer {
        /// The layer.
        layer: LayerName,
        /// The entry's id within it.
        id: EntryId,
    },
    /// `urn:iki:ledger:{ledger}:transition:seq:{n}`, `n` from 1.
    Transition {
        /// The ledger.
        ledger: LedgerName,
        /// The transition's place in the ledger's sequence.
        seq: NonZeroU64,
    },
}

impl Entry {
    /// The layer entry `id` in `layer`.
    pub fn layer(layer: &str, id: &str) -> Result<Entry, Refusal> {
        Ok(Entry::Layer {
            layer: LayerName::parse(layer)?,
            id: EntryId::parse(id)?,
        })
    }

    /// The transition numbered `seq` in `ledger`.
    pub fn transition(ledger: &str, seq: u64) -> Result<Entry, Refusal> {
        let seq = NonZeroU64::new(seq).ok_or_else(|| {
            Refusal::new("`0` is not a sequence number: a feed's positions start at 1.")
        })?;
        Ok(Entry::Transition {
            ledger: LedgerName::parse(ledger)?,
            seq,
        })
    }

    /// The entry's IRI.
    pub fn iri(&self) -> String {
        match self {
            Entry::Layer { layer, id } => format!("{LAYER}{layer}:entry:{id}"),
            Entry::Transition { ledger, seq } => {
                format!("{LEDGER}{ledger}:transition:seq:{seq}")
            }
        }
    }

    /// The feed the entry belongs to.
    pub fn feed(&self) -> Feed {
        match self {
            Entry::Layer { layer, .. } => Feed::Layer(layer.clone()),
            Entry::Transition { ledger, .. } => Feed::Transition(ledger.clone()),
        }
    }

    /// The entry's position when its name holds it (a transition), else `None`.
    pub fn seq(&self) -> Option<NonZeroU64> {
        match self {
            Entry::Layer { .. } => None,
            Entry::Transition { seq, .. } => Some(*seq),
        }
    }

    /// Read an entry's IRI.
    pub fn parse(iri: &str) -> Result<Entry, Refusal> {
        let refuse = || {
            Refusal::new(format!(
                "{} is not an entry: an entry is `urn:layer:{{layer}}:entry:{{id}}` or \
                 `urn:iki:ledger:{{ledger}}:transition:seq:{{n}}`.",
                shown(iri)
            ))
        };
        if let Some(rest) = iri.strip_prefix(LAYER) {
            let (layer, id) = rest.split_once(":entry:").ok_or_else(refuse)?;
            if layer.contains(':') || id.contains(':') {
                return Err(refuse());
            }
            return Entry::layer(layer, id);
        }
        if let Some(rest) = iri.strip_prefix(LEDGER) {
            let (ledger, n) = rest.split_once(":transition:seq:").ok_or_else(refuse)?;
            if ledger.contains(':') || n.contains(':') {
                return Err(refuse());
            }
            return Ok(Entry::Transition {
                ledger: LedgerName::parse(ledger)?,
                seq: parse_seq(n)?,
            });
        }
        Err(refuse())
    }
}

impl fmt::Display for Entry {
    /// The entry's [`iri`](Entry::iri).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.iri())
    }
}

/// A cursor's value: the feed's epoch, when the feed names one, and the last position the
/// consumer has acted on (`after`, 0 before the first entry).
///
/// A cursor is an ATOM in its consumer's own space, written after the effect is recorded, so
/// its NAME is the consumer's (a relay's is its subscription's); what every consumer shares is
/// this value and its text form, `{epoch}:{after}`, or `{after}` for a feed with no epoch. That
/// text is also a server-sent event's `id`, which a reconnect sends back as `Last-Event-ID`.
///
/// ```
/// use ikigai_events::Cursor;
///
/// let at = Cursor::parse("5f0c9e3a1b2d4c6e8f0a1b2c3d4e5f60:12").unwrap();
/// assert_eq!(at.after(), 12);
/// assert_eq!(at.epoch().map(|e| e.as_str()), Some("5f0c9e3a1b2d4c6e8f0a1b2c3d4e5f60"));
/// assert_eq!(at.to_string(), "5f0c9e3a1b2d4c6e8f0a1b2c3d4e5f60:12");
/// assert_eq!(Cursor::parse("0").unwrap(), Cursor::start(None));
/// assert!(Cursor::parse("e:01").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cursor {
    epoch: Option<FeedEpoch>,
    after: u64,
}

impl Cursor {
    /// A cursor after position `after` in the feed's `epoch`.
    pub fn new(epoch: Option<FeedEpoch>, after: u64) -> Cursor {
        Cursor { epoch, after }
    }

    /// A cursor before the first entry.
    pub fn start(epoch: Option<FeedEpoch>) -> Cursor {
        Cursor { epoch, after: 0 }
    }

    /// The feed's epoch, when it names one.
    pub fn epoch(&self) -> Option<&FeedEpoch> {
        self.epoch.as_ref()
    }

    /// The last position acted on.
    pub fn after(&self) -> u64 {
        self.after
    }

    /// This cursor moved past `seq`.
    pub fn advanced(&self, seq: NonZeroU64) -> Cursor {
        Cursor {
            epoch: self.epoch.clone(),
            after: seq.get(),
        }
    }

    /// Read a cursor's text form.
    pub fn parse(text: &str) -> Result<Cursor, Refusal> {
        match text.split_once(':') {
            None => Ok(Cursor {
                epoch: None,
                after: parse_u64(text, "a cursor position")?,
            }),
            Some((epoch, after)) => Ok(Cursor {
                epoch: Some(FeedEpoch::parse(epoch)?),
                after: parse_u64(after, "a cursor position")?,
            }),
        }
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.epoch {
            Some(epoch) => write!(f, "{epoch}:{}", self.after),
            None => write!(f, "{}", self.after),
        }
    }
}

/// A listener: the kernel's wait for a change, `urn:kernel:listener:id:{id}` (ikigai-core
/// PR 341's proposal; not built in the kernel yet).
///
/// ```
/// use ikigai_events::Listener;
///
/// let l = Listener::parse("urn:kernel:listener:id:7f3a").unwrap();
/// assert_eq!(l.id().as_str(), "7f3a");
/// assert_eq!(l.iri(), "urn:kernel:listener:id:7f3a");
/// assert!(Listener::parse("urn:kernel:listener:7f3a").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Listener {
    id: ListenerId,
}

impl Listener {
    /// The listener `id`.
    pub fn new(id: ListenerId) -> Listener {
        Listener { id }
    }

    /// Its id.
    pub fn id(&self) -> &ListenerId {
        &self.id
    }

    /// Its IRI.
    pub fn iri(&self) -> String {
        format!("{LISTENER}{}", self.id)
    }

    /// Read a listener's IRI.
    pub fn parse(iri: &str) -> Result<Listener, Refusal> {
        let id = iri.strip_prefix(LISTENER).ok_or_else(|| {
            Refusal::new(format!(
                "{} is not a listener: a listener is `urn:kernel:listener:id:{{id}}`.",
                shown(iri)
            ))
        })?;
        Ok(Listener {
            id: ListenerId::parse(id)?,
        })
    }
}

impl fmt::Display for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.iri())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_reads_back_as_itself() {
        for iri in [
            "urn:layer:talk:entry:1",
            "urn:layer:a-b:entry:01m84z0jyt0000000000001",
            "urn:iki:ledger:default:transition:seq:1",
            "urn:iki:ledger:a_b:transition:seq:18446744073709551615",
        ] {
            assert_eq!(Entry::parse(iri).unwrap().iri(), iri);
        }
        for iri in ["urn:layer:talk", "urn:iki:ledger:x:transition"] {
            assert_eq!(Feed::parse(iri).unwrap().source(), iri);
        }
        assert_eq!(
            Listener::parse("urn:kernel:listener:id:a.b").unwrap().iri(),
            "urn:kernel:listener:id:a.b"
        );
    }

    #[test]
    fn a_name_with_an_extra_segment_is_refused() {
        for iri in [
            "urn:layer:talk:entry:1:x",
            "urn:layer:talk:x:entry:1",
            "urn:layer:talk:entry:",
            "urn:iki:ledger:transition:seq:1",
            "urn:iki:ledger:a:b:transition:seq:1",
            "urn:iki:ledger:default:transition:seq:0",
        ] {
            assert!(Entry::parse(iri).is_err(), "{iri}");
        }
        for iri in [
            "urn:layer:talk:entries",
            "urn:layer:",
            "urn:iki:ledger:transition",
        ] {
            assert!(Feed::parse(iri).is_err(), "{iri}");
        }
    }

    #[test]
    fn a_cursor_advances_within_its_epoch() {
        let e = FeedEpoch::parse("e1").unwrap();
        let c = Cursor::start(Some(e)).advanced(NonZeroU64::new(5).unwrap());
        assert_eq!(c.to_string(), "e1:5");
        assert!(Cursor::parse("E1:5").is_err());
        assert!(Cursor::parse(":5").is_err());
        assert!(Cursor::parse("e1:").is_err());
        assert!(Cursor::parse("e1:5:6").is_err());
    }
}
