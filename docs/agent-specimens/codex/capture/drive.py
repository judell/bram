# Recovered from Claude's 2026-10-28 capture scratchpad.
# Recovery signed: Codex (main thread), 2026-10-28. See README.md for limitations.
import sys,os,pty,fcntl,termios,struct,select,time,signal,json,re
# usage: drive.py OUTDIR SCHEDULE.json -- cmd...   schedule: [[t_seconds, "keys"|null, "snapname"|null],...] ; total = last t
out=sys.argv[1]; sched=json.load(open(sys.argv[2])); cmd=sys.argv[4:]
pid,fd=pty.fork()
if pid==0:
    os.environ["COLUMNS"],os.environ["LINES"]="120","40"; os.execvp(cmd[0],cmd)
fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack("HHHH",40,120,0,0))
print("PID",pid,file=sys.stderr)
buf=b"";mark=0;start=time.time();i=0
def pump(d):
    global buf
    r,_,_=select.select([fd],[],[],d)
    if r:
        try: c=os.read(fd,65536)
        except OSError: return False
        if not c: return False
        buf+=c
    return True
alive=True
while i<len(sched) and alive:
    t,keys,snap=sched[i]
    if time.time()-start>=t:
        if snap:
            open(f"{out}/{snap}.raw","wb").write(buf); 
        if snap: mark=len(buf)
        if keys: os.write(fd,keys.encode().decode("unicode_escape").encode())
        i+=1
    else: alive=pump(0.2)
for s in (signal.SIGTERM,signal.SIGKILL):
    try: os.kill(pid,s)
    except ProcessLookupError: break
    time.sleep(0.3)
open(f"{out}/_all.raw","wb").write(buf)
