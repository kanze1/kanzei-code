"""Exercise actual CLI/schedule admission with an isolated provider and SQLite."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import threading


class Provider:
    def __init__(self):
        self.requests = []
        self.lock = threading.Lock()
        provider = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers["Content-Length"])
                request = json.loads(self.rfile.read(length))
                with provider.lock:
                    provider.requests.append(request)
                    number = len(provider.requests)
                body = (
                    "data: "
                    + json.dumps({"choices": [{"index": 0, "delta": {"content": f"done-{number}"}, "finish_reason": "stop"}]})
                    + "\n\ndata: [DONE]\n\n"
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)


def invoke(binary, arguments, project, environment):
    child = subprocess.run(
        [str(binary), *arguments], cwd=project, env=environment,
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=40,
    )
    return {
        "exit_code": child.returncode,
        "stdout": child.stdout.decode(errors="replace"),
        "stderr": child.stderr.decode(errors="replace"),
    }


def run(binary, scenario):
    provider = Provider()
    try:
        with tempfile.TemporaryDirectory(prefix="kz-c6-admission-") as temporary:
            home = Path(temporary)
            project = home / "project"
            state_dir = project / ".kanzei"
            schedules = state_dir / "schedules"
            schedules.mkdir(parents=True)
            port = provider.server.server_address[1]
            (state_dir / "kanzei.toml").write_text(
                'proxy = "off"\n[models]\nprimary = "mock:test-model"\n'
                '[providers.mock]\nprotocol = "openai"\n'
                f'base_url = "http://127.0.0.1:{port}/v1"\n',
                encoding="utf-8",
            )
            environment = dict(
                os.environ, HOME=str(home), USERPROFILE=str(home),
                KANZEI_HOME=str(home / ".kanzei"), KANZEI_MODEL="mock:test-model",
                KANZEI_PROXY="off", KANZEI_AGENT="readonly", KANZEI_PROFILE="readonly",
            )
            environment.pop("KANZEI_PROJECT_ROOT", None)
            cli = ["run", "--readonly", "--no-subagents", "--project-root", str(project)]
            # Create the real schema through the actual binary and retain its
            # normal 200/Completed control before any fault is installed.
            bootstrap = invoke(binary, [*cli, "bootstrap normal input"], project, environment)
            database = state_dir / "state.db"
            with sqlite3.connect(database) as connection:
                bootstrap_completed = connection.execute(
                    "SELECT COUNT(*) FROM session_events WHERE event_type='session.turn_completed'"
                ).fetchone()[0]
                boundary = connection.execute("SELECT COALESCE(MAX(rowid),0) FROM session_events").fetchone()[0]
                rejected_fact = {
                    "cli-reject": "session.user_message_committed",
                    "cli-double-reject": "session.user_message_committed",
                    "schedule-reject": "session.user_message_committed",
                    "schedule-double-reject": "session.user_message_committed",
                    "schedule-steering-reject": "session.steering_message_committed",
                }.get(scenario)
                if rejected_fact:
                    connection.execute(
                        "CREATE TRIGGER reject_admission BEFORE INSERT ON session_events "
                        f"WHEN NEW.event_type='{rejected_fact}' "
                        "BEGIN SELECT RAISE(ABORT,'injected admission rejection'); END;"
                    )
                if "double-reject" in scenario:
                    connection.execute(
                        "CREATE TRIGGER reject_failed_admission BEFORE INSERT ON session_events "
                        "WHEN NEW.event_type='session.turn_failed' "
                        "BEGIN SELECT RAISE(ABORT,'injected failed admission outcome'); END;"
                    )
            connection.close()
            before_posts = len(provider.requests)
            if scenario.startswith("cli-"):
                result = invoke(binary, [*cli, "actual tested input"], project, environment)
                outcome = None
            else:
                two = scenario in ("schedule-two-prompt", "schedule-steering-reject")
                (schedules / "admission.md").write_text(
                    "---\nname: admission\nenabled: true\nwhen: 每 15 分钟\n"
                    "host: app\nagent: readonly\nmodel: primary\ntimeout: 30s\n"
                    "max_steps: 2\nsteps:\n  - prompt: first legal prompt\n"
                    + ("  - prompt: second legal prompt\n" if two else "")
                    + "---\nAdmission fixture\n",
                    encoding="utf-8",
                )
                result = invoke(binary, ["schedule", "run", "admission", "--project-root", str(project)], project, environment)
                outcome = json.loads(result["stdout"])
            with sqlite3.connect(database) as connection:
                if outcome:
                    session = outcome["run_session_id"]
                    rows = connection.execute(
                        "SELECT event_type,payload_json FROM session_events WHERE session_id=? ORDER BY sequence", (session,)
                    ).fetchall()
                else:
                    rows_with_session = connection.execute(
                        "SELECT session_id,event_type,payload_json FROM session_events WHERE rowid>? ORDER BY rowid", (boundary,)
                    ).fetchall()
                    session = next((row[0] for row in rows_with_session if row[1] in ("run.failed", "run.completed")), rows_with_session[0][0])
                    rows = [(kind, payload) for sid, kind, payload in rows_with_session if sid == session]
                status = connection.execute("SELECT status FROM sessions WHERE session_id=?", (session,)).fetchone()[0]
                inputs = connection.execute("SELECT input_id,prompt,status FROM session_inputs WHERE session_id=? ORDER BY created_at", (session,)).fetchall()
                receipts = [json.loads(row[0]) for row in connection.execute(
                    "SELECT payload_json FROM session_events WHERE event_type='schedule.run_finished' AND rowid>?", (boundary,)
                )]
            connection.close()
            facts = [{"type": kind, "payload": json.loads(payload)} for kind, payload in rows]
            evidence = {
                "scenario": scenario, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "bootstrap": bootstrap, "bootstrap_completed": bootstrap_completed,
                "bootstrap_posts": before_posts, "tested_posts": len(provider.requests) - before_posts,
                "provider_requests": provider.requests[before_posts:], "result": result,
                "session_id": session, "session_status": status, "inputs": inputs,
                "outcome": outcome, "facts": facts, "receipts": receipts,
            }
            return evidence
    finally:
        provider.close()


def verify(evidence):
    scenario = evidence["scenario"]
    facts = evidence["facts"]
    count = lambda kind: sum(fact["type"] == kind for fact in facts)
    assert evidence["bootstrap"]["exit_code"] == 0, "normal CLI bootstrap failed"
    assert evidence["bootstrap_completed"] == 1
    assert evidence["bootstrap_posts"] == 1
    assert all(request["model"] == "test-model" for request in evidence["provider_requests"])
    rejected = "reject" in scenario
    if rejected:
        expected_posts = 1 if scenario == "schedule-steering-reject" else 0
        double = "double-reject" in scenario
        assert evidence["tested_posts"] == expected_posts, "rejected admission reached the real provider"
        assert evidence["result"]["exit_code"] != 0
        assert evidence["session_status"] == ("idle" if scenario == "cli-double-reject" else "failed")
        assert count("session.turn_failed") == (0 if double else 1)
        assert count("session.turn_completed") == 0
        assert count("session.turn_stopped") == 0
        assert count("session.steering_message_committed") == 0
        assert count("session.user_message_committed") == expected_posts
        assert count("session.assistant_message_committed") == expected_posts
        if scenario.startswith("cli-"):
            assert count("run.failed") == (0 if double else 1)
            assert next(row[2] for row in evidence["inputs"] if row[1] == "actual tested input") == ("running" if double else "failed")
            assert "injected admission rejection" in evidence["result"]["stderr"]
            if double:
                assert "injected failed admission outcome" in evidence["result"]["stderr"]
        else:
            assert evidence["outcome"]["ok"] is False
            assert "injected admission rejection" in evidence["outcome"]["error"]
            assert evidence["receipts"][0]["ok"] is False
    elif scenario == "schedule-two-prompt":
        assert evidence["tested_posts"] == 2
        assert evidence["result"]["exit_code"] == 0
        assert evidence["outcome"]["ok"] is True
        assert evidence["receipts"][0]["ok"] is True
        assert count("session.user_message_committed") == 1
        assert count("session.steering_message_committed") == 1
        assistants = [fact["payload"]["step_id"] for fact in facts if fact["type"] == "session.assistant_message_committed"]
        assert assistants == [1, 2], "local runner step 1 must map to two distinct durable steps"
        assert count("session.turn_completed") == 1
        assert count("session.turn_failed") == count("session.turn_stopped") == 0
        assert evidence["session_status"] == "completed"
        second = json.dumps(evidence["provider_requests"][1]["messages"])
        assert "first legal prompt" in second and "second legal prompt" in second and "done-2" in second
    else:
        assert evidence["tested_posts"] == 1
        assert evidence["result"]["exit_code"] == 0
        assert count("session.user_message_committed") == count("session.assistant_message_committed") == 1
        assert count("session.turn_completed") == 1
        assert count("session.turn_failed") == count("session.turn_stopped") == 0
        assert evidence["session_status"] == "idle"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    scenarios = ["cli-accept", "cli-reject", "cli-double-reject", "schedule-reject", "schedule-double-reject", "schedule-two-prompt", "schedule-steering-reject"]
    parser.add_argument("--scenario", choices=[*scenarios, "all"], default="all")
    args = parser.parse_args()
    evidence = [run(args.binary.resolve(), scenario) for scenario in (scenarios if args.scenario == "all" else [args.scenario])]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(evidence, ensure_ascii=False), flush=True)
    for result in evidence:
        verify(result)
