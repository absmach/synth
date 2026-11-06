import sys
ns={}; exec(open("/home/synth/who.py").read().split("pcb=parse")[0], ns)
P,F=ns["parse"],ns["find"]
for f in sys.argv[1:]:
    p=P(open(f).read()); o=[]
    for fp in F(p,"footprint"):
        ref=[q[2] for q in F(fp,"property") if q[1]=="Reference"][0]
        if ref in ("C_IN2","C_IN1","C_BULK","R_SN1","L1","U1"):
            a=F(fp,"at")[0]; o.append(ref+"="+a[1]+","+a[2])
    print(f.split("/")[0],f.split("/")[1]," ".join(sorted(o)))
