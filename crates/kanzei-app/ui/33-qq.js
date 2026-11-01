import { openChannelConversation } from "./34-channel-conversations.js";
import { renderQqSetup } from "./34-qq-setup.js";
import { socialManagement, setSocialActive, socialUpdated } from "./34-social-state.js";
import { openMenu } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { $, defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { on } from "./01-runtime-events.js";
import { currentProject, activeProcessId, toastError } from "./03-shell.js";
import { setQqConversationRoots } from "./03-general-scope.js";

const element=(tag,text,cls)=>{const node=document.createElement(tag);if(text!=null)node.textContent=text;if(cls)node.className=cls;return node;};
const button=(text,action,cls="ghost")=>{const node=element("button",text,cls);node.type="button";node.addEventListener("click",()=>{node.disabled=true;Promise.resolve().then(action).catch(error=>toastError(String(error))).finally(()=>{node.disabled=false;});});return node;};
const key=identity=>JSON.stringify([identity.account,identity.kind,identity.peer]);
const rootKey=root=>String(root||"").replaceAll("\\","/").replace(/\/$/,"").toLowerCase();
let snapshot=null,selected=null,tab="connection",reading=0,form=null,dirty=false,tokenDraft="",groupDraft=null,editingId=null;
export function qqSnapshot(){return snapshot;}
function field(form,label,name,value,type="text") {
  const row=element("label",null,"qq-field");row.append(element("span",t(label)));
  const input=element(type==="textarea"?"textarea":"input");input.name=name;if(type!=="textarea")input.type=type;
  if(type==="checkbox")input.checked=!!value;else input.value=value??"";
  if(form.id!=="qq-group-editor")input.addEventListener("input",()=>{dirty=true;});row.append(input);form.append(row);return input;
}
export async function refreshQq(){
  const own=++reading;
  const next=await invoke("qq_status");if(own!==reading)return;
  if(dirty&&snapshot)next.settings={...snapshot.settings,groups:next.settings.groups};snapshot=next;
  $("qq-page-error").textContent=next.error||"";setQqConversationRoots(next.peers.map(peer=>peer.root));
  $("qq-state").textContent=next.connected?`${t("已连接")} ${next.nickname||next.account}`:t("未连接");
  syncSelection();if(socialManagement("qq")&&!dirty)renderPage();socialUpdated();
}
function syncSelection(){
  const active=snapshot?.peers.find(peer=>rootKey(peer.root)===rootKey(currentProject)&&peer.process_id===activeProcessId);
  selected=active||(selected&&snapshot?.peers.find(peer=>key(peer.identity)===key(selected.identity)))||snapshot?.peers[0]||null;
  document.body.dataset.qqConversation=String(!!active);
}
export async function openQqPeer(peer){setSocialActive(true);const opened=await openChannelConversation(()=>invoke("qq_peer_open",{identity:peer.identity}),"social");if(!opened)return null;selected=opened;syncSelection();socialUpdated();$("prompt").focus();return opened;}
function captureSettings(){if(!form)return;for(const input of form.querySelectorAll("[name]")){if(input.name==="token")tokenDraft=input.value;else snapshot.settings[input.name]=input.type==="checkbox"?input.checked:input.name==="model"?input.value||null:input.value;}}
export function showQqSection(next){captureSettings();if(next)tab=next;renderPage();}
function changeTab(next){showQqSection(next);}
function renderPage(){
  const host=$("qq-content");host.replaceChildren();form=null;if(!snapshot)return;
  $("qq-tabs").querySelectorAll("button").forEach(b=>{b.classList.toggle("active",b.dataset.tab===tab);b.setAttribute("aria-selected",String(b.dataset.tab===tab));});
  const settings=snapshot.settings;
  if(tab==="connection"||tab==="persona"){
    if(tab==="connection")renderQqSetup(host,async()=>{dirty=false;await refreshQq();});
    form=element("form",null,"qq-form");form.id="qq-settings-form";
    if(tab==="connection"){
      field(form,"启用 QQ","enabled",settings.enabled,"checkbox");field(form,"允许好友私聊唤起","friends_enabled",settings.friends_enabled,"checkbox");
      field(form,"模型","model",settings.model||"").placeholder=t("使用默认模型");
      const advanced=element("details"),summary=element("summary",t("手动 OneBot 配置")),fields=element("div",null,"qq-form");
      field(fields,"OneBot 地址","url",settings.url);const token=field(fields,"连接令牌","token",tokenDraft,"password");token.autocomplete="new-password";token.placeholder=snapshot.token_configured?t("已配置，留空保留"):t("OneBot 访问令牌");field(fields,"机器人 QQ 号","account",settings.account);advanced.append(summary,fields);form.append(advanced);
    }else field(form,"共享人设","persona",settings.persona,"textarea");
    const save=element("button",t("保存"),"primary");save.type="submit";form.append(save);
    form.addEventListener("submit",event=>{event.preventDefault();captureSettings();save.disabled=true;void invoke("qq_config_save",{settings:snapshot.settings,token:tokenDraft||null}).then(async()=>{dirty=false;tokenDraft="";await refreshQq();}).catch(e=>toastError(String(e))).finally(()=>{save.disabled=false;});});host.append(form);return;
  }
  if(tab==="groups"){renderGroups(host);return;}
  if(tab==="memory"||tab==="log"){
    const select=element("select");select.setAttribute("aria-label",t("选择 QQ 会话"));
    for(const peer of snapshot.peers){const option=element("option",peer.title);option.value=key(peer.identity);option.selected=selected&&key(peer.identity)===key(selected.identity);select.append(option);}
    select.addEventListener("change",()=>{selected=snapshot.peers.find(peer=>key(peer.identity)===select.value);void renderRecords(host).catch(e=>toastError(String(e)));});
    host.append(select);const content=element("div");content.id="qq-records";host.append(content);void renderRecords(host).catch(e=>toastError(String(e)));
  }
}
function groupDefaults(id="",name=""){return{id,name,enabled:true,interval_seconds:120,proactive_minutes:0,quiet_start:0,quiet_end:8,persona:""};}
function editGroup(group=null){groupDraft=structuredClone(group||groupDefaults());editingId=group?.id||null;renderPage();$("qq-group-editor [name='id']")?.focus();}
function committedGroups(settings){reading++;snapshot.settings.groups=settings.groups;if(socialManagement("qq"))renderPage();}
async function removeGroup(id){const settings=await invoke("qq_group_delete",{id});if(editingId===id){groupDraft=null;editingId=null;}committedGroups(settings);await refreshQq();}
function renderGroups(host){
  const toolbar=element("div",null,"qq-toolbar");
  toolbar.append(button(t("添加群"),()=>editGroup(),"primary"),button(t("从 QQ 选择群"),async()=>{
    const directory=await invoke("qq_directory");
    openMenu(toolbar.lastElementChild,directory.groups.map(group=>({label:group.group_name||String(group.group_id),desc:String(group.group_id),disabled:snapshot.settings.groups.some(item=>item.id===String(group.group_id)),onSelect:()=>{groupDraft=groupDefaults(String(group.group_id),group.group_name);editingId=null;renderPage();}})),{label:t("选择群聊")});
  }));host.append(toolbar);
  if(groupDraft){
    const editor=element("form",null,"qq-form");editor.id="qq-group-editor";
    const heading=element("div",null,"qq-rule-heading");heading.append(element("strong",editingId?t("编辑群规则"):t("添加群")));editor.append(heading);
    const id=field(editor,"群号","id",groupDraft.id);id.required=true;id.readOnly=!!editingId;id.inputMode="numeric";
    field(editor,"名称","name",groupDraft.name);field(editor,"参与群聊","enabled",groupDraft.enabled,"checkbox");
    field(editor,"发言间隔（秒）","interval_seconds",groupDraft.interval_seconds,"number").min="1";
    for(const [name,label]of[["quiet_start","安静时段开始"],["quiet_end","安静时段结束"]]){const input=field(editor,label,name,groupDraft[name],"number");input.min="0";input.max="23";}
    field(editor,"本群人设","persona",groupDraft.persona,"textarea");
    editor.addEventListener("input",()=>{for(const input of editor.querySelectorAll("[name]"))groupDraft[input.name]=input.type==="checkbox"?input.checked:input.type==="number"?Number(input.value):input.value;});
    const actions=element("div",null,"qq-toolbar"),save=element("button",t("保存群聊规则"),"primary");save.type="submit";
    actions.append(save,button(t("取消"),()=>{groupDraft=null;editingId=null;renderPage();}));if(editingId)actions.append(button(t("删除规则"),()=>removeGroup(editingId),"ghost danger"));editor.append(actions);
    editor.addEventListener("submit",event=>{event.preventDefault();save.disabled=true;void invoke("qq_group_save",{group:{...groupDraft,id:groupDraft.id.trim(),name:groupDraft.name.trim()},existingId:editingId}).then(async settings=>{groupDraft=null;editingId=null;committedGroups(settings);await refreshQq();}).catch(e=>toastError(String(e))).finally(()=>{save.disabled=false;});});host.append(editor);return;
  }
  const list=element("div",null,"qq-rule-list");
  if(!snapshot.settings.groups.length)list.append(element("p",t("暂无群规则"),"dim"));
  for(const group of snapshot.settings.groups){const row=element("article",null,"qq-rule-row");row.dataset.groupId=group.id;const identity=element("div",null,"qq-rule-identity");identity.append(element("strong",group.name||group.id),element("span",group.id,"dim"));const state=element("span",t(group.enabled?"已启用":"已暂停"),"qq-rule-state");row.append(identity,state,button(t("编辑"),()=>editGroup(group)),button(t("删除规则"),()=>removeGroup(group.id),"ghost danger"));list.append(row);}host.append(list);
}
async function renderRecords(host) {
  const content = host.querySelector("#qq-records"); content.replaceChildren(); if (!selected) return;
  const identity = selected.identity, generation = key(identity), targetTab = tab;
  const data = await invoke(tab === "memory" ? "qq_memories" : "qq_peer_log", { identity });
  if (!selected || generation !== key(selected.identity) || targetTab !== tab || !content.isConnected) return;
  if (tab === "memory") {
    content.append(button(t("新建记忆"), () => memoryEditor(content, identity)));
    for (const entry of data) {
      const row = element("article", null, "qq-memory-entry");
      row.append(element("strong", entry.title), element("span", `${entry.status} ${entry.scope}`, "dim"), element("p", entry.body), button(t("编辑"), () => memoryEditor(content, identity, entry))); content.append(row);
    }
  } else {
    for (const message of data.messages) { const row = element("article", null, "qq-log-entry"); row.append(element("small", `${message.nickname} (${message.sender}) ${new Date(message.at).toLocaleString()}`), element("p", message.text)); content.append(row); }
    content.append(element("h2", t("发送回执")));
    for (const receipt of data.deliveries) { const row = element("article", null, "qq-log-entry"); row.append(element("strong", receipt.state), element("p", receipt.body)); if (receipt.error) row.append(element("p", receipt.error, "dim")); content.append(row); }
  }
}
function memoryEditor(host, identity, entry = null) {
  host.querySelector("#qq-memory-editor")?.remove();
  const editor = element("form", null, "qq-form"); editor.id = "qq-memory-editor";
  field(editor, "标题", "title", entry?.title || ""); field(editor, "召回线索", "description", entry?.description || ""); field(editor, "正文", "body", entry?.body || "", "textarea");
  const save = element("button", t("保存记忆"), "primary"); save.type = "submit"; editor.append(save);
  editor.addEventListener("submit", event => {
    event.preventDefault();
    const values = Object.fromEntries(new FormData(editor));
    void invoke("qq_memory_save", { identity, id: entry?.id || null, expectedHash: entry?.revision || null, ...values }).then(() => { dirty = false; renderPage(); }).catch(error => toastError(String(error)));
  }); host.prepend(editor);
}

defer(()=>{
  $("qq-refresh").addEventListener("click",()=>{void refreshQq().catch(e=>toastError(String(e)));});
  for(const[id,label]of[["connection","连接"],["groups","群聊规则"],["persona","人设"],["memory","记忆"],["log","记录"]]){const item=button(t(label),()=>changeTab(id));item.dataset.tab=id;item.setAttribute("role","tab");$("qq-tabs").append(item);}
  document.addEventListener("kz:social-management",()=>{if(socialManagement("qq"))renderPage();else captureSettings();});
  document.addEventListener("kz:conversation-selected",syncSelection);
  document.addEventListener("kz:language",()=>{if(socialManagement("qq")&&!dirty)renderPage();});
  on("kz:qq",()=>{void refreshQq().catch(e=>toastError(String(e)));});
  void refreshQq().catch(e=>toastError(String(e)));
});
