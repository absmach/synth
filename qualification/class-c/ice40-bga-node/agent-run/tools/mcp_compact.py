#!/usr/bin/env python3
"""mcp_compact.py <workspace> <binary> <out_dir> <mode> [key=value ...]: one synth_compact_layout call through the Synth MCP server."""
import json, os, subprocess, sys, time
ws, binary, out_dir, mode = [os.path.abspath(a) if i < 3 else a for i, a in enumerate(sys.argv[1:5])]
extra = dict(kv.split("=",1) for kv in sys.argv[5:])
env = dict(os.environ)
env["SYNTH_USER_REGISTRY_DIR"] = os.path.join(ws, "user-registry")
env["SYNTH_ROUTER_TIMEOUT_SECS"] = env.get("SYNTH_ROUTER_TIMEOUT_SECS", "1800")
p = subprocess.Popen([binary, "mcp", "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=open(os.path.join(ws, "mcp-compact.err"), "w"), env=env, text=True, cwd=ws)
n = 0
def call(method, params):
    global n; n += 1
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params}) + "\n"); p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line: raise SystemExit("server closed")
        msg = json.loads(line)
        if msg.get("id") == n: return msg
call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "mcp_compact.py", "version": "1"}})
p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"); p.stdin.flush()
synth = [f for f in os.listdir(ws) if f.endswith(".synth")][0]
side = [f for f in os.listdir(ws) if f.endswith(".placement.layout.toml")][0]
args = {"file_path": os.path.join(ws, synth), "layout_file_path": os.path.join(ws, side), "out_dir": out_dir, "mode": mode}
for k, v in extra.items():
    args[k] = float(v) if v.replace('.','',1).isdigit() else v
resp = call("tools/call", {"name": "synth_compact_layout", "arguments": args})
p.stdin.close()
try: p.wait(timeout=30)
except subprocess.TimeoutExpired: p.kill()
print(json.dumps(resp.get("result", resp))[:3000])
