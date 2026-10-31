import hashlib,re,os
base=os.path.expanduser("~/runs/irr-d2-repro")
def h(b): return hashlib.sha256(b).hexdigest()[:10]
def mask(t):
    out=[];i=0
    pat=re.compile(r'\((?:segment|via|zone)\b')
    while True:
        m=pat.search(t,i)
        if not m: out.append(t[i:]);break
        j=m.start();out.append(t[i:j]);d=0;k=j
        while True:
            c=t[k]
            if c=="(":d+=1
            elif c==")":
                d-=1
                if d==0:break
            k+=1
        out.append(re.sub(r'\(uuid "[^"]*"\)','(uuid "-")',t[j:k+1]));i=k+1
    return "".join(out)
p=[open(f"{base}/out{i}/irrigation_controller.kicad_pcb").read() for i in (1,2,3)]
m=[h(mask(x).encode()) for x in p]
print("routed pcb, uuids of segments, vias and zones masked:",m,"SAME" if len(set(m))==1 else "DIFF")
