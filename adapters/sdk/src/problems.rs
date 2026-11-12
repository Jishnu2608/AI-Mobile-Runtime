//! Shared parsers that turn tool output lines into [`Problem`]s with file and line.
//!
//! | Tool | Example |
//! |---|---|
//! | kotlinc (K2) | `e: file:///C:/p/app/src/main/java/x/Main.kt:12:5 Unresolved reference 'foo'.` |
//! | kotlinc (old) | `e: C:\p\app\src\main\java\x\Main.kt: (12, 5): Unresolved reference: foo` |
//! | javac | `C:\p\app\src\main\java\x\Main.java:12: error: cannot find symbol` |
//! | AAPT | `ERROR: C:\p\app\src\main\res\layout\a.xml:12: AAPT: error: ...` |
//! | Dart / Flutter | `lib/main.dart:12:5: Error: Expected ';' after this.` |
//! | Metro / Babel | `ERROR  SyntaxError: C:\p\App.js: Unexpected token (12:5)` |
//! | React Native runtime | `ERROR  ReferenceError: Property 'x' doesn't exist` (no location) |
//!
//! Paths inside the project are reported relative to it (forward slashes).

use std::path::Path;

use crate::{Problem, Severity};

/// `file:///C:/a/b` or `C:\a\b` → path relative to `root` when inside it.
#[must_use]
pub fn relative(path: &str, root: &Path) -> String {
    let p = path
        .strip_prefix("file:///")
        .unwrap_or(path)
        .replace('\\', "/");
    let r = root.display().to_string().replace('\\', "/");
    let r = r.trim_end_matches('/');
    if p.len() > r.len() && p[..r.len()].eq_ignore_ascii_case(r) && p.as_bytes()[r.len()] == b'/' {
        p[r.len() + 1..].to_string()
    } else {
        p
    }
}

fn num(s: &str) -> Option<u32> {
    s.trim().parse().ok()
}

/// Splits `path:line:col rest` where `path` may contain a drive colon (`C:/...`).
fn split_location(s: &str) -> Option<(&str, u32, Option<u32>, &str)> {
    // skip a drive letter so its colon is not taken as the line separator
    let skip = if s.len() > 2 && s.as_bytes()[1] == b':' {
        2
    } else {
        0
    };
    let first = s[skip..].find(':')? + skip;
    let path = &s[..first];
    let rest = &s[first + 1..];
    let line_end = rest.find(|c: char| !c.is_ascii_digit())?;
    let line = num(&rest[..line_end])?;
    let mut rest = &rest[line_end..];
    let mut col = None;
    if let Some(r) = rest.strip_prefix(':') {
        let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
        if end > 0 {
            col = num(&r[..end]);
            rest = &r[end..];
        } else {
            rest = r;
        }
    }
    Some((path, line, col, rest.trim_start_matches(':').trim()))
}

fn problem(
    sev: Severity,
    msg: &str,
    file: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    src: &str,
) -> Problem {
    Problem {
        severity: sev,
        message: msg.trim().to_string(),
        file,
        line,
        column: col,
        source: src.to_string(),
    }
}

/// Kotlin compiler line.
fn kotlin(line: &str, root: &Path) -> Option<Problem> {
    let (sev, rest) = if let Some(r) = line.strip_prefix("e: ") {
        (Severity::Error, r)
    } else {
        (Severity::Warning, line.strip_prefix("w: ")?)
    };
    // old format: `path: (12, 5): message`
    if let Some(at) = rest.find(": (") {
        let (path, tail) = (&rest[..at], &rest[at + 3..]);
        if let Some(close) = tail.find("): ") {
            let mut lc = tail[..close].split(',');
            let line_no = lc.next().and_then(num);
            let col = lc.next().and_then(num);
            return Some(problem(
                sev,
                &tail[close + 3..],
                Some(relative(path, root)),
                line_no,
                col,
                "kotlinc",
            ));
        }
    }
    let rest = rest.strip_prefix("file:///").unwrap_or(rest);
    match split_location(rest) {
        Some((path, l, c, msg)) => Some(problem(
            sev,
            msg,
            Some(relative(path, root)),
            Some(l),
            c,
            "kotlinc",
        )),
        None => Some(problem(sev, rest, None, None, None, "kotlinc")),
    }
}

/// javac line: `path.java:12: error: message`.
fn javac(line: &str, root: &Path) -> Option<Problem> {
    let (path, l, _, rest) = split_location(line)?;
    if !path.to_ascii_lowercase().ends_with(".java") {
        return None;
    }
    let (sev, msg) = if let Some(m) = rest.strip_prefix("error:") {
        (Severity::Error, m)
    } else {
        (Severity::Warning, rest.strip_prefix("warning:")?)
    };
    Some(problem(
        sev,
        msg,
        Some(relative(path, root)),
        Some(l),
        None,
        "javac",
    ))
}

/// AAPT resource error.
fn aapt(line: &str, root: &Path) -> Option<Problem> {
    let rest = line
        .strip_prefix("ERROR: ")
        .or_else(|| line.strip_prefix("ERROR:"))?;
    let at = rest.find(": AAPT: ")?;
    let (loc, msg) = (&rest[..at], &rest[at + 8..]);
    let with_colon = format!("{loc}:");
    let (path, l, c, _) = split_location(&with_colon).unwrap_or((loc, 0, None, ""));
    let msg = msg.strip_prefix("error:").unwrap_or(msg);
    Some(problem(
        Severity::Error,
        msg,
        Some(relative(path, root)),
        (l > 0).then_some(l),
        c,
        "aapt",
    ))
}

/// Dart / Flutter front-end line: `lib/main.dart:12:5: Error: message`.
fn dart(line: &str, root: &Path) -> Option<Problem> {
    let (path, l, c, rest) = split_location(line.trim())?;
    if !path.to_ascii_lowercase().ends_with(".dart") {
        return None;
    }
    let (sev, msg) = if let Some(m) = rest.strip_prefix("Error:") {
        (Severity::Error, m)
    } else {
        (Severity::Warning, rest.strip_prefix("Warning:")?)
    };
    Some(problem(
        sev,
        msg,
        Some(relative(path, root)),
        Some(l),
        c,
        "dart",
    ))
}

/// Metro / Babel: `[ERROR  ]SyntaxError: <path>: <message> (<line>:<col>)` and JS runtime errors
/// forwarded by Metro (`ERROR  TypeError: ...`).
fn metro(line: &str, root: &Path) -> Option<Problem> {
    let t = line.trim();
    let t = t.strip_prefix("ERROR").map_or(t, str::trim_start);
    let kind_end = t.find(": ")?;
    let kind = &t[..kind_end];
    if !kind.ends_with("Error") || kind.contains(' ') {
        return None;
    }
    let rest = &t[kind_end + 2..];
    // with a location: `<path>.(js|jsx|ts|tsx): message (12:5)`
    for ext in [".tsx: ", ".ts: ", ".jsx: ", ".js: "] {
        if let Some(at) = rest.find(ext) {
            let path = &rest[..at + ext.len() - 2];
            let msg = &rest[at + ext.len()..];
            let (msg, l, c) = match (msg.rfind(" ("), msg.ends_with(')')) {
                (Some(open), true) => {
                    let mut lc = msg[open + 2..msg.len() - 1].split(':');
                    (
                        &msg[..open],
                        lc.next().and_then(num),
                        lc.next().and_then(num),
                    )
                }
                _ => (msg, None, None),
            };
            return Some(problem(
                Severity::Error,
                &format!("{kind}: {msg}"),
                Some(relative(path, root)),
                l,
                c,
                "metro",
            ));
        }
    }
    // runtime errors only when Metro marked them as errors
    line.trim_start().starts_with("ERROR").then(|| {
        problem(
            Severity::Error,
            &format!("{kind}: {rest}"),
            None,
            None,
            None,
            "react-native",
        )
    })
}

/// One output line → a problem, if it is one.
#[must_use]
pub fn parse_line(line: &str, root: &Path) -> Option<Problem> {
    let t = line.trim_end();
    kotlin(t, root)
        .or_else(|| javac(t, root))
        .or_else(|| aapt(t, root))
        .or_else(|| dart(t, root))
        .or_else(|| metro(t, root))
}

/// Follows a growing log file (a tool's stdout/stderr) and hands out complete new lines.
#[derive(Debug)]
pub struct LogFollower {
    path: std::path::PathBuf,
    pos: u64,
    pending: String,
}

impl LogFollower {
    /// Starts at the current end of the file (only new output is read).
    #[must_use]
    pub fn from_end(path: &Path) -> Self {
        let pos = std::fs::metadata(path).map_or(0, |m| m.len());
        Self {
            path: path.to_path_buf(),
            pos,
            pending: String::new(),
        }
    }

    /// Complete lines written since the last call.
    pub fn new_lines(&mut self) -> Vec<String> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let len = f.metadata().map_or(0, |m| m.len());
        if len < self.pos {
            self.pos = 0; // the file was recreated
        }
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        self.pos += buf.len() as u64;
        self.pending.push_str(&String::from_utf8_lossy(&buf));
        let mut out = Vec::new();
        while let Some(nl) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=nl).collect();
            out.push(line.trim_end().to_string());
        }
        out
    }
}

/// Gradle's summary after `* What went wrong:` (up to the next `* ` section).
#[must_use]
pub fn gradle_failure(output: &str) -> Option<String> {
    let mut lines = output
        .lines()
        .skip_while(|l| !l.starts_with("* What went wrong:"));
    lines.next()?;
    let text: Vec<&str> = lines
        .take_while(|l| !l.starts_with("* "))
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    (!text.is_empty()).then(|| text.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(r"C:\work\demo")
    }

    #[test]
    fn kotlin_k2_and_old_formats() {
        let p = parse_line(
            "e: file:///C:/work/demo/app/src/main/java/com/x/MainActivity.kt:12:5 Unresolved reference 'foo'.",
            root(),
        )
        .unwrap();
        assert_eq!(
            p.file.as_deref(),
            Some("app/src/main/java/com/x/MainActivity.kt")
        );
        assert_eq!((p.line, p.column), (Some(12), Some(5)));
        assert_eq!(p.message, "Unresolved reference 'foo'.");
        assert_eq!(
            (p.severity, p.source.as_str()),
            (Severity::Error, "kotlinc")
        );

        let o = parse_line(
            r"w: C:\work\demo\app\src\Main.kt: (3, 9): Variable 'x' is never used",
            root(),
        )
        .unwrap();
        assert_eq!(o.severity, Severity::Warning);
        assert_eq!(o.file.as_deref(), Some("app/src/Main.kt"));
        assert_eq!((o.line, o.column), (Some(3), Some(9)));
    }

    #[test]
    fn javac_aapt_and_dart() {
        let j = parse_line(
            r"C:\work\demo\app\src\main\java\x\Main.java:12: error: cannot find symbol",
            root(),
        )
        .unwrap();
        assert_eq!(
            (j.file.as_deref(), j.line),
            (Some("app/src/main/java/x/Main.java"), Some(12))
        );
        assert_eq!(j.message, "cannot find symbol");

        let a = parse_line(
            r"ERROR: C:\work\demo\app\src\main\res\layout\main.xml:7: AAPT: error: attribute android:texxt not found.",
            root(),
        )
        .unwrap();
        assert_eq!(
            (a.file.as_deref(), a.line),
            (Some("app/src/main/res/layout/main.xml"), Some(7))
        );
        assert!(a.message.contains("texxt"));

        let d = parse_line(
            "lib/main.dart:24:7: Error: Expected ';' after this.",
            root(),
        )
        .unwrap();
        assert_eq!(
            (d.file.as_deref(), d.line, d.column),
            (Some("lib/main.dart"), Some(24), Some(7))
        );
        assert_eq!(d.source, "dart");
    }

    #[test]
    fn metro_and_react_native_errors() {
        let p = parse_line(
            r#"ERROR  SyntaxError: C:\work\demo\App.js: Unexpected token, expected "," (12:5)"#,
            root(),
        )
        .unwrap();
        assert_eq!(
            (p.file.as_deref(), p.line, p.column),
            (Some("App.js"), Some(12), Some(5))
        );
        assert!(p.message.starts_with("SyntaxError: Unexpected token"));
        assert_eq!(p.source, "metro");
        let r = parse_line(
            "ERROR  ReferenceError: Property 'missingThing' doesn't exist",
            root(),
        )
        .unwrap();
        assert_eq!((r.file, r.source.as_str()), (None, "react-native"));
        assert!(parse_line("Android Bundled 812ms index.js (650 modules)", root()).is_none());
        assert!(parse_line("TypeError: not marked as error by Metro", root()).is_none());
    }

    #[test]
    fn follower_reads_only_new_complete_lines() {
        let f = std::env::temp_dir().join(format!("umr-follow-{}.log", std::process::id()));
        std::fs::write(&f, "old line\n").unwrap();
        let mut fol = LogFollower::from_end(&f);
        assert_eq!(fol.new_lines(), Vec::<String>::new());
        std::fs::write(&f, "old line\nnew 1\nnew 2\npartial").unwrap();
        assert_eq!(fol.new_lines(), ["new 1", "new 2"]);
        std::fs::write(&f, "old line\nnew 1\nnew 2\npartial end\n").unwrap();
        assert_eq!(fol.new_lines(), ["partial end"]);
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn ordinary_lines_are_not_problems() {
        for l in [
            "> Task :app:compileDebugKotlin",
            "BUILD SUCCESSFUL in 7s",
            "e:",
            "Launching lib/main.dart on sdk gphone64 x86 64 in debug mode...",
            "C:\\x\\notes.txt:3: error: nope",
        ] {
            assert!(parse_line(l, root()).is_none() || l == "e:", "{l}");
        }
        let out = "FAILURE: Build failed with an exception.\n\n* What went wrong:\nExecution failed for task ':app:compileDebugKotlin'.\n> Compilation error. See log for more details\n\n* Try:\n> Run with --stacktrace";
        assert_eq!(
            gradle_failure(out).as_deref(),
            Some("Execution failed for task ':app:compileDebugKotlin'. > Compilation error. See log for more details")
        );
        assert_eq!(relative("D:/other/x.kt", root()), "D:/other/x.kt");
    }
}
