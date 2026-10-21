import { createReadResource } from "./00-resource.js";
import { invoke } from "./00-ipc.js";
import { activeProcessId, activeSessionId, currentProject } from "./03-shell.js";
import { contextBillLabel } from "./13-memory.js";
import { t } from "./02-i18n.js";
import { contextGroups, requestSources } from "./07-context-model.js";
import { resourceLinks } from "./04-resource-types.js";
import { structuredNav } from "./04-structured.js";
import { conversationEpoch } from "./15-views-misc.js";

let activeView = null;
const node=(tag,cls,text)=>{const el=document.createElement(tag);if(cls)el.className=cls;if(text!==undefined)el.textContent=text;return el;};
const number=value=>Number(value).toLocaleString();
const button=(label,action)=>{const el=node("button","ghost mini",label);el.type="button";el.addEventListener("click",action);return el;};
const sameOwner=view=>activeView===view && view.host.isConnected && view.scope.sessionId===activeSessionId && view.scope.projectDir===currentProject && view.scope.epoch===conversationEpoch(activeSessionId);

function row(label,value) {
  const el=node("div","context-fact");el.append(node("span","dim",label),node("span","",value));return el;
}

async function inspect(view, block, destination) {
  const body=node("pre","context-original",t("读取记录中…"));destination.replaceChildren(body);
  try {
    const text=await invoke("context_content",{projectDir:view.scope.projectDir,processId:view.scope.processId,sequence:view.data.selected.sequence,sha256:block.sha256});
    if(sameOwner(view) && body.isConnected) body.textContent=text;
  } catch(error) { if(sameOwner(view) && body.isConnected){body.textContent=String(error);body.setAttribute("role","alert");} }
}

function blockDetails(view, block) {
  const detail=node("details","context-block"), summary=node("summary");
  const title=block.kind==="system"?contextBillLabel(block.label):block.label;
  const size=block.kind==="attachment"?`${number(block.bytes)} ${t("编码字节")}`:`${number(block.chars)} ${t("字符")}`;
  summary.append(node("span","context-block-label",title),node("span","dim",size));detail.append(summary);
  const body=node("div","context-block-body");body.append(row(t("输入角色"),block.role),row(t("内容指纹"),block.sha256));
  if(block.source?.input) body.append(node("pre","context-parameters",JSON.stringify(block.source.input,null,2)));
  const original=node("div");body.append(original);
  detail.addEventListener("toggle",()=>{if(detail.open && !original.children.length){if(block.inspectable)void inspect(view,block,original);else original.append(node("p","dim",t("二进制附件保留身份与大小，原件在原始消息中。")));}});
  detail.append(body);return detail;
}

function assembly(view) {
  const manifest=view.data.selected.manifest, box=node("div");
  box.append(row(t("请求输入估算"),`${number(manifest.estimated_tokens)} tokens`),node("p","dim context-note",t("分项按字符统计；输入估算与服务端计费分别记录。")));
  if(manifest.execution) {
    const execution=node("details","context-assembly-group");execution.append(node("summary","",t("运行快照")));
    execution.append(node("pre","context-parameters",JSON.stringify({execution:manifest.execution,provider_channel:manifest.provider_channel,request_parameters:manifest.request_parameters},null,2)));box.append(execution);
  }
  for(const group of contextGroups(manifest)) {
    const detail=node("details","context-assembly-group"), summary=node("summary");
    summary.append(node("span","",t(group.label)),node("span","dim",`${number(group.chars)} ${t("字符")} · ${group.blocks.length}`));detail.append(summary);
    for(const block of group.blocks) detail.append(blockDetails(view,block));box.append(detail);
  }
  const order=node("details","context-assembly-group");order.append(node("summary","",t("实际装配顺序")));
  const list=node("ol","context-order");for(const block of manifest.blocks)list.append(node("li","",`${block.position+1} · ${block.role} · ${block.kind==="system"?contextBillLabel(block.label):block.label}`));order.append(list);box.append(order);
  if(manifest.source_reads?.length) {
    const reads=node("details","context-assembly-group");reads.append(node("summary","",t("来源装载结果")));
    for(const source of manifest.source_reads) {
      const status={loaded:"已装入",empty:"内容为空",failed:"读取失败"}[source.status]||source.status;
      reads.append(row(contextBillLabel(source.key),`${t(status)} · ${source.refresh_each_step?t("每步刷新"):t("运行快照")}`));
      if(source.error)reads.append(node("p","context-note",source.error));
    }
    box.append(reads);
  }
  const changes=node("details","context-assembly-group");changes.append(node("summary","",t("请求整理与协议指纹")));
  changes.append(row(t("消息数"),number(manifest.message_count)),row(t("合并的重复读取"),number(manifest.repeated_results)),row(t("重复正文减少"),`${number(manifest.omitted_bytes)} bytes`),row(t("请求协议"),manifest.protocol),row(t("线上请求指纹"),manifest.wire_sha256));
  for(const change of manifest.adjustments || []) {
    const label={pruned:"机械整理",compacted:"压缩纪要",work_item:"工作切换整理"}[change.kind] || change.kind;
    const report=change.report || change;
    changes.append(row(t(label),`${number(report.before_tokens)} → ${number(report.after_tokens)} tokens`));
  }
  changes.append(node("p","dim context-note",t("清单记录协议转换前的最终装配输入；协议指纹对应客户端实际构造的请求体。")));box.append(changes);
  return box;
}

function sources(view) {
  const box=node("div"), rows=requestSources(view.data.selected.manifest,view.data.web);
  box.append(node("p","dim context-note",t("工具结果以选中请求为准；网页获取记录属于本段对话，不代表正文已装入。")));
  if(!rows.length)box.append(node("p","dim",t("该请求没有已记录的资料结果。")));
  for(const source of rows) {
    const detail=node("details","context-source"), summary=node("summary");summary.append(node("span","context-source-title",source.label));
    const status=source.included?(source.tool==="attachment"?t("附件已携带"):source.failed?t("错误结果已携带"):t("结果片段已携带")):t("已获取原文 · 纳入状态未记录");
    summary.append(node("span","dim context-source-meta",`${source.tool} · ${status}`));detail.append(summary);
    for(const acquisition of source.acquisitions || []) {
      detail.append(row(t("获取时间"),acquisition.fetched_at_utc),row(t("原文指纹"),acquisition.artifact?.sha256||""),row(t("原文快照"),acquisition.artifact?.path||""));
    }
    if(/^https?:\/\//i.test(source.label)) detail.append(button(t("打开原始链接"),()=>structuredNav.openUrl(source.label)));
    for(const block of source.blocks || [])detail.append(blockDetails(view,block));
    box.append(detail);
  }
  const mentioned=node("div","context-mentioned");box.append(mentioned);
  const sequence=view.data.selected.sequence;
  const messages=(view.data.selected.manifest.blocks || []).filter(block=>block.kind==="message"&&block.role==="user"&&block.inspectable);
  // Reuse the shared resource parser. Inspecting message links is independent
  // from fetching their destinations, and does not manufacture a read receipt.
  void Promise.allSettled(messages.map(block=>invoke("context_content",{projectDir:view.scope.projectDir,processId:view.scope.processId,sequence,sha256:block.sha256}))).then(results=>{
    if(!sameOwner(view)||!mentioned.isConnected||view.data.selected.sequence!==sequence)return;
    const seen=new Set(rows.flatMap(source=>[source.label,source.input?.url,source.acquisition?.original_url]).filter(Boolean));
    for(const result of results) {
      if(result.status==="rejected"){mentioned.append(node("p","dim",`${t("消息链接未能完整读取")}: ${result.reason}`));continue;}
      for(const link of resourceLinks(result.value)) {
        if(!/^https?:\/\//i.test(link.url)||seen.has(link.url))continue;seen.add(link.url);
        const item=node("div","context-source");item.append(button(link.url,()=>structuredNav.openUrl(link.url)),node("p","dim context-note",t("消息中的链接 · 尚无对应正文读取记录")));mentioned.append(item);
      }
    }
  });
  return box;
}

function paint(view) {
  if(!sameOwner(view))return;
  const data=view.data, host=view.host;host.replaceChildren();
  if(!data.selected){host.append(node("p","dim context-note",t("这段对话尚未记录请求装配清单。后续请求会在这里保留来源、顺序和内容版本。")));return;}
  const selector=node("select","context-request-select");selector.setAttribute("aria-label",t("选择请求记录"));
  for(const request of data.requests){const option=node("option","",`${request.sequence===data.requests[0].sequence?t("最近请求"):t("历史请求")} · #${request.sequence} · ${t("第")}${request.step}${t("步")} · ${request.model}`);option.value=String(request.sequence);option.selected=request.sequence===data.selected.sequence;selector.append(option);}
  selector.addEventListener("change",()=>{view.sequence=Number(selector.value);void load(view);});host.append(selector);
  host.append(node("p","dim context-note",`${t("已发起请求")} · ${data.selected.turnId} · ${new Date(data.selected.at).toLocaleString()}`));
  const tabs=node("div","context-tabs"), body=node("div","context-view-body");tabs.setAttribute("role","tablist");tabs.setAttribute("aria-label",t("上下文查看方式"));
  for(const [id,label,render] of [["assembly","装配详情",assembly],["sources","来源资料",sources]]) {
    const tab=button(t(label),()=>{view.tab=id;paint(view);host.querySelector(`[data-context-tab="${id}"]`).focus();});tab.dataset.contextTab=id;tab.setAttribute("role","tab");tab.setAttribute("aria-selected",String(view.tab===id));tab.id=`context-tab-${id}`;tab.setAttribute("aria-controls","context-tab-body");tabs.append(tab);
    if(view.tab===id){body.id="context-tab-body";body.setAttribute("role","tabpanel");body.setAttribute("aria-labelledby",tab.id);body.append(render(view));}
  }
  tabs.addEventListener("keydown",event=>{if(event.key!=="ArrowLeft"&&event.key!=="ArrowRight")return;event.preventDefault();view.tab=view.tab==="assembly"?"sources":"assembly";paint(view);host.querySelector(`[data-context-tab="${view.tab}"]`).focus();});
  host.append(tabs,body);
}

async function load(view) {
  const status=node("p","dim",t("读取装配记录中…"));view.host.replaceChildren(status);
  try {const data=await view.resource.load({force:true});if(!sameOwner(view))return;if(data.sessionId!==view.scope.sessionId)throw new Error(t("上下文记录不属于当前对话"));if(data.selected&&data.selected.manifest?.format_version!==1)throw new Error(t("不支持的上下文记录格式"));view.data=data;view.sequence=data.selected?.sequence??null;paint(view);}
  catch(error){if(sameOwner(view)){status.textContent=String(error);status.setAttribute("role","alert");}}
}

export function renderRequestContext(host) {
  const view={host,scope:{projectDir:currentProject,processId:activeProcessId,sessionId:activeSessionId,epoch:conversationEpoch(activeSessionId)},sequence:null,tab:"assembly",data:null};
  activeView=view;
  if(!view.scope.projectDir||!view.scope.sessionId){host.replaceChildren(node("p","dim",t("选择对话后查看上下文装配。")));return;}
  view.resource=createReadResource(()=>invoke("context_snapshot",{projectDir:view.scope.projectDir,processId:view.scope.processId,sequence:view.sequence}));
  void load(view);
}

export function notifyContextRequest(sessionId) {
  const view=activeView;if(!view||!sameOwner(view)||sessionId!==view.scope.sessionId)return;
  // Keep the inspected request fixed while a run continues. A new request is
  // an explicit navigation action, so text selection and open evidence survive.
  if(view.host.querySelector(".context-new-request"))return;
  const latest=button(t("有新请求 · 查看最新装配"),()=>{view.sequence=null;void load(view);});latest.classList.add("context-new-request");view.host.prepend(latest);
}
