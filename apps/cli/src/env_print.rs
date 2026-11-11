//! Human-readable output of the environment report for `umr doctor`.

use umr_env::model::{EnvReport, Health, Origin, Severity};

fn gb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let v = bytes as f64 / 1_073_741_824.0;
    format!("{v:.1} GB")
}

fn health(h: &Health) -> String {
    match h {
        Health::Ok => "works".into(),
        Health::Broken(why) => format!("NOT WORKING: {why}"),
        Health::NotChecked(why) => format!("not checked ({why})"),
    }
}

fn origins(o: &[Origin]) -> String {
    o.iter()
        .map(|x| match x {
            Origin::Managed => "managed",
            Origin::EnvVar => "env var",
            Origin::Path => "PATH",
            Origin::Registry => "registry",
            Origin::CommonDir => "install folder",
            Origin::AndroidStudio => "Android Studio",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Prints the report in sections.
pub fn print(r: &EnvReport) {
    system(r);
    java(r);
    android(r);
    tools(r);
    notes(r);
    readiness(r);
}

fn system(r: &EnvReport) {
    let h = &r.host;
    println!("\nSystem");
    println!(
        "  Windows build {}{} ({}){}",
        h.windows_build,
        h.windows_display_version
            .as_deref()
            .map(|d| format!(" {d}"))
            .unwrap_or_default(),
        h.arch,
        if h.windows_11 {
            ""
        } else {
            "  — Windows 11 required"
        }
    );
    println!(
        "  Memory  {} total, {} free",
        gb(h.ram_total_bytes),
        gb(h.ram_available_bytes)
    );
    println!(
        "  Disk    {} free of {} (data folder volume)",
        gb(h.disk_free_bytes),
        gb(h.disk_total_bytes)
    );
    let v = &h.virtualization;
    println!(
        "  Emulator acceleration: {} (WHPX {}, AEHD {}, hypervisor {})",
        if v.acceleration_available {
            "available"
        } else {
            "NOT available"
        },
        v.whpx,
        if v.aehd_installed {
            "installed"
        } else {
            "not installed"
        },
        if v.hypervisor_present {
            "running"
        } else {
            "not running"
        }
    );
    println!(
        "  Long paths: {}",
        if h.long_paths_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    if let Some(p) = h
        .proxy
        .https_proxy
        .as_ref()
        .or(h.proxy.http_proxy.as_ref())
        .or(h.proxy.system_proxy.as_ref())
    {
        println!("  Proxy: {p}");
    }
}

fn java(r: &EnvReport) {
    println!("\nJava");
    if r.jdks.is_empty() {
        println!("  none found");
    }
    for j in &r.jdks {
        println!(
            "  {} {}{} — {} [{}] {}",
            if j.is_jdk { "JDK" } else { "JRE" },
            j.version.as_deref().unwrap_or("?"),
            j.vendor
                .as_deref()
                .map(|v| format!(" ({v})"))
                .unwrap_or_default(),
            j.home,
            origins(&j.origins),
            health(&j.health)
        );
    }
}

fn android(r: &EnvReport) {
    println!("\nAndroid SDK");
    if r.android_sdks.is_empty() {
        println!("  none found");
    }
    for s in &r.android_sdks {
        println!("  {} [{}]", s.root, origins(&s.origins));
        println!(
            "    adb: {}   emulator: {}",
            health(&s.adb),
            health(&s.emulator)
        );
        if s.packages.is_empty() {
            println!("    no packages");
        }
        for p in &s.packages {
            println!("    {:<52} {}", p.path, p.version.as_deref().unwrap_or(""));
        }
        if !s.licenses.is_empty() {
            println!("    licences accepted: {}", s.licenses.join(", "));
        }
    }
}

fn tools(r: &EnvReport) {
    println!("\nTools");
    if r.tools.is_empty() {
        println!("  none found");
    }
    for t in &r.tools {
        println!(
            "  {:<15} {:<18} {} [{}] {}",
            t.name,
            t.version.as_deref().unwrap_or("?"),
            t.path,
            origins(&t.origins),
            health(&t.health)
        );
    }
}

fn notes(r: &EnvReport) {
    if !r.conflicts.is_empty() {
        println!("\nConfiguration notes");
        for c in &r.conflicts {
            let mark = match c.severity {
                Severity::Warn => "[warn]",
                Severity::Info => "[info]",
            };
            println!("  {mark} {}\n         {}", c.message, c.hint);
        }
    }
}

fn readiness(r: &EnvReport) {
    println!("\nReadiness");
    for p in &r.readiness {
        if p.ready {
            println!("  [ready]   {}", p.profile);
        } else {
            println!("  [missing] {}: {}", p.profile, p.missing.join("; "));
        }
    }
    println!(
        "\n(scan took {} ms; nothing was installed or changed)",
        r.duration_ms
    );
}
