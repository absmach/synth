import re,sys,math
d=sys.argv[1]
# drill
holes=[];tools={};cur=None
for l in open(d+'/irrigation_controller.drl'):
    l=l.strip()
    m=re.match(r'T(\d+)C([\d.]+)',l)
    if m: tools[m.group(1)]=float(m.group(2));continue
    m=re.match(r'^T(\d+)$',l)
    if m: cur=tools.get(m.group(1));continue
    m=re.match(r'X(-?[\d.]+)Y(-?[\d.]+)$',l)
    if m and cur: holes.append((float(m.group(1)),float(m.group(2)),cur))
# silk
ap={};cur=None;segs=[];x=y=0;flashes=[];inreg=False;reg=[]
for l in open(d+'/irrigation_controller-F_Silkscreen.gto'):
    l=l.strip()
    m=re.match(r'%ADD(\d+)([A-Z]+),([\d.]+)',l)
    if m: ap[m.group(1)]=(m.group(2),float(m.group(3)));continue
    m=re.match(r'^D(\d+)\*$',l)
    if m and int(m.group(1))>=10: cur=m.group(1);continue
    if l.startswith('G36'): inreg=True;reg=[];continue
    if l.startswith('G37'):
        inreg=False
        for a,b in zip(reg,reg[1:]): segs.append((a,b,0.0))
        continue
    m=re.match(r'X(-?\d+)Y(-?\d+)D0([123])\*',l)
    if m:
        nx,ny=int(m.group(1))/1e6,int(m.group(2))/1e6;op=m.group(3)
        if op=='2':
            if inreg: reg=[(nx,ny)]
        elif op=='1':
            if inreg: reg.append((nx,ny))
            else: segs.append(((x,y),(nx,ny),ap[cur][1] if cur in ap else 0.1))
        elif op=='3': flashes.append((nx,ny,ap.get(cur)))
        x,y=nx,ny
def dist(p,a,b):
    ax,ay=a;bx,by=b;px,py=p
    dx,dy=bx-ax,by-ay;L=dx*dx+dy*dy
    t=0 if L==0 else max(0,min(1,((px-ax)*dx+(py-ay)*dy)/L))
    return math.hypot(px-(ax+t*dx),py-(ay+t*dy))
hits=[]
for hx,hy,dia in holes:
    best=1e9
    for a,b,w in segs:
        v=dist((hx,hy),a,b)-w/2-dia/2
        best=min(best,v)
    hits.append((round(hx,2),round(hy,2),dia,round(best,3)))
print(len(holes),'holes',len(segs),'silk segs',len(flashes),'flashes')
for m in (0,0.1,0.2,0.3,0.5):
    print('margin',m,sum(1 for h in hits if h[3]<=m), 'vias(0.3):',sum(1 for h in hits if h[3]<=m and h[2]<0.35))
for h in sorted(hits,key=lambda h:h[3])[:30]: print(h)
