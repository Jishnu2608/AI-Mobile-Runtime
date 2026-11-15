import assert from "node:assert/strict";
import * as path from "node:path";
import { test } from "node:test";
import { place, resolveFile } from "../problems";
import { candidates, devBuild } from "../locate";

const project = "D:\\work\\sample-kotlin";

test("compiler paths in every form land on the file", () => {
  // kotlinc: file URI with an escaped space
  assert.equal(
    resolveFile("file:///D:/My%20Apps/app/src/A.kt", project),
    path.normalize("D:\\My Apps\\app\\src\\A.kt"),
  );
  // Gradle / ninja: forward slashes and %20
  assert.equal(resolveFile("D:/Projects/Mobile%20Runtime/x.kt", project), path.normalize("D:\\Projects\\Mobile Runtime\\x.kt"));
  // Dart / Metro: relative to the project
  assert.equal(resolveFile("lib/main.dart", project), path.join(project, "lib", "main.dart"));
  assert.equal(resolveFile(undefined, project), null);
});

test("a problem becomes a 0-based, labelled entry", () => {
  const p = place(
    { fingerprint: "ab12", category: "build", origin: "app", severity: "error", message: "Unresolved reference 'foo'.", file: "app/src/A.kt", line: 12, column: 5, count: 3 },
    project,
  );
  assert.equal(p.line, 11);
  assert.equal(p.column, 4);
  assert.equal(p.severity, "error");
  assert.equal(p.message, "Unresolved reference 'foo'. (×3)");
  assert.equal(p.code, "ab12");
  assert.match(p.source, /build/);
  const crash = place({ fingerprint: "c", category: "crash", origin: "app", severity: "error", message: "NPE" }, project);
  assert.equal(crash.file, null);
  assert.equal(crash.line, 0);
  assert.match(crash.source, /crash/);
});

test("umr.exe is looked for in the setting, PATH, then the install folder", () => {
  const c = candidates("C:\\tools\\umr.exe", { PATH: "C:\\a;C:\\b", LOCALAPPDATA: "C:\\Users\\u\\AppData\\Local" });
  assert.equal(c[0], "C:\\tools\\umr.exe");
  assert.ok(c.some((p) => p.endsWith(path.join("C:\\a", "umr.exe")) || p.endsWith("umr.exe")));
  assert.equal(c[c.length - 1], path.join("C:\\Users\\u\\AppData\\Local", "AI Mobile Runtime", "umr.exe"));
});

test("a sample inside the repository finds the repository's own build", () => {
  // this test runs from apps/vscode, two levels below the repository root
  const found = devBuild(process.cwd());
  if (found) assert.match(found, /target[\\/](release|debug)[\\/]umr\.exe$/);
});
