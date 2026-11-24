//! The tokens every identifier here is built from, each checked once, and the one refusal type.
//!
//! A token goes into an IRI segment, and most also into a capability token or an idempotency
//! key, so each has a FIXED alphabet that holds no `:`: a name that could contain a `:` could
//! spell a segment boundary, and an IRI matched exactly must not be forgeable by spelling. A
//! token outside its alphabet is refused, never percent-encoded, so every identifier has one
//! spelling.

use std::fmt;
use std::num::NonZeroU64;

/// A name, id or key this crate refuses: one sentence that names what was wrong and what to do.
///
/// The input it quotes is escaped with [`str::escape_debug`] (a control character never
/// reaches a terminal raw) and cut to [`SHOWN`] characters.
///
/// ```
/// use ikigai_events::LayerName;
///
/// let refusal = LayerName::parse("Talk").unwrap_err();
/// assert_eq!(
///     refusal.to_string(),
///     "`Talk` is not a layer name: use 1 to 24 lowercase letters, digits and hyphens, \
///      not starting or ending with a hyphen."
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    sentence: String,
}

impl Refusal {
    /// A refusal of one sentence. For a bridge crate that refuses in the same form: name what
    /// was wrong and what to do, quoting the refused input with [`shown`].
    pub fn new(sentence: impl Into<String>) -> Self {
        Refusal {
            sentence: sentence.into(),
        }
    }

    /// The sentence.
    pub fn sentence(&self) -> &str {
        &self.sentence
    }

    /// The refusal as the kernel's `InvalidArgument` for the argument `name`, for an endpoint
    /// that read the refused value from a request.
    pub fn into_invalid_argument(self, name: &str) -> ikigai_core::Error {
        ikigai_core::Error::InvalidArgument {
            name: name.to_string(),
            detail: self.sentence,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence)
    }
}

impl std::error::Error for Refusal {}

/// The most characters of a refused input a refusal quotes; a longer input is cut and ends
/// with `…`.
pub const SHOWN: usize = 64;

/// `input` as a refusal's sentence begins with it: in backticks, escaped, cut to [`SHOWN`]
/// characters; or `An empty value`, which reads better aloud than an empty pair of backticks.
///
/// ```
/// use ikigai_events::shown;
///
/// assert_eq!(shown("a\u{1b}[31m"), "`a\\u{1b}[31m`");
/// assert_eq!(shown(""), "An empty value");
/// ```
pub fn shown(input: &str) -> String {
    if input.is_empty() {
        return "An empty value".to_string();
    }
    let mut out = String::from("`");
    for (i, c) in input.chars().enumerate() {
        if i == SHOWN {
            out.push('…');
            break;
        }
        out.extend(c.escape_debug());
    }
    out.push('`');
    out
}

fn lower_alnum(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

/// RFC 3986's unreserved set, without `:` by construction.
fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// The longest layer name (ikigai-layers' `MAX_LAYER_NAME`).
pub const MAX_LAYER_NAME: usize = 24;
/// The longest ledger name (ikigai-ledger's `MAX_NAME`).
pub const MAX_LEDGER_NAME: usize = 64;
/// The longest entry id.
pub const MAX_ENTRY_ID: usize = 64;
/// The longest feed epoch (ikigai-layers' `MAX_EPOCH`).
pub const MAX_EPOCH: usize = 64;
/// The longest listener id.
pub const MAX_LISTENER_ID: usize = 128;
/// The longest item reference in a fencing key.
pub const MAX_ITEM: usize = 64;
/// The longest step name in a fencing key.
pub const MAX_STEP: usize = 64;

macro_rules! token {
    ($(#[$doc:meta])* $name:ident, $check:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Check `s` and keep it, or refuse it with one sentence.
            pub fn parse(s: &str) -> Result<Self, Refusal> {
                $check(s)?;
                Ok($name(s.to_string()))
            }

            /// The token as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = Refusal;
            fn from_str(s: &str) -> Result<Self, Refusal> {
                $name::parse(s)
            }
        }
    };
}

token!(
    /// A layer's name, as ikigai-layers names it: 1 to [`MAX_LAYER_NAME`] lowercase letters,
    /// digits and hyphens, not starting or ending with a hyphen.
    ///
    /// The layer host also refuses a Windows device name (`nul`, `com1`), because a layer's
    /// name is a directory there; that is the host's file-system rule, not part of the name.
    ///
    /// ```
    /// use ikigai_events::LayerName;
    ///
    /// assert!(LayerName::parse("tasks-bobby").is_ok());
    /// assert!(LayerName::parse("talk:x").is_err());
    /// assert!(LayerName::parse("abcdefghijklmnopqrstuvwxy").is_err()); // 25
    /// ```
    LayerName,
    check_layer
);

fn check_layer(s: &str) -> Result<(), Refusal> {
    let ok = !s.is_empty()
        && s.len() <= MAX_LAYER_NAME
        && s.bytes().all(|b| lower_alnum(b) || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-');
    if ok {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not a layer name: use 1 to {MAX_LAYER_NAME} lowercase letters, digits and \
             hyphens, not starting or ending with a hyphen.",
            shown(s)
        )))
    }
}

token!(
    /// A ledger's name, as ikigai-ledger names it: 1 to [`MAX_LEDGER_NAME`] lowercase
    /// letters, digits, `-` and `_`, starting and ending with a letter or a digit.
    ///
    /// The ledger also reserves a few names its bare forms use (`items`, `next`); a reserved
    /// name is refused by the ledger when it is served, not here.
    ///
    /// ```
    /// use ikigai_events::LedgerName;
    ///
    /// assert!(LedgerName::parse("default").is_ok());
    /// assert!(LedgerName::parse("a_b-1").is_ok());
    /// assert!(LedgerName::parse("a:b").is_err());
    /// ```
    LedgerName,
    check_ledger
);

fn check_ledger(s: &str) -> Result<(), Refusal> {
    let ok = !s.is_empty()
        && s.len() <= MAX_LEDGER_NAME
        && s.bytes().all(|b| lower_alnum(b) || b == b'-' || b == b'_')
        && s.bytes().next().is_some_and(lower_alnum)
        && s.bytes().last().is_some_and(lower_alnum);
    if ok {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not a ledger name: use 1 to {MAX_LEDGER_NAME} lowercase letters, digits, \
             `-` and `_`, starting and ending with a letter or a digit.",
            shown(s)
        )))
    }
}

token!(
    /// An entry's id within its layer: 1 to [`MAX_ENTRY_ID`] lowercase letters and digits.
    /// The layer host mints 23 characters of Crockford base32; this accepts any id in that
    /// alphabet's superset, so the identifier does not freeze the host's minting rule.
    ///
    /// ```
    /// use ikigai_events::EntryId;
    ///
    /// assert!(EntryId::parse("01m84z0jyt0000000000001").is_ok());
    /// assert!(EntryId::parse("Abc").is_err());
    /// ```
    EntryId,
    check_entry_id
);

fn check_entry_id(s: &str) -> Result<(), Refusal> {
    if !s.is_empty() && s.len() <= MAX_ENTRY_ID && s.bytes().all(lower_alnum) {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not an entry id: use 1 to {MAX_ENTRY_ID} lowercase letters and digits.",
            shown(s)
        )))
    }
}

token!(
    /// A feed's epoch: the incarnation of the feed's numbering, which changes when a door is
    /// restored from a backup (ikigai-layers' `check_epoch`). 1 to [`MAX_EPOCH`] lowercase
    /// letters, digits and hyphens; a minted one is 32 hex digits.
    ///
    /// ```
    /// use ikigai_events::FeedEpoch;
    ///
    /// assert!(FeedEpoch::parse("5f0c9e3a1b2d4c6e8f0a1b2c3d4e5f60").is_ok());
    /// assert!(FeedEpoch::parse("a b").is_err());
    /// ```
    FeedEpoch,
    check_epoch
);

fn check_epoch(s: &str) -> Result<(), Refusal> {
    let ok =
        !s.is_empty() && s.len() <= MAX_EPOCH && s.bytes().all(|b| lower_alnum(b) || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not a feed epoch: use 1 to {MAX_EPOCH} lowercase letters, digits and \
             hyphens.",
            shown(s)
        )))
    }
}

token!(
    /// A listener's id: 1 to [`MAX_LISTENER_ID`] characters of RFC 3986's unreserved set
    /// (ASCII letters, digits, `-`, `.`, `_`, `~`). The kernel mints it; this is the one
    /// segment its name may hold.
    ///
    /// ```
    /// use ikigai_events::ListenerId;
    ///
    /// assert!(ListenerId::parse("L-7f3a.b_c~d").is_ok());
    /// assert!(ListenerId::parse("a:b").is_err());
    /// ```
    ListenerId,
    check_listener_id
);

fn check_listener_id(s: &str) -> Result<(), Refusal> {
    if !s.is_empty() && s.len() <= MAX_LISTENER_ID && s.bytes().all(unreserved) {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not a listener id: use 1 to {MAX_LISTENER_ID} ASCII letters, digits, \
             `-`, `.`, `_` and `~`.",
            shown(s)
        )))
    }
}

token!(
    /// The item a fencing key is for: its display number (`4126`) or its id
    /// (`01m84z0jyt4qqbb2vq8kqbr`), 1 to [`MAX_ITEM`] lowercase letters and digits.
    ///
    /// ```
    /// use ikigai_events::ItemRef;
    ///
    /// assert!(ItemRef::parse("4126").is_ok());
    /// assert!(ItemRef::parse("#4126").is_err());
    /// ```
    ItemRef,
    check_item
);

fn check_item(s: &str) -> Result<(), Refusal> {
    if !s.is_empty() && s.len() <= MAX_ITEM && s.bytes().all(lower_alnum) {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not an item: use its number or its id, 1 to {MAX_ITEM} lowercase letters \
             and digits.",
            shown(s)
        )))
    }
}

token!(
    /// One step of the effect a fencing key guards (`push`, `open-pr`): 1 to [`MAX_STEP`]
    /// characters of RFC 3986's unreserved set.
    ///
    /// ```
    /// use ikigai_events::Step;
    ///
    /// assert!(Step::parse("open-pr").is_ok());
    /// assert!(Step::parse("a:b").is_err());
    /// ```
    Step,
    check_step
);

fn check_step(s: &str) -> Result<(), Refusal> {
    if !s.is_empty() && s.len() <= MAX_STEP && s.bytes().all(unreserved) {
        Ok(())
    } else {
        Err(Refusal::new(format!(
            "{} is not a step: use 1 to {MAX_STEP} ASCII letters, digits, `-`, `.`, `_` and \
             `~`.",
            shown(s)
        )))
    }
}

/// A sequence number in its one spelling: decimal digits, no sign, no leading zero, within
/// `u64`. `what` names the field in the refusal.
pub(crate) fn parse_u64(s: &str, what: &str) -> Result<u64, Refusal> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    match canonical.then(|| s.parse::<u64>().ok()).flatten() {
        Some(n) => Ok(n),
        None => Err(Refusal::new(format!(
            "{} is not {what}: use decimal digits with no sign and no leading zero, at most \
             {}.",
            shown(s),
            u64::MAX
        ))),
    }
}

/// A feed position, which starts at 1.
pub(crate) fn parse_seq(s: &str) -> Result<NonZeroU64, Refusal> {
    let n = parse_u64(s, "a sequence number")?;
    NonZeroU64::new(n)
        .ok_or_else(|| Refusal::new("`0` is not a sequence number: a feed's positions start at 1."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_escapes_and_cuts_what_it_quotes() {
        let r = LayerName::parse("a\u{1b}[31mb").unwrap_err();
        assert!(r.sentence().starts_with("`a\\u{1b}[31mb` is not"));
        let long = "x".repeat(100);
        let r = EntryId::parse(&format!("{long}A")).unwrap_err();
        assert!(r
            .sentence()
            .starts_with(&format!("`{}…` is not", "x".repeat(64))));
        let r = LedgerName::parse("").unwrap_err();
        assert!(r
            .sentence()
            .starts_with("An empty value is not a ledger name"));
    }

    #[test]
    fn a_sequence_number_has_one_spelling() {
        assert_eq!(parse_u64("0", "x").unwrap(), 0);
        assert_eq!(parse_u64("18446744073709551615", "x").unwrap(), u64::MAX);
        for bad in ["", "01", "+1", "-1", "1 ", "18446744073709551616", "١"] {
            assert!(parse_u64(bad, "x").is_err(), "{bad:?}");
        }
        assert!(parse_seq("0").is_err());
    }
}
