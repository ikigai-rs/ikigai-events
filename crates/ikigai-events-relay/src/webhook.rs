//! The Standard Webhooks headers, signed by the specification's reference implementation.

use ikigai_core::IdempotencyKey;
use ikigai_events::Refusal;

pub use standardwebhooks::{HEADER_WEBHOOK_ID, HEADER_WEBHOOK_SIGNATURE, HEADER_WEBHOOK_TIMESTAMP};

/// The three Standard Webhooks headers of one delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    /// `webhook-id`: the event key's text (`event:b3:{hex}`), the digest of the entry's
    /// `source` and IRI, so every attempt of one entry carries the same id and a receiver
    /// that keeps ids takes it once.
    pub id: String,
    /// `webhook-timestamp`: seconds since the Unix epoch, when this attempt was signed.
    pub timestamp: i64,
    /// `webhook-signature`: `v1,` and the base64 HMAC-SHA256 of `{id}.{timestamp}.{body}`.
    pub signature: String,
}

impl Signed {
    /// The headers as ikigai-http's `headers=` block: one `Name: Value` per line.
    pub fn header_block(&self) -> String {
        format!(
            "{HEADER_WEBHOOK_ID}: {}\n{HEADER_WEBHOOK_TIMESTAMP}: {}\n{HEADER_WEBHOOK_SIGNATURE}: {}",
            self.id, self.timestamp, self.signature
        )
    }
}

/// Sign one delivery: `body` under `secret` (`whsec_` and base64, or bare base64), for the
/// event keyed `key`, at `timestamp`.
///
/// ```
/// use ikigai_events::event_key;
/// use ikigai_events_relay::sign;
///
/// let key = event_key("urn:layer:talk", "urn:layer:talk:entry:7").unwrap();
/// let signed = sign("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw", &key, 1614265330, b"{}").unwrap();
/// assert_eq!(signed.id, key.as_str());
/// assert!(signed.signature.starts_with("v1,"));
/// assert!(sign("whsec_not base64!", &key, 1, b"{}").is_err());
/// ```
pub fn sign(
    secret: &str,
    key: &IdempotencyKey,
    timestamp: i64,
    body: &[u8],
) -> Result<Signed, Refusal> {
    let hook = standardwebhooks::Webhook::new(secret.trim()).map_err(|_| {
        Refusal::new(
            "The webhook's secret is not a Standard Webhooks secret: store `whsec_` followed \
             by the key in base64.",
        )
    })?;
    let signature = hook.sign(key.as_str(), timestamp, body).map_err(|_| {
        Refusal::new("The event's JSON is not UTF-8, so it cannot be signed: lower it again.")
    })?;
    Ok(Signed {
        id: key.as_str().to_string(),
        timestamp,
        signature,
    })
}
