"""Real native docs_snapshot + production JS resource performance, isolated Windows storage.

python scripts/resource-native-perf.py target/debug/kzapp.exe
No cargo, live runtime, provider request, user database, or original document write.
"""
import ctypes
from ctypes import wintypes
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time


def counters(pid):
    class Io(ctypes.Structure):
        _fields_ = [(name, ctypes.c_ulonglong) for name in
                    ("readOperations", "writeOperations", "otherOperations", "readBytes", "writeBytes", "otherBytes")]

    kernel = ctypes.windll.kernel32
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel.GetProcessIoCounters.argtypes = [wintypes.HANDLE, ctypes.POINTER(Io)]
    handle = kernel.OpenProcess(0x400, False, pid)
    if not handle:
        raise ctypes.WinError()
    created, exited, system, user = [wintypes.FILETIME() for _ in range(4)]
    io = Io()
    try:
        if not kernel.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(system), ctypes.byref(user)):
            raise ctypes.WinError()
        if not kernel.GetProcessIoCounters(handle, ctypes.byref(io)):
            raise ctypes.WinError()
        ms = lambda value: ((value.dwHighDateTime << 32) | value.dwLowDateTime) / 10000
        return {"cpuKernelMs": ms(system), "cpuUserMs": ms(user),
                **{name: getattr(io, name) for name, _ in Io._fields_}}
    finally:
        kernel.CloseHandle(handle)


def manifest(directory):
    return [{"path": path.relative_to(directory).as_posix(), "bytes": path.stat().st_size,
             "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            for path in sorted(directory.rglob("*")) if path.is_file() and path.suffix in {".md", ".jsonl"}]


def main():
    if os.name != "nt" or len(sys.argv) not in {2, 4}:
        raise SystemExit("Pass a freshly built Windows kzapp.exe [--phase all|core|events]")
    phase = sys.argv[3] if len(sys.argv) == 4 else "all"
    if phase not in {"all", "core", "events"} or (len(sys.argv) == 4 and sys.argv[2] != "--phase"):
        raise SystemExit("Expected --phase all|core|events")
    repository = Path(__file__).resolve().parent.parent
    output = repository / "output/design-audit-20261016/round2/resource-native"
    root = output / str(time.time_ns())
    project, profile = root / "project", root / "profile"
    home = profile / ".kanzei"
    for folder in [project, home, profile / "AppData/Local", profile / "AppData/Roaming"]:
        folder.mkdir(parents=True)
    documents = project / ".kanzei/project"
    source = repository / ".kanzei/project"
    source_files = []
    for path in sorted(source.rglob("*")):
        if not path.is_file() or path.suffix not in {".md", ".jsonl"}:
            continue
        data = path.read_bytes()
        relative = path.relative_to(source)
        destination = documents / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
        source_files.append({"path": relative.as_posix(), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    executable = root / "kzapp.exe"
    shutil.copy2(Path(sys.argv[1]).resolve(), executable)
    result = {"passed": False, "summary": "Measurement in progress", "fixture": str(root), "phase": phase,
              "source": {"root": str(source), "files": source_files, "fileCount": len(source_files),
                         "bytes": sum(item["bytes"] for item in source_files),
                         "manifestSha256": hashlib.sha256(json.dumps(source_files, sort_keys=True).encode()).hexdigest(),
                         "definition": "Exact bytes of every .md/.jsonl under current .kanzei/project; source locks, user state.db, Git history, research directories and artifact incidents are excluded."},
              "executableSha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
              "measurementBoundary": "Current debug build, warm OS and native DocStore caches, native TCP round trip including JSON. CPU and GetProcessIoCounters are native PID only. Read I/O is logical process I/O (including IPC), not physical disk. Production resource ESM executes in Node; browser event phase measures production handlers/render projection separately. Not an old-version comparison; no user DB/Git workload or provider execution is included."}
    modules = ["01-project-resources.js", "00-resource.js", "00-refresh.js", "00-ipc.js", "00-command-contract.js", "00-project-identity.js",
               "07-events.js", "14-docs-actions.js", "12-docs-pages.js", "11-docs-list.js", "00-event-subscriptions.js", "01-runtime-events.js"]
    result["productionModules"] = [{"path": f"crates/kanzei-app/ui/{name}", "sha256": hashlib.sha256((repository / "crates/kanzei-app/ui" / name).read_bytes()).hexdigest()} for name in modules]
    (home / "app.json").write_text(json.dumps({"projects": [], "current": None}), encoding="utf-8")
    (home / "kanzei.toml").write_text('[models]\nprimary="stub:primary"\nfast="stub:fast"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:9/v1"\ncontext_limit=64000\n', encoding="utf-8")
    env = os.environ.copy()
    for name in ["KANZEI_EMBEDDED_RUNTIME", "KANZEI_E2E_CDP", "WEBVIEW2_USER_DATA_FOLDER"]:
        env.pop(name, None)
    env.update(KANZEI_HOME=str(home), USERPROFILE=str(profile), HOME=str(profile),
               LOCALAPPDATA=str(profile / "AppData/Local"), APPDATA=str(profile / "AppData/Roaming"))
    helpers = runpy.run_path(str(repository / "scripts/runtime-headless-smoke.py"), run_name="fixture_helpers")
    log = (root / "runtime.log").open("w", encoding="utf-8")
    process = subprocess.Popen([str(executable), "--runtime-service"], cwd=project, env=env,
                               stdin=subprocess.DEVNULL, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
    server = None
    try:
        endpoint_path = helpers["until"](lambda: next(home.glob("runtime/*/endpoint.json"), None), "isolated endpoint")
        endpoint = json.loads(endpoint_path.read_text(encoding="utf-8"))
        assert endpoint["pid"] == process.pid, "endpoint must belong to fixture"

        class Metrics(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                data = json.dumps(counters(process.pid)).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        server = ThreadingHTTPServer(("127.0.0.1", 0), Metrics)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        result["surfaceBefore"] = helpers["process_surface"](process.pid)
        config = {"endpointPath": str(endpoint_path), "project": str(project), "output": str(root / "measurements.json"),
                  "metrics": f"http://127.0.0.1:{server.server_port}", "repository": str(repository), "phase": phase}
        config_path = root / "config.json"
        config_path.write_text(json.dumps(config), encoding="utf-8")
        completed = subprocess.run(["node", str(repository / "scripts/resource-native-perf.mjs"), str(config_path)], cwd=repository, check=False)
        if (root / "measurements.json").exists():
            result.update(json.loads((root / "measurements.json").read_text(encoding="utf-8")))
        assert completed.returncode == 0 and result.get("passed"), "production resource measurement failed"
        result["normalizedFixtureFiles"] = manifest(documents)
        result["surfaceAfter"] = helpers["process_surface"](process.pid)
        assert not result["surfaceAfter"]["applicationWindows"]
        assert not any("webview" in item["name"].lower() for item in result["surfaceAfter"]["children"])
        request = json.dumps({"action": "shutdown", "token": endpoint["token"]}).encode()
        host, port = endpoint["addr"].rsplit(":", 1)
        with socket.create_connection((host, int(port)), timeout=15) as client:
            client.sendall(struct.pack(">I", len(request)) + request)
            def exact(size):
                data = bytearray()
                while len(data) < size:
                    part = client.recv(size - len(data))
                    if not part:
                        raise RuntimeError("Incomplete shutdown receipt")
                    data.extend(part)
                return data
            receipt = json.loads(exact(struct.unpack(">I", exact(4))[0]))
        assert receipt["ok"] and receipt["value"].get("state") == "quiescent", receipt
        assert process.wait(timeout=15) == 0
        result["shutdown"] = receipt["value"]
    except Exception as error:
        result["passed"] = False
        if "error" in result:
            result["measurementError"] = result["error"]
        result["error"] = str(error)
        raise
    finally:
        if process.poll() is None:
            process.terminate()  # Only the exclusively owned fixture process.
            process.wait(timeout=15)
        if server:
            server.shutdown()
        log.close()
        executable.unlink(missing_ok=True)
        data = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
        (root / "acceptance.json").write_text(data, encoding="utf-8")
        if phase in {"all", "core"}:
            (output / "core-acceptance.json").write_text(data, encoding="utf-8")
        if phase == "events":
            (output / "events-acceptance.json").write_text(data, encoding="utf-8")
            core_path = output / "core-acceptance.json"
            if core_path.is_file():
                core = json.loads(core_path.read_text(encoding="utf-8"))
                aggregate = {**core, "passed": core["passed"] and result["passed"],
                             "phase": "composed",
                             "coreEvidence": str(core_path), "browserEventEvidence": str(root / "acceptance.json"),
                             "browserFinal": result, "summary": core["summary"] + " Final event replay: " + result["summary"]}
                aggregate["phases"] = {**core.get("phases", {}), **result.get("phases", {})}
                data = json.dumps(aggregate, ensure_ascii=False, indent=2) + "\n"
        (output / "acceptance.json").write_text(data, encoding="utf-8")
        print(json.dumps({"artifact": str(output / "acceptance.json"), "passed": result["passed"], "summary": result["summary"]}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
