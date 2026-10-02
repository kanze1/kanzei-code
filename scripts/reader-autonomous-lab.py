"""Run one real-model reader decision through the CLI, with isolated state and memory.

Uses the configured provider; credentials remain in child environment variables.
Usage: python scripts/reader-autonomous-lab.py <reader-root> <lab-directory>
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import time
import tomllib

repo = Path(__file__).resolve().parents[1]
source = Path(sys.argv[1]).resolve()
lab = Path(sys.argv[2]).resolve()
project = lab / "decision-project"
runtime = lab / "decision-runtime"
(project / ".kanzei").mkdir(parents=True, exist_ok=True)
runtime.mkdir(parents=True, exist_ok=True)

# Read routing only, never copy personal agents or memory into the experiment.
config_path = Path(os.environ.get("KANZEI_HOME", str(Path.home() / ".kanzei"))) / "kanzei.toml"
config = tomllib.loads(config_path.read_text(encoding="utf-8-sig")) if config_path.exists() else {}
env = dict(os.environ, KANZEI_HOME=str(runtime), KANZEI_PROJECT_ROOT=str(project), KANZEI_AGENT="readonly")
if config.get("proxy"):
    env["KANZEI_PROXY"] = config["proxy"]
lines = ["[models]"]
for key, value in config.get("models", {}).items():
    if isinstance(value, (str, bool, int)):
        lines.append(f"{key} = {json.dumps(value, ensure_ascii=False)}")
for index, (name, values) in enumerate(config.get("providers", {}).items()):
    provider = dict(values)
    if provider.get("api_key"):
        key = f"KANZEI_READER_LAB_KEY_{index}"
        env[key] = provider.pop("api_key")
        provider["api_key_env"] = key
    lines.append(f"\n[providers.{json.dumps(name)}]")
    for key, value in provider.items():
        if isinstance(value, (str, bool, int)):
            lines.append(f"{key} = {json.dumps(value, ensure_ascii=False)}")
lines += ["\n[limits]", "max_tokens = 2048", "transport_retries = 0", "rate_limit_retries = 0", "stream_restarts = 0"]
(runtime / "kanzei.toml").write_text("\n".join(lines), encoding="utf-8")
prompt = """这是一次已授权的阅读器自主决策协议实验。只完成本题，不推进 backlog，不修改源码，不运行命令，不创建子代理，不写用户偏好。
请先调用 question，问题固定为“同名图片再次导入时是否覆盖旧附件？”，options 为“覆盖旧附件”和“保留两份原件”，default 为“覆盖旧附件”。第一次故意不填 decision，用来验证旧式提问在自动模式下如何恢复。
收到 DECISION_REQUIRED 后，使用 read 查看 lib/src/data/attachment_service.dart，并保持相同问题再次调用 question，填写 decision 的 answer、rationale、impact，只给可供用户复核的简短理由，不输出内部思维过程。根据现有实现和原件保留目标决定，不机械采用 default。
成功记账后立刻给一句结论，结束本轮。不要向用户追问。
"""
prompt_file = project / "prompt.txt"
prompt_file.write_text(prompt, encoding="utf-8")
command = [str(repo / "target/debug/kz.exe"), "run", "--new", "--readonly", "--autonomous", "--no-subagents", "--prompt-file", str(prompt_file)]
started = time.monotonic()
timed_out = False
with (lab / "decision-model.log").open("w", encoding="utf-8") as log:
    child = subprocess.Popen(command, cwd=source, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                             creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        code = child.wait(timeout=180)
    except subprocess.TimeoutExpired:
        timed_out = True
        child.kill()
        code = child.wait(timeout=10)

db = project / ".kanzei/state.db"
decisions = []
if db.exists():
    with sqlite3.connect(f"{db.as_uri()}?mode=ro", uri=True) as connection:
        columns = [row[1] for row in connection.execute("pragma table_info(session_events)")]
        payload_column = "payload_json" if "payload_json" in columns else "payload"
        type_column = "event_type" if "event_type" in columns else "kind"
        for row in connection.execute(f"select {payload_column} from session_events where {type_column} = ? order by sequence", ("decision.updated",)):
            decisions.append(json.loads(row[0]))
latest = decisions[-2:]
protocol_passed = (code == 0 and not timed_out and len(latest) == 2
                   and [item["status"] for item in latest] == ["deciding", "decided"]
                   and latest[0]["id"] == latest[1]["id"]
                   and latest[0]["run_id"] == latest[1]["run_id"]
                   and latest[1]["resolution"]["answer"] == "保留两份原件"
                   and latest[1]["review"] is None)
result = {"exit_code": code, "timed_out": timed_out, "protocol_passed": protocol_passed, "duration_ms": round((time.monotonic() - started) * 1000),
          "source": str(source), "project": str(project), "isolated_memory": str(runtime), "decisions": decisions}
(lab / "decision-model.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({"exit_code": code, "timed_out": timed_out, "protocol_passed": protocol_passed, "decision_events": len(decisions), "result": str(lab / "decision-model.json")}, ensure_ascii=False))
sys.exit(0 if protocol_passed else 1)
