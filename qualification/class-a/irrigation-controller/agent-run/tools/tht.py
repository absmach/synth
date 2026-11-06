import sys,math
sys.argv=[sys.argv[0],sys.argv[1]]
exec(open('/home/synth/who.py').read().split("vias=[")[0])
th=[];sm=[]
for fp in find(pcb,'footprint'):
    at=find(fp,'at')[0];fx,fy=float(at[1]),float(at[2]);rot=float(at[3]) if len(at)>3 else 0
    ref=[p[2] for p in find(fp,'property') if p[1]=='Reference'][0]
    c,s=math.cos(math.radians(-rot)),math.sin(math.radians(-rot))
    for pad in find(fp,'pad'):
        pa=find(pad,'at')[0];px,py=float(pa[1]),float(pa[2])
        sz=find(pad,'size')[0];w,h=float(sz[1]),float(sz[2])
        x,y=fx+px*c-py*s,fy+px*s+py*c
        # rotated extents
        tot=math.radians(rot+(float(pa[3]) if len(pa)>3 else 0))
        ex=abs(w*math.cos(tot))/2+abs(h*math.sin(tot))/2;ey=abs(w*math.sin(tot))/2+abs(h*math.cos(tot))/2
        rec=(ref,pad[1],x,y,ex,ey)
        (th if pad[2]=='thru_hole' else sm if pad[2]=='smd' else []).append(rec) if pad[2] in('thru_hole','smd') else None
res=[]
for a in th:
    for b in sm:
        if a[0]==b[0]: continue
        gx=max(0,abs(a[2]-b[2])-a[4]-b[4]);gy=max(0,abs(a[3]-b[3])-a[5]-b[5])
        res.append((math.hypot(gx,gy),a[0]+'.'+a[1],b[0]+'.'+b[1]))
res.sort()
print(len(th),'tht pads',len(sm),'smd pads')
for r in res[:20]: print(round(r[0],2),r[1],r[2])
