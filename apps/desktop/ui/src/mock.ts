// Sample answers for UI development in a plain browser (shapes match the real Core API).
const licenceText =
  "(Sample licence text for UI development. The real app shows the publisher's full licence, fetched from the official source.)";

let accepted = false;
let approvalAnswered = false;
let polls = 0;
let mockAngle = 180;
/** `?fold=book|flip` makes the mock device a foldable. */
function mockFold(): string | null {
  const f = new URLSearchParams(window.location.search).get("fold");
  return f === "book" ? "fold" : f === "flip" ? "flip" : null;
}

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

const sampleProblems = [
  { fingerprint: "a1b2c3d4e5f60718", category: "build", origin: "app", severity: "error", source: "dart", message: "Expected ';' after this.", file: "lib/main.dart", line: 30, count: 2, seq: 1, first_seen_ms: 1, stack_frames: 0 },
  { fingerprint: "b1b2c3d4e5f60718", category: "crash", origin: "app", severity: "error", source: "logcat", message: "java.lang.NumberFormatException: For input string: \"abc\"", file: "Calc.kt", line: 8, count: 3, seq: 2, first_seen_ms: 2, stack_frames: 3 },
  { fingerprint: "c1b2c3d4e5f60718", category: "network", origin: "environment", severity: "error", source: "gradle", message: "Could not GET 'https://dl.google.com/dl/android/maven2/x.pom'. Received status code 502", count: 1, seq: 3, first_seen_ms: 3, stack_frames: 0 },
];

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
      // `?installfail=1` ends the mock install with a failed download (AUD-P2-20)
      const fail = new URLSearchParams(window.location.search).get("installfail") === "1";
      const done = polls > 12;
      const img = 1_500_000_000;
      const failed = fail && polls > 6;
      return {
        id: "mockjob",
        state: failed ? "failed" : done ? "done" : "running",
        message: failed ? "Android system image: download stopped (connection reset)" : undefined,
        log: "D:\\UMR\\logs\\install-mockjob.log",
        items: [
          { component: "android-platform", title: "Android SDK Platform", state: polls > 3 ? "done" : "installing", bytes_done: 0 },
          {
            component: "android-system-image",
            title: "Android system image (Google APIs, x86_64)",
            state: failed ? "failed" : done ? "done" : "downloading",
            bytes_done: Math.min(img, polls * 140_000_000),
            bytes_total: img,
            message: failed ? "connection reset after 840 MB; run setup again to resume" : undefined,
          },
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
    case "project.run":
      await new Promise((r) => setTimeout(r, 1500));
      return { status: "running", package: "com.example.my_app", serial: "emulator-5554", adapter: "flutter", seconds: 41, watching: true, capabilities: { hot_reload: true, hot_restart: true } };
    case "preview.open":
      return { url: "", profile: mockFold() ?? "standard" };
    case "device.posture": {
      // `?fold=book|flip` shows a foldable in a plain browser (AUD-P1-40/44)
      const a = (_params as any)?.angle;
      if (typeof a === "number") mockAngle = a;
      const closed = mockAngle <= 30;
      const flip = mockFold() === "flip";
      const label = closed ? "Closed" : mockAngle >= 180 ? "Open (flat)" : mockAngle > 150 ? `Open ${mockAngle}°` : `Half open ${mockAngle}°`;
      return {
        serial: "emulator-5554", kind: flip ? "flip" : "book", angle: mockAngle, posture: closed ? "closed" : mockAngle > 150 ? "opened" : "half-opened",
        label, steps: [0, 30, 60, 90, 120, 180], device_state: closed ? "CLOSED" : "OPENED",
        cover: closed ? (flip ? { x: 0, y: 756, width: 1080, height: 1124, label: 'cover screen 3.4"' } : { x: 442, y: 0, width: 884, height: 2208, label: 'outer screen 6.2"' }) : null,
      };
    }
    case "device.auth_state": {
      // `?locked=1` shows the lock screen sensor in a plain browser
      const locked = new URLSearchParams(window.location.search).get("locked") === "1";
      return { serial: "emulator-5554", locked, sensor_active: locked, enrolled: 1, lock_disabled: false };
    }
    case "device.fingerprint":
      return { serial: "emulator-5554", id: 1, touch: "tap" };
    case "project.stop":
      return { stopped: { package: "com.example.my_app" } };
    case "device.profiles":
      return {
        profiles: [
          { profile: { id: "standard", class: "phone" } },
          { profile: { id: "fold", class: "Book foldable", fold: { kind: "book" } } },
          { profile: { id: "flip", class: "Flip foldable", fold: { kind: "flip" } } },
        ],
      };
    case "diagnostics.list":
      return {
        entries: (_params as any)?.since ? [] : sampleProblems,
        counts: { build: 1, crash: 1, network: 1 },
        total: sampleProblems.length,
        cursor: 3,
        run: { project: "C:\work\demo" },
      };
    case "diagnostics.get":
      return {
        ...(sampleProblems.find((p) => p.fingerprint === (_params as any)?.fingerprint) ?? sampleProblems[1]),
        stack: ["at java.lang.Integer.parseInt(Integer.java:615)", "at com.example.demo.Calc.parse(Calc.kt:8)", "at com.example.demo.MainScreenKt.MainScreen(MainScreen.kt:94)"],
        code: { first: 92, line: 94, lines: ["  val total = remember { mutableStateOf(0) }", "  Column {", '    Text(text = Calc.parse(input).toString())', "  }", "}"] },
      };
    case "session.status":
      return { session: { avd: `umr_${mockFold() ?? "standard"}_api36`, profile: mockFold() ?? "standard", serial: "emulator-5554", state: "crashed" } };
    case "diagnostics.flag":
      return { flagged: (_params as any)?.fingerprint };
    case "home.summary": {
      const now = Date.now();
      return {
        recent: [
          { path: "C:\\work\\demo", adapter: "flutter", last_run_unix: now / 1000 - 600, runs: 7, exists: true },
          { path: "C:\\work\\shop-app", adapter: "react-native", last_run_unix: now / 1000 - 86400 * 2, runs: 3, exists: true },
          { path: "C:\\work\\old-kotlin", adapter: "gradle-android", last_run_unix: now / 1000 - 86400 * 9, runs: 1, exists: false },
        ],
        session: { state: "running", profile: "standard", app: "com.example.demo", serial: "emulator-5554", project: "C:\\work\\demo" },
        run: { project: "C:\\work\\demo", adapter: "flutter", package: "com.example.demo", serial: "emulator-5554" },
        problems: { total: 2, by_category: { build: 1, crash: 1 } },
        feed: [
          { at_ms: now - 60_000, kind: "fixed", message: "Expected ';' after this.", seconds: 41 },
          { at_ms: now - 101_000, kind: "told", message: "Expected ';' after this." },
          { at_ms: now - 3_600_000, kind: "fixed", message: "Bad state: No element", seconds: 95 },
        ],
        fixed_today: 2,
      };
    }
    case "logs.tail":
      if ((_params as any)?.since) return { lines: [], cursor: 4 };
      return {
        lines: [
          { seq: 1, source: "umr", level: "I", tag: "building", text: "lib/main.dart" },
          { seq: 2, source: "tool", level: "I", text: "Launching lib/main.dart on sdk gphone64 x86 64 in debug mode..." },
          { seq: 3, source: "logcat", level: "W", tag: "flutter", pid: 4321, text: "A RenderFlex overflowed by 42 pixels on the bottom." },
          { seq: 4, source: "logcat", level: "E", tag: "AndroidRuntime", pid: 4321, text: "FATAL EXCEPTION: main" },
        ],
        cursor: 4,
      };
    case "approvals.list":
      return { pending: approvalAnswered ? [] : [{ id: "a1", client: "claude-code", client_id: "c_1", action: "run project", detail: "C:/work/demo", created_ms: 1 }] };
    case "approvals.decide":
      approvalAnswered = true;
      return { id: "a1", decision: "allow" };
    default:
      throw new Error(`mock: no sample for ${method}`);
  }
}

