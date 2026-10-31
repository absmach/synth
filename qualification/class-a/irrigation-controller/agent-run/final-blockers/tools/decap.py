import sys,math
f=sys.argv[1]
sys.argv=[sys.argv[0],f]
exec(open('/home/synth/who.py').read().split("vias=[")[0])
pads={}
refs={}
for fp in find(pcb,'footprint'):
    at=find(fp,'at')[0];fx,fy=float(at[1]),float(at[2]);rot=float(at[3]) if len(at)>3 else 0
    ref=[p[2] for p in find(fp,'property') if p[1]=='Reference'][0]
    c,s=math.cos(math.radians(-rot)),math.sin(math.radians(-rot))
    for pad in find(fp,'pad'):
        pa=find(pad,'at')[0];x,y=float(pa[1]),float(pa[2])
        net=[n[2] for n in find(pad,'net')]
        pads.setdefault(ref,[]).append((pad[1],fx+x*c-y*s,fy+x*s+y*c,net[0] if net else ''))
SUP=('+3V3','+12V_VALVE','VIN_FUSED')
ics={'U1':'IN pin','U2':'VDD','U4':'VDD','U5':'VCC'}
rows=[]
for ref,pl in pads.items():
    if not ref.startswith('C') or ref.startswith('C_X'): continue
    nets=[p[3] for p in pl]
    sup=[p for p in pl if p[3] in SUP]
    if not sup or 'GND' not in nets: continue
    best=None
    for ic in ('U1','U2','U4','U5'):
        for p in pads.get(ic,[]):
            if p[3]==sup[0][3] and p[3] in SUP:
                d=math.hypot(p[1]-sup[0][1],p[2]-sup[0][2])
                if best is None or d<best[0]: best=(d,ic,p[0])
    rows.append((ref,sup[0][3],best))
for r in sorted(rows,key=lambda r:(r[2][0] if r[2] else 99)):
    print(r[0],r[1],'->',(f"{r[2][1]}.{r[2][2]} {r[2][0]:.1f} mm" if r[2] else 'no IC supply pin on net'))
