//! DEV-002: AVDs written from our own hardware definitions (not `avdmanager` device ids, and
//! not `android emulator create`, which picks its own image — POC-001 finding 4).
//!
//! Lazy: an AVD is created the first time its profile is used. All profiles share the one
//! installed system image per API level; each AVD adds only its config (the data partition is
//! created by the emulator on first boot). Files go to the managed AVD home
//! (`ANDROID_AVD_HOME`, below the toolchains folder).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{Profile, PROFILES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvdInfo {
    pub name: String,
    pub profile: String,
    pub api: String,
    pub ini: String,
    pub dir: String,
    pub exists: bool,
}

/// `umr_<profile>_api<api>` (dots in API levels become `_`).
#[must_use]
pub fn avd_name(profile: &str, api: &str) -> String {
    format!("umr_{profile}_api{}", api.replace('.', "_"))
}

/// Image folder below the SDK for an API level, tag and ABI.
#[must_use]
pub fn image_rel(api: &str, tag: &str, abi: &str) -> String {
    format!("system-images\\android-{api}\\{tag}\\{abi}\\")
}

/// Installed `google_apis` `x86_64` images in an SDK, newest API first.
#[must_use]
pub fn installed_apis(sdk: &Path, tag: &str, abi: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(sdk.join("system-images"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let api = name.strip_prefix("android-")?.to_string();
            e.path()
                .join(tag)
                .join(abi)
                .join("system.img")
                .is_file()
                .then_some(api)
        })
        .collect();
    let key = |a: &String| {
        a.split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    out.sort_by_key(|a| std::cmp::Reverse(key(a)));
    out
}

/// `config.ini` text for a profile (DEC-006 defaults).
#[must_use]
pub fn config_ini(p: &Profile, api: &str, tag: &str, abi: &str) -> String {
    let name = avd_name(p.id, api);
    let mut s = String::new();
    let mut kv = |k: &str, v: &str| {
        let _ = writeln!(s, "{k}={v}");
    };
    kv("AvdId", &name);
    kv(
        "avd.ini.displayname",
        &format!("UMR {} (API {api})", p.class),
    );
    kv("avd.ini.encoding", "UTF-8");
    kv("abi.type", abi);
    kv(
        "hw.cpu.arch",
        if abi.starts_with("arm64") {
            "arm64"
        } else {
            "x86_64"
        },
    );
    kv("hw.cpu.ncore", "4");
    kv("image.sysdir.1", &image_rel(api, tag, abi));
    kv("tag.id", tag);
    kv(
        "tag.display",
        if tag == "google_apis" {
            "Google APIs"
        } else {
            tag
        },
    );
    kv("target", &format!("android-{api}"));
    kv("PlayStore.enabled", "false");
    kv("hw.lcd.width", &p.width.to_string());
    kv("hw.lcd.height", &p.height.to_string());
    kv("hw.lcd.density", &p.density.to_string());
    kv("hw.ramSize", &p.ram_mb.to_string());
    kv("disk.dataPartition.size", &format!("{}G", p.data_gb));
    kv("hw.gpu.enabled", "yes");
    kv("hw.gpu.mode", "host");
    kv("hw.keyboard", "yes");
    kv("hw.mainKeys", "no");
    kv("hw.initialOrientation", "portrait");
    kv("hw.accelerometer", "yes");
    kv("hw.gyroscope", "yes");
    kv("hw.sensors.orientation", "yes");
    kv("hw.sensors.proximity", "yes");
    kv("hw.audioInput", "yes");
    kv("hw.camera.back", "none");
    kv("hw.camera.front", "none");
    kv("showDeviceFrame", "no");
    kv("skin.dynamic", "yes");
    kv("fastboot.forceColdBoot", "no");
    kv("fastboot.forceFastBoot", "yes");
    if let Some(f) = &p.fold {
        kv("hw.sensor.hinge", "yes");
        kv("hw.sensor.hinge.count", "1");
        kv("hw.sensor.hinge.type", &f.hinge_type.to_string());
        kv("hw.sensor.hinge.sub_type", "1");
        kv("hw.sensor.hinge.ranges", f.hinge_range);
        kv("hw.sensor.hinge.defaults", &f.hinge_default.to_string());
        kv("hw.sensor.hinge.areas", f.hinge_area);
        let list: Vec<String> = f.postures.iter().map(|x| x.index().to_string()).collect();
        kv("hw.sensor.posture_list", &list.join(","));
        kv(
            "hw.sensor.hinge_angles_posture_definitions",
            &f.posture_angles.replace(' ', ""),
        );
        if let Some((x, y, w, h)) = f.folded_region {
            kv("hw.displayRegion.0.1.xOffset", &x.to_string());
            kv("hw.displayRegion.0.1.yOffset", &y.to_string());
            kv("hw.displayRegion.0.1.width", &w.to_string());
            kv("hw.displayRegion.0.1.height", &h.to_string());
        }
    }
    s
}

/// Where an AVD lives.
#[must_use]
pub fn info(avd_home: &Path, profile: &str, api: &str) -> AvdInfo {
    let name = avd_name(profile, api);
    let ini = avd_home.join(format!("{name}.ini"));
    let dir = avd_home.join(format!("{name}.avd"));
    AvdInfo {
        exists: ini.is_file() && dir.join("config.ini").is_file(),
        name,
        profile: profile.into(),
        api: api.into(),
        ini: ini.display().to_string(),
        dir: dir.display().to_string(),
    }
}

/// Creates (or refreshes the config of) the AVD for a profile. User data in an existing AVD is
/// kept; only `config.ini` and the pointer `.ini` are (re)written.
pub fn ensure(
    avd_home: &Path,
    sdk: &Path,
    profile: &Profile,
    api: &str,
    tag: &str,
    abi: &str,
) -> Result<AvdInfo, String> {
    let image = sdk.join(image_rel(api, tag, abi));
    if !image.join("system.img").is_file() {
        return Err(format!(
            "system image for API {api} ({tag}, {abi}) is not installed in {}",
            sdk.display()
        ));
    }
    let i = info(avd_home, profile.id, api);
    let dir = PathBuf::from(&i.dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join("config.ini"), config_ini(profile, api, tag, abi))
        .map_err(|e| e.to_string())?;
    let pointer = format!(
        "avd.ini.encoding=UTF-8\npath={}\npath.rel=avd\\{}.avd\ntarget=android-{api}\n",
        dir.display(),
        i.name
    );
    std::fs::write(&i.ini, pointer).map_err(|e| e.to_string())?;
    Ok(info(avd_home, profile.id, api))
}

/// All catalogue profiles with their AVD state for one API level.
#[must_use]
pub fn list(avd_home: &Path, api: &str) -> Vec<(&'static Profile, AvdInfo)> {
    PROFILES
        .iter()
        .map(|p| (p, info(avd_home, p.id, api)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::find;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-avd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_sdk(root: &Path, api: &str) -> PathBuf {
        let img = root
            .join("sdk")
            .join("system-images")
            .join(format!("android-{api}"))
            .join("google_apis")
            .join("x86_64");
        std::fs::create_dir_all(&img).unwrap();
        std::fs::write(img.join("system.img"), b"x").unwrap();
        root.join("sdk")
    }

    #[test]
    fn config_for_phone_and_foldable() {
        let c = config_ini(find("standard").unwrap(), "36", "google_apis", "x86_64");
        for line in [
            "hw.lcd.width=1080",
            "hw.lcd.height=2400",
            "hw.lcd.density=420",
            "hw.ramSize=4096",
            "hw.gpu.mode=host",
            "PlayStore.enabled=false",
            "image.sysdir.1=system-images\\android-36\\google_apis\\x86_64\\",
            "target=android-36",
            "AvdId=umr_standard_api36",
        ] {
            assert!(c.lines().any(|l| l == line), "missing {line}\n{c}");
        }
        assert!(!c.contains("hw.sensor.hinge"), "phones have no hinge");
        let f = config_ini(find("fold").unwrap(), "36", "google_apis", "x86_64");
        for line in [
            "hw.sensor.hinge=yes",
            "hw.sensor.hinge.type=1",
            "hw.sensor.hinge.areas=884-0-1-2208",
            "hw.sensor.posture_list=1,2,3",
            "hw.sensor.hinge_angles_posture_definitions=0-30,30-150,150-180",
            "hw.displayRegion.0.1.width=884",
        ] {
            assert!(f.lines().any(|l| l == line), "missing {line}\n{f}");
        }
        let flip = config_ini(find("flip").unwrap(), "36", "google_apis", "x86_64");
        assert!(flip.contains("hw.sensor.hinge.type=0") && !flip.contains("hw.displayRegion"));
    }

    #[test]
    fn ensure_creates_lazily_and_keeps_user_data() {
        let d = tmp("ensure");
        let sdk = fake_sdk(&d, "36");
        let home = d.join("avd");
        assert!(!info(&home, "standard", "36").exists);
        let i = ensure(
            &home,
            &sdk,
            find("standard").unwrap(),
            "36",
            "google_apis",
            "x86_64",
        )
        .unwrap();
        assert!(i.exists && i.name == "umr_standard_api36");
        let pointer = std::fs::read_to_string(&i.ini).unwrap();
        assert!(
            pointer.contains("target=android-36") && pointer.contains("path="),
            "{pointer}"
        );
        std::fs::write(PathBuf::from(&i.dir).join("userdata-qemu.img"), b"data").unwrap();
        ensure(
            &home,
            &sdk,
            find("standard").unwrap(),
            "36",
            "google_apis",
            "x86_64",
        )
        .unwrap();
        assert!(
            PathBuf::from(&i.dir).join("userdata-qemu.img").is_file(),
            "user data kept"
        );
        let all = list(&home, "36");
        assert_eq!(
            all.iter().filter(|(_, a)| a.exists).count(),
            1,
            "only the used profile exists"
        );
        assert!(
            ensure(
                &home,
                &sdk,
                find("fold").unwrap(),
                "35",
                "google_apis",
                "x86_64"
            )
            .is_err(),
            "missing image"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn installed_apis_newest_first() {
        let d = tmp("apis");
        let sdk = fake_sdk(&d, "35");
        fake_sdk(&d, "36");
        fake_sdk(&d, "36.1");
        std::fs::create_dir_all(
            sdk.join("system-images")
                .join("android-37")
                .join("google_apis"),
        )
        .unwrap();
        assert_eq!(
            installed_apis(&sdk, "google_apis", "x86_64"),
            ["36.1", "36", "35"]
        );
        assert_eq!(avd_name("fold", "36.1"), "umr_fold_api36_1");
        let _ = std::fs::remove_dir_all(&d);
    }
}
