import sys,math
f,net=sys.argv[1],sys.argv[2]
sys.argv=[sys.argv[0],f]
exec(open('/home/synth/who.py').read().split("vias=[")[0])
nid=None
for n in find(pcb,'net'):
    if len(n)>2 and n[2]==net: nid=n[1]
pads=[];segs=[];vias=[]
for fp in find(pcb,'footprint'):
    at=find(fp,'at')[0];fx,fy=float(at[1]),float(at[2]);rot=float(at[3]) if len(at)>3 else 0
    ref=[p[2] for p in find(fp,'property') if p[1]=='Reference'][0]
    c,s=math.cos(math.radians(-rot)),math.sin(math.radians(-rot))
    for pad in find(fp,'pad'):
        nn=find(pad,'net')
        if nn and nn[0][2]==net:
            pa=find(pad,'at')[0];x,y=float(pa[1]),float(pa[2]);pads.append((f"{ref}.{pad[1]}",fx+x*c-y*s,fy+x*s+y*c))
for sg in find(pcb,'segment'):
    nn=find(sg,'net')
    if nn and nn[0][1]==nid:
        a=find(sg,'start')[0];b=find(sg,'end')[0];l=find(sg,'layer')[0][1]
        segs.append(((float(a[1]),float(a[2])),(float(b[1]),float(b[2])),l))
for v in find(pcb,'via'):
    nn=find(v,'net')
    if nn and nn[0][1]==nid:
        a=find(v,'at')[0];vias.append((float(a[1]),float(a[2])))
print(net,"pads",len(pads),"segs",len(segs),"vias",len(vias))
nodes=[('p',p[0],(p[1],p[2])) for p in pads]+[('v','via',v) for v in vias]
par=list(range(len(nodes)))
def fd(i):
    while par[i]!=i: par[i]=par[par[i]];i=par[i]
    return i
def un(i,j): par[fd(i)]=fd(j)
def near(p,q,t=0.12): return math.hypot(p[0]-q[0],p[1]-q[1])<t
# attach each seg endpoint chain: build points
pts=[]
for a,b,l in segs: pts+= [a,b]
# union segment endpoints themselves
sp=list(range(len(pts)))
def fs(i):
    while sp[i]!=i: sp[i]=sp[sp[i]];i=sp[i]
    return i
for k in range(len(segs)): sp[fs(2*k)]=fs(2*k+1)
for i in range(len(pts)):
    for j in range(i+1,len(pts)):
        if near(pts[i],pts[j]): sp[fs(i)]=fs(j)
comp={}
for i,n in enumerate(nodes):
    for j,p in enumerate(pts):
        if near(n[2],p,0.5): un(i,len(nodes)) if False else None
groups={}
for i,n in enumerate(nodes):
    keys=set(fs(j) for j,p in enumerate(pts) if near(n[2],p,0.35))
    groups[i]=keys
# union nodes sharing a segment group
for i in groups:
    for j in groups:
        if i<j and groups[i]&groups[j]: un(i,j)
cl={}
for i,n in enumerate(nodes): cl.setdefault(fd(i),[]).append(n[1])
for k,v in cl.items(): print(" cluster:",v)
