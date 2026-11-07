#!/usr/bin/env python3
"""gerber_content_hash.py <gerbers.zip> ...: print a hash of each package's Gerber and drill content.

KiCad stamps every Gerber, the drill file and the job file with the time it ran, and the drill
map PDF carries one too. Those lines are masked and the PDF and CSV files are left out, so two
exports of the same board give the same hash.
"""
import hashlib, re, sys, zipfile
for path in sys.argv[1:]:
    z = zipfile.ZipFile(path)
    h = hashlib.sha256(); names = []
    for n in sorted(z.namelist()):
        if n.endswith((".pdf", ".csv")): continue
        t = z.read(n).decode("latin1")
        t = re.sub(r"(?m)^.*(CreationDate|Created by|DRILL file).*$", "", t)
        t = re.sub(r'"CreationDate"\s*:\s*"[^"]*"', "", t)
        h.update(n.encode() + t.encode("latin1")); names.append(n)
    print(f"{h.hexdigest()[:10]}  {path}  ({len(names)} files)")
