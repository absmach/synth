#!/usr/bin/env python3
"""mcp_export.py <workspace> <binary> <out_dir> <trace.json>

Exports the workspace's .synth and placement sidecar through the Synth MCP server (`synth mcp --stdio`), the
`synth_export` tool, the call an agent makes, and records the session: the server's name and version, the tools it
lists, the request, the response and the SHA-256 of every file written. Paths in the trace are relative to the
workspace. The workspace holds the .synth file, its placement sidecar and user-registry/."""
import hashlib, json, os, subprocess, sys, time

ws, binary, out_dir, trace_path = (os.path.abspath(a) if i < 3 else a for i, a in enumerate(sys.argv[1:5]))
env = dict(os.environ)  # run under `. ~/env.sh` so the router, KiCad and Python paths are set
# the server config an agent run uses names these two explicitly
env.setdefault("KICAD_ROUTING_TOOLS_REPO", "/home/synth/krt")
env.setdefault("SYNTH_KRT_PYTHON", "/home/synth/krtvenv/bin/python")
# the routing record names the revision the binary was built from; the export call cannot know it
env.setdefault("SYNTH_SOURCE_REVISION", "48a0a490b2c3dde076a90e55ba05b82ce4137605")
env["SYNTH_USER_REGISTRY_DIR"] = os.path.join(ws, "user-registry")
env["SYNTH_ROUTER_TIMEOUT_SECS"] = env.get("SYNTH_ROUTER_TIMEOUT_SECS", "900")
rel = lambda path: os.path.relpath(path, ws)

p = subprocess.Popen([binary, "mcp", "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=open(os.path.join(ws, "mcp-server.err"), "w"), env=env, text=True, cwd=ws)
n = 0
log = []


def call(method, params):
    global n
    n += 1
    request = {"jsonrpc": "2.0", "id": n, "method": method, "params": params}
    p.stdin.write(json.dumps(request) + "\n")
    p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line:
            raise SystemExit("server closed")
        msg = json.loads(line)
        if msg.get("id") == n:
            return request, msg


t0 = time.time()
req, init = call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                "clientInfo": {"name": "mcp_export.py", "version": "1"}})
p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
p.stdin.flush()
_, tools = call("tools/list", {})
names = sorted(t["name"] for t in tools["result"]["tools"])

synth = [f for f in os.listdir(ws) if f.endswith(".synth")][0]
side = [f for f in os.listdir(ws) if f.endswith(".placement.layout.toml")][0]
args = {"file_path": os.path.join(ws, synth), "layout_file_path": os.path.join(ws, side), "out_dir": out_dir}
req, resp = call("tools/call", {"name": "synth_export", "arguments": args})
seconds = round(time.time() - t0)
p.stdin.close()
try:
    p.wait(timeout=30)
except subprocess.TimeoutExpired:
    p.kill()

text = resp.get("result", {}).get("content", [{}])[0].get("text", "")
try:
    body = json.loads(text)
except ValueError:
    body = {"text": text[:2000]}
summary = {k: body[k] for k in body if isinstance(body[k], (str, int, float, bool)) or body[k] is None}
files = {}
for root, _, fs in os.walk(out_dir):
    for f in sorted(fs):
        path = os.path.join(root, f)
        files[rel(path)] = hashlib.sha256(open(path, "rb").read()).hexdigest()[:16]
trace = {
    "server": init.get("result", {}).get("serverInfo"),
    "protocol": init.get("result", {}).get("protocolVersion"),
    "tools_listed": names,
    "request": {"method": "tools/call", "tool": "synth_export",
                "arguments": {"file_path": rel(args["file_path"]), "layout_file_path": rel(args["layout_file_path"]),
                              "out_dir": rel(out_dir)}},
    "is_error": bool(resp.get("result", {}).get("isError")) or "error" in resp,
    "error": resp.get("error"),
    "response_summary": summary,
    "seconds": seconds,
    "output_sha256_prefix16": files,
}
json.dump(trace, open(trace_path, "w"), indent=1, sort_keys=True)
print(json.dumps({"is_error": trace["is_error"], "seconds": seconds, "files": len(files),
                  "summary": {k: summary[k] for k in list(summary)[:8]}}))
