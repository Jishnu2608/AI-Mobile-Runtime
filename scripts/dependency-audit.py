"""Dependency audit (REL-003): queries OSV (osv.dev) for every crate in Cargo.lock and every npm
package in the desktop UI lockfile. Sends only public package names and versions.
Usage: python -I scripts/dependency-audit.py <repository root>"""
import json, os, re, subprocess, sys

root = sys.argv[1] if len(sys.argv) > 1 else "."
lock = open(os.path.join(root, "Cargo.lock"), encoding="utf-8").read()
pkgs = []
for m in re.finditer(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\n(source = "([^"]+)")?', lock):
    if m.group(4):  # registry crates only (workspace members have no source)
        pkgs.append(("crates.io", m.group(1), m.group(2)))
npm = json.load(open(os.path.join(root, "apps", "desktop", "ui", "package-lock.json"), encoding="utf-8"))
for path, info in npm.get("packages", {}).items():
    if path.startswith("node_modules/") and "version" in info:
        pkgs.append(("npm", path.split("node_modules/")[-1], info["version"]))

def post(url, body):
    r = subprocess.run(["curl", "-sS", "--max-time", "60", "-H", "Content-Type: application/json", "-d", "@-", url],
                       input=json.dumps(body), capture_output=True, text=True)
    return json.loads(r.stdout)

hits = []
for i in range(0, len(pkgs), 500):
    chunk = pkgs[i:i + 500]
    res = post("https://api.osv.dev/v1/querybatch",
               {"queries": [{"package": {"ecosystem": e, "name": n}, "version": v} for e, n, v in chunk]})
    for (e, n, v), r in zip(chunk, res.get("results", [])):
        for vuln in r.get("vulns", []) or []:
            hits.append((e, n, v, vuln["id"]))
print(f"{len(pkgs)} packages checked ({sum(1 for p in pkgs if p[0]=='crates.io')} crates, {sum(1 for p in pkgs if p[0]=='npm')} npm)")
for e, n, v, vid in hits:
    d = json.loads(subprocess.run(["curl", "-sS", "--max-time", "30", f"https://api.osv.dev/v1/vulns/{vid}"], capture_output=True, text=True).stdout)
    sev = (d.get("database_specific") or {}).get("severity") or ",".join(s.get("score", "")[:44] for s in d.get("severity", [])) or "n/a"
    fixed = sorted({ev["fixed"] for a in d.get("affected", []) for rg in a.get("ranges", []) for ev in rg.get("events", []) if "fixed" in ev})
    kind = (d.get("database_specific") or {}).get("informational") or ""
    print(f"{e} {n} {v}: {vid} [{sev}] {kind} {d.get('summary','')[:110]} | fixed: {fixed}")
print(f"{len(hits)} advisories")
