//! Finder-style name ordering, like Swift's `localizedStandardCompare`.

use std::cmp::Ordering;
use std::iter::Peekable;
use std::str::Chars;

/// Compares two names the way Finder sorts them (Swift `localizedStandardCompare`).
///
/// - Letters compare case-insensitively, and a letter with a diacritic sorts next to its base letter
///   (`é` between `e` and `f`); when two names differ only in diacritics, the plain letter comes first.
/// - Runs of digits compare by numeric value, so `web2` sorts before `web10`. Leading zeros are ignored.
///
/// Names that differ only in case or leading zeros compare [`Ordering::Equal`]; a stable sort keeps their
/// original order.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    compare(a, b, true).then_with(|| compare(a, b, false))
}

fn compare(a: &str, b: &str, fold: bool) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let order = compare_numbers(&digit_run(&mut left), &digit_run(&mut right));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                left.next();
                right.next();
                let order = if fold {
                    fold_diacritic(x)
                        .to_lowercase()
                        .cmp(fold_diacritic(y).to_lowercase())
                } else {
                    x.to_lowercase().cmp(y.to_lowercase())
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn digit_run(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        run.push(c);
    }
    run
}

fn compare_numbers(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Strips the diacritic from common Latin letters (Latin-1 and Latin Extended-A, which covers Hungarian).
fn fold_diacritic(c: char) -> char {
    match c {
        'À'..='Å' | 'Ā' | 'Ă' | 'Ą' => 'A',
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => 'C',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'Ď' | 'Đ' => 'D',
        'ď' | 'đ' => 'd',
        'È'..='Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => 'E',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => 'G',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'Ì'..='Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' => 'I',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'Ł' | 'Ĺ' | 'Ļ' | 'Ľ' => 'L',
        'ł' | 'ĺ' | 'ļ' | 'ľ' => 'l',
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' => 'N',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'Ò'..='Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' => 'O',
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'Ŕ' | 'Ŗ' | 'Ř' => 'R',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'Ś' | 'Ŝ' | 'Ş' | 'Š' => 'S',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'Ţ' | 'Ť' => 'T',
        'ţ' | 'ť' => 't',
        'Ù'..='Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => 'U',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'Ý' | 'Ÿ' => 'Y',
        'ý' | 'ÿ' => 'y',
        'Ź' | 'Ż' | 'Ž' => 'Z',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(ToString::to_string).collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        names
    }

    #[test]
    fn digit_runs_compare_numerically() {
        assert_eq!(
            sorted(&["web10", "web2", "web1", "web02x"]),
            ["web1", "web2", "web02x", "web10"]
        );
        assert_eq!(natural_cmp("a9b", "a10a"), Ordering::Less);
        assert_eq!(natural_cmp("v1.10", "v1.9"), Ordering::Greater);
    }

    #[test]
    fn case_insensitive_and_stable_on_ties() {
        assert_eq!(natural_cmp("Alpha", "alpha"), Ordering::Equal);
        assert_eq!(natural_cmp("01", "1"), Ordering::Equal);
        assert_eq!(sorted(&["b", "B", "a", "A"]), ["a", "A", "b", "B"]);
        assert_eq!(sorted(&["Zeta", "alpha", "Beta"]), ["alpha", "Beta", "Zeta"]);
    }

    #[test]
    fn prefixes_sort_first() {
        assert_eq!(natural_cmp("web", "web01"), Ordering::Less);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("a", ""), Ordering::Greater);
    }

    #[test]
    fn diacritics_sort_next_to_their_base_letter() {
        assert_eq!(sorted(&["f", "é", "e"]), ["e", "é", "f"]);
        assert_eq!(
            sorted(&["Pál", "Oszkár", "Ősz", "Osz"]),
            ["Osz", "Ősz", "Oszkár", "Pál"]
        );
        assert_eq!(natural_cmp("É", "é"), Ordering::Equal);
    }
}
