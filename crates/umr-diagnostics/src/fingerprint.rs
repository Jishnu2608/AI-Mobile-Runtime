//! Fingerprints and dedupe (DIA-003).
//!
//! The same problem repeats: a Flutter overflow logs every frame, Gradle prints an error once per
//! variant, a crash loop restarts the app. A fingerprint is equal for repeats and differs for
//! distinct problems; [`Dedupe`] keeps one entry per fingerprint with a count.
//!
//! Fingerprint input: category, source, the message with volatile parts masked (numbers, hex
//! addresses, quoted temp paths, pids), file and line, and for crashes the first app (or first)
//! stack frame. The line number stays: the same message on two lines is two problems.

use std::collections::HashMap;

use serde::Serialize;

use crate::classify::is_app_frame;
use crate::model::{Category, Diagnostic};

/// Masks parts that change between repeats of one problem.
#[must_use]
pub fn normalise(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '0' && chars.peek().is_some_and(|n| *n == 'x' || *n == 'X') {
            chars.next();
            while chars.peek().is_some_and(char::is_ascii_hexdigit) {
                chars.next();
            }
            out.push('#');
        } else if c.is_ascii_digit() {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            out.push('#');
        } else if c.is_whitespace() {
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// 64-bit FNV-1a: stable across builds and Rust versions (unlike `DefaultHasher`), so stored
/// fingerprints stay comparable.
pub(crate) fn fnv1a(data: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 16 hex digits.
#[must_use]
pub fn fingerprint(d: &Diagnostic) -> String {
    let frame = if d.category == Category::Crash {
        d.stack
            .iter()
            .find(|f| is_app_frame(f))
            .or_else(|| d.stack.first())
            .map(|f| normalise(f))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let key = format!(
        "{:?}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        d.category,
        d.source,
        normalise(&d.message),
        d.file.as_deref().unwrap_or("").replace('\\', "/"),
        d.line.map_or(String::new(), |l| l.to_string()),
        frame
    );
    format!("{:016x}", fnv1a(&key))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub diagnostic: Diagnostic,
    /// How many times it was seen (≥ 1).
    pub count: u32,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// Changes on every add (new or repeat): readers pass the highest they saw as `since`.
    pub seq: u64,
}

/// What [`Dedupe::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// A new fingerprint.
    New,
    /// A repeat; the entry's count after this one.
    Repeat(u32),
}

/// One entry per fingerprint, in first-seen order, capped at `limit` entries (the entry seen
/// least recently is dropped first) so a flood of distinct messages cannot grow it without end.
#[derive(Debug)]
pub struct Dedupe {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    limit: usize,
    seq: u64,
}

impl Dedupe {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
            limit: limit.max(1),
            seq: 0,
        }
    }

    pub fn add(&mut self, d: Diagnostic, now_ms: u64) -> Added {
        self.seq += 1;
        if let Some(&i) = self.index.get(&d.fingerprint) {
            let e = &mut self.entries[i];
            e.count = e.count.saturating_add(1);
            e.last_seen_ms = now_ms;
            e.seq = self.seq;
            return Added::Repeat(e.count);
        }
        if self.entries.len() >= self.limit {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_seen_ms)
                .map_or(0, |(i, _)| i);
            self.entries.remove(oldest);
            self.reindex();
        }
        self.index.insert(d.fingerprint.clone(), self.entries.len());
        self.entries.push(Entry {
            diagnostic: d,
            count: 1,
            first_seen_ms: now_ms,
            last_seen_ms: now_ms,
            seq: self.seq,
        });
        Added::New
    }

    fn reindex(&mut self) {
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.diagnostic.fingerprint.clone(), i))
            .collect();
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Entries added or repeated after `since` (first-seen order).
    pub fn since(&self, since: u64) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| e.seq > since)
    }

    #[must_use]
    pub fn get(&self, fingerprint: &str) -> Option<&Entry> {
        self.index.get(fingerprint).map(|&i| &self.entries[i])
    }

    #[must_use]
    pub fn contains(&self, fingerprint: &str) -> bool {
        self.index.contains_key(fingerprint)
    }

    /// The cursor after everything added so far.
    #[must_use]
    pub fn cursor(&self) -> u64 {
        self.seq
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Keeps only the entries `keep` accepts (e.g. drops build errors a successful reload fixed).
    pub fn retain(&mut self, keep: impl Fn(&Entry) -> bool) {
        let before = self.entries.len();
        self.entries.retain(|e| keep(e));
        if self.entries.len() != before {
            self.seq += 1;
            self.reindex();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }
}
