import re,sys,math,json,collections
t=open(sys.argv[1]).read()
nets=dict((int(a),b) for a,b in re.findall(r'\(net (\d+) "([^"]*)"\)\s*\n',t))
segs=collections.defaultdict(list); vias=collections.defaultdict(list)
for m in re.finditer(r'\(segment\s+\(start ([\d.\-]+) ([\d.\-]+)\)\s+\(end ([\d.\-]+) ([\d.\-]+)\)\s+\(width ([\d.]+)\)\s+\(layer "([^"]+)"\)[\s\S]*?\(net (\d+)\)',t):
    n=nets.get(int(m[7]));segs[n].append((float(m[1]),float(m[2]),float(m[3]),float(m[4]),float(m[5]),m[6]))
for m in re.finditer(r'\(via\s+\(at ([\d.\-]+) ([\d.\-]+)\)\s+\(size ([\d.]+)\)\s+\(drill ([\d.]+)\)[\s\S]*?\(net (\d+)\)',t):
    vias[nets.get(int(m[5]))].append((float(m[1]),float(m[2])))
gnd=vias['GND']
def L(n): return sum(math.hypot(a-c,b-d) for a,b,c,d,w,l in segs[n])
def pts(n,step=0.1):
    out=[]
    for a,b,c,d,w,l in segs[n]:
        k=max(1,int(math.hypot(c-a,d-b)/step))
        for i in range(k+1): out.append((a+(c-a)*i/k,b+(d-b)*i/k,l))
    return out
res={}
for lane in ('D0','D1','D2','CK'):
    side={}
    for s in ('P','N'):
        a=f'{lane}_{s}_SRC'; b=f'HDMI_{lane}_{s}'
        side[s]=dict(src=round(L(a),2),hdmi=round(L(b),2),total=round(L(a)+L(b),2),vias=len(vias[a])+len(vias[b]),widths=sorted(set(w for n in (a,b) for *_,w,l in segs[n])),layers=sorted(set(l for n in (a,b) for *_,l in segs[n])))
    # spacing on the connector-side nets
    P=pts(f'HDMI_{lane}_P'); N=pts(f'HDMI_{lane}_N')
    gaps=[]
    for x,y,l in P:
        best=min((math.hypot(x-u,y-v) for u,v,m in N if m==l),default=None)
        if best is not None: gaps.append(best)
    # coupled gap: centre distance minus width
    cen=sorted(gaps)
    side['centre_gap_median']=round(cen[len(cen)//2],3) if cen else None
    side['centre_gap_min']=round(cen[0],3) if cen else None
    side['skew']=round(abs(side['P']['total']-side['N']['total']),2)
    # return vias: each pair via -> nearest GND via
    rv=[]
    for s in ('P','N'):
        for n in (f'{lane}_{s}_SRC',f'HDMI_{lane}_{s}'):
            for v in vias[n]:
                rv.append(round(min(math.hypot(v[0]-g[0],v[1]-g[1]) for g in gnd),2))
    side['via_to_gnd_via']=rv
    res[lane]=side
tot={l:(res[l]['P']['total']+res[l]['N']['total'])/2 for l in res}
res['inter_pair_mean_len']={k:round(v,2) for k,v in tot.items()}
res['inter_pair_mismatch']=round(max(tot.values())-min(tot.values()),2)
print(json.dumps(res,indent=1))
