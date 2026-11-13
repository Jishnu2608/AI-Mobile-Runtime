//! TEST-006: the failure corpus (`tests/failure-corpus/*.txt`) through the product's own
//! diagnostics path, and the M5 exit gate: ≥ 90 % correct category, ≥ 80 % correct `file:line`.
//! `cargo test -p umr-diagnostics --test corpus -- --nocapture` prints every case.

use std::path::{Path, PathBuf};

use umr_adapter::problems::{flutter_exception, gradle_failure, parse_line};
use umr_adapter::{AdapterError, Problem, Severity};
use umr_diagnostics::{
    from_adapter_error, from_anr, from_crash, from_message, from_problem, parse_anr,
    parse_crash_buffer, Category, Diagnostic, Origin,
};

const ROOT: &str = r"C:\p";

#[derive(Debug)]
struct Case {
    file: String,
    name: String,
    kind: String,
    source: Option<String>,
    fallback: Option<String>,
    expect: String,
    origin: Option<String>,
    at: Option<String>,
    text: String,
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/failure-corpus")
}

fn load() -> Vec<Case> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus_dir())
        .expect("tests/failure-corpus")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap().replace("\r\n", "\n");
        let fname = f.file_name().unwrap().to_string_lossy().to_string();
        for block in text.split("\n=== ").map(|b| b.trim_start_matches("=== ")) {
            if block.trim().is_empty() {
                continue;
            }
            let (head, body) = block
                .split_once("\n---\n")
                .unwrap_or_else(|| panic!("{fname}: no '---' in {block:.40}"));
            let mut lines = head.lines();
            let name = lines.next().unwrap().trim().to_string();
            let mut c = Case {
                file: fname.clone(),
                name,
                kind: String::new(),
                source: None,
                fallback: None,
                expect: String::new(),
                origin: None,
                at: None,
                text: body.trim_end_matches('\n').to_string(),
            };
            for l in lines {
                let (k, v) = l
                    .split_once(':')
                    .unwrap_or_else(|| panic!("{fname}/{}: bad header {l}", c.name));
                let v = v.trim().to_string();
                match k.trim() {
                    "kind" => c.kind = v,
                    "source" => c.source = Some(v),
                    "fallback" => c.fallback = Some(v),
                    "expect" => c.expect = v,
                    "origin" => c.origin = Some(v),
                    "at" => c.at = Some(v),
                    other => panic!("{fname}/{}: unknown header {other}", c.name),
                }
            }
            out.push(c);
        }
    }
    out
}

fn category(s: &str) -> Category {
    match s {
        "environment" => Category::Environment,
        "build" => Category::Build,
        "runtime" => Category::Runtime,
        "crash" => Category::Crash,
        "device" => Category::Device,
        "network" => Category::Network,
        other => panic!("unknown category {other}"),
    }
}

fn problem(source: &str, message: &str) -> Problem {
    Problem {
        severity: Severity::Error,
        message: message.to_string(),
        file: None,
        line: None,
        column: None,
        source: source.to_string(),
    }
}

fn first_problem(text: &str) -> Option<Problem> {
    text.lines().find_map(|l| parse_line(l, Path::new(ROOT)))
}

/// What the product makes of the case's text (mirrors the adapters and the Core).
fn diagnose(c: &Case) -> Diagnostic {
    let root = Path::new(ROOT);
    let first_line = c.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    match c.kind.as_str() {
        // gradle-android / flutter build: the parsed problems, else Gradle's summary
        "gradle" | "flutter-build" => {
            let src = if c.kind == "gradle" {
                "gradle"
            } else {
                "flutter"
            };
            first_problem(&c.text).map_or_else(
                || {
                    let summary = gradle_failure(&c.text).unwrap_or_else(|| first_line.to_string());
                    from_problem(&problem(src, &summary))
                },
                |p| from_problem(&p),
            )
        }
        // flutter run's app log: compiler lines, then framework / unhandled exceptions
        "flutter-log" => first_problem(&c.text)
            .or_else(|| flutter_exception(&c.text, root))
            .map_or_else(
                || from_problem(&problem("flutter", first_line)),
                |p| from_problem(&p),
            ),
        // react-native: Metro lines; a bundle without a parsed problem fails with its summary
        "metro" => first_problem(&c.text).map_or_else(
            || {
                from_adapter_error(&AdapterError::BuildFailed {
                    summary: "Metro could not bundle the app".into(),
                    problems: Vec::new(),
                })
                .remove(0)
            },
            |p| from_problem(&p),
        ),
        "tool" => {
            let src = c.source.as_deref().unwrap_or("tool");
            first_problem(&c.text).map_or_else(
                || from_problem(&problem(src, &c.text)),
                |p| from_problem(&p),
            )
        }
        "crash" => from_crash(&parse_crash_buffer(&c.text)[0]),
        "anr" => from_anr(&parse_anr(&c.text)[0]),
        "install" => from_adapter_error(&AdapterError::InstallFailed(c.text.clone())).remove(0),
        "missing" => from_adapter_error(&AdapterError::Missing(c.text.clone())).remove(0),
        "message" => from_message(
            c.source.as_deref().unwrap_or("core"),
            &c.text,
            category(c.fallback.as_deref().unwrap_or("environment")),
        ),
        other => panic!("{}/{}: unknown kind {other}", c.file, c.name),
    }
}

fn origin_name(o: Origin) -> &'static str {
    match o {
        Origin::App => "app",
        Origin::Environment => "environment",
        Origin::Unknown => "unknown",
    }
}

#[test]
fn m5_exit_gate() {
    let cases = load();
    let (mut cat_ok, mut loc_ok, mut loc_n, mut org_ok, mut org_n) = (0, 0, 0, 0, 0);
    let mut lines = Vec::new();
    for c in &cases {
        let d = diagnose(c);
        let got_at = match (&d.file, d.line) {
            (Some(f), Some(l)) => Some(format!("{f}:{l}")),
            _ => None,
        };
        let cat = d.category == category(&c.expect);
        cat_ok += usize::from(cat);
        let mut notes = Vec::new();
        if !cat {
            notes.push(format!("category {:?}", d.category));
        }
        if let Some(want) = c.at.as_deref().filter(|a| *a != "none") {
            loc_n += 1;
            if got_at.as_deref() == Some(want) {
                loc_ok += 1;
            } else {
                notes.push(format!("at {got_at:?}, want {want}"));
            }
        }
        if let Some(want) = &c.origin {
            org_n += 1;
            if origin_name(d.origin) == want {
                org_ok += 1;
            } else {
                notes.push(format!("origin {}, want {want}", origin_name(d.origin)));
            }
        }
        lines.push(format!(
            "{} {:<13} {:<40} {}",
            if notes.is_empty() { "ok  " } else { "MISS" },
            c.file.trim_end_matches(".txt"),
            c.name,
            notes.join("; ")
        ));
    }
    #[allow(clippy::cast_precision_loss)]
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    println!("{}", lines.join("\n"));
    println!(
        "\n{} cases · category {cat_ok}/{} = {:.1} % · file:line {loc_ok}/{loc_n} = {:.1} % · origin {org_ok}/{org_n} = {:.1} %",
        cases.len(),
        cases.len(),
        pct(cat_ok, cases.len()),
        pct(loc_ok, loc_n),
        pct(org_ok, org_n)
    );
    assert!(
        cases.len() >= 60,
        "TEST-006 needs ≥ 60 cases, has {}",
        cases.len()
    );
    for cat in [
        "environment",
        "build",
        "runtime",
        "crash",
        "device",
        "network",
    ] {
        assert!(
            cases.iter().filter(|c| c.expect == cat).count() >= 5,
            "few {cat} cases"
        );
    }
    assert!(
        pct(cat_ok, cases.len()) >= 90.0,
        "M5: category accuracy below 90 %"
    );
    assert!(
        pct(loc_ok, loc_n) >= 80.0,
        "M5: file:line accuracy below 80 %"
    );
}
