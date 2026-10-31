#!/usr/bin/env python3
"""apply_advice_par.py <workspace> <binary> [cap_w cap_h] [workers]: apply Synth's placement advice in released steps, testing single moves in parallel.

Like apply_advice.py, but when a batch fails its moves are tried one each in parallel workers (each with its own copy of
the workspace and its own Synth server), all from the same base layout. The moves that pass alone are then applied
together and confirmed with one export; if the combination fails they are added one at a time in order of arrival.
"""
import concurrent.futures as cf, json, math, os, re, shutil, subprocess, sys, threading, time

ws, binary = sys.argv[1], sys.argv[2]
CAP_W, CAP_H = (float(sys.argv[3]), float(sys.argv[4])) if len(sys.argv) > 4 else (75.1, 65.4)
WORKERS = int(sys.argv[5]) if len(sys.argv) > 5 else 5
BATCH, ROUNDS, GAP, RIM = 4, 6, 0.6, 1.35
BASE_ENV = dict(os.environ)
for line in open(os.path.expanduser("~/env.sh")):
    if line.startswith("export "):
        k, _, v = line[7:].strip().partition("="); BASE_ENV[k] = os.path.expandvars(v.strip('"'))
BASE_ENV["SYNTH_ROUTER_TIMEOUT_SECS"] = "900"
synth = [f for f in os.listdir(ws) if f.endswith(".synth")][0]; name = synth[:-6]
lock = threading.Lock()
def log(*a):
    with lock: print(time.strftime("%H:%M:%S"), *a, flush=True)
def nums(s): return [float(v) for v in re.findall(r'-?\d+\.?\d*', s)]
def find(o, k):
    if isinstance(o, dict):
        for kk, v in o.items():
            if kk == k: yield v
            yield from find(v, k)
    elif isinstance(o, list):
        for v in o: yield from find(v, k)

class Server:
    """One Synth MCP server working in its own directory."""
    def __init__(self, d):
        self.d, self.n = d, 0
        env = dict(BASE_ENV); env["SYNTH_USER_REGISTRY_DIR"] = d + "/user-registry"
        self.p = subprocess.Popen([binary, "mcp", "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(d + "/server.err", "w"), env=env, text=True, cwd=d)
        self.call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "advice", "version": "0"}})
    def call(self, m, pr):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": m, "params": pr}) + "\n"); self.p.stdin.flush()
        while True:
            line = self.p.stdout.readline()
            if not line: raise SystemExit("server closed")
            msg = json.loads(line)
            if msg.get("id") == self.n: return msg
    def tool(self, nm, args):
        r = self.call("tools/call", {"name": nm, "arguments": args})
        return {"_error": r["error"]} if "error" in r else json.loads(r["result"]["content"][0]["text"])

def read_pcb(path):
    t = open(path).read(); ex, ey = [], []
    for m in re.finditer(r'\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid', t, re.S):
        if 'Edge.Cuts' in m.group(4):
            a, b = nums(m.group(2)), nums(m.group(3)); ex += [a[0], b[0]]; ey += [a[1], b[1]]
    out = (min(ex), min(ey), max(ex), max(ey)); parts = {}
    for blk in re.split(r'\n\t\(footprint\n', t)[1:]:
        ref = re.search(r'"Reference"\s+"([^"]+)"', blk).group(1)
        at = re.search(r'\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)', blk)
        cx, cy, rot = float(at.group(1)), float(at.group(2)), float(at.group(3) or 0); r = math.radians(rot); xs, ys = [], []
        for g in re.finditer(r'\((fp_line|fp_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid', blk, re.S):
            if 'F.CrtYd' in g.group(4):
                for q in (nums(g.group(2)), nums(g.group(3))):
                    xs.append(cx + q[0]*math.cos(r) + q[1]*math.sin(r)); ys.append(cy - q[0]*math.sin(r) + q[1]*math.cos(r))
        parts[ref] = (min(xs), min(ys), max(xs), max(ys)) if xs else (cx, cy, cx, cy)
    return out, parts

def edge_gaps(out, parts):
    res = {}
    for r, b in parts.items():
        if r.startswith("J"):
            res[r] = min(b[0]-out[0], b[1]-out[1], out[2]-b[2], out[3]-b[3])
    return res

def set_pos(text, ref, x, y, rot=None):
    pat = re.compile(r'(\[components\.%s\]\n)(.*?)(?=\n\[|\Z)' % re.escape(ref), re.S)
    m = pat.search(text)
    if not m:
        return text.rstrip("\n") + f'\n\n[components.{ref}]\nx = {x:.2f}\ny = {y:.2f}\nrotation = {int(rot or 0)}\nsource = "agent"\npriority = "hard"\ndx = 0.0\ndy = 0.0\n'
    body = m.group(2)
    body = re.sub(r'^x = .*$', f'x = {x:.2f}', body, flags=re.M); body = re.sub(r'^y = .*$', f'y = {y:.2f}', body, flags=re.M)
    if rot is not None: body = re.sub(r'^rotation = .*$', f'rotation = {int(rot)}', body, flags=re.M)
    return text[:m.start(2)] + body + text[m.end(2):]

def overlaps(r, parts, skip):
    return any(o != skip and not (r[2]+GAP <= ob[0] or r[0]-GAP >= ob[2] or r[3]+GAP <= ob[1] or r[1]-GAP >= ob[3]) for o, ob in parts.items())

def free_spot(ref, anchor, parts, out):
    if ref not in parts or anchor not in parts: return None
    b = parts[ref]; w, h = b[2]-b[0], b[3]-b[1]; a = parts[anchor]; acx, acy = (a[0]+a[2])/2, (a[1]+a[3])/2
    cands = []
    for dx in [k*0.5 for k in range(-40, 41)]:
        for dy in [k*0.5 for k in range(-40, 41)]:
            cx, cy = acx+dx, acy+dy; r = (cx-w/2, cy-h/2, cx+w/2, cy+h/2)
            if r[0] < out[0]+RIM or r[1] < out[1]+RIM or r[2] > out[2]-RIM or r[3] > out[3]-RIM: continue
            if max(a[0]-r[2], r[0]-a[2], a[1]-r[3], r[1]-a[3], -9) > 3.0: continue
            if not overlaps(r, parts, ref): cands.append((math.hypot(cx-acx, cy-acy), cx, cy))
    return min(cands)[1:] if cands else None

def moves_from(acts, parts, out, skip):
    pri = {"move_off_edge_rim": 0, "match_repeated_circuit": 1, "align_like_parts": 2, "place_support_part_near": 3, "place_test_point_near": 3, "place_beside_neighbour": 4}
    chosen = {}
    for a in sorted((a for a in acts if a["action"] in pri), key=lambda a: pri[a["action"]]):
        ref = a["refdes"]
        if ref in chosen or (ref, a["action"]) in skip: continue
        if a.get("suggested_x_mm") is not None:
            cx, cy = a["suggested_x_mm"], a["suggested_y_mm"]
            if ref in parts:
                b = parts[ref]; w, h = b[2]-b[0], b[3]-b[1]
                if overlaps((cx-w/2, cy-h/2, cx+w/2, cy+h/2), parts, ref): continue
                parts[ref] = (cx-w/2, cy-h/2, cx+w/2, cy+h/2)
            chosen[ref] = (a["action"], cx, cy, a.get("suggested_rotation_deg"))
        else:
            spot = free_spot(ref, a.get("relative_to"), parts, out)
            if spot:
                chosen[ref] = (a["action"], round(spot[0], 2), round(spot[1], 2), None)
                b = parts[ref]; w, h = b[2]-b[0], b[3]-b[1]; parts[ref] = (spot[0]-w/2, spot[1]-h/2, spot[0]+w/2, spot[1]+h/2)
    return chosen

def prepare(dst):
    os.makedirs(dst, exist_ok=True)
    shutil.copy(f"{ws}/{synth}", dst)
    shutil.rmtree(dst + "/user-registry", ignore_errors=True); shutil.copytree(ws + "/user-registry", dst + "/user-registry")

def evaluate(server, sidecar_text, moves, tag):
    """Export the layout with `moves` applied to `sidecar_text` inside the server's directory; return (ok, why, pcb)."""
    d = server.d; text = sidecar_text
    for ref, (act, x, y, rot) in moves.items(): text = set_pos(text, ref, x, y, rot)
    sc = f"{d}/{name}.placement.layout.toml"; open(sc, "w").write(text)
    od = f"{d}/{tag}"; shutil.rmtree(od, ignore_errors=True)
    r = server.tool("synth_export", {"file_path": f"{d}/{synth}", "layout_file_path": sc, "out_dir": od, "registry_path": "/home/synth/synth/registry/parts", "allow_incomplete": False})
    if "_error" in r: return False, f"error {str(r['_error'])[:120]}", None
    rr, rc, bl = list(find(r, "release_ready")), list(find(r, "route_complete")), list(find(r, "blocking_reasons"))
    if not (rr and rr[0] and rc and rc[0]) or any(bl): return False, f"release={rr[:1]} blocking={str(bl)[:140]}", None
    out, parts = read_pcb(f"{od}/{name}.kicad_pcb"); w, h = out[2]-out[0], out[3]-out[1]; g = edge_gaps(out, parts)
    if w > CAP_W+0.05 or h > CAP_H+0.05: return False, f"outline {w:.2f}x{h:.2f} over cap", None
    if any(v > 0.35 for v in g.values()): return False, "connector gap " + str({k: round(v, 2) for k, v in g.items() if v > 0.35}), None
    return True, f"{w:.2f}x{h:.2f}", f"{od}/{name}.kicad_pcb"

main = Server(ws)
pool_dirs = [f"{ws}/_w{i}" for i in range(WORKERS)]
for d in pool_dirs: prepare(d)
servers = [Server(d) for d in pool_dirs]
free = list(range(WORKERS)); free_lock = threading.Lock()
def with_server(fn):
    while True:
        with free_lock:
            if free: i = free.pop(); break
        time.sleep(0.5)
    try: return fn(servers[i])
    finally:
        with free_lock: free.append(i)

side = f"{ws}/{name}.placement.layout.toml"
count = [0]
def tag():
    with lock: count[0] += 1; return f"adv_{count[0]:02d}"
def desc(moves): return ", ".join(f"{r}->({x:.1f},{y:.1f}) {a[:6]}" for r, (a, x, y, _) in moves.items())

def apply_to_side(moves):
    text = open(side).read()
    for ref, (act, x, y, rot) in moves.items(): text = set_pos(text, ref, x, y, rot)
    open(side, "w").write(text)

def run_batch(moves):
    base = open(side).read(); t = tag()
    ok, why, pcb = with_server(lambda s: evaluate(s, base, moves, t))
    log(t, "OK" if ok else "FAIL", why, "|", desc(moves))
    if ok: apply_to_side(moves)
    return ok, pcb

def run_singles(items):
    """Try each move alone in parallel from the current base, then combine the passes."""
    base = open(side).read()
    def one(item):
        ref, mv = item; t = tag()
        ok, why, pcb = with_server(lambda s: evaluate(s, base, {ref: mv}, t))
        log(t, "OK" if ok else "FAIL", why, "|", desc({ref: mv})); return ref, mv, ok
    with cf.ThreadPoolExecutor(WORKERS) as ex: res = list(ex.map(one, items))
    return [(r, m) for r, m, ok in res if ok], [(r, m) for r, m, ok in res if not ok]

cur = f"{ws}/export/{name}.kicad_pcb"
skip, kept = set(), 0
for rnd in range(ROUNDS):
    out, parts = read_pcb(cur)
    d = main.tool("synth_place_with_hints", {"file_path": f"{ws}/{synth}", "layout_file_path": side, "hints": [], "run_routing": False})
    acts = [a for v in find(d, "recommended_actions") for a in v]
    log(f"round {rnd}: {len(acts)} advice actions", sorted({a['action'] for a in acts}))
    moves = moves_from(acts, parts, out, skip)
    if not moves: log("no more usable advice"); break
    items = list(moves.items()); progressed = False
    for i in range(0, len(items), BATCH):
        batch = dict(items[i:i+BATCH]); ok, pcb = run_batch(batch)
        if ok: cur, kept, progressed = pcb, kept + len(batch), True; continue
        if len(batch) == 1:
            ref = next(iter(batch)); skip.add((ref, batch[ref][0])); continue
        passed, failed = run_singles(list(batch.items()))
        for ref, mv in failed: skip.add((ref, mv[0]))
        if not passed: continue
        combo = dict(passed); ok, pcb = run_batch(combo)
        if ok: cur, kept, progressed = pcb, kept + len(combo), True
        else:
            for ref, mv in passed:     # add them one at a time
                ok, pcb = run_batch({ref: mv})
                if ok: cur, kept, progressed = pcb, kept + 1, True
                else: skip.add((ref, mv[0]))
    if not progressed: log("a whole round failed; stopping"); break
shutil.copy(cur, f"{ws}/advice_final.kicad_pcb"); log("DONE kept", kept, "moves; final", cur)
