import sys,math
f=sys.argv[1]
sys.argv=[sys.argv[0],f]
exec(open('/home/synth/who.py').read().split("vias=[")[0])
def segdist(p,a,b):
    dx,dy=b[0]-a[0],b[1]-a[1];L=dx*dx+dy*dy
    t=0 if L==0 else max(0,min(1,((p[0]-a[0])*dx+(p[1]-a[1])*dy)/L))
    return math.hypot(p[0]-a[0]-t*dx,p[1]-a[1]-t*dy)
vias=[(float(find(v,'at')[0][1]),float(find(v,'at')[0][2]),float(find(v,'size')[0][1])/2) for v in find(pcb,'via')]
pads=[];segs=[];widths={};thick={}
for fp in find(pcb,'footprint'):
    at=find(fp,'at')[0];fx,fy=float(at[1]),float(at[2]);rot=float(at[3]) if len(at)>3 else 0
    c,s=math.cos(math.radians(-rot)),math.sin(math.radians(-rot))
    tr=lambda pt:(fx+pt[0]*c-pt[1]*s,fy+pt[0]*s+pt[1]*c)
    for pad in find(fp,'pad'):
        pa=find(pad,'at')[0];sz=find(pad,'size')[0]
        x,y=tr((float(pa[1]),float(pa[2])));w,h=float(sz[1]),float(sz[2])
        tot=math.radians(rot+(float(pa[3]) if len(pa)>3 else 0))
        pads.append((x,y,abs(w*math.cos(tot))/2+abs(h*math.sin(tot))/2,abs(w*math.sin(tot))/2+abs(h*math.cos(tot))/2))
    for ln in find(fp,'fp_line'):
        if any(l[1] in('F.SilkS','B.SilkS') for l in find(ln,'layer')):
            w=float(find(find(ln,'stroke')[0],'width')[0][1]);widths[w]=widths.get(w,0)+1
            a=tr((float(find(ln,'start')[0][1]),float(find(ln,'start')[0][2])));b=tr((float(find(ln,'end')[0][1]),float(find(ln,'end')[0][2])))
            segs.append((a,b,w))
    for p in find(fp,'property'):
        if any(l[1] in('F.SilkS','B.SilkS') for l in find(p,'layer')):
            for e in find(p,'effects'):
                for fo in find(e,'font'):
                    for th in find(fo,'thickness'): thick[float(th[1])]=thick.get(float(th[1]),0)+1
print('silk line widths',dict(sorted(widths.items())),'label thickness',dict(sorted(thick.items())))
def padgap(pt):
    best=9
    for x,y,ex,ey in pads:
        dx=max(0,abs(pt[0]-x)-ex);dy=max(0,abs(pt[1]-y)-ey);best=min(best,math.hypot(dx,dy))
    return best
nv=np=0
for a,b,w in segs:
    n=max(1,int(math.hypot(b[0]-a[0],b[1]-a[1])/0.05))
    over=False;near=False
    for k in range(n+1):
        pt=(a[0]+(b[0]-a[0])*k/n,a[1]+(b[1]-a[1])*k/n)
        if any(math.hypot(pt[0]-vx,pt[1]-vy)<vr+w/2 for vx,vy,vr in vias): over=True
        if padgap(pt)-w/2<0.15-0.005: near=True
    nv+=over;np+=near
print('silk lines',len(segs),'touching a via',nv,'closer than 0.15 to a pad (approx rect)',np)
