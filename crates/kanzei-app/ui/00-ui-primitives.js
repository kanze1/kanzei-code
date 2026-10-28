// ---------- #7 动效原语(纪律见 style.css「分区:动效」与 ui-a11y-smoke「#7 动效纪律」) ----------
// 循环档时长全是 2400ms 的约数。频繁重建的节点(侧栏线路行每个 kz:status 整行重画)
// 若不对齐,每次重建都从第 0 帧重来,呼吸点看起来一直在「抽」。motionSync 把节点的
// animation-delay 写成「全局时钟在 2400ms 周期里的负偏移」,新节点与旧节点同相。
export const MOTION_EPOCH_MS = 2400;
export function motionSync(el) {
  if (typeof el?.style?.setProperty !== "function") return el;
  const now = typeof performance !== "undefined" && typeof performance.now === "function"
    ? performance.now()
    : Date.now();
  el.style.setProperty("--kz-sync", `${-Math.round(now % MOTION_EPOCH_MS)}ms`);
  return el;
}
/// 一次性动效:只在状态**真正跳变**的调用点挂上(历史回放不经过这些点),定时摘除。
/// 不依赖 animationend——窗口隐藏/减少动效时它可能不来,类会一直挂着,下次就不再重播。
export function motionOnce(el, cls, ms = 600) {
  if (!el?.classList) return el;
  const timers = (el._kzMotionTimers ??= {});
  clearTimeout(timers[cls]);
  el.classList.remove(cls);
  // 强制一次重排:同一元素连续触发时让浏览器看见「摘掉再挂上」,动画才会重播。
  void el.offsetWidth;
  el.classList.add(cls);
  timers[cls] = setTimeout(() => {
    el.classList.remove(cls);
    delete timers[cls];
  }, ms);
  return el;
}
/// 计数写入:文本不变不动;数值上升时 tick 一次(下降/清空不播,那不是「又多了一个」)。
export function motionCount(el, text) {
  if (!el) return el;
  const next = String(text ?? "");
  const previous = el.textContent ?? "";
  if (previous === next) return el;
  el.textContent = next;
  const before = Number.parseInt(previous, 10);
  const after = Number.parseInt(next, 10);
  if (previous && Number.isFinite(before) && Number.isFinite(after) && after > before) motionOnce(el, "kz-tick", 320);
  return el;
}
// 输入法组合态(拼音/注音选词中):此刻的 Enter 是在选词、Esc 是在撤候选,都不能当提交/关闭。
// isComposing 之外再认 keyCode 229:部分运行时在 compositionend 之后的同一次 keydown 里 isComposing 已是 false,
// keyCode 仍是 229。全站「Enter 提交」的输入框(命令面板、提问框、对话搜索、记忆检索……)共用这一份判断。
export function isImeComposing(event) {
  return Boolean(event) && (event.isComposing === true || event.keyCode === 229);
}
// Some WebView2/IME combinations deliver a non-composing Enter immediately
// after compositionend. Consume that companion once, then allow the next key.
export function createImeSubmitGuard(input) {
  const companionWindowMs = 500;
  let composing = false, endedAt = -Infinity;
  input.addEventListener("compositionstart", () => { composing = true; endedAt = -Infinity; });
  input.addEventListener("compositionend", () => { composing = false; endedAt = performance.now(); });
  return event => {
    if (composing || isImeComposing(event)) return true;
    if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) return false;
    if (performance.now() - endedAt >= companionWindowMs) return false;
    endedAt = -Infinity;
    event.preventDefault();
    return true;
  };
}
// localStorage 里的 JSON 可能被手改坏;读不出来就当没有,绝不让偏好读取抛异常
// localStorage 里的 JSON 可能被手改坏;读不出来就当没有,绝不让偏好读取抛异常
// 把整个初始化带崩。
export function readJson(key, fallback) {
  try {
    const parsed = JSON.parse(localStorage.getItem(key) || "null");
    return parsed && typeof parsed === "object" ? parsed : fallback;
  } catch {
    return fallback;
  }
}
export function writeJson(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 配额满等情况:偏好丢失可以接受,不该打断当前操作 */
  }
}
