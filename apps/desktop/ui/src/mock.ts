// Sample answers for UI development in a plain browser (shapes match the real Core API).
const licenceText =
  "Terms and Conditions\n\nThis is the Android Software Development Kit License Agreement\n\n1. Introduction\n\n1.1 The Android Software Development Kit (referred to in the License Agreement as the \"SDK\") is licensed to you subject to the terms of the License Agreement. (Sample text: the real app shows Google's full text.)";

let accepted = false;
let polls = 0;

const plan = {
  profile: "android-native",
  items: [
    { component: "jdk", title: "Eclipse Temurin JDK", version: "17.0.21", action: { kind: "reuse", path: "D:\\UMR\\jdks\\temurin-17.0.21_9" }, approx_download_bytes: 0, reason: "AGP 9.0.1 needs JDK 17+" },
    { component: "android-platform", title: "Android SDK Platform", package: "platforms/android-36", version: "36", action: { kind: "install" }, approx_download_bytes: 124780544, reason: "compileSdk = 36" },
    { component: "android-system-image", title: "Android system image (Google APIs, x86_64)", package: "system-images/android-36/google_apis/x86_64", version: "36", action: { kind: "install" }, approx_download_bytes: 4585422848, reason: "emulator image for API 36" },
  ],
  licences: [{ id: "android-sdk", title: "Android Software Development Kit License Agreement", url: "https://developer.android.com/studio/terms", requires_acceptance: true }],
  admin_actions: [],
  conflicts: [],
  notes: [],
  total_download_bytes: 4710203392,
  total_disk_bytes: 4710203392,
  disk_free_bytes: 290000000000,
  disk_ok: true,
};

export async function mockCall(method: string, _params: unknown): Promise<any> {
  await new Promise((r) => setTimeout(r, 250));
  switch (method) {
    case "env.scan":
      return {
        host: { windows_build: 26300, windows_11: true, ram_total_bytes: 17179869184, disk_free_bytes: 290000000000, long_paths_enabled: true, virtualization: { whpx: "usable", acceleration_available: true } },
        jdks: [{ home: "D:\\UMR\\jdks\\temurin-17.0.21_9", major: 17, is_jdk: true, health: { state: "ok" } }],
        android_sdks: [{ root: "D:\\UMR\\android-sdk", packages: [1, 2, 3, 4, 5, 6] }],
        tools: [{ name: "node", version: "24.15.0" }, { name: "git", version: "2.52.0" }],
        conflicts: [],
        readiness: [
          { profile: "android-native", ready: true, missing: [] },
          { profile: "flutter-android", ready: false, missing: ["Flutter SDK"] },
          { profile: "react-native-android", ready: true, missing: [] },
          { profile: "expo-go", ready: true, missing: [] },
        ],
      };
    case "project.detect":
      return {
        detection: {
          root: "D:\\Work\\my-app",
          framework: "android-native",
          confidence: 90,
          evidence: [
            { file: "settings.gradle(.kts)", says: "Gradle build" },
            { file: "build scripts / version catalog", says: "uses the Android Gradle Plugin" },
            { file: "app/src/main/AndroidManifest.xml", says: "Android app module" },
          ],
          candidates: [],
        },
        trusted: false,
      };
    case "project.trust":
      return { path: "D:\\Work\\my-app", trusted: true };
    case "install.prepare":
      return {
        plan,
        prepared: {
          steps: plan.items.filter((i) => i.action.kind === "install").map((i) => ({ ...i, licences: ["android-sdk-license"], kind: { type: "sdk-package", package: i.package } })),
          licences: [{ id: "android-sdk-license", title: "Android Software Development Kit License Agreement", url: "https://developer.android.com/studio/terms", text: licenceText, sha256: "ab", accepted }],
          android_sdk: "D:\\UMR\\android-sdk",
        },
      };
    case "install.licence.accept":
      accepted = true;
      return { accepted: true };
    case "install.start":
      polls = 0;
      return { job_id: "mockjob", steps: 2 };
    case "install.status": {
      polls++;
      const done = polls > 8;
      return {
        id: "mockjob",
        state: done ? "done" : "running",
        log: "D:\\UMR\\logs\\install-mockjob.log",
        items: [
          { component: "android-platform", title: "Android SDK Platform", state: polls > 3 ? "done" : "installing", bytes_done: 0 },
          { component: "android-system-image", title: "Android system image (Google APIs, x86_64)", state: done ? "done" : polls > 3 ? "installing" : "pending", bytes_done: 0 },
        ],
      };
    }
    case "install.cancel":
      return { cancelling: true };
    case "components.list":
      return {
        components: [
          { id: "jdk/temurin@17.0.21+9", version: "17.0.21+9", ok: true, size_bytes: 317718528 },
          { id: "android/platforms/android-36", version: "2", ok: true, size_bytes: 105906176 },
          { id: "android/system-images/android-36/google_apis/x86_64", version: "7", ok: true, size_bytes: 4585422848 },
        ],
        disk: { total_bytes: 6764573491 },
      };
    case "admin.run":
      return { action: "enable-whpx", status: "already-on", message: "Windows Hypervisor Platform is on and usable" };
    default:
      throw new Error(`mock: no sample for ${method}`);
  }
}
