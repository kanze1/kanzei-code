"""Isolated native runtime acceptance; never connects to an existing runtime.

Usage: python scripts/runtime-headless-smoke.py <fresh kzapp.exe>
Only the spawned fixture process is stopped. Model responses and all storage are local.
"""
import base64
import ctypes
from ctypes import wintypes
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import struct
import subprocess
import sys
import threading
import time


def until(predicate, label, timeout=45):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError(f"{label} timed out")


def process_surface(pid):
    user32, kernel = ctypes.windll.user32, ctypes.windll.kernel32
    windows = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
    user32.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
    user32.GetWindow.restype = wintypes.HWND
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]

    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid:
            name = ctypes.create_unicode_buffer(256)
            title = ctypes.create_unicode_buffer(256)
            rect = wintypes.RECT()
            user32.GetClassNameW(hwnd, name, len(name))
            user32.GetWindowTextW(hwnd, title, len(title))
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            visible = bool(user32.IsWindowVisible(hwnd))
            extended_style = user32.GetWindowLongPtrW(hwnd, -20) & 0xFFFFFFFF
            zero_size = rect.right == rect.left and rect.bottom == rect.top
            # Tao 0.35.3 event_loop.rs:649 requests a zero-sized layered,
            # transparent, non-activating tool window to dispatch OS messages.
            # Windows may enforce a minimum outer rect. Record its actual geometry.
            # Its private class, empty title and four styles identify its OS role.
            tao_event_target = (name.value == "Tao Thread Event Target" and not title.value
                                and extended_style & 0x080800A0 == 0x080800A0)
            ime_helper = name.value == "IME" and zero_size and not visible
            windows.append({"handle": hex(hwnd), "class": name.value, "title": title.value,
                            "visible": visible, "rect": [rect.left, rect.top, rect.right, rect.bottom],
                            "style": hex(user32.GetWindowLongPtrW(hwnd, -16) & 0xFFFFFFFF),
                            "extendedStyle": hex(extended_style), "owner": hex(user32.GetWindow(hwnd, 4) or 0),
                            "role": "tao_event_target" if tao_event_target else ("ime_helper" if ime_helper else "application")})
        return True

    user32.EnumWindows(visit, 0)

    class ProcessEntry(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD),
                    ("pid", wintypes.DWORD), ("heap", ctypes.c_size_t),
                    ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
                    ("parent", wintypes.DWORD), ("priority", wintypes.LONG),
                    ("flags", wintypes.DWORD), ("name", wintypes.WCHAR * 260)]

    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
    if snapshot == ctypes.c_void_p(-1).value:
        raise ctypes.WinError()
    entries = []
    try:
        entry = ProcessEntry()
        entry.size = ctypes.sizeof(entry)
        more = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            entries.append({"pid": entry.pid, "parent": entry.parent, "name": entry.name})
            more = kernel.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    owners = {pid}
    while True:
        expanded = owners | {entry["pid"] for entry in entries if entry["parent"] in owners}
        if expanded == owners:
            break
        owners = expanded
    class MemoryCounters(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("page_faults", wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in ("peak_working_set", "working_set", "quota_peak_paged",
            "quota_paged", "quota_peak_nonpaged", "quota_nonpaged", "pagefile", "peak_pagefile", "private")]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    handle = kernel.OpenProcess(0x410, False, pid)
    if not handle:
        raise ctypes.WinError()
    counters = MemoryCounters()
    counters.size = ctypes.sizeof(counters)
    try:
        ctypes.windll.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(MemoryCounters), wintypes.DWORD]
        if not ctypes.windll.psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.size):
            raise ctypes.WinError()
    finally:
        kernel.CloseHandle(handle)
    return {"windows": windows, "applicationWindows": [item for item in windows if item["role"] == "application"],
            "children": [entry for entry in entries if entry["pid"] in owners - {pid}],
            "workingSetBytes": counters.working_set, "privateBytes": counters.private}


def main():
    if os.name != "nt" or len(sys.argv) != 2:
        raise SystemExit("Pass a freshly built Windows kzapp.exe")
    root = Path("output/design-audit-20261016/round2/runtime-native") / str(time.time_ns())
    root = root.resolve()
    project, profile = root / "project", root / "profile"
    app_home = profile / ".kanzei"
    for directory in [project, app_home, profile / "AppData/Local", profile / "AppData/Roaming"]:
        directory.mkdir(parents=True)
    executable = root / "kzapp.exe"
    shutil.copy2(Path(sys.argv[1]).resolve(), executable)
    result = {"checks": [], "requests": [], "executable": str(executable),
              "sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
              "boundary": "Isolated runtime and storage; no real provider or application content window. All OS HWNDs and geometry are recorded, including Tao's layered non-activating event target and system IME helper."}
    seen = {mode: threading.Event() for mode in ["complete", "stop", "probe"]}
    release = threading.Event()

    class Model(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            last_user = next((m for m in reversed(body.get("messages", [])) if m.get("role") == "user"), {})
            last_user_text = json.dumps(last_user)
            mode = "probe" if "HEADLESS_PROBE" in last_user_text else ("stop" if "HEADLESS_STOP" in last_user_text else "complete")
            result["requests"].append({"mode": mode, "model": body.get("model")})
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            if mode == "probe":
                tool_receipt = next((message for message in body.get("messages", [])
                                     if message.get("tool_call_id") == "fixture-ui-probe"), None)
                if tool_receipt is None:
                    frame = {"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0,
                        "id": "fixture-ui-probe", "type": "function", "function": {
                            "name": "ui_dom", "arguments": json.dumps({"selector": "body"})}}]},
                        "finish_reason": None}]}
                    terminal = {"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}
                    self.wfile.write(("data: " + json.dumps(frame) + "\n\ndata: " + json.dumps(terminal)
                                      + "\n\ndata: [DONE]\n\n").encode())
                    self.wfile.flush()
                    return
                result["probeReceipt"] = tool_receipt.get("content")
                seen["probe"].set()
            content = {"complete": "HEADLESS_COMPLETE_OK", "stop": "HEADLESS_PARTIAL", "probe": "HEADLESS_PROBE_DONE"}[mode]
            frame = {"choices": [{"index": 0, "delta": {"content": content}, "finish_reason": None}]}
            self.wfile.write(("data: " + json.dumps(frame) + "\n\n").encode())
            self.wfile.flush()
            seen[mode].set()
            if mode == "stop":
                release.wait(60)
                return
            frame = {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                     "usage": {"prompt_tokens": 30, "completion_tokens": 4}}
            self.wfile.write(("data: " + json.dumps(frame) + "\n\ndata: [DONE]\n\n").encode())
            self.wfile.flush()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Model)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    (app_home / "app.json").write_text(json.dumps({"projects": [], "current": None}), encoding="utf-8")
    (app_home / "kanzei.toml").write_text(
        '[models]\nprimary = "stub:primary"\nfast = "stub:fast"\n[providers.stub]\n'
        f'protocol = "openai"\nbase_url = "http://127.0.0.1:{server.server_port}/v1"\ncontext_limit = 64000\n', encoding="utf-8")
    env = os.environ.copy()
    for name in ["KANZEI_EMBEDDED_RUNTIME", "KANZEI_E2E_CDP", "WEBVIEW2_USER_DATA_FOLDER"]:
        env.pop(name, None)
    env.update(KANZEI_HOME=str(app_home), USERPROFILE=str(profile), HOME=str(profile),
               LOCALAPPDATA=str(profile / "AppData/Local"), APPDATA=str(profile / "AppData/Roaming"))
    log = (root / "runtime.log").open("w", encoding="utf-8")
    started = time.monotonic()
    process = subprocess.Popen([str(executable), "--runtime-service"], cwd=project, env=env,
                               stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                               creationflags=subprocess.CREATE_NO_WINDOW)
    endpoint = None

    def check(value, label):
        if not value:
            raise AssertionError(label)
        result["checks"].append(label)

    def rpc(action, **payload):
        request = json.dumps({"action": action, "token": endpoint["token"], **payload}).encode()
        host, port = endpoint["addr"].rsplit(":", 1)
        with socket.create_connection((host, int(port)), timeout=60) as client:
            client.sendall(struct.pack(">I", len(request)) + request)

            def exact(length):
                chunks = bytearray()
                while len(chunks) < length:
                    part = client.recv(length - len(chunks))
                    if not part:
                        raise RuntimeError("runtime closed an incomplete response")
                    chunks.extend(part)
                return chunks

            reply = json.loads(exact(struct.unpack(">I", exact(4))[0]))
        if not reply["ok"]:
            raise RuntimeError(reply["value"])
        return reply["value"]

    def invoke(command, **args):
        return rpc("invoke", command=command, args=args)

    def rejects(command, expected, **args):
        try:
            invoke(command, **args)
        except RuntimeError as error:
            check(expected in str(error), f"{command} rejects the request: {expected}")
        else:
            raise AssertionError(f"{command} unexpectedly accepted invalid arguments or host")

    try:
        endpoint_path = until(lambda: next(app_home.glob("runtime/*/endpoint.json"), None), "native endpoint")
        endpoint = json.loads(endpoint_path.read_text(encoding="utf-8"))
        check(endpoint["pid"] == process.pid, "endpoint belongs to the isolated fixture PID")
        rpc("ping")
        result["readyMs"] = round((time.monotonic() - started) * 1000, 2)
        surface = process_surface(process.pid)
        result["surfaceBeforeRun"] = surface
        check(not surface["applicationWindows"], "runtime owns no application content window; OS message HWNDs are recorded")
        check(not any("webview" in item["name"].lower() for item in surface["children"]), "runtime starts no WebView2 child")
        rejects("preview_open", "此命令需要桌面界面")
        rejects("missing_fixture_command", "command is not available")
        rejects("process_list", "missing field")
        rejects("process_list", "unknown field", projectDir=str(project), mistyped=True)
        rejects("conversation_page", "invalid type", projectDir=str(project), limit="1")
        invoke("projects_init", path=str(project), name="Headless isolated acceptance")
        png = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a9WQAAAAASUVORK5CYII=")
        image_path = project / ".kanzei/artifacts/tool-images" / (hashlib.sha256(png).hexdigest() + ".png")
        image_path.parent.mkdir(parents=True, exist_ok=True)
        image_path.write_bytes(png)
        tool_image = invoke("tool_image", projectDir=str(project), rel=image_path.relative_to(project).as_posix())
        delivered_image = invoke("delivered_image", projectDir=str(project), path=str(image_path))
        check(base64.b64decode(tool_image["png"]) == png and base64.b64decode(delivered_image["png"]) == png,
              "tool and delivered images are readable natively without a preview window")
        owner = invoke("process_create", projectDir=str(project), profile="dev", model="stub:primary")
        common = {"projectDir": str(project), "processId": owner["id"]}

        def idle():
            return any(item["id"] == owner["id"] and not item["running"]
                       for item in invoke("process_list", projectDir=str(project)))

        invoke("run_prompt", **common, prompt="HEADLESS_COMPLETE", profile="dev", agent="dev-pair",
               model="stub:primary", autonomous=False, autoAllow=False)
        check(seen["complete"].wait(30), "real runner reached the isolated mock provider")
        until(idle, "completed owner")
        transcript = invoke("conversation_get", **common)
        check("HEADLESS_COMPLETE_OK" in json.dumps(transcript), "completed response is queryable from durable conversation")
        page = invoke("conversation_page", **common, limit=1)
        check(len(page["items"]) == 1 and not page["hasMore"], "native pagination returns metadata and an explicit end")
        invoke("conversation_clear", **common)
        invoke("run_prompt", **common, prompt="HEADLESS_STOP", profile="dev", agent="dev-pair",
               model="stub:primary", autonomous=False, autoAllow=False)
        check(seen["stop"].wait(30), "second real runner is blocked in a streaming provider request")
        check(any(item["id"] == owner["id"] and item["running"]
                  for item in invoke("process_list", projectDir=str(project))),
              "process query reports the actual running owner")
        invoke("run_prompt", **common, prompt="HEADLESS_QUEUED", delivery="queue", profile="dev",
               agent="dev-pair", model="stub:primary", autonomous=False, autoAllow=False)
        pending = invoke("list_pending_inputs", **common)
        queued = next((item for item in pending if item["prompt"] == "HEADLESS_QUEUED"), None)
        check(queued is not None, "native pending-input query returns a real admitted queued input")
        check(invoke("cancel_input", **common, inputId=queued["input_id"]),
              "queued input cancellation returns its durable receipt")
        invoke("stop_run", **common)
        until(idle, "stopped owner")
        release.set()
        page = invoke("conversation_page", **common, limit=1)
        check(len(page["items"]) == 1 and page["hasMore"] and page["nextBeforeSequence"] is not None,
              "native history page reports the older completed segment")
        older = invoke("conversation_page", **common, limit=1, beforeSequence=page["nextBeforeSequence"])
        check(len(older["items"]) == 1 and not older["hasMore"]
              and older["items"][0]["sequence"] < page["items"][0]["sequence"],
              "exclusive history cursor returns the older segment without duplication")
        probe_started = time.monotonic()
        invoke("run_prompt", **common, prompt="HEADLESS_PROBE", profile="dev", agent="dev-pair",
               model="stub:primary", autonomous=False, autoAllow=False)
        check(seen["probe"].wait(15), "a real runner receives its UI probe tool result")
        result["disconnectedProbeMs"] = round((time.monotonic() - probe_started) * 1000, 2)
        probe_receipt = str(result["probeReceipt"])
        check("桌面窗口未连接" in probe_receipt and "超时" not in probe_receipt,
              "a service UI probe without a desktop returns an explicit disconnection result")
        until(idle, "probe result committed")
        events = rpc("events", after=0)
        check(any(item["name"] == "kz:done" and item["payload"].get("outcome") == "completed"
                  for item in events["events"]), "application events reach the runtime journal without an application window")
        surface = process_surface(process.pid)
        result["surfaceAfterRun"] = surface
        check(not surface["applicationWindows"] and not any("webview" in item["name"].lower() for item in surface["children"]),
              "run, query and stop created no graphical runtime surface")
        receipt = rpc("shutdown")
        check(receipt.get("stopped") and receipt.get("state") == "quiescent", "shutdown acknowledges actual owner quiescence")
        check(process.wait(timeout=15) == 0, "isolated runtime exits cleanly after the shutdown receipt")
        database = sqlite3.connect((project / ".kanzei/state.db").as_uri() + "?mode=ro", uri=True)
        try:
            result["durableEventCounts"] = dict(database.execute("SELECT event_type, COUNT(*) FROM session_events GROUP BY event_type"))
            result["inputStates"] = dict(database.execute("SELECT status, COUNT(*) FROM session_inputs GROUP BY status"))
            kinds = set(result["durableEventCounts"])
            check("session.turn_completed" in kinds and "session.turn_stopped" in kinds,
                  "completed and stopped typed terminals survive runtime exit")
        finally:
            database.close()
        result["passed"] = True
    except Exception as error:
        result["passed"] = False
        result["error"] = str(error)
        raise
    finally:
        release.set()
        if process.poll() is None:
            # This handle is exclusively the fixture process created above.
            process.terminate()
            process.wait(timeout=15)
        server.shutdown()
        log.close()
        (root / "acceptance.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"artifact": str(root / "acceptance.json"), "passed": result.get("passed"),
                          "checks": len(result["checks"])}, ensure_ascii=False))


if __name__ == "__main__":
    main()
