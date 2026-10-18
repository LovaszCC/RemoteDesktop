//! Value types with the exact encodings GRDB uses in the Swift app.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

/// A new record id: an uppercase hyphenated UUID v4, like Swift's `UUID().uuidString`.
#[must_use]
pub fn new_id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase()
}

const WITH_MILLIS: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");
const WITHOUT_MILLIS: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// A UTC point in time, stored as GRDB's text form `YYYY-MM-DD HH:MM:SS.SSS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(OffsetDateTime);

/// A stored date that is not in GRDB's text form.
#[derive(Debug, thiserror::Error)]
#[error("invalid timestamp {0:?}")]
pub struct TimestampError(String);

impl Timestamp {
    /// The current time, truncated to milliseconds so it survives a round-trip through the database.
    #[must_use]
    pub fn now() -> Self {
        Self::from_datetime(OffsetDateTime::now_utc())
    }

    /// Wraps a date-time, converting it to UTC and truncating it to milliseconds.
    #[must_use]
    pub fn from_datetime(value: OffsetDateTime) -> Self {
        let utc = value.to_offset(UtcOffset::UTC);
        // Dropping sub-millisecond digits keeps the nanosecond below 1e9, so this never falls back.
        Self(
            utc.replace_nanosecond(utc.nanosecond() / 1_000_000 * 1_000_000)
                .unwrap_or(utc),
        )
    }

    /// Seconds since the Unix epoch (whole seconds; for fixtures and tests).
    ///
    /// # Panics
    /// If `seconds` is outside the range `time` can represent.
    #[must_use]
    pub fn from_unix(seconds: i64) -> Self {
        Self(OffsetDateTime::from_unix_timestamp(seconds).expect("timestamp in range"))
    }

    /// The wrapped UTC date-time.
    #[must_use]
    pub fn datetime(self) -> OffsetDateTime {
        self.0
    }

    /// Parses GRDB's text form; a value without milliseconds is accepted too.
    ///
    /// # Errors
    /// If `text` is not in either form.
    pub fn parse(text: &str) -> Result<Self, TimestampError> {
        PrimitiveDateTime::parse(text, WITH_MILLIS)
            .or_else(|_| PrimitiveDateTime::parse(text, WITHOUT_MILLIS))
            .map(|value| Self(value.assume_utc()))
            .map_err(|_| TimestampError(text.to_owned()))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.0.format(WITH_MILLIS).map_err(|_| fmt::Error)?;
        f.write_str(&text)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A secret sealed by the vault (`nonce ‖ ciphertext ‖ tag`). Never printed, never sent to the webview.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed(Vec<u8>);

impl Sealed {
    /// Wraps sealed bytes read from the database or produced by the vault.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The sealed bytes, for storage.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Sealed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sealed({} bytes)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_uppercase_uuids() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id, id.to_uppercase());
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_ne!(new_id(), id);
    }

    #[test]
    fn timestamp_round_trips_grdb_text() {
        let stamp = Timestamp::parse("2026-10-16 09:30:25.458").unwrap();
        assert_eq!(stamp.to_string(), "2026-10-16 09:30:25.458");
        assert_eq!(
            Timestamp::from_unix(1_767_225_600).to_string(),
            "2026-01-01 00:00:00.000"
        );
    }

    #[test]
    fn timestamp_accepts_value_without_millis() {
        assert_eq!(
            Timestamp::parse("2025-01-01 00:00:00").unwrap().to_string(),
            "2025-01-01 00:00:00.000"
        );
        assert!(Timestamp::parse("2025-01-01T00:00:00Z").is_err());
    }

    #[test]
    fn now_is_truncated_to_millis() {
        let now = Timestamp::now();
        assert_eq!(Timestamp::parse(&now.to_string()).unwrap(), now);
    }

    #[test]
    fn timestamp_serde_uses_text() {
        let stamp = Timestamp::from_unix(0);
        assert_eq!(
            serde_json::to_string(&stamp).unwrap(),
            "\"1970-01-01 00:00:00.000\""
        );
        let back: Timestamp = serde_json::from_str("\"1970-01-01 00:00:00.000\"").unwrap();
        assert_eq!(back, stamp);
    }

    #[test]
    fn sealed_debug_hides_bytes() {
        let sealed = Sealed::from_bytes(vec![1, 2, 3]);
        assert_eq!(format!("{sealed:?}"), "Sealed(3 bytes)");
        assert_eq!(sealed.as_bytes(), &[1, 2, 3]);
    }
}
