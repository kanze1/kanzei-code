"""Stop only this deployment's recorded, independently launched process group."""
import os
import signal
import time
from pathlib import Path

root = Path(__file__).resolve().parent.parent
pid = int((root / "logs/serve.pid").read_text().strip())
process = Path("/proc") / str(pid)
if not process.exists():
    print("The recorded H3 service has already exited")
    raise SystemExit(0)
command = (process / "cmdline").read_bytes().replace(b"\0", b" ").decode()
if process.stat().st_uid != os.getuid() or os.getpgid(pid) != pid:
    raise RuntimeError("The recorded PID is not this user's independent process group")
if str(root / "runtime/bin/sglang") not in command or str(root / "models/MiniMax-H3") not in command:
    raise RuntimeError("The recorded PID no longer identifies the H3 service")
os.killpg(pid, signal.SIGTERM)
for _ in range(30):
    if not process.exists():
        print("H3 service stopped")
        break
    time.sleep(1)
else:
    raise TimeoutError("H3 has not exited yet; inspect the recorded group before further action")
