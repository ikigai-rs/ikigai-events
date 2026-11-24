//! The checks a format defines, each with a vetted parser where one exists: RFC 3339 for
//! `time`, RFC 3986 references for `source` and `dataschema`, RFC 4648 for `data_base64`, and
//! W3C Trace Context for `traceparent`. Each refuses with one sentence.

use base64::Engine as _;
use ikigai_events::{shown, Refusal};

/// `time`: an RFC 3339 `date-time`, read by the `time` crate's well-known RFC 3339 parser.
pub(crate) fn rfc3339(value: &str) -> Result<(), Refusal> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(|_| ())
        .map_err(|_| {
            Refusal::new(format!(
                "{} is not an RFC 3339 time: send `time` as a date-time with an offset, such as \
                 `2026-11-23T20:00:00Z`.",
                shown(value)
            ))
        })
}

/// An RFC 3986 URI-reference, read by `oxiri`.
///
/// `oxiri` parses RFC 3987 IRI-references. RFC 3987 section 2.2 builds the IRI grammar from the
/// URI grammar by adding only `ucschar` and `iprivate`, which are all outside ASCII, so an
/// ASCII string is a URI-reference exactly when it is an IRI-reference.
fn uri_reference(value: &str) -> bool {
    value.is_ascii() && oxiri::IriRef::parse(value).is_ok()
}

/// `source`: a non-empty URI-reference (RFC 3986 admits the empty one; CloudEvents does not).
pub(crate) fn source(value: &str) -> Result<(), Refusal> {
    if !value.is_empty() && uri_reference(value) {
        return Ok(());
    }
    Err(Refusal::new(format!(
        "{} is not a URI-reference: send `source` as an absolute or relative URI in ASCII, \
         percent-encoding anything else.",
        shown(value)
    )))
}

/// `dataschema`: an absolute URI, read by `oxiri` as [`uri_reference`] is.
pub(crate) fn dataschema(value: &str) -> Result<(), Refusal> {
    if value.is_ascii() && oxiri::Iri::parse(value).is_ok() {
        return Ok(());
    }
    Err(Refusal::new(format!(
        "{} is not an absolute URI: send `dataschema` as a URI with a scheme, in ASCII.",
        shown(value)
    )))
}

/// `data_base64`: base64 as RFC 4648 section 4 writes it (the standard alphabet, padded), read
/// by the `base64` crate's STANDARD engine.
pub(crate) fn base64(value: &str) -> Result<(), Refusal> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map(|_| ())
        .map_err(|_| {
            Refusal::new(format!(
                "{} is not base64: send `data_base64` in RFC 4648's standard alphabet, padded \
                 with `=`.",
                shown(value)
            ))
        })
}

/// `traceparent`: a W3C Trace Context header value (Trace Context Level 1, section 3.2).
///
/// PRIVATE COPY of a standard, on purpose: no crate in the ecosystem parses `traceparent` (the
/// shared HTTP home, ikigai-http, has no trace context), and the vetted implementation, the
/// OpenTelemetry SDK's `TraceContextPropagator`, keeps its parser private behind a propagator
/// interface, accepts ids shorter than the header's fixed widths, and would bring the SDK into
/// a wasm-clean library for a fixed-width hex check. The rules below are the section's, one
/// line each, and `tests/vectors/` pins them for the other faces. Move it to the shared home
/// when one holds trace context.
///
/// - `version-format = trace-id "-" parent-id "-" trace-flags`, every field lowercase hex;
/// - the version is two digits and never `ff`;
/// - version `00` is exactly 55 characters; a later version is at least 55, and anything past
///   them begins with `-`, so a version-00 reader reads the first 55;
/// - the trace id (32 digits) and the parent id (16) are never all zeros.
pub(crate) fn traceparent(value: &str) -> Result<(), Refusal> {
    let refuse = |why: &str| {
        Refusal::new(format!(
            "{} is not a W3C `traceparent`: {why}.",
            shown(value)
        ))
    };
    let lower_hex = |s: &str| s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    // ASCII first, so the cut at 55 below is always on a character boundary.
    if !value.is_ascii() || value.len() < 55 {
        return Err(refuse(
            "send `{version}-{trace-id}-{parent-id}-{flags}` in lowercase hex, 2, 32, 16 and 2 \
             digits",
        ));
    }
    let (head, rest) = value.split_at(55);
    let fields: Vec<&str> = head.split('-').collect();
    let widths = [2, 32, 16, 2];
    if fields.len() != 4
        || fields.iter().zip(widths).any(|(f, w)| f.len() != w)
        || !fields.iter().all(|f| lower_hex(f))
    {
        return Err(refuse(
            "send `{version}-{trace-id}-{parent-id}-{flags}` in lowercase hex, 2, 32, 16 and 2 \
             digits",
        ));
    }
    let version = fields[0];
    if version == "ff" {
        return Err(refuse("version `ff` is never valid"));
    }
    if version == "00" && !rest.is_empty() {
        return Err(refuse("a version `00` value is exactly 55 characters"));
    }
    if !rest.is_empty() && !rest.starts_with('-') {
        return Err(refuse(
            "what follows the first 55 characters of a later version begins with `-`",
        ));
    }
    if fields[1].bytes().all(|b| b == b'0') {
        return Err(refuse("its trace id is all zeros"));
    }
    if fields[2].bytes().all(|b| b == b'0') {
        return Err(refuse("its parent id is all zeros"));
    }
    Ok(())
}

/// A CloudEvents attribute name: one or more lowercase ASCII letters and digits (CloudEvents
/// 1.0, section 3.1.1). The specification's 20-character limit is a SHOULD, so a longer name
/// is taken.
pub(crate) fn attribute_name(name: &str) -> Result<(), Refusal> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Ok(());
    }
    Err(Refusal::new(format!(
        "{} is not a CloudEvents attribute name: use lowercase ASCII letters and digits only.",
        shown(name)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        for ok in [
            "2026-11-23T20:00:00Z",
            "2026-11-23T20:00:00.197Z",
            "2026-11-23T20:00:00+05:30",
            "2026-11-23t20:00:00z",
            // RFC 3339 section 5.6 lets an application separate the date and time with a
            // space, and the `time` crate's parser does.
            "2026-11-23 20:00:00Z",
        ] {
            assert!(rfc3339(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "2026-11-23",
            "2026-11-23T20:00:00",
            "2026-13-23T20:00:00Z",
            "yesterday",
        ] {
            assert!(rfc3339(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sources_and_schemas() {
        for ok in [
            "urn:layer:talk",
            "https://example.com/sensors/tn-1234567",
            "/relative/source",
            "relative",
            "mailto:a@example.org",
        ] {
            assert!(source(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "has space",
            "https://exämple.com/",
            "a\nb",
            "http://[::1",
        ] {
            assert!(source(bad).is_err(), "{bad:?}");
        }
        assert!(dataschema("https://example.com/schema.json").is_ok());
        assert!(dataschema("/relative").is_err());
    }

    #[test]
    fn traceparents() {
        let ok = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert!(traceparent(ok).is_ok());
        assert!(traceparent(&format!("cc{}-what-comes-next", &ok[2..])).is_ok());
        for bad in [
            "",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4bf92f3577b34da6a3ce929d0e0e473-600f067aa0ba902b7-01",
            "cc-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01x",
        ] {
            assert!(traceparent(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn base64_is_standard_and_padded() {
        assert!(base64("aGVsbG8=").is_ok());
        assert!(base64("").is_ok());
        assert!(base64("aGVsbG8").is_err());
        assert!(base64("aGVs bG8=").is_err());
        assert!(base64("aGVsbG8_").is_err());
    }
}
