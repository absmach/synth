#!/usr/bin/env python3
"""compact_drive.py <workspace> <binary> <tool-args-json>: drive synth_compact_layout to done, printing each attempt.

Runs on the droplet. <workspace> holds the .synth file, its sidecar and user-registry/. The args JSON is merged
over file_path, layout_file_path and out_dir.
"""
import json, os, subprocess, sys, time

ws, binary, extra = sys.argv[1], sys.argv[2], json.loads(sys.argv[3] if len(sys.argv) > 3 else "{}")
env = dict(os.environ)
for line in open(os.path.expanduser("~/env.sh")):
    if line.startswith("export "):
        k, _, v = line[7:].strip().partition("=")
        env[k] = os.path.expandvars(v.strip('"'))
env["SYNTH_USER_REGISTRY_DIR"] = os.path.join(ws, "user-registry")
env["SYNTH_ROUTER_TIMEOUT_SECS"] = env.get("SYNTH_ROUTER_TIMEOUT_SECS", "900")
p = subprocess.Popen([binary, "mcp", "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(os.path.join(ws, "drive.err"), "w"), env=env, text=True, cwd=ws)
n = 0
def call(method, params):
    global n
    n += 1
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params}) + "\n"); p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line: raise SystemExit("server closed")
        msg = json.loads(line)
        if msg.get("id") == n: return msg
call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "drive", "version": "0"}})
synth = [f for f in os.listdir(ws) if f.endswith(".synth")][0]
side = [f for f in os.listdir(ws) if f.endswith(".placement.layout.toml")][0]
args = {"file_path": os.path.join(ws, synth), "layout_file_path": os.path.join(ws, side), "out_dir": os.path.join(ws, "compact")}
args.update(extra)
t0 = time.time()
while True:
    r = call("tools/call", {"name": "synth_compact_layout", "arguments": args})
    if "error" in r:
        print("ERROR", r["error"]); break
    out = json.loads(r["result"]["content"][0]["text"])
    a = out.get("attempts", [])[-1] if out.get("attempts") else {}
    print(f'{time.time()-t0:6.0f}s {a.get("permille")} passed={a.get("passed")} {a.get("width_mm")}x{a.get("height_mm")} fill={round(a.get("fill_percent",0),1)} {a.get("reason","")[:90]}', flush=True)
    if out.get("done"): print("DONE", out.get("status"), {k: out[k] for k in out if k not in ("attempts",)}); break
p.stdin.close()
