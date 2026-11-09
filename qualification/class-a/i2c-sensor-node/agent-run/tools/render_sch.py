#!/usr/bin/env python3
"""render_sch.py <workspace> <binary> <out_dir>: render the workspace's schematic to PNG with synth_render_schematic (no routing); prints the files written."""
import json, os, subprocess, sys
ws, binary, out = sys.argv[1:4]
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
syn = [f for f in os.listdir(ws) if f.endswith(".synth")][0]
side = [f for f in os.listdir(ws) if f.endswith(".placement.layout.toml")][0]
r = call("tools/call", {"name": "synth_render_schematic", "arguments": {"file_path": os.path.join(ws, syn), "inline": False, "width_px": int(os.environ.get("WIDTH_PX", "1600")), "out_dir": out, "registry_path": os.path.join(os.path.dirname(binary), "..", "..", "registry", "parts")}})
txt = r["result"]["content"][0]["text"] if "result" in r else json.dumps(r)
print(txt[:800])
p.terminate()
