//! Rejecting rationales that cite line numbers, with one chance to resubmit.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// The forms of a line-number citation: a file path followed by `:` and a
/// digit, `line` or `lines` followed by a number, or a parenthesized number
/// range.
// {impl}`requirements.authoring.line_numbers.detection`
static LINE_NUMBERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:[\w.-]+/)*[\w-]+\.[A-Za-z0-9]+:\d",
        r"|(?i:\blines?)\s+\d",
        r"|\(\s*\d+\s*[-–]\s*\d+\s*\)",
    ))
    .expect("line-number pattern")
});

/// The first text in `rationale` that cites line numbers, if any.
// {impl}`requirements.authoring.line_numbers.detection`
pub(crate) fn cited(rationale: &str) -> Option<&str> {
    LINE_NUMBERS.find(rationale).map(|found| found.as_str())
}

/// The rationale of a submission's arguments.
// {cite}`habeas:util`
pub(crate) fn rationale(arguments: &Value) -> &str {
    arguments
        .get("rationale")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The state of an assessment's one allowed rejection.
#[derive(Default)]
pub(crate) struct Rejection {
    /// The UUID the rejection gave and the rejected submission's arguments.
    rejected: Option<(String, Value)>,
}

/// What to do with a submission that is otherwise valid.
pub(crate) enum Outcome {
    /// Accept the submission.
    Accept,
    /// Reject the submission with this tool result and continue.
    Reject(String),
    /// Fail the assessment with this error.
    Fail(String),
}

impl Rejection {
    /// The submission to accept in place of `arguments`: the rejected one
    /// when `arguments` gives exactly the UUID its rejection returned.
    // {impl}`requirements.authoring.line_numbers.resubmission`
    pub(crate) fn resubmitted(&self, arguments: &Value) -> Option<Value> {
        let (uuid, rejected) = self.rejected.as_ref()?;
        (rationale(arguments) == uuid).then(|| rejected.clone())
    }

    /// Decides an otherwise valid submission: accepted unless its rationale
    /// cites line numbers, rejected once with a fresh UUID, and failed after
    /// that.
    // {impl}`requirements.authoring.line_numbers.rejection`
    // {impl}`requirements.authoring.line_numbers.resubmission`
    pub(crate) fn decide(&mut self, submit: &str, arguments: &Value, guidance: &str) -> Outcome {
        let Some(found) = cited(rationale(arguments)).map(str::to_owned) else {
            return Outcome::Accept;
        };
        if self.rejected.is_some() {
            return Outcome::Fail(format!(
                "{submit} rationale cites line numbers ({found:?}) after a rejection for citing them"
            ));
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        self.rejected = Some((uuid.clone(), arguments.clone()));
        Outcome::Reject(format!(
            "Not accepted: the rationale cites line numbers ({found:?}). Line numbers \
             change whenever nearby code changes, so cite locations as the judgment rules \
             say:\n\n{guidance}\n\nCall {submit} once more, either with a rationale that \
             gives no line numbers, or with rationale set to exactly {uuid} to keep the \
             submission you just made as it was. Another rationale that cites line numbers \
             fails the assessment."
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // {test}`requirements.authoring.line_numbers.detection`
    #[test]
    fn line_numbers_are_detected_in_each_form() {
        for text in [
            "see src/lib.rs:12",
            "`tests/cli.rs:3-9` covers it",
            "Cargo.toml:4",
            "on line 7",
            "Lines 10 to 12",
            "`check` (12–30) handles it",
            "`check` (12-30)",
            "( 3 - 4 )",
        ] {
            assert!(cited(text).is_some(), "{text}");
        }
        for text in [
            "`src/lib.rs#fn[check]` covers it",
            "`src/lib.rs` and `requirements.ci.errors`",
            "the deadline 7 days",
            "inline 3 items",
            "two lines of output",
            "(see the example)",
            "a ratio of 3:4",
            "(1)",
        ] {
            assert_eq!(cited(text), None, "{text}");
        }
        assert_eq!(cited("at src/a.rs:5 and line 3"), Some("src/a.rs:5"));
    }

    // {test}`requirements.authoring.line_numbers.rejection`
    // {test}`requirements.authoring.line_numbers.resubmission`
    #[test]
    fn a_rejection_gives_a_uuid_that_keeps_the_rejected_submission() {
        let mut rejection = Rejection::default();
        let clean = json!({"determination": true, "rationale": "`src/lib.rs#fn[a]` is fine."});
        assert!(matches!(
            rejection.decide("submit_judgment", &clean, "- guidance"),
            Outcome::Accept
        ));
        let cited = json!({"determination": false, "rationale": "src/lib.rs:3 panics."});
        assert!(rejection.resubmitted(&cited).is_none());
        let Outcome::Reject(message) = rejection.decide("submit_judgment", &cited, "- guidance")
        else {
            panic!("expected a rejection");
        };
        assert!(message.contains("\"src/lib.rs:3\""), "{message}");
        assert!(message.contains("- guidance"), "{message}");
        let uuid = message
            .split_whitespace()
            .find_map(|word| uuid::Uuid::parse_str(word).ok())
            .expect("a UUID")
            .to_string();
        // Exactly the UUID keeps the rejected submission unchanged.
        let keep = json!({"determination": true, "rationale": uuid});
        assert_eq!(rejection.resubmitted(&keep), Some(cited.clone()));
        let other = json!({"determination": true, "rationale": format!("{uuid} ")});
        assert_eq!(rejection.resubmitted(&other), None);
        // A clean resubmission is accepted; another citing one fails.
        assert!(matches!(
            rejection.decide("submit_judgment", &clean, "- guidance"),
            Outcome::Accept
        ));
        assert!(matches!(
            rejection.decide("submit_judgment", &cited, "- guidance"),
            Outcome::Fail(message) if message.contains("after a rejection")
        ));
        // Each rejection gives a fresh UUID.
        let mut again = Rejection::default();
        let Outcome::Reject(second) = again.decide(
            "submit_judgment",
            &json!({"rationale": "line 4"}),
            "- guidance",
        ) else {
            panic!("expected a rejection");
        };
        assert!(!second.contains(&uuid));
    }
}
