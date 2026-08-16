// 研究工作台(R-276 批3)。
//
// 为什么独立成一个主视图,而不是继续在侧栏那两条列表上打补丁:侧栏一列一百多像素,
// 论文标题要换三行、按钮往哪放都别扭,还得和 req/defect 共用一套渲染分支——前两轮
// 补丁(D-413/D-414)每轮都出新问题,那本身就是「地方不对」的信号。研究工件是研究
// 模式的核心资产,值得一块自己的地方:左边卡片流(来源/发现),右边报告正文。
//
// 设计依据 docs/design/research_workspace.md:结果>过程(报告是主角)、溯源冗余
// (卡片里能开、报告里能跳)、数据已结构化的不许降级成字符串。

let researchTab = "sources";
let researchSnapshot = { sources: [], findings: [] };

/// 取字段值(大小写与中英别名都认;取不到给空串)。
function researchField(entry, ...names) {
  const wanted = names.map((n) => n.toLowerCase());
  const hit = (entry.fields ?? []).find(([k]) => wanted.includes(String(k).toLowerCase()));
  return hit ? String(hit[1]) : "";
}

/// 一张来源/发现卡片。与侧栏的一行不同,这里给全:完整标题不截断、要点摘要、
/// 可打开入口、可编辑与归档——研究工件与 req/defect 同权(D-413 的初衷)。
function researchCard(entry, kind) {
  const card = document.createElement("article");
  card.className = "research-card";
  card.dataset.docId = entry.id;

  const head = document.createElement("div");
  head.className = "research-card-head";
  const id = document.createElement("span");
  id.className = "research-card-id";
  id.textContent = entry.id;
  head.appendChild(id);

  const type = researchField(entry, "类型", "type") || researchField(entry, "域", "domain");
  if (type) {
    const badge = document.createElement("span");
    badge.className = "research-badge";
    badge.textContent = type;
    head.appendChild(badge);
  }
  const level = researchField(entry, "等级", "level");
  if (level) {
    const badge = document.createElement("span");
    // V 等级是研究报告的可信度分层,给它固定色阶而不是混进普通徽章。
    badge.className = `research-badge v-badge v-${level.toLowerCase().replace(/[^v0-9]/g, "")}`;
    badge.textContent = level;
    head.appendChild(badge);
  }
  const status = document.createElement("span");
  status.className = `research-badge st-${entry.status || ""}`;
  status.textContent = localizedDocStatus(entry.status || "");
  head.appendChild(status);
  card.appendChild(head);

  const title = document.createElement("div");
  title.className = "research-card-title";
  title.textContent = entry.title;
  card.appendChild(title);

  // 正文摘要:来源看「要点」,发现看「结论」——这是人扫一眼要读的东西。
  const gist = researchField(entry, "要点", "结论", "说明");
  if (gist) {
    const body = document.createElement("div");
    body.className = "research-card-gist";
    body.textContent = gist;
    card.appendChild(body);
  }

  const meta = [researchField(entry, "作者", "author"), researchField(entry, "年份", "year")]
    .filter(Boolean)
    .join(" · ");
  if (meta) {
    const line = document.createElement("div");
    line.className = "research-card-meta";
    line.textContent = meta;
    card.appendChild(line);
  }

  const actions = document.createElement("div");
  actions.className = "research-card-actions";
  // 打开:文献走 URL 进内置 viewer,代码域走证据锚跳文件定位(用户定调不跳出应用)。
  const openable = (entry.fields ?? []).find(([k, v]) => researchLinkField(k, v));
  if (openable) {
    const open = researchOpenLink(openable[0], String(openable[1]));
    open.className = "ghost mini research-open";
    open.textContent = `↗ ${t("打开")}`;
    actions.appendChild(open);
  }
  // refs 反向可跳:发现→来源。
  const refs = researchField(entry, "refs");
  for (const ref of refs.split(/[\s,]+/).filter(Boolean)) {
    const link = document.createElement("button");
    link.type = "button";
    link.className = "ref-link";
    link.textContent = ref;
    link.title = t("跳到该来源");
    link.addEventListener("click", () => researchFocus(ref));
    actions.appendChild(link);
  }
  const edit = document.createElement("button");
  edit.type = "button";
  edit.className = "ghost mini";
  edit.textContent = t("编辑");
  edit.addEventListener("click", () => researchEdit(entry, kind, card));
  actions.appendChild(edit);
  if ((entry.nextStatuses ?? []).length > 0) {
    for (const next of entry.nextStatuses) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ghost mini";
      btn.textContent = `→ ${localizedDocStatus(next)}`;
      btn.addEventListener("click", async () => {
        try {
          await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, status: next });
          toast(`${entry.id} → ${localizedDocStatus(next)}`);
          refreshResearch();
        } catch (error) {
          toastError(`${t("保存失败")}:${error}`);
        }
      });
      actions.appendChild(btn);
    }
  }
  card.appendChild(actions);
  return card;
}

/// 行内编辑:字段逐条给输入框,保存走 docs_update(与 req/defect 同一条写通道)。
function researchEdit(entry, kind, card) {
  if (card.querySelector(".research-edit")) {
    card.querySelector(".research-edit").remove();
    return;
  }
  const box = document.createElement("div");
  box.className = "research-edit";
  const inputs = new Map();
  for (const [key, value] of entry.fields ?? []) {
    const row = document.createElement("label");
    row.className = "research-edit-row";
    row.append(`${key}: `);
    const long = String(value).length > 60;
    const input = document.createElement(long ? "textarea" : "input");
    if (long) input.rows = 3;
    input.value = value;
    inputs.set(key, input);
    row.appendChild(input);
    box.appendChild(row);
  }
  const save = document.createElement("button");
  save.type = "button";
  save.className = "primary mini";
  save.textContent = t("保存");
  save.addEventListener("click", async () => {
    const fields = {};
    for (const [key, input] of inputs) fields[key] = input.value;
    try {
      await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, fields });
      toast(`${entry.id} ${t("已保存")}`);
      refreshResearch();
    } catch (error) {
      toastError(`${t("保存失败")}:${error}`);
    }
  });
  box.appendChild(save);
  card.appendChild(box);
}

/// 报告里点 [S-00x] 或卡片里点 refs → 滚到那张卡并高亮。溯源要能双向走。
function researchFocus(id) {
  const target = String(id).trim();
  researchTab = target.startsWith("F-") ? "findings" : "sources";
  renderResearchCards();
  const card = document.querySelector(`.research-card[data-doc-id="${target}"]`);
  if (!card) return;
  card.scrollIntoView({ block: "center" });
  card.classList.add("ref-highlight");
  setTimeout(() => card.classList.remove("ref-highlight"), 1600);
}

function renderResearchCards() {
  const host = $("research-cards");
  if (!host) return;
  const kind = researchTab === "findings" ? "finding" : "source";
  const entries = researchTab === "findings" ? researchSnapshot.findings : researchSnapshot.sources;
  $("research-tab-sources")?.classList.toggle("active", researchTab === "sources");
  $("research-tab-findings")?.classList.toggle("active", researchTab === "findings");
  $("research-tab-sources")?.setAttribute("aria-selected", String(researchTab === "sources"));
  $("research-tab-findings")?.setAttribute("aria-selected", String(researchTab === "findings"));
  host.innerHTML = "";
  if (!entries.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    // 空态给指引而不是一个孤零零的「(空)」(设计 §4.5)。
    empty.textContent = researchTab === "findings"
      ? t("还没有发现。研究时用 finding add 记录结论,每条须挂来源。")
      : t("还没有来源。研究时用 source add 登记文献或代码位置,引用前先登记。");
    host.appendChild(empty);
    return;
  }
  for (const entry of entries) host.appendChild(researchCard(entry, kind));
}

/// 报告正文:markdown 渲染,并把 [S-00x]/[F-00x] 变成可点角标(溯源三处冗余之一)。
function renderResearchReport(text) {
  const host = $("research-report");
  if (!host) return;
  host.innerHTML = renderMarkdown(text ?? "");
  // 渲染后回扫文本节点,把引用编号替换为按钮。只认已登记的编号,避免把普通
  // 文本里的 S-/F- 误变成死链。
  const known = new Set([
    ...researchSnapshot.sources.map((e) => e.id),
    ...researchSnapshot.findings.map((e) => e.id),
  ]);
  if (!known.size) return;
  const walker = document.createTreeWalker(host, 4 /* TEXT_NODE */);
  const targets = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (/\b[SF]-\d{3}\b/.test(node.nodeValue ?? "")) targets.push(node);
  }
  for (const node of targets) {
    const frag = document.createDocumentFragment();
    let last = 0;
    const text2 = node.nodeValue ?? "";
    for (const m of text2.matchAll(/\b[SF]-\d{3}\b/g)) {
      if (!known.has(m[0])) continue;
      if (m.index > last) frag.appendChild(document.createTextNode(text2.slice(last, m.index)));
      const link = document.createElement("button");
      link.type = "button";
      link.className = "ref-link";
      link.textContent = m[0];
      link.addEventListener("click", () => researchFocus(m[0]));
      frag.appendChild(link);
      last = m.index + m[0].length;
    }
    if (!last) continue;
    if (last < text2.length) frag.appendChild(document.createTextNode(text2.slice(last)));
    node.parentNode?.replaceChild(frag, node);
  }
}

async function refreshResearch() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    researchSnapshot = { sources: snapshot.sources ?? [], findings: snapshot.findings ?? [] };
  } catch (error) {
    log(`${t("研究工件刷新失败")}:${error}`, "warn");
  }
  const counts = [$("research-sources-count"), $("research-findings-count")];
  if (counts[0]) counts[0].textContent = String(researchSnapshot.sources.length);
  if (counts[1]) counts[1].textContent = String(researchSnapshot.findings.length);
  renderResearchCards();
  try {
    const doc = await invoke("docs_read", { projectDir: currentProject, kind: "report" });
    renderResearchReport(doc.content ?? "");
  } catch {
    // 报告还没产出是正常状态(研究刚开始),给指引不报错。
    renderResearchReport(`_${t("还没有报告。研究结束时把结论写进 .kanzei/research/report.md。")}_`);
  }
}

$("research-tab-sources")?.addEventListener("click", () => {
  researchTab = "sources";
  renderResearchCards();
});
$("research-tab-findings")?.addEventListener("click", () => {
  researchTab = "findings";
  renderResearchCards();
});
$("research-report-refresh")?.addEventListener("click", () => refreshResearch());

/// research 档才出现研究工作台;dev 档反过来藏起研究入口。
/// 用户定调:research 档下 dev 视图(需求/缺陷/测试/git)完全隐藏,模式感优先。
const DEV_ONLY_VIEWS = ["documents", "metrics", "arch"];
function syncResearchWorkspaceVisibility() {
  const isResearch = $("profile-select")?.value === "research";
  $("activity-research")?.classList.toggle("hidden", !isResearch);
  for (const view of DEV_ONLY_VIEWS) {
    document.querySelector(`.activity-item[data-view="${view}"]`)?.classList.toggle("hidden", isResearch);
  }
  // 正停在被隐藏的视图上时退回对话,避免留在一个入口已消失的页面里。
  const active = document.querySelector(".view.active")?.id ?? "";
  const strandedOnDev = isResearch && DEV_ONLY_VIEWS.some((v) => active === `view-${v}`);
  const strandedOnResearch = !isResearch && active === "view-research";
  if (strandedOnDev || strandedOnResearch) {
    document.querySelector('.activity-item[data-view="chat"]')?.click();
  }
}
