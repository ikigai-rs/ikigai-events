//! Standard Webhooks signatures, pinned against values computed outside this crate.

use ikigai_events::event_key;
use ikigai_events_relay::sign;

/// The specification's published example (secret, id, timestamp, payload and signature),
/// signed by the reference crate this relay signs with.
#[test]
fn the_reference_crate_reproduces_the_published_example() {
    let hook = standardwebhooks::Webhook::new("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw").unwrap();
    assert_eq!(
        hook.sign(
            "msg_p5jXN8AQM9LWM0D4loKWxJek",
            1614265330,
            br#"{"test": 2432232314}"#
        )
        .unwrap(),
        "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
    );
}

/// A relay delivery's signature, against HMAC-SHA256 computed independently (Python's
/// standard library `hmac` over `{webhook-id}.{webhook-timestamp}.{body}`, the key the
/// base64 after `whsec_`).
#[test]
fn a_delivery_signature_matches_an_independent_hmac() {
    let key = event_key("urn:layer:talk", "urn:layer:talk:entry:1").unwrap();
    assert_eq!(
        key.as_str(),
        "event:b3:1bd52c8ee82e459b0c22c5e862309fa4b2e40e6979b8bfe637cf8841058e3b16"
    );
    let signed = sign(
        "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw",
        &key,
        1_800_000_000,
        br#"{"specversion":"1.0"}"#,
    )
    .unwrap();
    assert_eq!(
        signed.signature,
        "v1,XIpWM34zxx1a7E3saMdNBFfHR7RWb6u1BaXe48w8jfM="
    );
    assert_eq!(
        signed.header_block(),
        format!(
            "webhook-id: {}\nwebhook-timestamp: 1800000000\nwebhook-signature: v1,XIpWM34zxx1a7E3saMdNBFfHR7RWb6u1BaXe48w8jfM=",
            key.as_str()
        )
    );
}

#[test]
fn a_bad_secret_is_refused_without_quoting_it() {
    let key = event_key("urn:layer:talk", "urn:layer:talk:entry:1").unwrap();
    assert_eq!(
        sign("whsec_%%%", &key, 1, b"{}").unwrap_err().to_string(),
        "The webhook's secret is not a Standard Webhooks secret: store `whsec_` followed by the \
         key in base64."
    );
}
