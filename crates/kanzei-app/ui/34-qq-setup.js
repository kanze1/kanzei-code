import { invoke } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { toastError } from "./03-shell.js";
let session = null, status = null, timer = null, generation = 0;
const node = (tag, text) => { const e = document.createElement(tag); if (text != null) e.textContent = text; return e; };
export function renderQqSetup(host, changed) {
  const box = node("section"); box.className = "qq-setup";
  box.append(node("h2", t("本机 QQ 配置")));
  const controls = node("div"); controls.className = "qq-toolbar";
  const connect = node("button", t("选择 NapCat 目录并连接")); connect.type = "button"; connect.className = "primary";
  const result = node("div"); result.className = "qq-setup-state";
  async function showStatus() {
    const own = generation, id = session;
    if (!id || document.body.dataset.view !== "qq" || !result.isConnected) return;
    try { const next = await invoke("qq_setup_status", { id }); if (own !== generation || id !== session || !result.isConnected) return; status = next; paint(); }
    catch (error) { if (own === generation) { clearInterval(timer); timer = null; result.replaceChildren(node("p", String(error))); } }
  }
  function paint() {
    result.replaceChildren(); if (!status) return;
    if (status.online) {
      clearInterval(timer); timer = null;
      result.append(node("p", `${t("QQ 已登录")} ${status.nickname || status.account}`));
      const row = node("label", t("连接端口")); row.className = "qq-field";
      const port = node("input"); port.type = "number"; port.value = "3001"; port.min = "1"; port.max = "65535"; row.append(port);
      const apply = node("button", t("配置并启用 QQ")); apply.type = "button"; apply.className = "primary";
      apply.addEventListener("click", async () => { apply.disabled = true; try { await invoke("qq_setup_apply", { id: session, port: Number(port.value) }); await changed(); } catch (error) { toastError(String(error)); } finally { apply.disabled = false; } });
      result.append(row, apply);
    } else {
      if (status.image) { const image = node("img"); image.src = status.image; image.alt = t("使用手机 QQ 扫码登录"); image.className = "social-login-qr"; result.append(image); }
      result.append(node("p", status.error || t("使用手机 QQ 扫码登录")));
    }
  }
  async function login(args) {
    const own = ++generation; clearInterval(timer); timer = null; connect.disabled = true;
    try { const data = await invoke("qq_setup_connect", args); if (own !== generation) return; session = data.id; status = null; await showStatus(); if (session && !status?.online) timer = setInterval(() => { void showStatus(); }, 2000); }
    catch (error) { if (own === generation) result.replaceChildren(node("p", String(error))); }
    finally { if (own === generation) connect.disabled = false; }
  }
  connect.addEventListener("click", async () => { try { const directory = await invoke("qq_setup_pick_directory"); if (directory) await login({ directory, url: null, password: null, totp: null }); } catch (error) { toastError(String(error)); } });
  controls.append(connect); box.append(controls);
  const advanced = node("details"); advanced.append(node("summary", t("手动连接管理面板")));
  const form = node("form"); form.className = "qq-form";
  const input = (label, name, value, type) => { const row = node("label", t(label)); row.className = "qq-field"; const control = node("input"); control.name = name; control.type = type || "text"; control.value = value; row.append(control); form.append(row); return control; };
  input("NapCat 管理地址", "url", "http://127.0.0.1:6099"); input("NapCat 管理密码", "password", "", "password"); input("两步验证码", "totp", "");
  const submit = node("button", t("连接")); submit.type = "submit"; form.append(submit);
  form.addEventListener("submit", event => { event.preventDefault(); void login({ directory: null, ...Object.fromEntries(new FormData(form)) }); });
  advanced.append(form); box.append(advanced, result); host.append(box);
  if (session) { paint(); if (!status?.online) { clearInterval(timer); timer = setInterval(() => { void showStatus(); }, 2000); void showStatus(); } }
}
