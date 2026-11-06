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

WORKERS = int(sys.argv[5]) if len(sys.argv) > 5 else 6
ROUNDS_MAX = int(sys.argv[6]) if len(sys.argv) > 6 else 4

def flat(o):
    for v in o:
        if isinstance(v, list): yield from flat(v)
        else: yield str(v)

def trial(server, sidecar_text, moves, tag):
    """Export with `moves` applied; return (score, blocks, pcb). Lower score is better; 0 means released within the caps."""
    d = server.d; text = sidecar_text
    for ref, (x, y) in moves.items(): text = set_pos(text, ref, x, y, None)
    sc = f"{d}/{name}.placement.layout.toml"; open(sc, "w").write(text)
    od = f"{d}/{tag}"; shutil.rmtree(od, ignore_errors=True)
    r = server.tool("synth_export", {"file_path": f"{d}/{synth}", "layout_file_path": sc, "out_dir": od, "registry_path": "/home/synth/synth/registry/parts", "allow_incomplete": False})
    if "_error" in r: return 999, [f"error {str(r['_error'])[:100]}"], None
    rr, rc = list(find(r, "release_ready")), list(find(r, "route_complete"))
    blocks = [b for bl in find(r, "blocking_reasons") for b in flat(bl)]
    pcb = f"{od}/{name}.kicad_pcb"
    score = len(blocks) + (0 if (rr and rr[0] and rc and rc[0]) else 1)
    if os.path.exists(pcb):
        out, parts = read_pcb(pcb); w, h = out[2]-out[0], out[3]-out[1]
        if w > CAP_W+0.05 or h > CAP_H+0.05: score += 50; blocks.append(f"outline {w:.2f}x{h:.2f} over cap")
        g = edge_gaps(out, parts)
        if any(v > 0.35 for v in g.values()): score += 50; blocks.append("connector gap")
    return score, blocks, pcb

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
    with lock: count[0] += 1; return f"rs_{count[0]:03d}"

base_text = open(side).read()
t0 = tag(); base_score, base_blocks, base_pcb = with_server(lambda s: trial(s, base_text, {}, t0))
log("base", base_score, [b[:90] for b in base_blocks])
PENALTY = 50 if any('connector gap' in b for b in base_blocks) else 0
for rnd in range(ROUNDS_MAX):
    if base_score <= PENALTY: break
    pts = [(float(a), float(b)) for blk in base_blocks for a, b in re.findall(r'\(([\d.-]+), ([\d.-]+)\) mm', blk)]
    if not pts or not base_pcb: log("no coordinates to work on"); break
    out, parts = read_pcb(base_pcb)
    cands = {}
    for ref, b in parts.items():
        if ref.startswith(("H", "MH")): continue
        axis = None
        if ref.startswith("J"):
            gl, gt, gr, gb = b[0]-out[0], b[1]-out[1], out[2]-b[2], out[3]-b[3]
            if min(gl, gr) <= 0.35: axis = "y"      # flush to a left/right edge: slide along y only
            elif min(gt, gb) <= 0.35: axis = "x"    # flush to a top/bottom edge: slide along x only
            else: continue                          # an inboard pin header is left alone
        cx, cy = (b[0]+b[2])/2, (b[1]+b[3])/2
        dist = min(math.hypot(cx-px, cy-py) for px, py in pts)
        if dist <= 9.0: cands[ref] = (dist, cx, cy, b, axis)
    trials = []
    for ref, (dist, cx, cy, b, axis) in sorted(cands.items(), key=lambda kv: kv[1][0]):
        w, h = b[2]-b[0], b[3]-b[1]
        dirs = ((1,0),(-1,0),(0,1),(0,-1),(1,1),(-1,-1),(1,-1),(-1,1))
        if axis == "x": dirs = ((1,0),(-1,0))
        if axis == "y": dirs = ((0,1),(0,-1))
        for step in (0.8, 1.6):
            for dx, dy in dirs:
                nx, ny = cx+dx*step, cy+dy*step; r = (nx-w/2, ny-h/2, nx+w/2, ny+h/2)
                if axis == "x":
                    if r[0] < out[0]+RIM or r[2] > out[2]-RIM: continue
                elif axis == "y":
                    if r[1] < out[1]+RIM or r[3] > out[3]-RIM: continue
                elif r[0] < out[0]+RIM or r[1] < out[1]+RIM or r[2] > out[2]-RIM or r[3] > out[3]-RIM: continue
                if overlaps(r, parts, ref): continue
                trials.append((ref, round(nx, 2), round(ny, 2)))
    import random; random.Random(rnd).shuffle(trials)
    trials = trials[:42]
    log(f"round {rnd}: {len(trials)} trials from {len(cands)} parts near {len(pts)} blocking points")
    cur_text = open(side).read()
    def one(tr):
        ref, nx, ny = tr; t = tag()
        sc, bl, pcb = with_server(lambda s: trial(s, cur_text, {ref: (nx, ny)}, t))
        log(t, "score", sc, f"{ref}->({nx},{ny})"); return sc, tr, bl, pcb
    best = (base_score, None, base_blocks, base_pcb); done_n = 0
    with cf.ThreadPoolExecutor(WORKERS) as ex:
        futs = [ex.submit(one, tr) for tr in trials]
        for f in cf.as_completed(futs):
            res = f.result(); done_n += 1
            if res[0] < best[0]: best = res
            if best[0] <= PENALTY or (best[1] is not None and done_n >= 15):
                for g in futs: g.cancel()
                break
    if best[1] is None: log("no single move improved the board; stopping"); break
    ref, nx, ny = best[1]; open(side, "w").write(set_pos(cur_text, ref, nx, ny, None))
    base_score, base_blocks, base_pcb = best[0], best[2], best[3]
    log(f"accepted {ref}->({nx},{ny}); score {base_score}", [b[:90] for b in base_blocks])
log("DONE score", base_score, "pcb", base_pcb)
shutil.copy(base_pcb, f"{ws}/release_search_final.kicad_pcb") if base_pcb else None
