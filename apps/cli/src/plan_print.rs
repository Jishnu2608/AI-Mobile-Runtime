//! Human-readable output of `umr plan`.

use umr_resolver::plan::{Action, Plan};
use umr_resolver::project::{Framework, ProjectFacts};

#[allow(clippy::cast_precision_loss)]
fn size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

fn framework(f: Framework) -> &'static str {
    match f {
        Framework::AndroidNative => "Android (Kotlin/Java, Gradle)",
        Framework::Flutter => "Flutter",
        Framework::ReactNative => "React Native",
        Framework::Expo => "Expo (no native folders)",
        Framework::Unknown => "not recognised",
    }
}

pub fn print(f: &ProjectFacts, p: &Plan) {
    println!("Project   {}", f.root);
    println!(
        "Framework {}  (profile {})",
        framework(f.framework),
        p.profile
    );
    if let Some(a) = &f.android {
        let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "?".into());
        println!(
            "Android   Gradle {} · AGP {} · compileSdk {} · minSdk {} · targetSdk {}",
            show(&a.gradle),
            show(&a.agp),
            show(&a.compile_sdk),
            show(&a.min_sdk),
            show(&a.target_sdk)
        );
    }
    for (label, v) in [
        ("Flutter", &f.flutter_pinned),
        ("Flutter", &f.flutter_constraint),
        ("Expo", &f.expo),
        ("RN", &f.react_native),
        ("Node", &f.node_constraint),
    ] {
        if let Some(v) = v {
            println!("{label:<9} {v}");
        }
    }
    println!("Knowledge Pack {}", p.pack_version);

    println!("\nComponents");
    for i in &p.items {
        let what = i
            .package
            .as_deref()
            .map_or_else(|| i.title.clone(), |pkg| format!("{} ({pkg})", i.title));
        let ver = i
            .version
            .as_deref()
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        match &i.action {
            Action::Reuse { path, .. } => println!("  [reuse]   {what}{ver}\n            {path}"),
            Action::Install => println!(
                "  [install] {what}{ver}  ~{}",
                size(i.approx_download_bytes)
            ),
        }
        println!("            why: {}", i.reason);
        for s in &i.sources {
            println!("            source: {s}");
        }
    }

    println!(
        "\nDownload  ~{}   Disk ~{}   Free {}{}",
        size(p.total_download_bytes),
        size(p.total_disk_bytes),
        size(p.disk_free_bytes),
        if p.disk_ok { "" } else { "   NOT ENOUGH SPACE" }
    );
    if !p.licences.is_empty() {
        println!("\nLicences (shown in full and accepted by you before anything is installed)");
        for l in &p.licences {
            let tag = if l.requires_acceptance {
                "accept"
            } else {
                "info"
            };
            println!("  [{tag}] {}  {}", l.title, l.url);
        }
    }
    if !p.admin_actions.is_empty() {
        println!("\nAdministrator steps (only with your consent)");
        for a in &p.admin_actions {
            let tag = if a.required {
                "required"
            } else {
                "recommended"
            };
            let reboot = if a.reboot { ", needs restart" } else { "" };
            println!("  [{tag}{reboot}] {}: {}", a.title, a.reason);
        }
    }
    if !p.conflicts.is_empty() {
        println!("\nConflicts in the project (the project is not changed)");
        for c in &p.conflicts {
            println!("  [{}] {}\n            fix: {}", c.id, c.message, c.hint);
            for s in &c.sources {
                println!("            source: {s}");
            }
        }
    }
    if !p.notes.is_empty() {
        println!("\nNotes");
        for n in &p.notes {
            println!("  - {n}");
        }
    }
    println!("\nNothing was installed. Run `umr install` to install the components above.");
}
