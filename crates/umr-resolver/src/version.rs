//! Lenient version numbers and npm-style ranges.
//!
//! Versions are compared numerically component by component (`9.10 > 9.9`); trailing text such as
//! `-rc1` or `+9` is ignored for ordering. Ranges support the subset used by `engines` fields and
//! pubspec constraints: `>=`, `>`, `<=`, `<`, `=`, `^`, `~`, `x`/`*` wildcards, space-separated
//! AND and `||` OR.

use std::cmp::Ordering;
use std::fmt;

/// A version as a list of numbers (`17.0.21` → `[17, 0, 21]`).
/// Missing trailing parts count as 0, so `8.0` == `8.0.0`.
#[derive(Debug, Clone)]
pub struct Version(pub Vec<u64>);

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl Version {
    /// Parses the leading numeric part: `v22.11.0` → 22.11.0, `9.1.0-rc1` → 9.1.0, `1.8.0_381` → 1.8.0.381.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(['v', 'V', '=']);
        let mut parts = Vec::new();
        for seg in s.split(['.', '_']) {
            let digits: String = seg.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                break;
            }
            parts.push(digits.parse().ok()?);
            if digits.len() != seg.len() {
                break; // e.g. "0-rc1": keep 0, stop
            }
        }
        (!parts.is_empty()).then_some(Self(parts))
    }

    #[must_use]
    pub fn major(&self) -> u64 {
        self.0.first().copied().unwrap_or(0)
    }

    fn component(&self, i: usize) -> u64 {
        self.0.get(i).copied().unwrap_or(0)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.0.len().max(other.0.len());
        (0..n)
            .map(|i| self.component(i).cmp(&other.component(i)))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s: Vec<String> = self.0.iter().map(u64::to_string).collect();
        write!(f, "{}", s.join("."))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

/// A version range: OR of ANDs of comparators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    any_of: Vec<Vec<(Op, Version)>>,
    text: String,
}

impl Range {
    /// Parses a range; `None` if nothing usable is found.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut any_of = Vec::new();
        for alt in text.split("||") {
            let mut all = Vec::new();
            let tokens: Vec<String> = normalise_tokens(alt);
            for t in &tokens {
                all.extend(comparators(t)?);
            }
            if all.is_empty() {
                // "*" or "x": anything
                all.push((Op::Ge, Version(vec![0])));
            }
            any_of.push(all);
        }
        (!any_of.is_empty()).then(|| Self {
            any_of,
            text: text.trim().to_string(),
        })
    }

    #[must_use]
    pub fn contains(&self, v: &Version) -> bool {
        self.any_of.iter().any(|all| {
            all.iter().all(|(op, bound)| match op {
                Op::Ge => v >= bound,
                Op::Gt => v > bound,
                Op::Le => v <= bound,
                Op::Lt => v < bound,
                Op::Eq => v == bound,
            })
        })
    }

    /// Lowest version mentioned as a lower bound (used to pick a minimum).
    #[must_use]
    pub fn minimum(&self) -> Option<Version> {
        self.any_of
            .iter()
            .filter_map(|all| {
                all.iter()
                    .filter(|(op, _)| matches!(op, Op::Ge | Op::Gt | Op::Eq))
                    .map(|(_, v)| v.clone())
                    .max()
            })
            .min()
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// Splits on whitespace but keeps operators attached: `">= 18"` → `[">=18"]`.
fn normalise_tokens(alt: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in alt.split_whitespace() {
        let raw = raw.trim_matches(|c| c == '"' || c == '\'' || c == ',');
        if raw.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if matches!(last.as_str(), ">=" | ">" | "<=" | "<" | "=" | "^" | "~") {
                last.push_str(raw);
                continue;
            }
        }
        out.push(raw.to_string());
    }
    out
}

fn comparators(t: &str) -> Option<Vec<(Op, Version)>> {
    let (op, rest) = if let Some(r) = t.strip_prefix(">=") {
        (Some(Op::Ge), r)
    } else if let Some(r) = t.strip_prefix("<=") {
        (Some(Op::Le), r)
    } else if let Some(r) = t.strip_prefix('>') {
        (Some(Op::Gt), r)
    } else if let Some(r) = t.strip_prefix('<') {
        (Some(Op::Lt), r)
    } else if let Some(r) = t.strip_prefix('=') {
        (Some(Op::Eq), r)
    } else {
        (None, t)
    };
    if matches!(rest, "*" | "x" | "X" | "") {
        return Some(Vec::new());
    }
    if let Some(r) = rest.strip_prefix('^') {
        let v = Version::parse(r)?;
        let upper = match v.0.as_slice() {
            [0, 0, p, ..] => Version(vec![0, 0, p + 1]),
            [0, m, ..] => Version(vec![0, m + 1]),
            [m, ..] => Version(vec![m + 1]),
            [] => return None,
        };
        return Some(vec![(Op::Ge, v), (Op::Lt, upper)]);
    }
    if let Some(r) = rest.strip_prefix('~') {
        let v = Version::parse(r)?;
        let upper = match v.0.as_slice() {
            [m] => Version(vec![m + 1]),
            [m, n, ..] => Version(vec![*m, n + 1]),
            [] => return None,
        };
        return Some(vec![(Op::Ge, v), (Op::Lt, upper)]);
    }
    // Wildcards: "22.x", "22.*" and a bare major "22" all mean "any 22.x".
    let has_wildcard = rest.split('.').any(|p| matches!(p, "x" | "X" | "*"));
    let v = Version::parse(rest)?;
    match op {
        None if has_wildcard || !rest.contains('.') => {
            let mut upper = v.0.clone();
            if let Some(last) = upper.last_mut() {
                *last += 1;
            }
            Some(vec![(Op::Ge, v), (Op::Lt, Version(upper))])
        }
        None => Some(vec![(Op::Eq, v)]),
        Some(op) => Some(vec![(op, v)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn r(s: &str) -> Range {
        Range::parse(s).unwrap()
    }

    #[test]
    fn parsing_and_ordering() {
        assert_eq!(v("17.0.21").0, vec![17, 0, 21]);
        assert_eq!(v("v22.11.0").0, vec![22, 11, 0]);
        assert_eq!(v("9.1.0-rc1").0, vec![9, 1, 0]);
        assert_eq!(v("1.8.0_381").0, vec![1, 8, 0, 381]);
        assert!(v("9.10") > v("9.9"));
        assert_eq!(v("8.0"), v("8.0.0"));
        assert!(v("8.0") < v("8.0.1"));
        assert_eq!(Version::parse("abc"), None);
    }

    #[test]
    fn ranges() {
        assert!(r(">=22.11.0").contains(&v("24.15.0")));
        assert!(!r(">=22.11.0").contains(&v("20.19.4")));
        assert!(r("^20.19.4 || >=22.12.0").contains(&v("20.20.0")));
        assert!(!r("^20.19.4 || >=22.12.0").contains(&v("21.0.0")));
        assert!(r(">=3.4.0 <4.0.0").contains(&v("3.13.7")));
        assert!(!r(">=3.4.0 <4.0.0").contains(&v("4.0.0")));
        assert!(r("^3.9.0").contains(&v("3.13.7")));
        assert!(r("~1.2.3").contains(&v("1.2.9")) && !r("~1.2.3").contains(&v("1.3.0")));
        assert!(r("22.x").contains(&v("22.13.1")) && !r("22.x").contains(&v("23.0.0")));
        assert!(r(">= 18").contains(&v("18.0.0")));
        assert!(r("*").contains(&v("1.0.0")));
        assert!(r("1.2.3").contains(&v("1.2.3")) && !r("1.2.3").contains(&v("1.2.4")));
        assert_eq!(r("^20.19.4 || >=22.12.0").minimum(), Some(v("20.19.4")));
    }
}
