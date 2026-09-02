//! The saved career file: one Independent Training result, as the plugin
//! writes it and the career viewer reads it.
//!
//! # The shape
//!
//! An envelope that is ours around a payload that is the game's, with a hard
//! line between the two. `docs/idle-career-format.example.json` is a complete
//! file; the tests here read it, so it cannot drift from the code.
//!
//! ```json
//! {
//!   "format": "honse-tracker/idle-career",
//!   "format_version": 1,
//!   "captured_at": "2026-09-02T01:24:56-04:00",
//!   "source": { "plugin_version": "0.4.0", "callback": "end",
//!               "response_type": "IdleSingleModeEndResponse" },
//!   "unreadable": [],
//!   "response": { "data": { "end_info": {}, "progress_log_info": {} } }
//! }
//! ```
//!
//! Everything under `response` is the game's own object, verbatim: its key
//! names, its types, its nulls. No ids resolved, no enums spelled out — that
//! is the viewer's job, and doing it here would make the file drift from the
//! API it claims to mirror. Everything outside `response` is ours, and new
//! metadata goes there, never inside the payload.
//!
//! Two departures from "verbatim", both of which a reader has to know:
//!
//! - Account ids are stripped at every depth before the file is written. A
//!   folder of these is for analysis and may be shared.
//! - A branch the reflection walk could not read is `null` in the payload,
//!   and its JSON pointer and the reason are listed under `unreadable`. A
//!   parser gets a valid null where it expected a value, and the explanation
//!   lives in one place at the top instead of as a marker string inside.
//!
//! `format_version` is bumped only when a reader written for the old number
//! would misread the new file. Adding an envelope key is not that.
//!
//! # Why one module owns both directions
//!
//! The plugin writes these and the viewer reads them. With the format in one
//! place, a change to it is one change; with it in both crates' heads, the
//! viewer breaks the first time the plugin moves a key. [`CareerDocument`] is
//! the whole interface either side needs, and the round-trip test at the bottom
//! is the contract between them.

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `format` string every file of this shape carries.
pub const FORMAT: &str = "honse-tracker/idle-career";
/// The version this code writes and the newest it reads.
pub const FORMAT_VERSION: u32 = 1;

/// Which of the game's two result callbacks produced a file.
///
/// `End` fires when a run is finalised; `Result` when its log is opened later.
/// Same payload shape either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Callback {
    End,
    Result,
}

impl Callback {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::End => "end",
            Self::Result => "result",
        }
    }

    /// The game's own type for the response this callback receives — the name
    /// a reader would look for in the client to learn what the fields mean.
    #[must_use]
    pub fn response_type(self) -> &'static str {
        match self {
            Self::End => "IdleSingleModeEndResponse",
            Self::Result => "IdleSingleModeResultResponse",
        }
    }
}

/// What produced a file: which plugin build, and which callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub plugin_version: String,
    pub callback: Callback,
    pub response_type: String,
}

impl Source {
    #[must_use]
    pub fn new(callback: Callback, plugin_version: &str) -> Self {
        Self {
            plugin_version: plugin_version.to_string(),
            callback,
            response_type: callback.response_type().to_string(),
        }
    }
}

/// One branch of the payload the reflection walk had to give up on. The
/// payload holds `null` at `at`; this says why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unreadable {
    /// A JSON pointer (RFC 6901) into `response`, e.g. `/data/end_info/home_info`.
    pub at: String,
    pub reason: String,
}

/// One saved career, in memory. Built by the plugin from a captured response
/// or parsed by the viewer from a file; the same type either way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CareerDocument {
    format: String,
    format_version: u32,
    captured_at: DateTime<FixedOffset>,
    source: Source,
    unreadable: Vec<Unreadable>,
    response: Value,
}

/// Why a file could not be read as a career document.
#[derive(Debug)]
pub enum FormatError {
    /// Not JSON, or JSON that fits neither this format nor the pre-format shape.
    Json(serde_json::Error),
    /// Well-formed JSON that is not one of these files at all.
    NotACareer,
    /// A newer plugin wrote it than this reader understands.
    UnsupportedVersion(u32),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "not valid JSON: {e}"),
            Self::NotACareer => write!(f, "not a {FORMAT} file"),
            Self::UnsupportedVersion(v) => {
                write!(f, "format version {v} is newer than this reader ({FORMAT_VERSION})")
            }
        }
    }
}

impl std::error::Error for FormatError {}

/// Field names that identify the account rather than the run. Removed before
/// anything reaches disk, at every depth.
///
/// Both spellings are listed because the reflection walk normalises the game's
/// `_ownerViewerId` backing fields to camelCase while the response types
/// themselves use snake_case.
const IDENTIFYING_FIELDS: &[&str] = &["viewer_id", "owner_viewer_id", "viewerId", "ownerViewerId"];

/// The stamp that leads every file name: `20260902_012456`.
const FILE_STAMP: &str = "%Y%m%d_%H%M%S";

impl CareerDocument {
    /// Wrap a response the plugin just walked. Strips the account id from the
    /// payload; everything else is kept as handed in.
    #[must_use]
    pub fn capture(
        source: Source,
        captured_at: DateTime<FixedOffset>,
        mut response: Value,
        unreadable: Vec<Unreadable>,
    ) -> Self {
        scrub(&mut response);
        Self {
            format: FORMAT.to_string(),
            format_version: FORMAT_VERSION,
            captured_at,
            source,
            unreadable,
            response,
        }
    }

    /// Read a file back.
    ///
    /// `file_name` is consulted only for files from before this format existed
    /// (plugin 0.3 and 0.4 wrote the bare payload with two `honse_*` keys in
    /// it), where the capture time lives in the name and nowhere else. Those
    /// still open rather than vanishing from the list on upgrade.
    pub fn parse(file_name: &str, text: &str) -> Result<Self, FormatError> {
        let value: Value = serde_json::from_str(text).map_err(FormatError::Json)?;
        match value.get("format").and_then(Value::as_str) {
            Some(FORMAT) => {
                let version = value.get("format_version").and_then(Value::as_u64).unwrap_or(0);
                if version > u64::from(FORMAT_VERSION) {
                    return Err(FormatError::UnsupportedVersion(
                        u32::try_from(version).unwrap_or(u32::MAX),
                    ));
                }
                serde_json::from_value(value).map_err(FormatError::Json)
            }
            Some(_) => Err(FormatError::NotACareer),
            None => Self::from_legacy(file_name, value).ok_or(FormatError::NotACareer),
        }
    }

    /// Lift a pre-format file into the envelope. Its capture time comes from
    /// the file name; a renamed legacy file is not recoverable and is refused.
    fn from_legacy(file_name: &str, mut value: Value) -> Option<Self> {
        let callback = match value.get("honse_source")?.as_str()? {
            "end" => Callback::End,
            "result" => Callback::Result,
            _ => return None,
        };
        let plugin_version = value.get("honse_tracker_version")?.as_str()?.to_string();
        let captured_at = captured_at_from_file_name(file_name)?;
        let map = value.as_object_mut()?;
        map.retain(|key, _| !key.starts_with("honse_"));
        Some(Self {
            format: FORMAT.to_string(),
            format_version: FORMAT_VERSION,
            captured_at,
            source: Source::new(callback, &plugin_version),
            unreadable: Vec::new(),
            response: value,
        })
    }

    /// The file as it is written: pretty-printed, keys sorted.
    ///
    /// Sorted keys are what `serde_json` does without `preserve_order`, and
    /// they are worth keeping on purpose: two exports diff cleanly.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// `20260902_014233-card101302-end.json`.
    ///
    /// Timestamp first so a folder sorts chronologically, then the card so a
    /// run is identifiable without opening it, then the callback. The card is
    /// looked up rather than required: a payload that has moved on still gets
    /// a name, just a duller one.
    #[must_use]
    pub fn file_name(&self) -> String {
        let stamp = self.captured_at.format(FILE_STAMP);
        let callback = self.source.callback.as_str();
        match self.card_id() {
            Some(card) => format!("{stamp}-card{card}-{callback}.json"),
            None => format!("{stamp}-{callback}.json"),
        }
    }

    #[must_use]
    pub fn captured_at(&self) -> DateTime<FixedOffset> {
        self.captured_at
    }

    #[must_use]
    pub fn source(&self) -> &Source {
        &self.source
    }

    #[must_use]
    pub fn unreadable(&self) -> &[Unreadable] {
        &self.unreadable
    }

    /// The game's payload: `{ "data": { ... } }`.
    #[must_use]
    pub fn response(&self) -> &Value {
        &self.response
    }

    /// The trainee's card, if the payload still has the shape it had when this
    /// was written.
    #[must_use]
    pub fn card_id(&self) -> Option<i64> {
        self.response.pointer("/data/end_info/chara_info/card_id")?.as_i64()
    }
}

/// Strip [`IDENTIFYING_FIELDS`] from every object in the tree.
fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !IDENTIFYING_FIELDS.contains(&key.as_str()));
            map.values_mut().for_each(scrub);
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}

/// The capture time a pre-format file name encodes, read as local time since
/// that is what the plugin stamped it with.
fn captured_at_from_file_name(file_name: &str) -> Option<DateTime<FixedOffset>> {
    let stamp = file_name.split('-').next()?;
    let naive = NaiveDateTime::parse_from_str(stamp, FILE_STAMP).ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|local| local.fixed_offset())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EXAMPLE: &str = include_str!("../../../docs/idle-career-format.example.json");

    fn stamp() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-09-02T01:24:56-04:00").expect("a valid stamp")
    }

    /// The contract between writer and reader: what goes in comes out, and the
    /// checked-in example is a file this code actually produces.
    #[test]
    fn the_example_round_trips() {
        let parsed = CareerDocument::parse("x.json", EXAMPLE).expect("the example parses");
        assert_eq!(parsed.source().callback, Callback::End);
        assert_eq!(parsed.source().plugin_version, "0.4.0");
        assert_eq!(parsed.captured_at(), stamp());
        assert_eq!(parsed.card_id(), Some(100_702));
        assert_eq!(parsed.unreadable().len(), 1);

        let rebuilt = CareerDocument::capture(
            Source::new(Callback::End, "0.4.0"),
            stamp(),
            parsed.response().clone(),
            parsed.unreadable().to_vec(),
        );
        assert_eq!(rebuilt, parsed, "capture builds what parse read");

        let written = rebuilt.to_json().expect("serialises");
        let again = CareerDocument::parse("x.json", &written).expect("reads its own output");
        assert_eq!(again, parsed);
        let example: Value = serde_json::from_str(EXAMPLE).expect("json");
        let ours: Value = serde_json::from_str(&written).expect("json");
        assert_eq!(
            ours, example,
            "the example on disk is byte-for-byte what we write, modulo whitespace"
        );
    }

    /// The one thing an export must never carry: the account it came from.
    /// Checked at depth, because the id appears six times in a real response
    /// and only one of them is near the top.
    #[test]
    fn the_account_id_is_stripped_at_every_depth() {
        let response = json!({
            "owner_viewer_id": 413,
            "data": {
                "end_info": { "chara_info": { "owner_viewer_id": 413, "card_id": 100702 } },
                "list": [ { "viewer_id": 413 }, { "ownerViewerId": 413, "keep": 1 } ]
            }
        });
        let doc = CareerDocument::capture(Source::new(Callback::End, "0"), stamp(), response, Vec::new());
        let text = doc.response().to_string();
        assert!(!text.contains("413"), "{text}");
        assert!(!text.contains("viewer"), "{text}");
        assert_eq!(doc.card_id(), Some(100_702), "everything else survives");
        assert_eq!(doc.response()["data"]["list"][1]["keep"], 1);
    }

    #[test]
    fn the_stamp_and_card_name_the_file() {
        let response = json!({ "data": { "end_info": { "chara_info": { "card_id": 101_302 } } } });
        let doc = CareerDocument::capture(Source::new(Callback::Result, "0"), stamp(), response, Vec::new());
        assert_eq!(doc.file_name(), "20260902_012456-card101302-result.json");
    }

    /// A payload that has changed shape must still be written. Losing the run
    /// because the filename could not be prettified would be the wrong trade.
    #[test]
    fn a_response_without_a_card_id_still_gets_a_name() {
        for response in [
            json!({}),
            json!({ "data": {} }),
            json!({ "data": { "end_info": null } }),
        ] {
            let doc = CareerDocument::capture(Source::new(Callback::End, "0"), stamp(), response, Vec::new());
            assert_eq!(doc.card_id(), None);
            assert_eq!(doc.file_name(), "20260902_012456-end.json");
        }
    }

    /// Files from before the format existed still open. Their stamp comes from
    /// the name, and the two `honse_*` keys move out of the payload.
    #[test]
    fn a_pre_format_file_is_lifted_into_the_envelope() {
        let legacy = json!({
            "data": { "end_info": { "chara_info": { "card_id": 100702 } } },
            "honse_source": "result",
            "honse_tracker_version": "0.3.0"
        })
        .to_string();
        let doc = CareerDocument::parse("20260901_220752-card100702-result.json", &legacy).expect("lifts");
        assert_eq!(doc.source().callback, Callback::Result);
        assert_eq!(doc.source().plugin_version, "0.3.0");
        assert_eq!(
            doc.captured_at().format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-09-01 22:07:52"
        );
        assert_eq!(doc.card_id(), Some(100_702));
        assert!(doc.response().get("honse_source").is_none(), "ours, not theirs");
        assert!(doc.unreadable().is_empty());
        assert_eq!(
            doc.file_name(),
            "20260901_220752-card100702-result.json",
            "the name survives a rewrite"
        );
    }

    #[test]
    fn what_is_not_a_career_is_refused() {
        let refused = |name: &str, text: &str| CareerDocument::parse(name, text).expect_err("refused");
        assert!(matches!(refused("x.json", "not json"), FormatError::Json(_)));
        assert!(matches!(
            refused("x.json", r#"{"format":"something-else"}"#),
            FormatError::NotACareer
        ));
        assert!(
            matches!(refused("x.json", r#"{"data":{}}"#), FormatError::NotACareer),
            "no honse_ keys"
        );
        assert!(
            matches!(
                refused(
                    "renamed.json",
                    r#"{"data":{},"honse_source":"end","honse_tracker_version":"0.3.0"}"#
                ),
                FormatError::NotACareer
            ),
            "a renamed legacy file has no recoverable stamp"
        );
        let future = format!(r#"{{"format":"{FORMAT}","format_version":{}}}"#, FORMAT_VERSION + 1);
        assert!(matches!(refused("x.json", &future), FormatError::UnsupportedVersion(_)));
    }
}
