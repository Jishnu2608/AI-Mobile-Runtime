// The extension's line to the Runtime Core: one `tumr rpc --stdio` process, newline-delimited
// JSON both ways, requests answered out of order (each carries its id). No `vscode` import, so it
// can be unit-tested with any program that speaks the same lines.
import { ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { createInterface } from "node:readline";

interface Waiting {
  resolve: (v: any) => void;
  reject: (e: Error) => void;
  timer: NodeJS.Timeout;
}

export class CoreClient {
  private proc: ChildProcessWithoutNullStreams | null = null;
  private waiting = new Map<number, Waiting>();
  private nextId = 1;

  /** `command` + `args` start the bridge (normally `tumr.exe rpc --stdio`). */
  constructor(
    private readonly command: string,
    private readonly args: string[] = ["rpc", "--stdio"],
  ) {}

  private ensure(): ChildProcessWithoutNullStreams {
    if (this.proc && this.proc.exitCode === null && !this.proc.killed) return this.proc;
    const p = spawn(this.command, this.args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
    p.stdout.setEncoding("utf8");
    createInterface({ input: p.stdout }).on("line", (line) => this.onLine(line));
    p.on("exit", () => this.failAll("the tumr bridge stopped"));
    p.on("error", (e) => this.failAll(`cannot start ${this.command}: ${e.message}`));
    this.proc = p;
    return p;
  }

  private onLine(line: string): void {
    let m: { id?: number; result?: unknown; error?: string };
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    if (typeof m.id !== "number") return;
    const w = this.waiting.get(m.id);
    if (!w) return;
    this.waiting.delete(m.id);
    clearTimeout(w.timer);
    if (m.error !== undefined) w.reject(new Error(m.error));
    else w.resolve(m.result);
  }

  private failAll(why: string): void {
    for (const [, w] of this.waiting) {
      clearTimeout(w.timer);
      w.reject(new Error(why));
    }
    this.waiting.clear();
    this.proc = null;
  }

  /** One Core API call; `timeoutMs` covers slow ones (a rebuild takes a few minutes at most). */
  call<T = any>(method: string, params: unknown = {}, timeoutMs = 600_000): Promise<T> {
    const p = this.ensure();
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiting.delete(id);
        reject(new Error(`${method}: no answer within ${Math.round(timeoutMs / 1000)} s`));
      }, timeoutMs);
      timer.unref();
      this.waiting.set(id, { resolve, reject, timer });
      p.stdin.write(JSON.stringify({ id, method, params }) + "\n");
    });
  }

  dispose(): void {
    const p = this.proc;
    this.failAll("closed");
    if (p) {
      p.stdin.destroy();
      p.stdout.destroy();
      p.stderr.destroy();
      p.kill();
    }
  }
}
