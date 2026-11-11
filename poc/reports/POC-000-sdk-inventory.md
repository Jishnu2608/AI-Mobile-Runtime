# POC-000 — Read-only inventory of existing Android SDK

**Date:** 2026-11-11 · **Host:** Windows 11 Home 10.0.26300, Intel Core Ultra 7 258V, ~31.5 GB RAM
**Method:** read `source.properties` files, list directories, run `emulator -accel-check`. Nothing modified.

## Findings
| Item | Result |
|---|---|
| SDK root | `%LOCALAPPDATA%\Android\Sdk` (Android Studio default path), ~4.45 GB |
| `ANDROID_HOME` / `ANDROID_SDK_ROOT` | unset |
| `adb` on PATH | no (platform-tools present but not on PATH) |
| cmdline-tools (sdkmanager/avdmanager) | **missing** |
| platform-tools | 37.0.0 |
| build-tools | 36.1.0, 37.0.0 |
| platforms | android-36.1 ("Android SDK Platform 16") |
| sources | android-36.1 |
| emulator | 36.5.11 |
| system images | android-37.0 / `google_apis_playstore_ps16k` / x86_64 (rev 4, 16 KB page size, with Play Store) |
| extras | AEHD driver package (files only; install state not checked) |
| licences accepted | `android-sdk-license` only |
| AVDs | none (`%USERPROFILE%\.android` does not exist) |
| Android Studio | not found (`C:\Program Files\Android` exists but empty) |
| JDK | none on PATH, `JAVA_HOME` unset |
| Acceleration | `emulator -accel-check`: **WHPX (10.0.26300) is installed and usable** |
| CPU virtualization flag | `Win32_Processor.VirtualizationFirmwareEnabled = False` while `HypervisorPresent = True` |

## Interpretation
1. Real-world case of provisioning experiment 4 ("SDK installed, components missing"): Studio was likely installed and removed, leaving the SDK. No AVD, no cmdline-tools, no JDK, no env vars.
2. Platform android-36.1 and system image android-37.0 do not match. A project targeting API 36 would need another image, or API 37 platform, depending on project `compileSdk`.
3. The image is a Play Store image with 16 KB pages. Fine for running apps; Google APIs image is preferred for our default profiles (adb root possible). Decision for POC-001.
4. **Detection lesson (feeds ENV-001 / POC-002):** when Hyper-V/WHPX is active, WMI reports `VirtualizationFirmwareEnabled = False`. Do not use that flag alone. Use `HypervisorPresent` + `emulator -accel-check` as the authority.
5. sdkmanager cannot run here without cmdline-tools **and** a JDK → the Android CLI backend (which manages SDK packages itself) may have an advantage on such machines. Check in POC-001.

## Policy
SDK is user-owned. Our product would reuse it read-only, or install missing parts only with user consent (`master_build.md` §11). POC-001 will use a separate managed root, not this SDK.

## Status
Done. Validated by direct inspection.
