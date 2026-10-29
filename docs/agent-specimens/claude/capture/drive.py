# Generalized from docs/agent-specimens/codex/capture/drive.py (Claude subagent, 2026-10-28).
# Changes vs the Codex copy: optional cwd (-C DIR) and env scrubbing happen in run.sh;
# snapshots can be taken without keys; a "wait for PTY EOF" tail so a quitting child is not killed early.
import sys,os,pty,fcntl,termios,struct,select,time,signal,json
# usage: drive.py OUTDIR SCHEDULE.json -- cmd...
# schedule: [[t_seconds_since_start, "keys"|null, "snapname"|null], ...]
# keys use \u001b / \r / \u0003 JSON escapes. Snapshot = cumulative bytes BEFORE that entry's keys are sent.
out=sys.argv[1]; sched=json.load(open(sys.argv[2])); cmd=sys.argv[4:]
pid,fd=pty.fork()
if pid==0:
    os.environ["COLUMNS"],os.environ["LINES"]="120","40"; os.execvp(cmd[0],cmd)
fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack("HHHH",40,120,0,0))
print("PID",pid,file=sys.stderr)
buf=b"";start=time.time();i=0
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
        if snap: open(f"{out}/{snap}.raw","wb").write(buf)
        if keys: os.write(fd,keys.encode().decode("unicode_escape").encode())
        i+=1
    else: alive=pump(0.2)
# if the child already exited, flush remaining output
while alive and pump(0.2) and False: pass
for s in (signal.SIGTERM,signal.SIGKILL):
    try: os.kill(pid,s)
    except ProcessLookupError: break
    time.sleep(0.3)
open(f"{out}/_all.raw","wb").write(buf)
