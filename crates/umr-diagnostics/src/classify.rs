//! Classifier (DIA-002): collector output → [`Diagnostic`] with one of six categories and an
//! app-versus-environment origin.
//!
//! Order of decision:
//! 1. What the collector already knows: crash buffer / ANR ⇒ `Crash`; a missing tool ⇒
//!    `Environment`; an install failure ⇒ `Device` unless the text says otherwise.
//! 2. Text rules ([`RULES`]), first match wins: device, then network, then environment. These
//!    override a compiler's "it is a build error" because Gradle reports a failed download or a
//!    wrong JDK through the same failure summary as a compile error.
//! 3. The reporting tool's default: compilers / bundlers ⇒ `Build`; JS and Flutter framework
//!    errors in a running app ⇒ `Runtime`.
//!
//! Origin: a location inside the project (relative path, outside `build/`, `node_modules/`,
//! `.dart_tool/`, …) or an app stack frame ⇒ `App`; environment, device and network ⇒
//! `Environment`; otherwise `Unknown`.

use umr_adapter::{AdapterError, Problem, Severity};

use crate::crash::{AnrEvent, CrashEvent, CrashKind};
use crate::fingerprint::fingerprint;
use crate::model::{Category, Diagnostic, Origin};

/// Text patterns, matched case-insensitively against the message. First match wins.
pub const RULES: &[(Category, &str)] = &[
    // device
    (Category::Device, "device offline"),
    (Category::Device, "device unauthorized"),
    (Category::Device, "unauthorized device"),
    (Category::Device, "no connected devices"),
    (Category::Device, "no devices/emulators found"),
    (Category::Device, "device not found"),
    (Category::Device, "device '"),
    (Category::Device, "install_failed_insufficient_storage"),
    (Category::Device, "install_failed_older_sdk"),
    (Category::Device, "install_failed_update_incompatible"),
    (Category::Device, "install_failed_no_matching_abis"),
    (Category::Device, "install_failed_version_downgrade"),
    (Category::Device, "install_failed_user_restricted"),
    (Category::Device, "emulator exited"),
    (Category::Device, "emulator process"),
    (Category::Device, "boot timed out"),
    (Category::Device, "adb: failed to"),
    (Category::Device, "error: closed"),
    (Category::Device, "lost connection to device"),
    (Category::Device, "device disconnected"),
    // network
    (Category::Network, "unknownhostexception"),
    (Category::Network, "unknown host"),
    (Category::Network, "failed host lookup"),
    (Category::Network, "getaddrinfo"),
    (Category::Network, "enotfound"),
    (Category::Network, "econnrefused"),
    (Category::Network, "econnreset"),
    (Category::Network, "etimedout"),
    (Category::Network, "connection refused"),
    (Category::Network, "connection reset"),
    (Category::Network, "connect timed out"),
    (Category::Network, "read timed out"),
    (Category::Network, "sockettimeoutexception"),
    (Category::Network, "socketexception"),
    (Category::Network, "could not get '"),
    (Category::Network, "could not head '"),
    (Category::Network, "could not download"),
    (Category::Network, "unable to find valid certification path"),
    (Category::Network, "pkix path building failed"),
    (Category::Network, "self signed certificate"),
    (Category::Network, "unable to get local issuer certificate"),
    (Category::Network, "proxy authentication required"),
    (Category::Network, "network is unreachable"),
    (Category::Network, "network request failed"),
    (Category::Network, "could not connect to development server"),
    (Category::Network, "unable to load script"),
    (Category::Network, "no internet connection"),
    (Category::Network, "got socket error trying to find package"),
    (Category::Network, "pub get failed"),
    (Category::Network, "request to https://registry"),
    (Category::Network, "fetch failed"),
    // environment
    (Category::Environment, "sdk location not found"),
    (Category::Environment, "android_home"),
    (Category::Environment, "android_sdk_root"),
    (Category::Environment, "java_home"),
    (
        Category::Environment,
        "unsupported class file major version",
    ),
    (Category::Environment, "requires java"),
    (Category::Environment, "incompatible java"),
    (Category::Environment, "could not determine java version"),
    (
        Category::Environment,
        "failed to find target with hash string",
    ),
    (Category::Environment, "failed to find build tools"),
    (Category::Environment, "failed to find platform sdk"),
    (Category::Environment, "did not have a source.properties"),
    (Category::Environment, "no installed build tools found"),
    (Category::Environment, "licenses have not been accepted"),
    (Category::Environment, "licences have not been accepted"),
    (Category::Environment, "license for package"),
    (Category::Environment, "ndk is not installed"),
    (Category::Environment, "ndk not configured"),
    (Category::Environment, "no version of ndk matched"),
    (Category::Environment, "cmake '"),
    (Category::Environment, "minimum supported gradle version"),
    (
        Category::Environment,
        "is not recognized as an internal or external command",
    ),
    (Category::Environment, "command not found"),
    (Category::Environment, "no such file or directory: 'node'"),
    (Category::Environment, "unsupported engine"),
    (Category::Environment, "requires node"),
    (Category::Environment, "the current dart sdk version is"),
    (Category::Environment, "requires sdk version"),
    (Category::Environment, "outofmemoryerror"),
    (Category::Environment, "java heap space"),
    (Category::Environment, "metaspace"),
    (Category::Environment, "gradle build daemon disappeared"),
    (Category::Environment, "daemon disappeared unexpectedly"),
    (Category::Environment, "no space left on device"),
    (Category::Environment, "not enough space"),
    (Category::Environment, "eaddrinuse"),
    (Category::Environment, "address already in use"),
    (Category::Environment, "port 8081"),
    (Category::Environment, "eperm"),
    (Category::Environment, "access is denied"),
    (Category::Environment, "filename too long"),
    (Category::Environment, "filename or extension is too long"),
    (Category::Environment, "path too long"),
    (Category::Environment, "hardware acceleration"),
    (Category::Environment, "whpx"),
    (Category::Environment, "hypervisor"),
];

/// Category from text alone, if any rule matches.
#[must_use]
pub fn rule_category(message: &str) -> Option<Category> {
    let m = message.to_ascii_lowercase();
    RULES.iter().find(|(_, p)| m.contains(p)).map(|(c, _)| *c)
}

/// Default category for problems a tool reported without a rule match.
fn source_default(source: &str, message: &str) -> Category {
    let m = message.to_ascii_lowercase();
    match source {
        "react-native" => Category::Runtime,
        "flutter" if m.contains("exception caught by") || m.contains("unhandled exception") => {
            Category::Runtime
        }
        _ if m.contains("unhandled exception") => Category::Runtime,
        _ => Category::Build,
    }
}

/// Folders inside a project that hold generated or third-party code.
const NOT_APP_DIRS: &[&str] = &[
    "build",
    "node_modules",
    ".dart_tool",
    ".gradle",
    ".pub-cache",
    "generated",
    ".expo",
    "Pods",
];

/// A path the adapters reported relative to the project (outside generated folders).
#[must_use]
pub fn is_app_file(file: &str) -> bool {
    let f = file.replace('\\', "/");
    let absolute = f.starts_with('/') || (f.len() > 1 && f.as_bytes()[1] == b':');
    !f.is_empty()
        && !absolute
        && !f.contains("://")
        && !f.split('/').any(|seg| NOT_APP_DIRS.contains(&seg))
}

fn origin(category: Category, app_located: bool) -> Origin {
    match category {
        Category::Environment | Category::Device => Origin::Environment,
        _ if app_located => Origin::App,
        Category::Network => Origin::Environment,
        _ => Origin::Unknown,
    }
}

#[allow(clippy::too_many_arguments)]
fn make(
    category: Category,
    app_located: bool,
    severity: Severity,
    source: &str,
    message: &str,
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    stack: Vec<String>,
) -> Diagnostic {
    let mut d = Diagnostic {
        category,
        origin: origin(category, app_located),
        severity,
        source: source.to_string(),
        message: message.trim().to_string(),
        file,
        line,
        column,
        stack,
        fingerprint: String::new(),
    };
    d.fingerprint = fingerprint(&d);
    d
}

/// A build or runtime [`Problem`] from an adapter (Gradle, kotlinc, javac, AAPT, Dart, Flutter,
/// Metro, React Native).
#[must_use]
pub fn from_problem(p: &Problem) -> Diagnostic {
    let category =
        rule_category(&p.message).unwrap_or_else(|| source_default(&p.source, &p.message));
    let app = p.file.as_deref().is_some_and(is_app_file);
    make(
        category,
        app,
        p.severity,
        &p.source,
        &p.message,
        p.file.clone(),
        p.line,
        p.column,
        Vec::new(),
    )
}

/// Java/Kotlin package prefixes that are never the app's own code.
const FRAMEWORK_FRAMES: &[&str] = &[
    "android.",
    "androidx.",
    "java.",
    "javax.",
    "jdk.",
    "sun.",
    "kotlin.",
    "kotlinx.",
    "dalvik.",
    "libcore.",
    "com.android.",
    "com.google.android.",
    "com.google.firebase.",
    "io.flutter.",
    "com.facebook.react.",
    "com.facebook.hermes.",
    "com.facebook.jni.",
    "com.swmansion.",
    "expo.modules.",
    "okhttp3.",
    "okio.",
    "retrofit2.",
    "org.jetbrains.",
    "org.chromium.",
];

/// `at a.b.C.m(File.kt:12)` → `a.b.C.m` belongs to the app (not a framework / library frame).
#[must_use]
pub fn is_app_frame(frame: &str) -> bool {
    let f = frame.trim().trim_start_matches("at ").trim();
    let class = f.split('(').next().unwrap_or(f);
    !class.is_empty() && !FRAMEWORK_FRAMES.iter().any(|p| class.starts_with(p))
}

fn frame_location(frame: &str) -> Option<(String, u32)> {
    let open = frame.rfind('(')?;
    let inner = frame[open + 1..].trim_end_matches(')');
    let (file, line) = inner.rsplit_once(':')?;
    (file.contains('.') && !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()))
        .then(|| (file.to_string(), line.parse().ok()))
        .and_then(|(f, l)| Some((f, l?)))
}

/// A crash from the crash buffer. The location is the first app frame when there is one (the
/// collector's own `file`/`line` is the first frame of any kind).
#[must_use]
pub fn from_crash(c: &CrashEvent) -> Diagnostic {
    let (app, file, line) = match c.kind {
        CrashKind::Fatal => match c
            .stack
            .iter()
            .filter(|f| is_app_frame(f))
            .find_map(|f| frame_location(f))
        {
            Some((f, l)) => (true, Some(f), Some(l)),
            None => (false, c.file.clone(), c.line),
        },
        // the app's own native library lives under /data/app/<package>…
        CrashKind::Native => (c.stack.iter().any(|f| f.contains("/data/app/")), None, None),
    };
    make(
        Category::Crash,
        app,
        Severity::Error,
        "logcat",
        &c.message,
        file,
        line,
        None,
        c.stack.clone(),
    )
}

/// An Application Not Responding report.
#[must_use]
pub fn from_anr(a: &AnrEvent) -> Diagnostic {
    let msg = match &a.reason {
        Some(r) => format!("ANR in {}: {r}", a.package),
        None => format!("ANR in {}", a.package),
    };
    make(
        Category::Crash,
        false,
        Severity::Error,
        "logcat",
        &msg,
        None,
        None,
        None,
        Vec::new(),
    )
}

/// Free text from any other layer (session, device, supervisor): rules, else `fallback`.
#[must_use]
pub fn from_message(source: &str, message: &str, fallback: Category) -> Diagnostic {
    let category = rule_category(message).unwrap_or(fallback);
    make(
        category,
        false,
        Severity::Error,
        source,
        message,
        None,
        None,
        None,
        Vec::new(),
    )
}

/// An adapter's error. A build failure becomes one diagnostic per problem it carries.
#[must_use]
pub fn from_adapter_error(e: &AdapterError) -> Vec<Diagnostic> {
    match e {
        AdapterError::BuildFailed { summary, problems } if problems.is_empty() => {
            vec![from_message("gradle", summary, Category::Build)]
        }
        AdapterError::BuildFailed { problems, .. } => problems.iter().map(from_problem).collect(),
        AdapterError::Missing(m)
        | AdapterError::NotApplicable(m)
        | AdapterError::Unsupported(m) => {
            vec![from_message("adapter", m, Category::Environment)]
        }
        AdapterError::InstallFailed(m) => vec![from_message("install", m, Category::Device)],
        AdapterError::LaunchFailed(m) => vec![from_message("launch", m, Category::Runtime)],
        AdapterError::Failed(m) => vec![from_message("adapter", m, Category::Environment)],
    }
}
