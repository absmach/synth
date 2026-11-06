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

EXTRA = {}
class Server:
    """One Synth MCP server working in its own directory."""
    def __init__(self, d):
        self.d, self.n = d, 0
        env = dict(BASE_ENV); env["SYNTH_USER_REGISTRY_DIR"] = d + "/user-registry"; env.update(EXTRA.get(d, {}))
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



import queue
def prepare(dst):
    os.makedirs(dst, exist_ok=True)
    shutil.copy(f"{ws}/{synth}", dst)
    shutil.rmtree(dst + "/user-registry", ignore_errors=True); shutil.copytree(ws + "/user-registry", dst + "/user-registry")

PER = int(sys.argv[5]) if len(sys.argv) > 5 else 6
NPAIRS = int(sys.argv[6]) if len(sys.argv) > 6 else 60
STEP = 0.8
ORDERS = ["inside_out", "original"]

def flat(o):
    for v in o:
        if isinstance(v, list): yield from flat(v)
        else: yield str(v)

def trial(server, sidecar_text, moves, tag):
    d = server.d; text = sidecar_text
    for ref, (x, y) in moves.items(): text = set_pos(text, ref, x, y, None)
    sc = f"{d}/{name}.placement.layout.toml"; open(sc, "w").write(text)
    od = f"{d}/{tag}"; shutil.rmtree(od, ignore_errors=True)
    r = server.tool("synth_export", {"file_path": f"{d}/{synth}", "layout_file_path": sc, "out_dir": od, "registry_path": "/home/synth/synth/registry/parts", "allow_incomplete": False})
    if "_error" in r: return [f"error {str(r['_error'])[:100]}"], None
    blocks = [b for bl in find(r, "blocking_reasons") for b in flat(bl)]
    rr, rc = list(find(r, "release_ready")), list(find(r, "route_complete"))
    if not (rr and rr[0] and rc and rc[0]) and not blocks: blocks = ["not release ready"]
    pcb = f"{od}/{name}.kicad_pcb"
    return blocks, (pcb if os.path.exists(pcb) else None)

pools = {o: queue.Queue() for o in ORDERS}
servers = []
for o in ORDERS:
    for i in range(PER):
        d = f"{ws}/_w_{o}_{i}"; prepare(d)
        EXTRA[d] = {"SYNTH_KRT_ORDERING": o, "SYNTH_ROUTER_ORDERING_RETRIES": "0"}
        s = Server(d); servers.append(s); pools[o].put(s)

def run_trial(order, text, moves, tg):
    s = pools[order].get()
    try: return trial(s, text, moves, tg)
    finally: pools[order].put(s)

side = f"{ws}/{name}.placement.layout.toml"
count = [0]
def tag():
    with lock: count[0] += 1; return f"fx_{count[0]:03d}"

def points_of(blocks):
    out = []
    for b in blocks:
        m = re.search(r'signal via at \(([\d.-]+), ([\d.-]+)\)', b)
        if m: out.append((float(m.group(1)), float(m.group(2))))
    return out
def pad_blocks(blocks): return [b for b in blocks if "inside a pad" in b]

cur_text = open(side).read()
# Base under every order.
log("base under each net order")
with cf.ThreadPoolExecutor(len(ORDERS)) as ex:
    res = list(ex.map(lambda o: (o, *run_trial(o, cur_text, {}, tag())), ORDERS))
for o, bl, pcb in res: log("base", o, len(bl), [b[:70] for b in bl][:2])
state = min(res, key=lambda r: len(r[1]))
state_order, state_blocks, state_pcb = state
log("best base order", state_order, len(state_blocks))
solved = []
for target_no in range(8):
    if not state_blocks: break
    pts = points_of(state_blocks)
    if not pts: log("remaining blocks are not return vias:", [b[:100] for b in state_blocks]); break
    p = pts[0]
    out, parts = read_pcb(state_pcb)
    import random, itertools
    cand_parts = [(d, r, bb) for d, r, bb in sorted(((math.hypot((bb[0]+bb[2])/2-p[0], (bb[1]+bb[3])/2-p[1]), r, bb) for r, bb in parts.items() if not r.startswith(("J", "H", "MH", "U"))))[:6]]
    opts = {}
    for dist, ref, bb in cand_parts:
        cx, cy = (bb[0]+bb[2])/2, (bb[1]+bb[3])/2; w, h = bb[2]-bb[0], bb[3]-bb[1]; lst = []
        for step in (0.8, 1.6):
            for dx, dy in ((1,0),(-1,0),(0,1),(0,-1)):
                nx, ny = cx+dx*step, cy+dy*step; r = (nx-w/2, ny-h/2, nx+w/2, ny+h/2)
                if r[0] < out[0]+RIM or r[1] < out[1]+RIM or r[2] > out[2]-RIM or r[3] > out[3]-RIM: continue
                lst.append((round(nx, 2), round(ny, 2)))
        opts[ref] = lst
    combos = []
    for r1, r2 in itertools.combinations(list(opts), 2):
        for m1 in opts[r1]:
            for m2 in opts[r2]:
                combos.append({r1: m1, r2: m2})
    random.Random(7).shuffle(combos)
    combos = combos[:NPAIRS]
    moves = [("none", None)] + [("+".join(c), c) for c in combos]
    log(f"target {target_no}: via at {p}, parts {[(r, round(d, 1)) for d, r, _ in cand_parts]}, {len(moves)} moves x {len(ORDERS)} orders; {len(state_blocks)} blocks now")
    def one(item):
        order, (label, mv) = item
        bl, pcb = run_trial(order, cur_text, (mv or {}), tag())
        cleared = not any(abs(q[0]-p[0]) < 0.6 and abs(q[1]-p[1]) < 0.6 for q in points_of(bl))
        new_pad = len(pad_blocks(bl)) > len(pad_blocks(state_blocks))
        ok = cleared and not new_pad and len(bl) < len(state_blocks)
        log("  ", order, label, mv, "blocks", len(bl), "CLEARED" if ok else "")
        return ok, len(bl), order, label, mv, bl, pcb
    items = [(o, m) for m in moves for o in ORDERS]
    wins = []; done_n = 0
    with cf.ThreadPoolExecutor(len(ORDERS) * PER) as ex:
        futs = [ex.submit(one, it) for it in items]
        for f in cf.as_completed(futs):
            r = f.result(); done_n += 1
            if r[0]: wins.append(r)
            if r[1] == 0 or (wins and done_n >= 14):
                for g in futs: g.cancel()
                break
    if not wins: log("no combination cleared this via; stopping"); break
    ok, n, order, ref, mv, bl, pcb = min(wins, key=lambda r: (r[1], 0 if r[4] is None else 1))
    if mv:
        for rr_, xy in mv.items(): cur_text = set_pos(cur_text, rr_, xy[0], xy[1], None)
        open(side, "w").write(cur_text)
    state_order, state_blocks, state_pcb = order, bl, pcb
    solved.append((p, ref, mv, order)); log(f"ACCEPT via {p}: move {ref}->{mv} with order {order}; {n} blocks left")
log("DONE solved", solved, "blocks left", len(state_blocks), "order", state_order, "pcb", state_pcb)
if state_pcb: shutil.copy(state_pcb, f"{ws}/fix_final.kicad_pcb")
