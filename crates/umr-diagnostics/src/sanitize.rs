//! Log sanitiser (DIA-005): masks secrets and personal data before any text leaves the Core
//! (`diagnostics.*`, `logs.tail`, and so the CLI, the desktop panels and — Phase 6 — agents).
//!
//! | What | Example | Becomes |
//! |---|---|---|
//! | private key blocks | `-----BEGIN RSA PRIVATE KEY----- …` | `[private key]` |
//! | JSON Web Tokens | `eyJhbGciOi….eyJzdWIi….sig` | `[jwt]` |
//! | known token formats | GitHub `ghp_…`, Slack `xoxb-…`, AWS `AKIA…`, Google `AIza…`, `sk-…`, npm, GitLab | `[token]` |
//! | `Authorization: Bearer|Basic …` | | `Authorization: Bearer [redacted]` |
//! | secret-named values | `password=…`, `"api_key": "…"`, `token: …`, `?access_token=…` | name kept, value `[redacted]` |
//! | credentials in URLs | `https://user:pass@host` | `https://[redacted]@host` |
//! | e-mail addresses | `someone@example.com` | `[email]` |
//! | the Windows / Unix user name in home paths | `C:\Users\alex\…`, `/home/alex/…` | `C:\Users\[user]\…` |
//!
//! Not masked on purpose: hashes and ids (SHA-256 of an APK, fingerprints, package names) —
//! they carry no secret and agents need them to correlate.

use std::sync::OnceLock;

use regex::{Captures, Regex};

struct Rules {
    private_key: Regex,
    jwt: Regex,
    tokens: Regex,
    auth_header: Regex,
    named: Regex,
    url_creds: Regex,
    email: Regex,
    win_home: Regex,
    unix_home: Regex,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| {
        let re = |s: &str| Regex::new(s).expect("sanitiser pattern");
        Rules {
            private_key: re(
                r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(-----END [A-Z ]*PRIVATE KEY-----|$)",
            ),
            jwt: re(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}"),
            tokens: re(concat!(
                r"\b(?:",
                r"gh[pousr]_[A-Za-z0-9]{30,}",
                r"|github_pat_[A-Za-z0-9_]{30,}",
                r"|glpat-[A-Za-z0-9_-]{20,}",
                r"|xox[abposr]-[A-Za-z0-9-]{10,}",
                r"|AKIA[0-9A-Z]{16}",
                r"|AIza[0-9A-Za-z_-]{35}",
                r"|sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}",
                r"|npm_[A-Za-z0-9]{30,}",
                r")"
            )),
            auth_header: re(r"(?i)\b(authorization\s*[:=]\s*(?:bearer|basic|token)\s+)[^\s,;]+"),
            named: re(concat!(
                r#"(?i)(["']?\b(?:[a-z0-9_.-]*(?:password|passwd|pwd|secret|token|api[_-]?key|apikey|"#,
                r#"access[_-]?key|private[_-]?key|client[_-]?secret|credentials?|signature|sig|auth))"#,
                r#"["']?\s*[:=]\s*["']?)([^"'\s&,;]{3,})"#
            )),
            url_creds: re(r"\b([a-zA-Z][a-zA-Z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@"),
            email: re(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b"),
            win_home: re(r"(?i)\b([a-z]:[\\/]+users[\\/]+)([^\\/\s:*?<>|]+)"),
            unix_home: re(r"(/home/|/Users/)([^/\s]+)"),
        }
    })
}

/// Names under `Users` that are not a person.
const SHARED_HOMES: &[&str] = &["public", "default", "all users", "[user]"];

/// `text` with secrets and personal data masked (see the module table).
#[must_use]
pub fn sanitize(text: &str) -> String {
    let r = rules();
    let t = r.private_key.replace_all(text, "[private key]");
    let t = r.jwt.replace_all(&t, "[jwt]");
    let t = r.tokens.replace_all(&t, "[token]");
    let t = r.auth_header.replace_all(&t, "${1}[redacted]");
    let t = r.url_creds.replace_all(&t, "${1}[redacted]@");
    let t = r.named.replace_all(&t, |c: &Captures| {
        if c[2].starts_with('[') {
            c[0].to_string()
        } else {
            format!("{}[redacted]", &c[1])
        }
    });
    let t = r.email.replace_all(&t, "[email]");
    let t = r.win_home.replace_all(&t, |c: &Captures| {
        if SHARED_HOMES.contains(&c[2].to_ascii_lowercase().as_str()) {
            c[0].to_string()
        } else {
            format!("{}[user]", &c[1])
        }
    });
    let t = r.unix_home.replace_all(&t, "${1}[user]");
    t.into_owned()
}

/// [`sanitize`] applied to every text field of a diagnostic.
#[must_use]
pub fn sanitize_diagnostic(d: &crate::Diagnostic) -> crate::Diagnostic {
    let mut d = d.clone();
    d.message = sanitize(&d.message);
    d.file = d.file.as_deref().map(sanitize);
    d.stack = d.stack.iter().map(|f| sanitize(f)).collect();
    d
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn secrets_are_masked() {
        let cases = [
            ("token ghp_abcdefghijklmnopqrstuvwxyz0123456789 used", "token [token] used"),
            ("key AKIAIOSFODNN7EXAMPLE end", "key [token] end"),
            ("AIzaSyA-1234567890abcdefghijklmnopqrstu x", "[token] x"),
            ("slack xoxb-1234567890-abcdefghij", "slack [token]"),
            ("ANTHROPIC sk-ant-api03-abcdefghijklmnopqrstuvwx", "ANTHROPIC [token]"),
            (
                "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
                "jwt [jwt]",
            ),
            ("Authorization: Bearer abc.def-123", "Authorization: Bearer [redacted]"),
            ("password=hunter22 user=x", "password=[redacted] user=x"),
            (r#"{"api_key": "abc123def", "n": 1}"#, r#"{"api_key": "[redacted]", "n": 1}"#),
            ("DB_PASSWORD: s3cr3t!", "DB_PASSWORD: [redacted]"),
            ("GET /x?access_token=qwerty12&page=2", "GET /x?access_token=[redacted]&page=2"),
            ("ws://127.0.0.1:5555/preview?token=Zm9vYmFy", "ws://127.0.0.1:5555/preview?token=[redacted]"),
            ("https://alice:pa55@repo.example.com/maven", "https://[redacted]@repo.example.com/maven"),
            ("mail me at jane.doe@example.org now", "mail me at [email] now"),
            (r"C:\Users\Alex\AppData\Local\x.log", r"C:\Users\[user]\AppData\Local\x.log"),
            ("file:///C:/Users/alex/proj/lib/main.dart:3", "file:///C:/Users/[user]/proj/lib/main.dart:3"),
            ("/home/alex/.gradle/caches", "/home/[user]/.gradle/caches"),
            (
                "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY----- after",
                "[private key] after",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(sanitize(input), want, "{input}");
        }
    }

    #[test]
    fn ordinary_diagnostics_are_left_alone() {
        for s in [
            "lib/main.dart:30:7: Error: Expected ';' after this.",
            "e: file:///D:/p/app/src/main/java/x/Main.kt:12:5 Unresolved reference 'foo'.",
            "java.lang.NullPointerException: Attempt to invoke virtual method 'int java.lang.String.length()' on a null object reference",
            "at com.example.app.MainActivity.onCreate(MainActivity.kt:42)",
            "Could not GET 'https://dl.google.com/dl/android/maven2/x.pom'. Received status code 502",
            "sha256 d090a50fbd6bdd3cd85e9c6cadbbdccec9b598332fc0c5148c52acc58b053e23",
            r"C:\Users\Public\Documents\x",
            "Execution failed for task ':app:compileDebugKotlin'.",
            "Task :app:signingReport",
            "A RenderFlex overflowed by 42 pixels on the bottom.",
        ] {
            assert_eq!(sanitize(s), s);
        }
    }

    #[test]
    fn masking_is_idempotent() {
        let once = sanitize("password=x1y2z3 jane@example.org C:\\Users\\bob\\x");
        assert_eq!(sanitize(&once), once);
    }
}
