import sys,math
def parse(s):
    i=0;st=[[]];tok=''
    n=len(s)
    while i<n:
        c=s[i]
        if c=='"':
            j=i+1
            while s[j]!='"':
                j+= 2 if s[j]=='\\' else 1
            st[-1].append(s[i+1:j]);i=j+1;continue
        if c in '()':
            if c=='(':st.append([])
            else:
                x=st.pop();st[-1].append(x)
            i+=1;continue
        if c.isspace(): i+=1;continue
        j=i
        while j<n and not s[j].isspace() and s[j] not in '()': j+=1
        st[-1].append(s[i:j]);i=j
    return st[0][0]
def find(n,k): return [c for c in n if isinstance(c,list) and c and c[0]==k]
pcb=parse(open(sys.argv[1]).read())
vias=[(28.9,31.9),(36.1,39.1),(51.7,45.0),(30.0,20.0),(51.8,20.0),(47.2,17.4),(41.2,22.2)]
def d(p,a,b):
    dx,dy=b[0]-a[0],b[1]-a[1];L=dx*dx+dy*dy
    t=0 if L==0 else max(0,min(1,((p[0]-a[0])*dx+(p[1]-a[1])*dy)/L))
    return math.hypot(p[0]-a[0]-t*dx,p[1]-a[1]-t*dy)
for v in vias:
    out=[]
    for fp in find(pcb,'footprint'):
        at=find(fp,'at')[0];fx,fy=float(at[1]),float(at[2]);rot=float(at[3]) if len(at)>3 else 0
        ref=[p[2] for p in find(fp,'property') if p[1]=='Reference'][0]
        c,s=math.cos(math.radians(-rot)),math.sin(math.radians(-rot))
        def tr(pt): return (fx+pt[0]*c-pt[1]*s, fy+pt[0]*s+pt[1]*c)
        for ln in find(fp,'fp_line'):
            if ['layer','F.SilkS'] in [x for x in find(ln,'layer')] or any(l[1]=='F.SilkS' for l in find(ln,'layer')):
                a=tr((float(find(ln,'start')[0][1]),float(find(ln,'start')[0][2])));b=tr((float(find(ln,'end')[0][1]),float(find(ln,'end')[0][2])))
                dd=d(v,a,b)
                if dd<0.5: out.append((ref,'outline',round(dd,2)))
        for p in find(fp,'property'):
            if p[1]=='Reference':
                lay=[l[1] for l in find(p,'layer')]
                pa=find(p,'at')[0]
                if 'F.SilkS' in lay:
                    a=(float(pa[1]),float(pa[2]))
                    # property at is relative to footprint? report distance both ways
                    out.append((ref,'label@'+str(a),round(math.hypot(fx+a[0]-v[0],fy+a[1]-v[1]),1)))
    out=[o for o in out if o[1]=='outline' or o[2]<4]
    print(v,out)
