// UI2-0926 #14 后台任务侧栏的自动开合策略(docs/design/subagent_presentation.md §5.6/§7)。
// 零 import、零 DOM、时钟由调用方传入:冒烟直接喂事件断言,变异守卫可精确打点。
//
// 口径:
// - 「本次运行」= 从用户手动发出一条消息到下一条用户消息;鞭挞自动续跑的轮次继承同一次运行
//   (否则用户关掉侧栏后,下一轮鞭挞马上又弹开)。
// - 自动打开只由「活动线路上开始了实时的子代理 / 跑满 3 秒的终端命令」触发;历史回放不触发。
// - 用户在本次运行里关过 → 本次运行内不再自动打开(直到下一条用户消息)。
// - 自动打开的侧栏在全部结束后 6 秒自动收起;指针悬停/焦点在侧栏内时暂停计时;
//   有未确认的「值得停留的失败」(实时子代理失败/超时/中断/未启动,或跑满 3 秒后失败的终端命令)时不收起。
// - 用户手动打开(rail/↗/命令面板)= pinned,不自动收起;用户关闭 = 取消 pinned + 确认失败 + (有活时)压制本次运行。
// - 抽屉态(停靠后对话列不足 600px)不自动打开,只亮徽标;窗口变宽后若仍处于自动态则显示。
// - 只在对话视图显示;切到别的视图不改任何状态,回来按状态重算。
export const SIDE_AUTO_CLOSE_MS = 6000;
export const SIDE_TERMINAL_AUTO_MS = 3000;
export const SIDE_CHAT_MIN = 600;
export const SIDE_WIDTH_MIN = 320;
export const SIDE_WIDTH_MAX = 760;

/// 默认宽度 clamp(360, 26vw, 520):1600 宽 416,2000 宽 520。
export function sideDefaultWidth(viewportWidth) {
  return Math.round(Math.min(520, Math.max(360, (Number(viewportWidth) || 0) * 0.26)));
}
/// 停靠还是抽屉:停靠后对话列(主区宽 − 侧栏宽)不少于 600px 才停靠。
export function sideDockMode({ mainWidth, panelWidth }) {
  return mainWidth - panelWidth >= SIDE_CHAT_MIN ? "side" : "drawer";
}
/// 宽度上限:不超过 760,且停靠时给对话列留 600(但不低于下限 320)。
export function sideMaxWidth(mainWidth) {
  return Math.max(SIDE_WIDTH_MIN, Math.min(SIDE_WIDTH_MAX, (Number(mainWidth) || 0) - SIDE_CHAT_MIN));
}
/// 宽度夹紧到 [320, sideMaxWidth]。
export function sideClampWidth(width, mainWidth) {
  return Math.round(Math.min(sideMaxWidth(mainWidth), Math.max(SIDE_WIDTH_MIN, Number(width) || 0)));
}

export function createSideModel() {
  return { pinned: false, lines: new Map() };
}
function lineOf(model, sid) {
  const key = sid || "";
  let line = model.lines.get(key);
  if (!line) {
    line = { userRun: 0, suppressedRun: -1, auto: false, settledAt: 0, hover: false, holds: new Set() };
    model.lines.set(key, line);
  }
  return line;
}

/// 事件入口(就地改 model,返回 model)。event.type:
///   user-run   用户手动发消息(sendText 的非鞭挞分支):开启新的一次运行,上次的压制与失败都算看过了
///   work-start 实时子代理开始 / 终端命令跑满 3 秒(调用方负责只报实时的)
///   failure    值得停留的实时失败({ key, hold })
///   ack        用户确认了失败(「知道了」;{ key } = 只确认打开了详情的那一次)
///   interact   指针进出 / 焦点进出侧栏({ on, now })
///   user-open  用户手动打开
///   user-close 用户手动关闭({ active }:此刻还有活就压制本次运行)
export function sideEvent(model, event, prefs = { autoOpen: true, autoClose: true }) {
  const line = lineOf(model, event.sid);
  switch (event.type) {
    case "user-run":
      line.userRun += 1;
      line.suppressedRun = -1;
      line.holds.clear();
      break;
    case "work-start":
      line.settledAt = 0;
      if (prefs.autoOpen && line.suppressedRun !== line.userRun) line.auto = true;
      break;
    case "failure":
      if (event.hold) line.holds.add(String(event.key));
      break;
    case "ack": // 带 key = 只确认这一次失败(打开了它的详情);不带 = 这条线路的失败都算看过了
      if (event.key !== undefined) line.holds.delete(String(event.key));
      else line.holds.clear();
      break;
    case "interact":
      line.hover = Boolean(event.on);
      if (!line.hover && line.settledAt) line.settledAt = event.now ?? line.settledAt; // 离开后重新计时
      break;
    case "user-open":
      model.pinned = true;
      break;
    case "user-close":
      model.pinned = false;
      line.auto = false;
      line.settledAt = 0;
      line.holds.clear();
      if ((event.active ?? 0) > 0) line.suppressedRun = line.userRun;
      break;
    default:
      break;
  }
  return model;
}

/// 决策:活动线路此刻该不该显示、以什么形态、多久后再算一次、徽标是什么。
/// env: { sid, view, dock: "side"|"drawer", active, now, prefs }
export function sideDecide(model, env) {
  const prefs = env.prefs ?? { autoOpen: true, autoClose: true };
  const line = lineOf(model, env.sid);
  const active = Math.max(0, env.active | 0);
  const now = env.now ?? 0;
  if (active > 0) line.settledAt = 0;
  else if (line.auto && !line.settledAt) line.settledAt = now;
  const holding = line.holds.size > 0;
  let lingerMs = null;
  if (line.auto && active === 0 && !holding && prefs.autoClose && !line.hover) {
    const left = SIDE_AUTO_CLOSE_MS - (now - line.settledAt);
    if (left <= 0) line.auto = false; // 到点:退出自动态
    else lingerMs = left;
  }
  const chat = env.view === "chat";
  const autoShown = line.auto && env.dock === "side";
  const visible = chat && (model.pinned || autoShown);
  const badge = active > 0 ? { count: active, tone: "run" } : holding ? { count: line.holds.size, tone: "err" } : null;
  return {
    visible,
    reason: !visible ? null : model.pinned ? "pinned" : "auto",
    dock: visible ? env.dock : null,
    lingerMs, // 非对话视图照样计时,回来时状态已正确
    badge,
  };
}
