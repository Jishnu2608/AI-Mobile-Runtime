# Clean-VM environment matrix (TEST-001)

Phase 1 exit criterion: provisioning experiments **1, 3, 6, 7, 8, 10** pass on clean Windows 11
VMs, with **no change** to the user's or the system's `PATH`, `JAVA_HOME`, `ANDROID_HOME`
(registry diff), and no files left in the known leak locations.

> The planning document references these experiment numbers without listing them. The table
> below is the working definition derived from the spec's acceptance criteria — **to be
> confirmed by the owner** (todo TEST-001).

| # | Scenario (VM starting state) | Setup | Must hold |
|---|---|---|---|
| 1 | Clean Windows 11, nothing installed | none | `umr install <native sample>` reaches all-reuse on re-plan; ≤ 5 decisions (licence, confirm, UAC if WHPX off, restart) |
| 3 | Existing partial user SDK + Android Studio JBR | install Android Studio (or copy an SDK with only `platform-tools`) | user SDK reused only when complete; otherwise managed SDK; user SDK untouched (file hashes) |
| 6 | Conflicting JDKs: JDK 8 first on PATH, stale `JAVA_HOME` | `setup.ps1 -Experiment 6` | `umr doctor` lists the conflicts; installs/builds still use the chosen JDK; user env unchanged |
| 7 | Standard (non-admin) user, WHPX off | create a standard user; `Disable-WindowsOptionalFeature HypervisorPlatform` | plan lists "Turn on Windows Hypervisor Platform" (required, restart); `umr admin enable-whpx` shows UAC asking for admin credentials; declining = nothing changed |
| 8 | HTTPS proxy | `setup.ps1 -Experiment 8 -Proxy http://host:3128` | metadata and downloads go through the proxy; checksums still verified |
| 10 | Interrupted install | start `umr install`, cut the network or reboot mid-download | next start recovers (no staging, no half components); `umr install` resumes downloads and finishes |

## Running one experiment

1. Create a Windows 11 VM (Hyper-V "Quick Create" or any hypervisor with nested virtualization
   enabled so the emulator can use WHPX). Take a snapshot **before** anything else.
2. Copy `target\release\{umr,umr-core,umr-elevate}.exe` and this folder into the VM.
3. In the VM:

   ```powershell
   .\collect.ps1 -Out before.json            # registry/env + leak-location snapshot
   .\setup.ps1 -Experiment 6                 # only for experiments that need a setup
   .\umr.exe doctor
   .\umr.exe install C:\samples\android-native
   .\collect.ps1 -Out after.json
   .\compare.ps1 before.json after.json      # fails on any global change or leak
   ```
4. Record the result in `results.md` (date, VM image, experiment, pass/fail, notes). Revert the
   snapshot before the next experiment.

The sample project in `samples\android-native` is the minimal native project the resolver
reads (no build is run in Phase 1).
