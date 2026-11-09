import re,sys,math
t=open(sys.argv[1]).read()
def nums(s): return [float(v) for v in re.findall(r"-?\d+\.?\d*",s)]
ex=[];ey=[]
for m in re.finditer(r"\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid",t,re.S):
    if "Edge.Cuts" in m.group(4):
        a,b=nums(m.group(2)),nums(m.group(3));ex+=[a[0],b[0]];ey+=[a[1],b[1]]
x0,y0,x1,y1=min(ex),min(ey),max(ex),max(ey)
W,H=int(x1-x0)+1,int(y1-y0)+1
g=[["." for _ in range(W)] for _ in range(H)]
tot=0;rows=[]
for blk in re.split(r"\n\t\(footprint\n",t)[1:]:
    ref=re.search(r"\"Reference\"\s+\"([^\"]+)\"",blk).group(1)
    at=re.search(r"\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)",blk)
    cx,cy,rot=float(at.group(1)),float(at.group(2)),float(at.group(3) or 0);r=math.radians(rot);xs=[];ys=[]
    for q in re.finditer(r"\((fp_line|fp_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid",blk,re.S):
        if "F.CrtYd" in q.group(4):
            for p in (nums(q.group(2)),nums(q.group(3))):
                xs.append(cx+p[0]*math.cos(r)+p[1]*math.sin(r));ys.append(cy-p[0]*math.sin(r)+p[1]*math.cos(r))
    if not xs: continue
    a=(min(xs),min(ys),max(xs),max(ys));tot+=(a[2]-a[0])*(a[3]-a[1]);rows.append((ref,round(a[2]-a[0],1),round(a[3]-a[1],1)))
    for yy in range(int(a[1]-y0),int(a[3]-y0)+1):
        for xx in range(int(a[0]-x0),int(a[2]-x0)+1):
            if 0<=yy<H and 0<=xx<W: g[yy][xx]=ref[0] if g[yy][xx]=="." else "#"
print("board",x1-x0,y1-y0,"courtyard sum",round(tot),"fill",round(100*tot/((x1-x0)*(y1-y0)),1))
print("\n".join("".join(r) for r in g))
print(sorted(rows,key=lambda r:-r[1]*r[2])[:12])
