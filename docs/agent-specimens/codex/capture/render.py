# Recovered from Claude's 2026-10-28 capture scratchpad.
# Recovery signed: Codex (main thread), 2026-10-28. See README.md for limitations.
import sys,re
# minimal VT renderer -> final screen text
def render(data,rows=40,cols=120):
    g=[[" "]*cols for _ in range(rows)];r=c=0
    s=data.decode("utf-8","replace");i=0
    while i<len(s):
        ch=s[i]
        if ch=="\x1b":
            m=re.match(r"\x1b\[([0-9;?<>=]*)([ -/]*)([@-~])",s[i:])
            if m:
                p,_,f=m.groups();a=[int(x) if x.isdigit() else 0 for x in p.lstrip("?<>=").split(";")] if p else []
                n=a[0] if a and a[0] else 1
                if f in"Hf": r=min(rows-1,(a[0] or 1)-1 if a else 0); c=min(cols-1,(a[1] or 1)-1 if len(a)>1 else 0)
                elif f=="A": r=max(0,r-n)
                elif f=="B": r=min(rows-1,r+n)
                elif f=="C": c=min(cols-1,c+n)
                elif f=="D": c=max(0,c-n)
                elif f=="G": c=min(cols-1,n-1)
                elif f=="d": r=min(rows-1,n-1)
                elif f=="K":
                    k=a[0] if a else 0
                    rng=range(c,cols) if k==0 else range(0,c+1) if k==1 else range(cols)
                    for x in rng:g[r][x]=" "
                elif f=="J":
                    k=a[0] if a else 0
                    if k in(2,3):
                        g=[[" "]*cols for _ in range(rows)]
                    elif k==0:
                        for x in range(c,cols):g[r][x]=" "
                        for y in range(r+1,rows):g[y]=[" "]*cols
                elif f=="X":
                    for x in range(c,min(cols,c+n)):g[r][x]=" "
                elif f=="S":
                    for _ in range(n): g.pop(0);g.append([" "]*cols)
                elif f=="T":
                    for _ in range(n): g.pop();g.insert(0,[" "]*cols)
                i+=m.end();continue
            m=re.match(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)",s[i:])
            if m:i+=m.end();continue
            m=re.match(r"\x1b[()][0-9A-Za-z]|\x1b[=>78MDE]",s[i:])
            if m:
                if m.group()=="\x1bM": r=max(0,r-1)
                i+=m.end();continue
            i+=1;continue
        if ch=="\r":c=0
        elif ch=="\n":
            if r==rows-1: g.pop(0);g.append([" "]*cols)
            else:r+=1
        elif ch>=" ":
            if c>=cols: c=0; r=min(rows-1,r+1)
            g[r][c]=ch;c+=1
        i+=1
    return "\n".join("".join(l).rstrip() for l in g).rstrip()+"\n"
if __name__=="__main__":
    print(render(open(sys.argv[1],"rb").read()),end="")
