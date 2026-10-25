import { defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { openDialog, closeSurface, isSurfaceOpen } from "./00-surface.js";
import { activeProcessId, processItems, toastError } from "./03-shell.js";
import { t } from "./02-i18n.js";
import { on } from "./01-runtime-events.js";
import { renderNativeControl } from "./native-control-view.js";

let controlRevision=-1,controlState=null;
function showControl(value) {
  if(value.revision<controlRevision)return;
  controlRevision=value.revision;controlState=value;
  const root=document.getElementById("native-control-indicator");if(!root)return;
  renderNativeControl(root,value,{t,stop:controlId=>invoke("native_desktop_stop",{controlId}),dismiss:controlId=>invoke("native_desktop_dismiss",{controlId})});
  void sync();
}
on("kz:native-control",event=>showControl(event.payload));
on("kz:stopped",()=>void sync());

let generation=0;
async function sync() {
  const version=++generation,owner=activeProcessId,button=document.getElementById("native-desktop-choose");
  if(!button)return;
  const profile=processItems.find(item=>item.id===owner)?.profile || "dev-auto";
  const development=["dev-auto","dev-pair"].includes(profile);
  button.disabled=!owner || !development;
  if(!owner){button.textContent=t("选择原生窗口");return;}
  try {
    const current=await invoke("native_desktop_get",{processId:owner});
    if(version!==generation || owner!==activeProcessId)return;
    button.textContent=current ? `${t("原生窗口")} · ${current.title}` : t("选择原生窗口");
    button.title=development ? t("选择本对话可操作的原生窗口") : t("当前模式不操作原生窗口");
  }catch(error){if(version===generation){button.textContent=t("原生窗口状态读取失败");button.title=String(error);}}
}
async function choose() {
  const owner=activeProcessId;if(!owner)return;
  const dialog=document.getElementById("native-desktop-dialog");
  if(isSurfaceOpen(dialog))return;
  const status=document.getElementById("native-desktop-status"),list=dialog.querySelector(".native-window-list");
  const disconnect=document.getElementById("native-desktop-disconnect"),cancel=document.getElementById("native-desktop-cancel");
  status.textContent=t("正在读取窗口列表");list.replaceChildren();
  disconnect.disabled=false;cancel.disabled=false;
  const events=new AbortController();
  const handle=openDialog(dialog,{onClose:()=>events.abort(),initialFocus:cancel});
  cancel.addEventListener("click",()=>closeSurface(dialog),{signal:events.signal});
  const select=async candidateId=>{
    if(owner!==activeProcessId){closeSurface(dialog);return;}
    for(const button of dialog.querySelectorAll("button"))button.disabled=true;
    try {await invoke("native_desktop_select",{processId:owner,candidateId});closeSurface(dialog);await sync();}
    catch(error){status.textContent=String(error);for(const button of dialog.querySelectorAll("button"))button.disabled=false;}
  };
  disconnect.addEventListener("click",()=>void select(null),{signal:events.signal});
  try {
    const result=await invoke("native_desktop_windows");
    if(handle.closed || owner!==activeProcessId){closeSurface(dialog);return;}
    status.textContent=result.windows.length ? t("选择要交给当前对话的窗口") : t("没有可选择的原生窗口");
    for(const window of result.windows){
      const button=document.createElement("button");button.type="button";button.className="kz-ctl";
      button.textContent=`${window.title} · ${window.application}`;button.dataset.windowCandidate=window.id;
      button.addEventListener("click",()=>void select(window.id));list.append(button);
    }
  }catch(error){status.textContent=String(error);}
}
defer(()=>{
  void invoke("native_desktop_status").then(showControl).catch(error=>toastError(String(error)));
  document.addEventListener("kz:language",()=>{if(controlState)showControl(controlState);});
  document.getElementById("native-desktop-choose")?.addEventListener("click",()=>void choose().catch(error=>toastError(String(error))));
  for(const event of ["kz:conversation-selected","kz:sessions-changed","kz:language"])document.addEventListener(event,()=>void sync());
  document.getElementById("profile-select")?.addEventListener("change",()=>void sync());
  void sync();
});
