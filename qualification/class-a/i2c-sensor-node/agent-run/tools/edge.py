import re,sys,math
def nums(s): return [float(v) for v in re.findall(r'-?\d+\.?\d*',s)]
def parse(path):
    t=open(path).read()
    ex=[];ey=[]
    for m in re.finditer(r'\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid',t,re.S):
        if 'Edge.Cuts' in m.group(4):
            a=nums(m.group(2));b=nums(m.group(3)); ex+=[a[0],b[0]]; ey+=[a[1],b[1]]
    x0,x1,y0,y1=min(ex),max(ex),min(ey),max(ey)
    print(path.split('/')[-1],f"outline x {x0}..{x1}  y {y0}..{y1}")
    for blk in re.split(r'\n\t\(footprint\n',t)[1:]:
        ref=re.search(r'"Reference"\s+"([^"]+)"',blk).group(1)
        if not re.match(r'(J|P|CN)',ref): continue
        at=re.search(r'\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)',blk)
        cx,cy=float(at.group(1)),float(at.group(2)); rot=float(at.group(3) or 0)
        xs=[];ys=[]; r=math.radians(rot)
        for g in re.finditer(r'\((fp_line|fp_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid',blk,re.S):
            if 'F.CrtYd' in g.group(4):
                for p in (nums(g.group(2)),nums(g.group(3))):
                    xs.append(cx+p[0]*math.cos(r)+p[1]*math.sin(r)); ys.append(cy-p[0]*math.sin(r)+p[1]*math.cos(r))
        if not xs: print(" ",ref,"no courtyard"); continue
        gaps={"left":min(xs)-x0,"right":x1-max(xs),"top":min(ys)-y0,"bottom":y1-max(ys)}
        side=min(gaps,key=gaps.get)
        print(f"  {ref:4s} rot={rot:5.0f}  nearest edge={side:6s}  courtyard-to-edge={gaps[side]:6.2f} mm   (all: "+", ".join(f"{k}={v:.1f}" for k,v in gaps.items())+")")
for p in sys.argv[1:]: parse(p)
