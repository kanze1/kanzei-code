// Optional mobile bridge: loaded only when its controls are opened.
import { $, defer, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, toast, toastError } from "./03-shell.js";
defer(() => {
  $("mobile-service-stop").addEventListener("click", async () => {
    try {
      await invoke("mobile_service_stop");
      $("mobile-service-status").textContent = t("移动端本机桥接已停止");
      $("mobile-service-start").classList.remove("hidden");
      $("mobile-service-stop").classList.add("hidden");
      $("mobile-pair-regenerate").disabled = true;
      $("mobile-device-list").textContent = t("启动服务后显示");
    } catch (error) {
      toastError(`${t("停止移动端桥接失败")}:${error}`, { retry: () => $("mobile-service-stop").click() });
    }
  });
});

// D-386:设备列表 + 逐台撤销 + 配对码再生。revoke/list/regenerate 命令 R-270 已注册,
// 此前 UI 零调用(多设备实际不可能、撤销=空集、配对码不可再生)。
export async function refreshMobileDevices() {
  const container = $("mobile-device-list");
  if (!container) return;
  try {
    const devices = await invoke("mobile_device_list");
    if (!devices || devices.length === 0) {
      container.innerHTML = `<span class="dim">${t("暂无已配对设备")}</span>`;
      return;
    }
    container.innerHTML = "";
    for (const device of devices) {
      const row = document.createElement("span");
      row.className = "mobile-device-row";
      row.textContent = `${device.device_id} · ${device.name} · `;
      const revoke = document.createElement("button");
      revoke.className = "ghost danger-text";
      revoke.textContent = t("撤销");
      revoke.addEventListener("click", async () => {
        try {
          await invoke("mobile_device_revoke", { deviceId: device.device_id });
          toast(`${t("已撤销设备")}: ${device.device_id}`);
          refreshMobileDevices();
        } catch (error) {
          toastError(`${t("撤销设备失败")}:${error}`);
        }
      });
      row.appendChild(revoke);
      container.appendChild(row);
      container.appendChild(document.createElement("br"));
    }
  } catch (error) {
    container.textContent = `${t("读取设备列表失败")}:${error}`;
  }
}

defer(() => {
  $("mobile-service-start").addEventListener("click", async () => {
    // 启动成功后启用配对码再生 + 加载设备列表(上面的 start 逻辑由事件顺序保证先执行)。
    const lan = !!$("mobile-service-lan")?.checked;
    try {
      const info = await invoke("mobile_service_start", { projectDir: currentProject, port: null, lan, notifications: !!$("mobile-notifications")?.checked });
      const lanLabel = lan ? "LAN" : t("回环");
      $("mobile-service-status").textContent = `${lanLabel} · ${info.address} · token ${info.token}`;
      $("mobile-service-start").classList.add("hidden");
      $("mobile-service-stop").classList.remove("hidden");
      $("mobile-pair-regenerate").disabled = false;
      refreshMobileDevices();
      toast(t("移动端本机桥接已启动"));
    } catch (error) {
      toastError(`${t("启动移动端桥接失败")}:${error}`, { retry: () => $("mobile-service-start").click() });
    }
  });
});
defer(() => {
  $("mobile-pair-regenerate").addEventListener("click", async () => {
    try {
      const newCode = await invoke("mobile_pair_code_regenerate");
      $("mobile-service-status").textContent = `${$("mobile-service-status").textContent.split(" · token ")[0]} · token ${newCode}`;
      toast(`${t("新配对码")}: ${newCode}`);
    } catch (error) {
      toastError(`${t("重新生成配对码失败")}:${error}`);
    }
  });
});
