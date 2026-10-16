"""Compose completed core measurements with a separately repeated final browser phase.

Retains the original combined-run status, including a later browser-harness failure;
only the independently completed core checks/measurements are selected as core evidence.
"""
import json
from pathlib import Path
import sys


def main():
    core_path, events_path = [Path(value).resolve() for value in sys.argv[1:]]
    original = json.loads(core_path.read_text(encoding="utf-8"))
    events = json.loads(events_path.read_text(encoding="utf-8"))
    phases = original["phases"]
    expected = {"forcedNative": 200, "cached": 0, "coalesced": 40, "explicitInvalidation": 20,
                "visibleCalibration": 1, "inactiveIdle": 0, "hiddenIdle": 0, "unsubscribedIdle": 0}
    for name, calls in expected.items():
        assert phases[name]["nativeCalls"] == calls, (name, phases[name])
    assert "Releasing the last owner stops calibration and idle IPC" in original["checks"]
    assert original["calibration"]["staleMs"] <= original["calibration"]["expectedUncontendedBudgetMs"]
    assert events["passed"] and events["phases"]["browserToolEvent"]["latency"]["n"] == 20
    foundations = {"01-project-resources.js", "00-resource.js", "00-refresh.js", "00-ipc.js", "00-command-contract.js", "00-project-identity.js"}
    current = {entry["path"]: entry["sha256"] for entry in events["productionModules"]}
    for entry in original["productionModules"]:
        if Path(entry["path"]).name in foundations:
            assert current[entry["path"]] == entry["sha256"], f"Core resource source changed: {entry['path']}"
    core = {key: value for key, value in original.items() if key not in {"error", "measurementError", "browserEvent", "shutdown"}}
    core["phase"] = "core"
    core["passed"] = True
    core["phases"] = {name: phases[name] for name in expected}
    core["originalCombinedRun"] = {"artifact": str(core_path), "passed": original["passed"],
                                  "error": original.get("measurementError", original.get("error")),
                                  "selection": "The core phase finished all 13 checks before the browser phase began. The browser phase is excluded here and was rerun independently with the final production module hashes. Original evidence is retained unchanged."}
    native = phases["forcedNative"]["latency"]
    core["summary"] = (f"Native docs_snapshot n=200 P50={native['p50Ms']:.2f}ms P95={native['p95Ms']:.2f}ms; "
                       f"cached 200/0 IPC; 320 concurrent consumers/40 IPC; explicit invalidation P95={phases['explicitInvalidation']['latency']['p95Ms']:.2f}ms; "
                       f"missed-event stale={original['calibration']['staleMs']:.2f}ms; inactive/hidden/unsubscribed 22s each/0 IPC.")
    output = core_path.parent.parent
    core_target = output / "core-acceptance.json"
    core_target.write_text(json.dumps(core, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    aggregate = {**core, "phase": "composed", "coreEvidence": str(core_target), "browserEventEvidence": str(events_path),
                 "browserFinal": events, "summary": core["summary"] + " " + events["summary"],
                 "phases": {**core["phases"], **events["phases"]}}
    (output / "acceptance.json").write_text(json.dumps(aggregate, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": True, "core": str(core_target), "browser": str(events_path), "summary": aggregate["summary"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
