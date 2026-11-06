import sys,math,re
f=sys.argv[1]
sys.argv=[sys.argv[0],f]
exec(open('/home/synth/who.py').read().split("vias=[")[0])
s=open(f).read()
ex=[];ey=[]
for m in re.finditer(r"\(gr_line\s*\(start ([-\d.]+) ([-\d.]+)\)\s*\(end ([-\d.]+) ([-\d.]+)\)[\s\S]{0,250}?Edge\.Cuts",s):
    ex+=[float(m.group(1)),float(m.group(3))];ey+=[float(m.group(2)),float(m.group(4))]
x0,x1,y0,y1=min(ex),max(ex),min(ey),max(ey)
print("outline",round(x1-x0,1),"x",round(y1-y0,1))
for fp in find(pcb,'footprint'):
    ref=[p[2] for p in find(fp,'property') if p[1]=='Reference'][0]
    if not ref.startswith('J'): continue
    at=find(fp,'at')[0];fx,fy=float(at[1]),float(at[2]);rot=float(at[3]) if len(at)>3 else 0
    c,sn=math.cos(math.radians(-rot)),math.sin(math.radians(-rot))
    xs=[];ys=[]
    for ln in find(fp,'fp_line')+find(fp,'fp_rect'):
        if any(l[1]=='F.CrtYd' for l in find(ln,'layer')):
            for k in ('start','end'):
                p=find(ln,k)[0];x,y=float(p[1]),float(p[2]);xs.append(fx+x*c-y*sn);ys.append(fy+x*sn+y*c)
    if not xs: continue
    g={'left':min(xs)-x0,'right':x1-max(xs),'top':min(ys)-y0,'bottom':y1-max(ys)}
    side=min(g,key=g.get)
    print(ref,side,round(g[side],2),{k:round(v,1) for k,v in g.items()})
