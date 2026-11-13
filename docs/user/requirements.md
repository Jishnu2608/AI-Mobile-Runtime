# Requirements

## Your PC
| | Needed | Notes |
|---|---|---|
| Windows | Windows 11, x64 | Windows 10 and Arm PCs are not supported yet |
| CPU virtualization | VT-x / AMD-V **enabled in the firmware (BIOS/UEFI)** | Task Manager → Performance → CPU shows "Virtualization: Enabled". If disabled, turn it on in the firmware settings — software cannot |
| Windows Hypervisor Platform | On | The app can turn it on for you (administrator prompt + restart): `umr admin enable-whpx` |
| RAM | 16 GB recommended, 8 GB minimum | With 8 GB use the `compact` profile and close other heavy apps |
| Disk | ~25 MB for the app; 8–15 GB for toolchains and one emulator image | You choose the folder (`[storage] toolchains_dir`); each extra emulator image is 2–4 GB |
| Network | Internet for the first setup and for dependency downloads (Gradle, pub, npm) | Proxies set in Windows are used; corporate TLS inspection works when its root certificate is in the Windows store |
| Long paths | Recommended | `umr admin enable-long-paths` (administrator prompt) |

## What the runtime installs for you (only what a project needs)
| Component | From | Used for |
|---|---|---|
| Eclipse Temurin JDK | adoptium.net | Gradle, Android tools |
| Android command-line tools, platform-tools, build-tools, platform, emulator, system image | Google (dl.google.com), via Android CLI | building and running Android apps |
| Flutter SDK | Google (storage.googleapis.com) | Flutter projects |
| Node.js | nodejs.org | React Native / Expo projects |

Every download comes from the official source over HTTPS and is checked against the publisher's checksum before it is used. Each licence is shown for you to accept first.

## What it never changes
Your system `PATH`, `JAVA_HOME`, `ANDROID_HOME`, existing JDKs or SDKs, or Android Studio. The runtime uses its own adb server port (5137), so Android Studio and your own `adb` (port 5037) keep working. It also does not modify your project's files.

## Not yet supported
macOS and Linux hosts, iOS, physical Android devices, more than one device at a time.
