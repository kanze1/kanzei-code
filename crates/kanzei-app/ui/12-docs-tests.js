import { $, defer, invoke } from "./01-core.js";
import { localizedStatusWord, t } from "./02-i18n.js";
import { currentProject, log } from "./03-shell.js";
import { renderTestRecordFields } from "./04-structured.js";
import { jumpToEntry } from "./11-docs-list.js";

// ---------- 需求页「测试」页签 ----------
// 原来渲染在 09-sessions.js 的末尾:把 active 与 archived 拼成一串再整体倒序,于是运行中的记录排在
// 114 条历史的最底;没有筛选、没有搜索;行 tooltip 还直接露出毫秒时间戳(UX-060)。现在:
//   · 先「进行中」(active)后「历史」(archived),各自新的在前;
//   · 状态筛选 + 搜索(编号/标题/关联条目/字段内容);
//   · 行上给可读时间(从 T-<秒> 编号推出),不再露原始时间戳;
//   · 历史一次只画一屏,「显示更多」再续。
// 读快照与回填关联拆开(UX-014):读不依赖写租约,先画;回填是可跳过的维护写,补成了才重读。

const PAGE = 100;
const view = { status: "all", query: "", shown: PAGE };
let lastSnapshot = null;
let refreshSeq = 0;

const STATUS_FILTERS = ["running", "passed", "failed", "skipped"];
const STATUS_ICON = { passed: "✓", failed: "×", running: "●" };

/// 记录编号 T-<epoch 秒> 里带着创建时刻;解析不出来就不显示时间(不猜)。
export function testRecordTime(id) {
  const secs = Number(String(id ?? "").replace(/^T-/, ""));
  if (!Number.isFinite(secs) || secs < 1e9) return "";
  // 约定是秒;旧数据里若是毫秒(13 位)也认,不把它当成几万年后。
  return new Date(secs > 1e11 ? secs : secs * 1000).toLocaleString();
}

function recordText(record) {
  const fields = (record.fields ?? []).map((field) => (Array.isArray(field) ? field.join(" ") : `${field.key ?? ""} ${field.value ?? ""}`));
  return [record.id, record.title, ...(record.refs ?? []), ...fields].join(" ").toLowerCase();
}

function matches(record) {
  if (view.status !== "all" && record.status !== view.status) return false;
  const query = view.query.trim().toLowerCase();
  return !query || recordText(record).includes(query);
}

function testRow(record) {
  const row = document.createElement("div");
  row.className = `test-entry test-${record.status}`;
  // UI-0926 #10:行头可点,展开后是结构化字段(命令列表、收尾时间、源码指纹路径…);
  // 字段只在首次展开时构建。字段同时接受 {key,value}(真实 IPC)与 [k,v] 两种形状。
  row.dataset.docId = record.id;
  const head = document.createElement("button");
  head.type = "button";
  head.className = "sv-test-head";
  head.setAttribute("aria-expanded", "false");
  const icon = document.createElement("span");
  icon.className = "test-icon";
  icon.textContent = STATUS_ICON[record.status] ?? "○";
  icon.setAttribute("aria-hidden", "true");
  const status = document.createElement("span");
  status.className = "test-status";
  status.textContent = localizedStatusWord(record.status);
  const title = document.createElement("span");
  title.className = "test-title";
  title.textContent = `${record.id} ${record.title}`;
  head.append(icon, status, title);
  // 悬空的 running(后端按 30 分钟没收尾标 stale):说破,别让它和「正在跑」长得一样。
  if (record.status === "running" && record.stale) {
    const stale = document.createElement("span");
    stale.className = "test-stale";
    stale.textContent = t("长时间未收尾");
    head.appendChild(stale);
  }
  const time = testRecordTime(record.id);
  if (time) {
    const stamp = document.createElement("span");
    stamp.className = "test-time dim";
    stamp.textContent = time;
    head.appendChild(stamp);
  }
  const detail = document.createElement("div");
  detail.className = "sv-test-detail hidden";
  head.addEventListener("click", () => {
    if (!detail.children.length) detail.appendChild(renderTestRecordFields(record.fields ?? []));
    const closed = detail.classList.toggle("hidden");
    head.setAttribute("aria-expanded", String(!closed));
  });
  row.appendChild(head);
  // R-130:测试→条目映射可见——关联的 R-/D- 条目号渲染成可点跳转的徽标,
  // 让「这条测试为哪个条目背书」一眼可见,点一下直接跳到该条目(含已归档的:jumpToEntry 先切页签再定位)。
  const refs = record.refs ?? [];
  if (refs.length) {
    const refRow = document.createElement("div");
    refRow.className = "test-entry-refs";
    for (const refId of refs) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "test-ref-chip";
      chip.textContent = refId;
      chip.title = `${t("跳转到")} ${refId}`;
      chip.addEventListener("click", () => jumpToEntry(refId, { expand: true }));
      refRow.appendChild(chip);
    }
    row.appendChild(refRow);
  }
  row.appendChild(detail);
  return row;
}

function groupHead(label, count) {
  const head = document.createElement("div");
  head.className = "doc-group-head";
  const name = document.createElement("span");
  name.className = "doc-group-label";
  name.textContent = label;
  const num = document.createElement("span");
  num.className = "doc-group-count";
  num.textContent = String(count);
  head.append(name, num);
  return head;
}

// 状态下拉的选项随界面语言重建(切语言不经过测试页的刷新);保住当前选择。
function syncStatusOptions() {
  const select = $("tests-status-filter");
  if (!select) return;
  const options = [["all", t("全部状态")], ...STATUS_FILTERS.map((value) => [value, localizedStatusWord(value)])];
  const signature = JSON.stringify(options);
  if (select.dataset.signature !== signature) {
    select.replaceChildren(...options.map(([value, label]) => new Option(label, value)));
    select.dataset.signature = signature;
  }
  select.value = view.status;
}

export function renderTestRuns(snapshot) {
  lastSnapshot = snapshot;
  syncStatusOptions();
  const list = $("test-list");
  if (!list) return;
  // 新的在前;「进行中」(还没收尾的)单独成段排在历史之前——它们才是此刻要看的。
  const active = [...(snapshot?.active ?? [])].reverse();
  const archived = [...(snapshot?.archived ?? [])].reverse();
  const total = active.length + archived.length;
  $("test-count").textContent = `${total}`;
  list.replaceChildren();
  const filtering = view.status !== "all" || view.query.trim() !== "";
  const activeShown = active.filter(matches);
  const archivedAll = archived.filter(matches);
  const summary = $("tests-summary");
  if (summary) {
    summary.textContent = !total ? ""
      : filtering ? `${t("显示")} ${activeShown.length + archivedAll.length} / ${t("共")} ${total} ${t("条")}`
        : `${t("共")} ${total} ${t("条")}`;
  }
  if (!total) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无测试记录");
    list.appendChild(empty);
    return;
  }
  if (!activeShown.length && !archivedAll.length) {
    const none = document.createElement("div");
    none.className = "doc-empty";
    none.textContent = t("没有符合条件的测试记录");
    list.appendChild(none);
    return;
  }
  if (activeShown.length) {
    list.appendChild(groupHead(t("进行中"), activeShown.length));
    for (const record of activeShown) list.appendChild(testRow(record));
  }
  if (archivedAll.length) {
    list.appendChild(groupHead(t("历史"), archivedAll.length));
    for (const record of archivedAll.slice(0, view.shown)) list.appendChild(testRow(record));
    if (archivedAll.length > view.shown) {
      const more = document.createElement("button");
      more.type = "button";
      more.className = "ghost mini tests-more";
      more.textContent = `${t("显示更多")}(${t("还有")} ${archivedAll.length - view.shown} ${t("条")})`;
      more.addEventListener("click", () => {
        view.shown += PAGE;
        renderTestRuns(lastSnapshot);
      });
      list.appendChild(more);
    }
  }
}

export async function refreshTests() {
  if (!currentProject) {
    renderTestRuns({ active: [], archived: [] });
    return;
  }
  const project = currentProject;
  const seq = ++refreshSeq;
  const current = () => seq === refreshSeq && project === currentProject;
  try {
    // UX-014:读快照不依赖写租约——agent 一轮几十分钟都占着租约,以前这里先等回填,页签就一直空白。
    const snapshot = await invoke("test_runs_snapshot", { projectDir: project });
    if (!current()) return;
    renderTestRuns(snapshot);
  } catch (error) {
    log(`${t("测试记录刷新失败")}:${error}`, "warn");
    return;
  }
  try {
    // R-130 验收③:旧记录里标题含 R-/D- 条目号的补写「关联」字段(幂等,无变化不写盘)。
    // 后端在写权被占时直接返回 skipped 而不排队;补写成功才重读一次,让新关联徽标出现。
    const result = await invoke("test_runs_init_refs", { projectDir: project });
    if (result?.backfilled > 0 && current()) {
      renderTestRuns(await invoke("test_runs_snapshot", { projectDir: project }));
    }
  } catch (error) {
    log(`${t("测试记录刷新失败")}:${error}`, "warn");
  }
}

defer(() => {
  syncStatusOptions();
  $("tests-status-filter")?.addEventListener("change", (event) => {
    view.status = event.target.value;
    view.shown = PAGE;
    if (lastSnapshot) renderTestRuns(lastSnapshot);
  });
  $("tests-search")?.addEventListener("input", (event) => {
    view.query = event.target.value;
    view.shown = PAGE;
    if (lastSnapshot) renderTestRuns(lastSnapshot);
  });
});
