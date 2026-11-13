//! Seeded cases for the classifier (DIA-002) and dedupe (DIA-003). Tool lines go through the
//! adapters' own parser (`problems::parse_line`) first, so each case runs the real chain
//! tool output → Problem → Diagnostic. The full ≥ 60-case corpus is TEST-006 (step 5.5).

use std::path::Path;

use umr_adapter::problems::parse_line;
use umr_adapter::{AdapterError, Problem, Severity};

use crate::crash::parse_crash_buffer;
use crate::fingerprint::{fingerprint, fnv1a, normalise};
use crate::{
    from_adapter_error, from_anr, from_crash, from_message, from_problem, parse_anr, Added,
    Category, Dedupe, Diagnostic, Origin,
};

const ROOT: &str = r"C:\p";

fn tool(line: &str) -> Diagnostic {
    let p = parse_line(line, Path::new(ROOT)).unwrap_or_else(|| panic!("not parsed: {line}"));
    from_problem(&p)
}

fn reported(source: &str, message: &str) -> Diagnostic {
    from_problem(&Problem {
        severity: Severity::Error,
        message: message.into(),
        file: None,
        line: None,
        column: None,
        source: source.into(),
    })
}

fn crash(text: &str) -> Diagnostic {
    from_crash(&parse_crash_buffer(text)[0])
}

const FRAMEWORK_ONLY_CRASH: &str = "11-13 20:25:15.994  1835  1835 E AndroidRuntime: FATAL EXCEPTION: main\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: Process: com.umr.flutter_demo, PID: 1835\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: android.app.RemoteServiceException$CrashedByAdbException: shell-induced crash\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.app.ActivityThread.throwRemoteServiceException(ActivityThread.java:2377)\n\
11-13 20:25:15.994  1835  1835 E AndroidRuntime: \tat android.os.Looper.loop(Looper.java:338)\n";

fn app_crash(pid: u32) -> String {
    format!(
        "11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: FATAL EXCEPTION: main\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: Process: com.umr.kotlindemo, PID: {pid}\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: java.lang.NumberFormatException: For input string: \"abc\"\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: \tat java.lang.Integer.parseInt(Integer.java:615)\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: \tat kotlin.text.StringsKt.toInt(Strings.kt:20)\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: \tat com.example.umrkotlindemo.Calc.parse(Calc.kt:8)\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: \tat com.example.umrkotlindemo.MainScreenKt.MainScreen(MainScreen.kt:94)\n\
11-13 20:30:00.000 {pid} {pid} E AndroidRuntime: \tat android.app.Activity.performCreate(Activity.java:8294)\n"
    )
}

fn native(lib: &str) -> String {
    format!(
        "11-13 20:40:00.000  5678  5678 F DEBUG   : *** *** *** *** *** *** *** *** *** *** *** ***\n\
11-13 20:40:00.000  5678  5678 F DEBUG   : pid: 5678, tid: 5678, name: app  >>> com.umr.ndkdemo <<<\n\
11-13 20:40:00.000  5678  5678 F DEBUG   : signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0\n\
11-13 20:40:00.000  5678  5678 F DEBUG   :       #00 pc 00001234  {lib}\n"
    )
}

struct Case {
    name: &'static str,
    got: Diagnostic,
    category: Category,
    origin: Origin,
    /// `file:line` expected, when the fault has a location.
    at: Option<&'static str>,
}

#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    use Category::{Build, Crash, Device, Environment, Network, Runtime};
    use Origin::{App, Environment as Env, Unknown};
    let c = |name, got, category, origin, at| Case {
        name,
        got,
        category,
        origin,
        at,
    };
    vec![
        // build — compiler / resource / bundler errors in the project
        c("kotlin k2", tool("e: file:///C:/p/app/src/main/java/x/Main.kt:12:5 Unresolved reference 'foo'."), Build, App, Some("app/src/main/java/x/Main.kt:12")),
        c("kotlin old", tool(r"e: C:\p\app\src\main\java\x\Main.kt: (7, 3): Type mismatch: inferred type is String but Int was expected"), Build, App, Some("app/src/main/java/x/Main.kt:7")),
        c("javac", tool(r"C:\p\app\src\main\java\x\Main.java:20: error: cannot find symbol"), Build, App, Some("app/src/main/java/x/Main.java:20")),
        c("aapt", tool(r"ERROR: C:\p\app\src\main\res\layout\main.xml:4: AAPT: error: attribute android:foo not found."), Build, App, Some("app/src/main/res/layout/main.xml:4")),
        c("dart", tool("lib/main.dart:30:7: Error: Expected ';' after this."), Build, App, Some("lib/main.dart:30")),
        c("babel", tool(r"ERROR  SyntaxError: C:\p\App.js: Unexpected token (12:5)"), Build, App, Some("App.js:12")),
        c("generated code", tool("e: file:///C:/p/app/build/generated/source/x/R.kt:3:1 Redeclaration: class R"), Build, Unknown, Some("app/build/generated/source/x/R.kt:3")),
        c("pub cache package", tool("C:/Users/u/AppData/Local/Pub/Cache/hosted/pub.dev/foo-1.0.0/lib/foo.dart:10:3: Error: The method 'x' isn't defined for the class 'Y'."), Build, Unknown, Some("C:/Users/u/AppData/Local/Pub/Cache/hosted/pub.dev/foo-1.0.0/lib/foo.dart:10")),
        c("gradle script", reported("gradle", "Execution failed for task ':app:mergeDebugResources'. > Resource compilation failed"), Build, Unknown, None),
        // runtime — the app runs and reported an error
        c("js runtime", tool("ERROR  ReferenceError: Property 'x' doesn't exist"), Runtime, Unknown, None),
        c("flutter framework", reported("flutter", "EXCEPTION CAUGHT BY RENDERING LIBRARY"), Runtime, Unknown, None),
        c("dart unhandled", reported("flutter", "[ERROR:flutter/runtime/dart_vm_initializer.cc(40)] Unhandled Exception: Bad state: No element"), Runtime, Unknown, None),
        // environment — toolchain / machine
        c("sdk location", reported("gradle", "SDK location not found. Define a valid SDK location with an ANDROID_HOME environment variable or by setting the sdk.dir path"), Environment, Env, None),
        c("class file version", reported("gradle", "Unsupported class file major version 65"), Environment, Env, None),
        c("agp needs java", reported("gradle", "Android Gradle plugin requires Java 17 to run. You are currently using Java 11."), Environment, Env, None),
        c("platform missing", reported("gradle", "Failed to find target with hash string 'android-34' in: C:\\UMR\\android-sdk"), Environment, Env, None),
        c("licence", reported("gradle", "License for package Android SDK Build-Tools 34 not accepted."), Environment, Env, None),
        c("daemon died", reported("gradle", "Gradle build daemon disappeared unexpectedly (it may have been killed or may have crashed)"), Environment, Env, None),
        c("heap", reported("gradle", "java.lang.OutOfMemoryError: Java heap space"), Environment, Env, None),
        c("ndk", reported("gradle", "No version of NDK matched the requested version 26.1.10909125"), Environment, Env, None),
        c("metro port", from_message("metro", "error listen EADDRINUSE: address already in use :::8081", Runtime), Environment, Env, None),
        c("missing tool", from_adapter_error(&AdapterError::Missing("flutter is not installed in the managed toolchain".into())).remove(0), Environment, Env, None),
        c("node engine", from_message("npm", "npm ERR! Unsupported engine for react-native@0.76.0: wanted: {\"node\":\">=18\"}", Build), Environment, Env, None),
        // network — downloads, Metro, DNS, TLS
        c("gradle download", reported("gradle", "Could not resolve all files for configuration ':app:debugRuntimeClasspath'. > Could not GET 'https://dl.google.com/dl/android/maven2/x.pom'. Received status code 502"), Network, Env, None),
        c("dns", reported("gradle", "Could not resolve com.squareup.okhttp3:okhttp:4.12.0. > java.net.UnknownHostException: repo.maven.apache.org"), Network, Env, None),
        c("tls proxy", reported("gradle", "PKIX path building failed: unable to find valid certification path to requested target"), Network, Env, None),
        c("pub", from_message("flutter", "Got socket error trying to find package flutter_lints at https://pub.dev.", Build), Network, Env, None),
        c("npm", from_message("npm", "npm ERR! request to https://registry.npmjs.org/react failed, reason: getaddrinfo ENOTFOUND registry.npmjs.org", Build), Network, Env, None),
        c("metro unreachable", reported("react-native", "Could not connect to development server."), Network, Env, None),
        c("app fetch", tool("ERROR  TypeError: Network request failed"), Network, Env, None),
        // device
        c("storage", from_adapter_error(&AdapterError::InstallFailed("Failure [INSTALL_FAILED_INSUFFICIENT_STORAGE]".into())).remove(0), Device, Env, None),
        c("signatures", from_adapter_error(&AdapterError::InstallFailed("INSTALL_FAILED_UPDATE_INCOMPATIBLE: Package com.x signatures do not match".into())).remove(0), Device, Env, None),
        c("older sdk", from_adapter_error(&AdapterError::InstallFailed("INSTALL_FAILED_OLDER_SDK: Requires newer sdk version #35".into())).remove(0), Device, Env, None),
        c("offline", from_message("adb", "error: device offline", Environment), Device, Env, None),
        c("unauthorized", from_message("adb", "adb: device unauthorized.", Environment), Device, Env, None),
        c("not found", from_message("adb", "error: device 'emulator-5554' not found", Environment), Device, Env, None),
        c("install other", from_adapter_error(&AdapterError::InstallFailed("adb install exited with 1".into())).remove(0), Device, Env, None),
        // crash
        c("framework-only crash", crash(FRAMEWORK_ONLY_CRASH), Crash, Unknown, Some("ActivityThread.java:2377")),
        c("app crash", crash(&app_crash(900)), Crash, App, Some("Calc.kt:8")),
        c("native in app lib", crash(&native("/data/app/~~x==/com.umr.ndkdemo-y==/lib/x86_64/libnative.so")), Crash, App, None),
        c("native in system lib", crash(&native("/system/lib64/libc.so (abort+100)")), Crash, Unknown, None),
        c("anr", from_anr(&parse_anr("I ActivityManager: ANR in com.x (com.x/.Main)\nI ActivityManager: Reason: Input dispatching timed out")[0]), Crash, Unknown, None),
    ]
}

fn at(d: &Diagnostic) -> Option<String> {
    Some(format!("{}:{}", d.file.as_deref()?, d.line?))
}

#[test]
fn every_seeded_case_is_classified() {
    let cases = cases();
    let mut wrong = Vec::new();
    for c in &cases {
        if c.got.category != c.category || c.got.origin != c.origin || at(&c.got).as_deref() != c.at
        {
            wrong.push(format!(
                "{}: got {:?}/{:?} at {:?}, want {:?}/{:?} at {:?}",
                c.name,
                c.got.category,
                c.got.origin,
                at(&c.got),
                c.category,
                c.origin,
                c.at
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} wrong:\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

#[test]
fn every_category_is_covered() {
    let cases = cases();
    for cat in [
        Category::Environment,
        Category::Build,
        Category::Runtime,
        Category::Crash,
        Category::Device,
        Category::Network,
    ] {
        assert!(
            cases.iter().filter(|c| c.category == cat).count() >= 3,
            "{cat:?}"
        );
    }
}

#[test]
fn build_failure_without_problems_uses_its_summary() {
    let d = from_adapter_error(&AdapterError::BuildFailed {
        summary: "Could not GET 'https://repo.maven.apache.org/x'".into(),
        problems: Vec::new(),
    });
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].category, Category::Network);
    let d = from_adapter_error(&AdapterError::BuildFailed {
        summary: String::new(),
        problems: vec![
            parse_line("lib/a.dart:1:1: Error: x", Path::new(ROOT)).unwrap(),
            parse_line("lib/b.dart:2:1: Error: y", Path::new(ROOT)).unwrap(),
        ],
    });
    assert_eq!(d.len(), 2);
}

#[test]
fn fnv_matches_the_reference_value() {
    // FNV-1a 64 test vector
    assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
}

#[test]
fn normalise_masks_volatile_parts() {
    assert_eq!(
        normalise("A RenderFlex overflowed by 42.5 pixels  on the bottom at 0x7ffd1234."),
        "A RenderFlex overflowed by #.# pixels on the bottom at #."
    );
}

#[test]
fn a_flood_of_one_problem_becomes_one_entry() {
    let mut dd = Dedupe::new(100);
    for i in 0..1000u32 {
        let d = reported(
            "flutter",
            &format!("EXCEPTION CAUGHT BY RENDERING LIBRARY: overflowed by {i} pixels"),
        );
        let added = dd.add(d, u64::from(i));
        assert_eq!(
            added,
            if i == 0 {
                Added::New
            } else {
                Added::Repeat(i + 1)
            }
        );
    }
    assert_eq!(dd.len(), 1, "1000 repeats → 1 entry (≥ 95 % reduction)");
    let e = &dd.entries()[0];
    assert_eq!((e.count, e.first_seen_ms, e.last_seen_ms), (1000, 0, 999));
}

#[test]
fn the_same_message_on_another_line_is_another_problem() {
    let mut dd = Dedupe::new(100);
    dd.add(
        tool("lib/main.dart:30:7: Error: Expected ';' after this."),
        0,
    );
    dd.add(
        tool("lib/main.dart:31:7: Error: Expected ';' after this."),
        1,
    );
    dd.add(
        tool("lib/other.dart:30:7: Error: Expected ';' after this."),
        2,
    );
    dd.add(
        tool("lib/main.dart:30:9: Error: Expected ';' after this."),
        3,
    ); // same line, another column
    assert_eq!(dd.len(), 3);
}

#[test]
fn a_crash_loop_is_one_crash_but_another_crash_is_not() {
    let mut dd = Dedupe::new(100);
    for pid in [900, 901, 902] {
        dd.add(crash(&app_crash(pid)), u64::from(pid));
    }
    dd.add(crash(FRAMEWORK_ONLY_CRASH), 1000);
    assert_eq!(dd.len(), 2);
    assert_eq!(dd.entries()[0].count, 3);
}

#[test]
fn categories_never_share_a_fingerprint() {
    let a = from_message("x", "same text", Category::Build);
    let b = from_message("x", "same text", Category::Runtime);
    assert_ne!(a.fingerprint, b.fingerprint);
    assert_eq!(a.fingerprint, fingerprint(&a));
    assert_eq!(a.fingerprint.len(), 16);
}

#[test]
fn the_limit_drops_the_least_recently_seen() {
    let mut dd = Dedupe::new(2);
    dd.add(from_message("x", "one", Category::Build), 0);
    dd.add(from_message("x", "two", Category::Build), 1);
    dd.add(from_message("x", "one", Category::Build), 2); // "one" is now the most recent
    dd.add(from_message("x", "three", Category::Build), 3); // drops "two"
    let msgs: Vec<&str> = dd
        .entries()
        .iter()
        .map(|e| e.diagnostic.message.as_str())
        .collect();
    assert_eq!(msgs, ["one", "three"]);
    assert_eq!(
        dd.add(from_message("x", "one", Category::Build), 4),
        Added::Repeat(3)
    );
}

#[test]
fn the_cursor_returns_new_and_repeated_entries() {
    let mut dd = Dedupe::new(10);
    dd.add(from_message("x", "one", Category::Build), 0);
    dd.add(from_message("x", "two", Category::Build), 1);
    let cur = dd.cursor();
    assert_eq!(dd.since(cur).count(), 0);
    dd.add(from_message("x", "one", Category::Build), 2);
    let newer: Vec<&str> = dd
        .since(cur)
        .map(|e| e.diagnostic.message.as_str())
        .collect();
    assert_eq!(newer, ["one"], "a repeat counts as new for readers");
    let fp = dd.entries()[1].diagnostic.fingerprint.clone();
    assert!(dd.contains(&fp) && dd.get(&fp).is_some_and(|e| e.count == 1));
}

#[test]
fn retain_drops_fixed_entries() {
    let mut dd = Dedupe::new(10);
    dd.add(from_message("x", "compile", Category::Build), 0);
    dd.add(from_message("x", "crash", Category::Crash), 1);
    let cur = dd.cursor();
    dd.retain(|e| e.diagnostic.category != Category::Build);
    assert_eq!(dd.len(), 1);
    assert!(dd.cursor() > cur, "readers notice the change");
    assert_eq!(
        dd.add(from_message("x", "crash", Category::Crash), 2),
        Added::Repeat(2)
    );
}
