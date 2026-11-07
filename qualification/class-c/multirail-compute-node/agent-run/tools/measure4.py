import re,math,collections,sys,json
t=re.sub(r'\s+',' ',open(sys.argv[1]).read())
xs=[];ys=[]
for m in re.finditer(r'\(start ([\d.\-]+) ([\d.\-]+)\) \(end ([\d.\-]+) ([\d.\-]+)\) \(stroke \(width [\d.]+\) \(type \w+\) \) \(layer "Edge.Cuts"\)',t):
    xs+= [float(m[1]),float(m[3])]; ys+=[float(m[2]),float(m[4])]
bx0,bx1,by0,by1=min(xs),max(xs),min(ys),max(ys)
out={'board':[bx0,by0,bx1,by1]}
nets=dict((int(a),b) for a,b in re.findall(r'\(net (\d+) "([^"]*)"\)',t))
vias=[(float(m[1]),float(m[2]),float(m[3]),float(m[4]),nets.get(int(m[5]))) for m in re.finditer(r'\(via (?:\(locked[^)]*\) )?\(at ([\d.\-]+) ([\d.\-]+)\) \(size ([\d.]+)\) \(drill ([\d.]+)\)[^@]*?\(net (\d+)\)',t)]
out['vias']=len(vias); out['via_sizes']={f'{k[0]}/{k[1]}':v for k,v in collections.Counter((v[2],v[3]) for v in vias).items()}
fps=t.split(' (footprint ')[1:]
info={}; pads={}
for f in fps:
    ref=re.search(r'\(property "Reference" "([^"]+)"',f)[1]
    at=re.search(r'\(layer "[^"]+"\) \(uuid "[^"]+"\) \(at ([\d.\-]+) ([\d.\-]+)(?: ([\d.\-]+))?\)',f)
    fx,fy,rot=float(at[1]),float(at[2]),float(at[3] or 0); a=math.radians(-rot)
    pts=[]
    for m in re.finditer(r'\(fp_(?:rect|line) \(start ([\d.\-]+) ([\d.\-]+)\) \(end ([\d.\-]+) ([\d.\-]+)\) (?:\(stroke \(width [\d.]+\) \(type \w+\) \) )?(?:\(fill \w+\) )?\(layer "F.CrtYd"\)',f):
        for x,y in ((float(m[1]),float(m[2])),(float(m[3]),float(m[4]))):
            pts.append((fx+x*math.cos(a)-y*math.sin(a),fy+x*math.sin(a)+y*math.cos(a)))
    if pts: info[ref]=(min(p[0] for p in pts),min(p[1] for p in pts),max(p[0] for p in pts),max(p[1] for p in pts))
    pl=[]
    for p in re.finditer(r'\(pad "([^"]*)" (\w+) (\w+) \(at ([\d.\-]+) ([\d.\-]+)(?: [\d.\-]+)?\) \(size ([\d.]+) ([\d.]+)\)',f):
        x,y=float(p[4]),float(p[5]); pl.append((p[1],p[2],round(fx+x*math.cos(a)-y*math.sin(a),3),round(fy+x*math.sin(a)+y*math.cos(a),3),float(p[6]),float(p[7])))
    pads[ref]=pl
d=lambda b:min(b[0]-bx0,b[1]-by0,bx1-b[2],by1-b[3])
rows=sorted((round(d(b),2),r) for r,b in info.items())
out['courtyard_to_edge_closest']=rows[:14]
# through-hole to SMD pad distance (centre-to-edge approx: pad centre distance minus half sizes)
tht=[(r,p) for r,pl in pads.items() for p in pl if p[1]=='thru_hole']
smd=[(r,p) for r,pl in pads.items() for p in pl if p[1]=='smd']
best=None
for r,p in tht:
    for r2,q in smd:
        if r==r2: continue
        dx=max(abs(p[2]-q[2])-(p[4]+q[4])/2,0); dy=max(abs(p[3]-q[3])-(p[5]+q[5])/2,0)
        dist=math.hypot(dx,dy)
        if best is None or dist<best[0]: best=(round(dist,2),r,p[0],r2,q[0])
out['tht_to_smd_min']=best
u1=pads['U1']; ep=[p for p in u1 if p[0]=='9'][0]
inside=[v for v in vias if abs(v[0]-ep[2])<=ep[4]/2+0.1 and abs(v[1]-ep[3])<=ep[5]/2+0.1]
out['U1_EP']=dict(pad=ep,vias=[(v[0],v[1],v[2],v[3],v[4]) for v in inside])
out['edge_connectors']={r:round(d(info[r]),2) for r in ('J1','J2','J3','J4') if r in info}
print(json.dumps(out,indent=1))
