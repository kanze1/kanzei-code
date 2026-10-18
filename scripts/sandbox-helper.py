"""Engine-owned Linux bubblewrap executor, embedded by the sandbox tool.

One JSON request on stdin, followed by an optional cancel message or EOF.
Only /usr and an immutable input directory enter the container. All writes are
to bounded tmpfs mounts, and no output file is published back to the project.
"""

import json
import math
import os
from pathlib import Path
import resource
import selectors
import signal
import subprocess
import sys
import time


CAPTURE_BYTES = 1024 * 1024
WORKSPACE_BYTES = 64 * 1024 * 1024
TEMP_BYTES = 16 * 1024 * 1024


def limits(seconds):
    resource.setrlimit(resource.RLIMIT_CPU, (math.ceil(seconds), math.ceil(seconds) + 1))
    resource.setrlimit(resource.RLIMIT_AS, (1024**3, 1024**3))
    resource.setrlimit(resource.RLIMIT_NOFILE, (256, 256))
    resource.setrlimit(resource.RLIMIT_NPROC, (128, 128))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def execute(request):
    inputs = str(Path(request["inputs"]).resolve(strict=True))
    seconds = request["timeout_ms"] / 1000
    if not 0 < seconds <= 120 or not isinstance(request["command"], str):
        raise ValueError("invalid sandbox deadline or command")
    status_read, status_write = os.pipe()
    command = [
        "/usr/bin/bwrap", "--unshare-all", "--unshare-user", "--disable-userns",
        "--die-with-parent", "--new-session", "--cap-drop", "ALL", "--clearenv",
        "--json-status-fd", str(status_write), "--ro-bind", "/usr", "/usr",
        "--symlink", "usr/bin", "/bin", "--symlink", "usr/sbin", "/sbin",
        "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
        "--proc", "/proc", "--dev", "/dev",
        "--ro-bind", inputs, "/input",
        "--size", str(WORKSPACE_BYTES), "--tmpfs", "/workspace",
        "--size", str(TEMP_BYTES), "--tmpfs", "/tmp",
        "--dir", "/home", "--dir", "/home/sandbox", "--chdir", "/workspace",
        "--setenv", "PATH", "/usr/bin:/bin", "--setenv", "HOME", "/home/sandbox",
        "--setenv", "LANG", "C.UTF-8", "--setenv", "TMPDIR", "/tmp",
        "--", "/bin/sh", "-c", 'cp -R /input/. /workspace/ && exec /bin/sh -c "$1"',
        "sandbox", request["command"],
    ]
    try:
        process = subprocess.Popen(
            command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, pass_fds=(status_write,), start_new_session=True,
            preexec_fn=lambda: limits(seconds),
        )
    finally:
        os.close(status_write)
    output = bytearray()
    status = bytearray()
    status_pending = bytearray()
    total_bytes = 0
    outcome = "completed"
    deadline = time.monotonic() + seconds
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ, "output")
    selector.register(status_read, selectors.EVENT_READ, "status")
    selector.register(sys.stdin.fileno(), selectors.EVENT_READ, "control")
    terminated = False
    try:
        while any(key.data != "control" for key in selector.get_map().values()):
            remaining = deadline - time.monotonic()
            if remaining <= 0 and not terminated:
                outcome = "timeout"
                os.killpg(process.pid, signal.SIGKILL)
                terminated = True
            # After killing the namespace owner, drain pipes until kernel cleanup closes them.
            for key, _ in selector.select(None if terminated else max(0, remaining)):
                chunk = os.read(key.fd, 65536)
                if key.data == "control":
                    # EOF also cancels: losing the owning app cannot leave a Linux job running.
                    if not terminated and process.poll() is None:
                        outcome = "cancelled"
                        os.killpg(process.pid, signal.SIGKILL)
                        terminated = True
                    selector.unregister(key.fd)
                elif not chunk:
                    selector.unregister(key.fd)
                elif key.data == "status":
                    if len(status) + len(chunk) > 16384:
                        raise ValueError("bubblewrap status exceeds protocol limit")
                    status.extend(chunk)
                    status_pending.extend(chunk)
                    while b"\n" in status_pending:
                        line, _, remainder = status_pending.partition(b"\n")
                        status_pending = bytearray(remainder)
                        item = json.loads(line)
                        if "child-pid" in item:
                            print(json.dumps({"event": "started", "namespaces": item}), flush=True)
                else:
                    total_bytes += len(chunk)
                    output.extend(chunk[:max(0, CAPTURE_BYTES - len(output))])
        code = process.wait()
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        selector.close()
        process.stdout.close()
        os.close(status_read)
    documents = [json.loads(line) for line in status.splitlines() if line.strip()]
    started = next((item for item in documents if "child-pid" in item), None)
    namespaces = ["mnt", "net", "pid", "ipc", "uts"]
    verified = started is not None and all(
        str(started.get(name + "-namespace")) != os.readlink("/proc/self/ns/" + name).split("[")[1].rstrip("]")
        and isinstance(started.get(name + "-namespace"), int)
        for name in namespaces
    )
    if not verified:
        outcome = "refused"
    return {
        "schema": 1, "backend": "bubblewrap", "status": outcome,
        "isolation_verified": verified, "namespaces": started,
        "exit_code": code, "output": output.decode("utf-8", errors="replace"),
        "output_bytes": total_bytes, "output_truncated": total_bytes > CAPTURE_BYTES,
        "workspace_bytes": WORKSPACE_BYTES, "tmp_bytes": TEMP_BYTES,
        "network": "denied", "host_writes": "none", "published_files": [],
    }


def main():
    try:
        request = bytearray()
        while True:
            char = os.read(sys.stdin.fileno(), 1)
            if not char or char == b"\n":
                break
            request.extend(char)
            if len(request) > 65536:
                raise ValueError("sandbox request exceeds protocol limit")
        result = execute(json.loads(request))
    except (OSError, ValueError, KeyError, TypeError) as error:
        result = {"schema": 1, "backend": "bubblewrap", "status": "refused",
                  "isolation_verified": False, "error": str(error)}
    print(json.dumps({"event": "result", "result": result}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
