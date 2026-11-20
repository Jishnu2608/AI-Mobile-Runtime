import assert from "node:assert/strict";
import { test } from "node:test";
import { CoreClient } from "../core";

// A stand-in for `tumr rpc --stdio`: answers `echo` with its params, `slow` after 200 ms (so the
// answers arrive out of order), anything else with an error.
const FAKE = `
const rl = require("node:readline").createInterface({ input: process.stdin });
rl.on("line", (l) => {
  const r = JSON.parse(l);
  const send = (m) => process.stdout.write(JSON.stringify(m) + "\\n");
  if (r.method === "echo") send({ id: r.id, result: r.params });
  else if (r.method === "slow") setTimeout(() => send({ id: r.id, result: "late" }), 200);
  else send({ id: r.id, error: "method '" + r.method + "' is not available" });
});`;

test("answers come back to the right caller, out of order", async () => {
  const c = new CoreClient(process.execPath, ["-e", FAKE]);
  try {
    const slow = c.call("slow");
    const fast = c.call("echo", { a: 1 });
    assert.deepEqual(await fast, { a: 1 });
    assert.equal(await slow, "late");
    await assert.rejects(c.call("install.start"), /not available/);
  } finally {
    c.dispose();
  }
});

test("a missing program fails the call instead of hanging", async () => {
  const c = new CoreClient("C:\\no\\such\\tumr.exe");
  await assert.rejects(c.call("echo", {}, 5000), /cannot start|stopped/);
  c.dispose();
});
