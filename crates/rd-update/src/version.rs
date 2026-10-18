//! Release versions (Swift `AppVersion`).

use std::cmp::Ordering;
use std::fmt;

/// A release version such as `1.2.3` or the tag `v1.2.3`, compared number by number (`1.10.0` > `1.9.0`).
/// Anything else (`0.0.0-dev.12`, empty) doesn't parse, so dev builds never look outdated.
#[derive(Debug, Clone)]
pub struct AppVersion {
    components: Vec<u64>,
}

impl AppVersion {
    /// Parses `1.2.3`, `v1.2.3` or `V1.2` (surrounding spaces ignored); `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim_matches(' ');
        let text = text
            .strip_prefix('v')
            .or_else(|| text.strip_prefix('V'))
            .unwrap_or(text);
        let components = text
            .split('.')
            .map(|part| {
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    None
                } else {
                    part.parse::<u64>().ok()
                }
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { components })
    }

    /// The numbers, as written (`1.2` has two).
    #[must_use]
    pub fn components(&self) -> &[u64] {
        &self.components
    }

    /// Both component lists padded with zeros to the same length, so `1.2` equals `1.2.0`.
    fn padded(&self, other: &Self) -> (Vec<u64>, Vec<u64>) {
        let count = self.components.len().max(other.components.len());
        let pad = |v: &Self| {
            let mut c = v.components.clone();
            c.resize(count, 0);
            c
        };
        (pad(self), pad(other))
    }
}

impl fmt::Display for AppVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.components.iter().map(u64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

impl PartialEq for AppVersion {
    fn eq(&self, other: &Self) -> bool {
        let (left, right) = self.padded(other);
        left == right
    }
}

impl Eq for AppVersion {}

impl PartialOrd for AppVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let (left, right) = self.padded(other);
        left.cmp(&right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> AppVersion {
        AppVersion::parse(text).unwrap_or_else(|| panic!("{text:?} should parse"))
    }

    #[test]
    fn parses() {
        for (text, components) in [
            ("1.2.3", vec![1, 2, 3]),
            ("v1.2.3", vec![1, 2, 3]),
            ("V2.0", vec![2, 0]),
            (" 1.10.0 ", vec![1, 10, 0]),
            ("7", vec![7]),
        ] {
            assert_eq!(v(text).components(), components.as_slice(), "{text:?}");
        }
    }

    #[test]
    fn rejects() {
        for text in [
            "",
            "v",
            "0.0.0-dev.12",
            "1..2",
            "1.2.",
            "1.-2",
            "abc",
            "1.2.3b",
            "١.٢",
        ] {
            assert!(AppVersion::parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn compares_numerically() {
        assert!(v("1.10.0") > v("1.9.0"));
        assert!(v("1.1.1") > v("1.1.0"));
        assert!(v("2.0") > v("1.99.99"));
        assert_eq!(v("1.2"), v("1.2.0"));
        assert!(v("1.2") >= v("1.2.0"));
        assert!(v("1.2.0.1") > v("1.2"));
    }

    #[test]
    fn description() {
        assert_eq!(v("v1.2.0").to_string(), "1.2.0");
    }
}
