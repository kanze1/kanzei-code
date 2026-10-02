// 仅用于真实前端的可交互预览/回归。全部写入停留在此 fixture，绝不访问真实项目。
export function decisionConsoleFixture(snapshot) {
  const state = structuredClone(snapshot);
  state.observed_at = Date.now();
  const choice = (p, n, question, answer, unit) => ({
    id: `dec-${String(n).padStart(64, "0")}`, revision: n, project: p.path,
    session_id: `session-${n}`, process_id: p.lines?.[0]?.id ?? `p${n}|${p.path}`,
    run_id: `preview-run-${n}`, call_id: `preview-call-${n}`, question,
    options: [{ label: answer, note: "示例选择" }], work_unit_id: unit,
    status: "decided", resolution: { answer, rationale: "复用当前系统已有能力，控制改动范围。", impact: "沿用现有存储与调用入口，后续可以按你的反馈调整。", preference_refs: [] },
    missing_fact: null, review: null, created_at: Date.now() - n * 60000,
  });
  state.projects.forEach((p, index) => {
    p.error = null;
    p.work_acceptances = [];
    p.rework = {};
    p.verification_jobs = index === 0 ? [{ id: "v-preview-1", unit_id: "R-379/W1", status: "running", source_fingerprint: "example-source-fingerprint", environment: "Preview only", log_path: "C:/example/verification/output.log", error: null }] : [];
    p.work_units = [{ format_version: 1, unit_id: `R-${379 + index}/W1`, requirement_id: `R-${379 + index}`,
      objective: index === 0 ? "自主决策与批量复核接入" : "Reader 搜索结果可用版本", scope: ["src"], dependencies: [],
      acceptance: ["核心路径有验证证据"], verification: ["targeted integration test"], base_revision: "preview-21c36e9d",
      status: index === 0 ? "verifying" : "done", claimed_by: p.lines?.[0]?.id ?? null, blocked_reason: null,
      last_checkpoint: null, evidence: index === 0 ? [] : [{ criterion: "核心路径有验证证据", evidence_refs: ["T-preview-1"] }],
      // Explicitly different times keep newest-first order deterministic and different from project order.
      terminal_reason: null, source_sequence: 8, created_at: Date.now() - 3600000, updated_at: state.observed_at - 120000 + index * 1000 }];
    if (index === 0) p.work_units[0].background_verification = { job_id: "v-preview-1", snapshot_fingerprint: "example-source-fingerprint", pending: true };
    p.decisions = [choice(p, index + 1, ["普通选择需要停下来问用户吗？", "搜索结果用分页还是连续滚动？", "新功能优先复用现有服务吗？"][index], ["自主决定并留档，继续执行", "分页展示，保留筛选条件", "先接入现有服务"][index], p.work_units[0].unit_id)];
  });
  state.projects[0].decisions.push(choice(state.projects[0], 9, "图谱使用现有渲染库吗？", "复用 force-graph，增加展开与关系动画", null));
  const receipt = d => ({ decision: structuredClone(d), preference_error: null,
    delivery: d.review?.rework_input_id ? {status:"queued",input_id:d.review.rework_input_id,session_id:d.session_id,error:null} : null });
  const review = ({ projectDir, decisionId, review: r }) => {
    const project = state.projects.find((p) => p.path === projectDir);
    const d = project?.decisions.find((item) => item.id === decisionId);
    if (!d) throw new Error("decision not found");
    if (d.review?.request_id === r.request_id) return receipt(d);
    if (d.revision !== r.expected_revision) throw new Error("decision changed; refresh before reviewing");
    if (r.action === "correct" && !r.feedback.trim()) throw new Error("feedback is required");
    if (r.action === "accept" && r.scope !== "once") throw new Error("accept does not create preferences");
    d.revision += 1;
    const input = r.action === "correct" ? `queue-${r.request_id}` : null;
    d.review = { request_id: r.request_id, action: r.action, feedback: r.feedback, scope: r.scope,
      reviewed_at: Date.now(), rework_input_id: input, preference_id: r.scope === "once" ? null : `${r.scope === "global" ? "GM" : "M"}-preview-${d.revision}` };
    if (input) project.rework[input] = "pending";
    return receipt(d);
  };
  return {
    workspace_snapshot: () => ({ ...structuredClone(state), observed_at: Date.now() }),
    decision_review: review,
    verification_cancel: ({ projectDir, jobId }) => {
      const p = state.projects.find((item) => item.path === projectDir);
      const job = p?.verification_jobs.find((item) => item.id === jobId);
      if (!job) throw new Error("verification not found");
      job.status = "cancelled";
      const unit = p.work_units.find((item) => item.unit_id === job.unit_id);
      unit.status = "blocked";
      unit.background_verification.pending = false;
      return null;
    },
    work_delivery_accept: ({ projectDir, unitId, sourceSequence }) => {
      const p = state.projects.find((item) => item.path === projectDir);
      const u = p?.work_units.find((item) => item.unit_id === unitId);
      if (!u || u.status !== "done" || u.source_sequence !== sourceSequence) throw new Error("delivery changed");
      if (p.decisions.some((d) => d.work_unit_id === unitId && d.review?.action === "correct")) throw new Error("needs revalidation");
      const result = { unit_id: unitId, source_sequence: sourceSequence, accepted_at: Date.now() };
      p.work_acceptances.push(result);
      return result;
    },
  };
}
