#!/usr/bin/env python3
"""describe_pl.py <workspace> <binary>: print the decoupling lines of synth_describe_placement for the workspace's source and sidecar."""
import json, os, subprocess, sys
ws, binary = sys.argv[1:3]
env = dict(os.environ)
for line in open(os.path.expanduser("~/env.sh")):
    if line.startswith("export "):
        k, _, v = line[7:].strip().partition("="); env[k] = os.path.expandvars(v.strip('"'))
env["SYNTH_USER_REGISTRY_DIR"] = os.path.join(ws, "user-registry")
p = subprocess.Popen([binary, "mcp", "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True, cwd=ws)
n = 0
def call(m, pr):
    global n; n += 1
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": m, "params": pr}) + "\n"); p.stdin.flush()
    while True:
        msg = json.loads(p.stdout.readline())
        if msg.get("id") == n: return msg
call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "d", "version": "0"}})
syn = [f for f in os.listdir(ws) if f.endswith(".synth")][0]; side = [f for f in os.listdir(ws) if f.endswith(".placement.layout.toml")][0]
r = call("tools/call", {"name": "synth_describe_placement", "arguments": {"file_path": os.path.join(ws, syn), "layout_file_path": os.path.join(ws, side), "registry_path": os.path.join(os.path.dirname(binary), "..", "..", "registry", "parts")}})
t = r["result"]["content"][0]["text"]
for l in t.splitlines():
    if any(w in l.lower() for w in ("decoup", "capacitor", "from its", "mm from")): print(l[:300])
print("---", len(t), "chars")
p.terminate()
