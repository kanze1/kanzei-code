"""Run the public schedule executor with a temporary provider and SQLite fault."""

import argparse
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile


def run(binary: Path, reject: bool) -> dict:
    with tempfile.TemporaryDirectory(prefix="kz-schedule-terminal-") as temporary:
        home = Path(temporary)
        project = home / "project"
        schedules = project / ".kanzei/schedules"
        schedules.mkdir(parents=True)
        result_file = project / "result.txt"
        result_file.write_text("original committed output", encoding="utf-8")
        (schedules / "terminal.md").write_text(
            "---\nname: terminal\nenabled: true\nwhen: 每 15 分钟\n"
            "host: app\nagent: readonly\nmodel: primary\ntimeout: 30s\n"
            "max_steps: 2\nsteps:\n  - prompt: return a short result\n"
            "writeback:\n  - notify\n  - file: result.txt\n---\nTerminal fixture\n",
            encoding="utf-8",
        )
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(30)
            port = listener.getsockname()[1]
            (project / ".kanzei/kanzei.toml").write_text(
                f'proxy = "off"\n[models]\nprimary = "mock:test-model"\n'
                '[providers.mock]\nprotocol = "openai"\n'
                f'base_url = "http://127.0.0.1:{port}/v1"\n',
                encoding="utf-8",
            )
            # The executor layers global config too. Redirect the child's entire
            # home; never load the user's configuration, login or production DB.
            environment = dict(
                os.environ,
                HOME=str(home),
                USERPROFILE=str(home),
                KANZEI_HOME=str(home / ".kanzei"),
                KANZEI_MODEL="mock:test-model",
                KANZEI_PROXY="off",
            )
            environment.pop("KANZEI_PROJECT_ROOT", None)
            child = subprocess.Popen(
                [str(binary), "schedule", "run", "terminal", "--project-root", str(project)],
                cwd=project,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            try:
                with listener.accept()[0] as client:
                    client.settimeout(30)
                    request = b""
                    while b"\r\n\r\n" not in request:
                        piece = client.recv(4096)
                        assert piece, "provider request closed before HTTP headers"
                        request += piece
                    head, payload = request.split(b"\r\n\r\n", 1)
                    length = next(
                        int(line.split(b":", 1)[1])
                        for line in head.split(b"\r\n")
                        if line.lower().startswith(b"content-length:")
                    )
                    while len(payload) < length:
                        piece = client.recv(4096)
                        assert piece, "provider request closed before HTTP body"
                        payload += piece
                    provider_request = json.loads(payload[:length])
                    assert provider_request["model"] == "test-model"
                    database = project / ".kanzei/state.db"
                    with sqlite3.connect(database) as connection:
                        active_facts = connection.execute(
                            "SELECT COUNT(*) FROM session_events WHERE event_type='session.turn_started'"
                        ).fetchone()[0]
                        assert active_facts == 1, "the real typed turn must precede the provider barrier"
                        if reject:
                            connection.execute(
                                "CREATE TRIGGER reject_schedule_terminal BEFORE INSERT ON session_events "
                                "WHEN NEW.event_type='session.turn_completed' "
                                "BEGIN SELECT RAISE(ABORT, 'injected schedule terminal rejection'); END;"
                            )
                    # Close the fault-injection connection before releasing LLM.
                    connection.close()
                    body = (
                        "data: "
                        + json.dumps({"choices": [{"index": 0, "delta": {"content": "done"}, "finish_reason": "stop"}]})
                        + "\n\ndata: [DONE]\n\n"
                    ).encode()
                    client.sendall(
                        f"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode()
                        + body
                    )
                stdout, stderr = child.communicate(timeout=30)
                # A second unexpected provider request cannot silently count as
                # our terminal path. It would time out rather than receive data.
                outcome = json.loads(stdout)
                with sqlite3.connect(database) as connection:
                    facts = connection.execute(
                        "SELECT event_type, payload_json FROM session_events WHERE session_id=? ORDER BY sequence",
                        (outcome["run_session_id"],),
                    ).fetchall()
                    status = connection.execute(
                        "SELECT status FROM sessions WHERE session_id=?", (outcome["run_session_id"],)
                    ).fetchone()[0]
                    run_receipts = [
                        json.loads(row[0])
                        for row in connection.execute(
                            "SELECT payload_json FROM session_events WHERE event_type='schedule.run_finished'"
                        )
                    ]
                connection.close()
                evidence = {
                    "scenario": "reject" if reject else "accept",
                    "exit_code": child.returncode,
                    "outcome_ok": outcome["ok"],
                    "outcome_error": outcome["error"],
                    "session_status": status,
                    "completed_facts": sum(kind == "session.turn_completed" for kind, _ in facts),
                    "assistant_facts": sum(kind == "session.assistant_message_committed" for kind, _ in facts),
                    "stopped_facts": sum(kind == "session.turn_stopped" for kind, _ in facts),
                    "failed_facts": sum(kind == "session.turn_failed" for kind, _ in facts),
                    "failed_payloads": [json.loads(payload) for kind, payload in facts if kind == "session.turn_failed"],
                    "run_receipts": [{"ok": receipt["ok"], "error": receipt["error"]} for receipt in run_receipts],
                    "file_text": result_file.read_text(encoding="utf-8"),
                    "writeback": outcome["writeback"],
                    "stderr": stderr.decode(errors="replace"),
                }
            except Exception as error:
                if child.poll() is None:
                    child.kill()
                stdout, stderr = child.communicate(timeout=30)
                raise RuntimeError(
                    f"fixture failed: {error}; child exit={child.returncode}; "
                    f"stdout={stdout.decode(errors='replace')!r}; stderr={stderr.decode(errors='replace')!r}"
                ) from error
            finally:
                if child.poll() is None:
                    child.kill()
                    child.communicate(timeout=30)
        return evidence


def verify(evidence: dict) -> None:
    assert evidence["assistant_facts"] == 1, "terminal rejection must not reject the earlier assistant commit"
    assert len(evidence["run_receipts"]) == 1
    if evidence["scenario"] == "reject":
        assert evidence["outcome_ok"] is False, "rejected typed Completed must not become a successful schedule outcome"
        assert evidence["exit_code"] != 0
        assert evidence["session_status"] == "failed"
        assert evidence["completed_facts"] == 0
        assert evidence["failed_facts"] == 1
        assert evidence["stopped_facts"] == 0
        assert "injected schedule terminal rejection" in json.dumps(evidence["failed_payloads"])
        assert evidence["run_receipts"][0]["ok"] is False
        assert "injected schedule terminal rejection" in evidence["outcome_error"]
        assert evidence["file_text"] == "original committed output", "failed run must retain the committed output"
        assert next(item for item in evidence["writeback"] if item["channel"] == "file:result.txt")["ok"] is False
    else:
        assert evidence["outcome_ok"] is True
        assert evidence["exit_code"] == 0
        assert evidence["session_status"] == "completed"
        assert evidence["completed_facts"] == 1
        assert evidence["failed_facts"] == 0
        assert evidence["stopped_facts"] == 0
        assert evidence["run_receipts"][0]["ok"] is True
        assert evidence["file_text"] == "done"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--scenario", choices=["accept", "reject", "both"], default="both")
    args = parser.parse_args()
    scenarios = [False, True] if args.scenario == "both" else [args.scenario == "reject"]
    evidence = [run(args.binary.resolve(), reject) for reject in scenarios]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(evidence, ensure_ascii=False), flush=True)
    for result in evidence:
        verify(result)
