/* Local audit viewer. Review notes never write project source or formal decisions. */
(() => {
  "use strict";
  const data = window.KANZEI_AUDIT;
  const $ = id => document.getElementById(id);
  const esc = value => String(value ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
  const storageKey = "kanzei.code-audit.reviews.v1";
  let reviews = {};
  try { const saved = JSON.parse(localStorage.getItem(storageKey) || "{}"); if (saved && typeof saved === "object" && !Array.isArray(saved)) reviews = saved; } catch { /* Empty state remains usable. */ }
  const viewInfo = {
    features: ["整理候选", "只列值得收拢、简化或改为可选的能力。"],
    decisions: ["A 决策对照", "保留原状态，逐条对照当前实现。"],
    files: ["源码索引", "按文件或符号检索，查看源码位置与关联问题。"],
    logic: ["实现说明", "查看运行、工作、记忆、研究与桌面的具体规则。"],
    issues: ["问题记录", "你已处理的问题放在这里，默认不再作为待办。"]
  };
  const labels = {HEAD:"已提交",modified:"已修改",untracked:"未跟踪"};
  const fileRecords = data.files.map(f => ({...f,id:f.path,title:f.path.split("/").pop(),issues:data.findings.filter(i => !i.handled && i.refs.some(([p]) => p === f.path)).map(i => i.id)}));
  const symbolRecords = fileRecords.flatMap(f => f.symbols.map(s => ({...s,id:`${f.path}:${s.line}`,title:s.name,path:f.path,domain:f.domain,status:f.status,lines:f.lines,issues:f.issues})));
  const logicDomains = {runtime:"运行引擎","work-decisions":"工作与决策","memory-storage":"记忆与存储","tools-research":"工具与研究","desktop-delivery":"桌面与交付","cleanup-decisions":"整理建议"};
  const collections = {issues:data.findings,features:data.candidates,decisions:data.decisions,files:fileRecords,logic:data.documents.map(d => ({...d,domain:logicDomains[d.domain] || d.domain}))};
  const state = {view:"features",query:"",domain:"",kind:"",priority:"",certainty:"",review:"",quick:"all",sort:"priority",page:1,selected:null};
  let filtered = [];
  let sourceState = null;
  let sourceRequest = 0;
  const priority = record => record.priority || Math.min(...(record.issues || []).map(id => data.findings.find(i => i.id === id)?.priority || 4), 4);
  const issueIds = record => record.issues || [];
  const records = () => state.view === "files" && state.quick === "symbols" ? symbolRecords : collections[state.view];
  const reviewed = id => reviews[id]?.status && reviews[id].status !== "unreviewed";
  const implemented = id => data.candidates.find(candidate => candidate.id === id)?.resolution?.status === "done";
  const finishedCandidate = id => implemented(id) || reviews[id]?.choice === "保留" || reviews[id]?.status === "done";
  const handledIssue = record => record.handled || reviews[record.id]?.status === "done";
  const sourceButton = (path,line=1,label=path) => `<button class="source-link" data-source="${esc(path)}" data-line="${Number(line)}">${esc(label)} <span aria-hidden="true">↗</span></button>`;
  const badge = (p,text) => `<span class="badge priority-${p}">${esc(text || `P${p}`)}</span>`;
  const codeLines = (text,start,target) => `<pre class="code-lines"><code>${String(text).split("\n").map((line,index) => `<span class="code-line ${index+start===target?"highlight":""}"><span class="code-number">${index+start}</span><span class="code-text">${esc(line)}</span></span>`).join("")}</code></pre>`;
  function toast(message) { $("toast").textContent=message; $("toast").hidden=false; clearTimeout(toast.timer); toast.timer=setTimeout(() => {$("toast").hidden=true;},2200); }
  function setHash() { const query = new URLSearchParams({view:state.view}); if(state.selected) query.set("id",state.selected); history.replaceState(null,"",`#${query}`); }
  function populateFilters() {
    const domainLabel = state.view === "files" ? "全部模块" : state.view === "decisions" ? "全部状态" : "全部功能域";
    const domains = state.view === "decisions" ? ["accepted","draft"] : [...new Set(records().map(r => r.domain).filter(Boolean))];
    $("domain").innerHTML=`<option value="">${domainLabel}</option>` + domains.map(x => `<option>${esc(x)}</option>`).join("");
    $("domain").value=state.domain;
    const kindRecords=state.view==="features"?data.candidates:data.findings;
    $("kind").innerHTML=`<option value="">${state.view==="features"?"全部整理方向":"全部问题类型"}</option>`+[...new Set(kindRecords.map(i=>i.kind))].map(x=>`<option>${esc(x)}</option>`).join("");
    $("kind").setAttribute("aria-label",state.view==="features"?"整理方向":"问题类型");
    $("kind").value=state.kind;
    for(const id of ["kind-wrap","review-wrap"]) $(id).hidden=!["issues","features"].includes(state.view);
    $("certainty-wrap").hidden=state.view!=="issues";
    $("priority-wrap").hidden=state.view!=="issues";
    $("sort").querySelector('[value="size"]').hidden=state.view!=="files";
    $("sort").querySelector('[value="priority"]').textContent=state.view==="issues"?"优先级排序":"默认排序";
  }
  function quickFilters() {
    const options = state.view==="issues" ? [["archive","已处理"],["all","待处理"]] : state.view==="features" ? [["all","待整理"],["consolidate","收拢 / 简化"],["optional","可选能力"],["reviewed","已整理"]] : state.view==="files" ? [["all","文件"],["symbols","符号"],["dirty","未提交文件"]] : [["all","全部"]];
    $("quick-filters").innerHTML=options.map(([key,label])=>`<button data-quick="${key}" class="quick-button ${state.quick===key?"active":""}" aria-pressed="${state.quick===key}">${label}</button>`).join("");
    if(state.view==="logic") $("quick-filters").innerHTML='<span class="certainty">实现规则 · 只读报告</span>';
  }
  function applyFilters() {
    const terms=state.query.toLowerCase().trim().split(/\s+/).filter(Boolean);
    filtered=records().filter(r => {
      if(state.domain && (state.view==="decisions"?r.status:r.domain)!==state.domain) return false;
      if(state.priority && priority(r)!==Number(state.priority)) return false;
      if(state.view==="issues") {
        if(state.quick==="archive" ? !handledIssue(r) : handledIssue(r)) return false;
        if(state.kind && r.kind!==state.kind || state.certainty && r.certainty!==state.certainty) return false;
        if(state.review==="unreviewed" && reviewed(r.id) || state.review==="reviewed" && !reviewed(r.id) || state.review==="done" && reviews[r.id]?.status!=="done") return false;
      } else if(state.view==="features") {
        if(state.quick==="reviewed" ? !finishedCandidate(r.id) : finishedCandidate(r.id)) return false;
        if(state.quick==="consolidate" && r.kind==="可选能力" || state.quick==="optional" && r.kind!=="可选能力") return false;
        if(state.kind && r.kind!==state.kind) return false;
        if(state.review==="unreviewed" && reviewed(r.id) || state.review==="reviewed" && !reviewed(r.id) || state.review==="done" && reviews[r.id]?.status!=="done") return false;
      }
      if(state.view==="files" && state.quick==="dirty" && r.status==="HEAD") return false;
      const haystack=[r.id,r.title,r.domain,r.kind,r.certainty,r.rationale,(r.member_names||[]).join(" "),r.description,r.implementation,r.resolution?.summary,r.resolution?.verification,r.recommendation,r.entry,r.path,r.header,r.signature,r.original,r.text,(r.paths||[]).join(" "),(r.refs||[]).map(x=>x[0]).join(" "),reviews[r.id]?.note].filter(Boolean).join(" ").toLowerCase();
      return terms.every(t=>haystack.includes(t));
    });
    if(state.sort==="name") filtered.sort((a,b)=>a.title.localeCompare(b.title,"zh-CN"));
    else if(state.sort==="size") filtered.sort((a,b)=>(b.lines||0)-(a.lines||0));
    else if(["issues","features"].includes(state.view)) filtered.sort((a,b)=>priority(a)-priority(b) || a.id.localeCompare(b.id));
    else if(state.quick==="flagged") filtered.sort((a,b)=>priority(a)-priority(b));
  }
  function row(record) {
    const selected=state.selected===record.id?"selected":"";
    if(state.view==="files") {
      const symbol=state.quick==="symbols";
      return `<button class="result-row file-row ${selected}" data-record="${esc(record.id)}" aria-pressed="${!!selected}"><span class="row-top"><span class="row-title ${symbol?"":"file-path"}">${esc(symbol?record.title:record.path)}</span><span class="row-lines">${symbol?`L${record.line}`:`${record.lines.toLocaleString()} 行`}</span></span><span class="row-summary">${esc(symbol?record.path:record.header||record.domain)}</span><span class="row-tags"><span>${esc(labels[record.status]||record.status)}</span><span>${esc(record.domain)}</span>${record.issues.length?badge(priority(record),`${record.issues.length} 项问题`):""}</span></button>`;
    }
    const isIssue=state.view==="issues";
    const summary=isIssue?record.implementation:state.view==="features"?record.resolution?.summary||record.rationale:state.view==="decisions"?record.implementation:record.text.replace(/^## .+\n/,"");
    const tags=isIssue?`<span class="badge">${handledIssue(record)?"已处理":"待处理"}</span><span>${esc(record.kind)}</span><span>${esc(record.domain)}</span><span class="certainty">${esc(record.certainty)}</span>`:`<span>${esc(record.domain||record.status||"")}</span><span>${esc(record.kind||"")}</span>${reviewed(record.id)?'<span class="review-mark">已标注</span>':""}`;
    return `<button class="result-row ${selected}" data-record="${esc(record.id)}" aria-pressed="${!!selected}"><span class="row-top"><span class="row-id">${esc(record.id)}</span><span class="row-title">${esc(record.title)}</span></span><span class="row-summary">${esc(summary)}</span><span class="row-tags">${state.view==="features"&&implemented(record.id)?'<span class="review-mark">已修复 / 整理</span>':""}${tags}</span></button>`;
  }
  function section(title,text,extra="") { return `<section class="detail-section ${extra}"><h3>${esc(title)}</h3><p>${esc(text)}</p></section>`; }
  function related(ids,view="issues") { return `<div class="related">${ids.map(id=>`<button data-target="${esc(id)}" data-target-view="${view}">${esc(id)}</button>`).join("")}</div>`; }
  function evidence(record) {
    return `<section class="detail-section"><h3>源码依据</h3>${record.evidence.map((key,index)=>{
      const ref=data.evidence[key];
      return `<details class="evidence" ${index===0?"open":""}><summary>${esc(ref.path)}:${ref.line}</summary>${codeLines(ref.text,ref.start,ref.line)}${sourceButton(ref.path,ref.line,"查看源码上下文")}</details>`;
    }).join("")}</section>`;
  }
  function reviewForm(record) {
    const current=reviews[record.id]||(record.handled?{status:"done"}:{});
    const choices=["尚未决定","修复","改写说明","统一边界","合并","简化","改为可选","归档","保留","删除候选","继续调查"];
    return `<section class="review-form"><h3>我的标注</h3><div class="review-fields"><label>处理判断<select id="review-choice">${choices.map(x=>`<option ${x===current.choice?"selected":""}>${x}</option>`).join("")}</select></label><label>进度<select id="review-status"><option value="unreviewed" ${!current.status||current.status==="unreviewed"?"selected":""}>未审阅</option><option value="reviewed" ${current.status==="reviewed"?"selected":""}>已审阅</option><option value="done" ${current.status==="done"?"selected":""}>已处理</option></select></label></div><label class="note-label">备注<textarea id="review-note" placeholder="记录你的判断、保留原因或下一步…">${esc(current.note||"")}</textarea></label><span class="save-state" id="save-state">${current.updatedAt?"已保存在此浏览器":"选择或填写后自动保存"}</span></section>`;
  }
  function renderDetail() {
    const record=records().find(r=>r.id===state.selected);
    if(!record) { $("detail").innerHTML='<p class="detail-intro">选择一项，查看实现和源码依据。</p>';return; }
    let content=`<div class="detail-kicker"><code>${esc(record.id)}</code>${state.view==="issues"?badge(record.priority):""}<span>${esc(record.domain||record.status||"")}</span></div><h2 class="detail-title">${esc(record.title)}</h2>`;
    if(state.view==="issues") {
      content+=`<p class="detail-intro">${esc(record.impact)}</p>`+section("原描述 / 现有约束",record.description)+section("实际实现",record.implementation)+section(`建议 · ${record.action}`,record.recommendation,"recommendation");
      if(record.certainty!=="源码确认") content+=section("还需确认",record.certainty==="待运行确认"?"命令差异已确认，具体运行结果仍需要实测。":"这是整理判断，不能仅凭相似或调用少就直接删除。" );
      if(record.decisions.length) content+=`<section class="detail-section"><h3>关联 A 决策</h3>${related(record.decisions,"decisions")}</section>`;
      content+=evidence(record)+reviewForm(record);
    } else if(state.view==="features") {
      if(record.resolution?.status==="done") content+=section(`已完成 · ${record.resolution.date}`,record.resolution.summary,"recommendation")+section("验证结果",record.resolution.verification);
      content+=section("为什么列入整理",record.rationale)+section("建议",record.recommendation,"recommendation")+section("涉及功能",record.member_names.join("、"))+section("使用入口",record.entry)+section("当前实现",record.implementation);
      if(record.issues.length) content+=`<section class="detail-section"><h3>优先查看这些问题</h3>${related(record.issues)}</section>`;
      content+=`<section class="detail-section"><h3>源码入口</h3>${record.paths.length?record.paths.map(p=>sourceButton(p)).join(""):'<p>在源码索引中按模块名查找；完整调用链见实现说明。</p>'}</section>`;
      content+=reviewForm(record);
    } else if(state.view==="decisions") {
      content+=section("当前实现与整理建议",record.implementation,"recommendation");
      if(record.issues.length) content+=`<section class="detail-section"><h3>关联问题</h3>${related(record.issues)}</section>`;
      content+=`<section class="detail-section"><h3>原决策 · ${esc(record.status)}</h3><p style="white-space:pre-line">${esc(record.original)}</p>${sourceButton(".kanzei/project/decisions.md",record.line,"查看正式决策原文")}</section>`;
    } else if(state.view==="files") {
      const file=fileRecords.find(f=>f.path===record.path);
      content+=`<dl class="metadata"><dt>路径</dt><dd>${esc(record.path)}</dd><dt>模块</dt><dd>${esc(record.domain)}</dd><dt>Git 状态</dt><dd>${esc(labels[record.status])}</dd><dt>源码行数</dt><dd>${record.lines.toLocaleString()}</dd>${file?`<dt>SHA256</dt><dd>${esc(file.sha256)}</dd>`:""}</dl>`;
      if(record.signature) content+=section("声明",record.signature);
      else if(record.header) content+=section("源码中的模块说明",record.header);
      content+=sourceButton(record.path,record.line||1,"查看源码");
      if(record.issues.length) content+=`<section class="detail-section"><h3>关联问题</h3>${related(record.issues)}</section>`;
      if(file?.symbols.length) content+=`<section class="detail-section"><h3>符号 · ${file.symbols.length} 条</h3><div class="symbol-list">${file.symbols.slice(0,100).map(s=>sourceButton(file.path,s.line,`${s.name} · L${s.line}`)).join("")}</div>${file.symbols.length>100?'<p>其余符号可用“符号”模式检索。</p>':""}</section>`;
    } else {
      content+=`<article class="document-reader">${record.html}</article><section class="detail-section"><h3>所属报告</h3><p>${esc(record.report)}</p></section>`;
    }
    $("detail").innerHTML=content;
    if(["issues","features"].includes(state.view)) {
      for(const name of ["review-choice","review-status","review-note"]) $(name).addEventListener(name==="review-note"?"input":"change",()=>saveReview(record.id));
    }
  }
  function saveReview(id) {
    const choice=$("review-choice").value;
    let status=$("review-status").value;
    if(choice!=="尚未决定" && status==="unreviewed") {status="reviewed";$("review-status").value=status;}
    reviews[id]={choice,status,note:$("review-note").value,updatedAt:new Date().toISOString(),snapshotHead:data.head};
    try {localStorage.setItem(storageKey,JSON.stringify(reviews));$("save-state").textContent="已保存在此浏览器";} catch {$("save-state").textContent="保存失败，请导出当前结果保留标注";}
    applyFilters();renderList();
  }
  function renderList() {
    const totalPages=Math.max(1,Math.ceil(filtered.length/50));state.page=Math.min(state.page,totalPages);
    const page=filtered.slice((state.page-1)*50,state.page*50);
    const label=state.view==="files"&&state.quick==="symbols"?"条符号":state.view==="files"?"个文件":"项";
    $("result-count").textContent=`${filtered.length.toLocaleString()} ${label}`;
    $("result-list").innerHTML=filtered.length?`<div class="list-header"><span>${state.view==="issues"?"问题与实际实现":state.view==="files"?"源码位置":"内容与实现"}</span><span>${state.view==="issues"?"默认优先处理":""}</span></div>`+page.map(row).join(""):"";
    $("empty-state").hidden=filtered.length>0;
    const allCompleted=state.view==="features"&&state.quick==="all"&&!state.query&&!state.domain&&!state.kind&&!state.review&&data.candidates.every(r=>implemented(r.id));
    $("empty-state").querySelector("h2").textContent=allCompleted?"本轮整理已完成":"没有匹配结果";
    $("empty-state").querySelector("p").textContent=allCompleted?"九项改动和验证结果在“已整理”中查看。":"换个关键词，或减少筛选条件。";
    $("pagination").innerHTML=totalPages>1?`<button class="button secondary" data-page="${state.page-1}" ${state.page===1?"disabled":""}>上一页</button><span>${state.page} / ${totalPages}</span><button class="button secondary" data-page="${state.page+1}" ${state.page===totalPages?"disabled":""}>下一页</button>`:"";
  }
  function render() {
    applyFilters();
    if(!filtered.some(r=>r.id===state.selected)) state.selected=filtered[(state.page-1)*50]?.id || filtered[0]?.id || null;
    renderList();renderDetail();setHash();
  }
  function navigate(view,id=null) {
    if(!viewInfo[view]) return;
    Object.assign(state,{view,query:"",domain:"",kind:"",priority:"",certainty:"",review:"",quick:view==="issues"?"archive":"all",sort:"priority",page:1,selected:id});
    $("search").value="";for(const key of ["kind","priority","certainty","review-filter"]) $(key).value="";
    $("sort").value="priority";$("page-title").textContent=viewInfo[view][0];$("page-description").textContent=viewInfo[view][1];
    $("navigation").innerHTML=Object.entries(viewInfo).map(([key,info])=>`<button data-view="${key}" class="nav-item ${key===view?"active":""}" ${key===view?'aria-current="page"':""}><span>${info[0]}</span><span class="nav-number">${collections[key].length}</span></button>`).join("");
    populateFilters();quickFilters();render();$("inspector").scrollTop=0;
  }
  function reset() {state.query="";state.domain="";state.kind="";state.priority="";state.certainty="";state.review="";state.quick=state.view==="issues"?"archive":"all";state.page=1;$("search").value="";for(const id of ["domain","kind","priority","certainty","review-filter"]) $(id).value="";quickFilters();render();}
  $("search").addEventListener("input",event=>{state.query=event.target.value;state.page=1;render();});
  for(const [id,key] of [["domain","domain"],["kind","kind"],["priority","priority"],["certainty","certainty"],["review-filter","review"],["sort","sort"]]) $(id).addEventListener("change",event=>{state[key]=event.target.value;state.page=1;render();});
  $("reset").addEventListener("click",reset);$("empty-reset").addEventListener("click",reset);
  document.addEventListener("click",event=>{
    const button=event.target.closest("button");if(!button)return;
    if(button.dataset.view) navigate(button.dataset.view);
    if(button.dataset.quick) {state.quick=button.dataset.quick;state.page=1;state.selected=null;quickFilters();populateFilters();render();}
    if(button.dataset.record) {state.selected=button.dataset.record;renderList();renderDetail();setHash();$("inspector").classList.add("mobile-open");$("inspector").scrollTop=0;}
    if(button.dataset.page) {state.page=Number(button.dataset.page);state.selected=null;render();$("content-split")?.scrollTo(0,0);document.querySelector(".results-region").scrollTop=0;}
    if(button.dataset.target) {navigate(button.dataset.targetView,button.dataset.target);$("inspector").classList.add("mobile-open");$("inspector").scrollTop=0;}
    if(button.dataset.source) openSource(button.dataset.source,Number(button.dataset.line||1));
  });
  $("inspector-close").addEventListener("click",()=>$("inspector").classList.remove("mobile-open"));
  document.addEventListener("keydown",event=>{if(event.key==="/"&&!/INPUT|TEXTAREA|SELECT/.test(event.target.tagName)){event.preventDefault();$("search").focus();}if(event.key==="Escape") $("inspector").classList.remove("mobile-open");});
  $("export").addEventListener("click",()=>{
    const output={exportedAt:new Date().toISOString(),snapshotHead:data.head,sourceSnapshotAt:data.snapshotAt,view:state.view,filters:{query:state.query,domain:state.domain,kind:state.kind,priority:state.priority,certainty:state.certainty,review:state.review,quick:state.quick},records:filtered.map(r=>({...r,review:reviews[r.id]||null})),allReviewNotes:reviews};
    const blob=new Blob([JSON.stringify(output,null,2)],{type:"application/json;charset=utf-8"});const url=URL.createObjectURL(blob);const link=document.createElement("a");link.href=url;link.download=`kanzei-audit-${state.view}-${new Date().toISOString().slice(0,10)}.json`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);toast(`已导出 ${filtered.length} 项及你的标注`);
  });
  async function openSource(path,line) {
    const request=++sourceRequest;
    $("source-title").textContent=path;$("source-meta").textContent="读取源码…";$("source-content").innerHTML="";$("source-range").textContent="";$("source-prev").disabled=true;$("source-next").disabled=true;
    if(!$("source-dialog").open) $("source-dialog").showModal();
    try {
      const response=await fetch(`/api/source?path=${encodeURIComponent(path)}&line=${line}`);
      if(!response.ok) throw new Error("无法读取源码");
      const result=await response.json();if(request!==sourceRequest)return;
      const expected=fileRecords.find(f=>f.path===path)?.sha256 || Object.values(data.evidence).find(e=>e.path===path)?.sha256;
      sourceState={path,...result};$("source-meta").textContent=`当前源码 · 共 ${result.total.toLocaleString()} 行${expected&&expected!==result.sha256?" · 已不同于报告快照":" · 与快照一致"}`;
      $("source-content").innerHTML=codeLines(result.text,result.start,line);$("source-range").textContent=`${result.start}–${result.end} / ${result.total}`;$("source-prev").disabled=result.start<=1;$("source-next").disabled=result.end>=result.total;
      $("source-content").scrollTop=0;
    } catch {
      if(request!==sourceRequest)return;
      const candidates=Object.values(data.evidence).filter(e=>e.path===path).sort((a,b)=>Math.abs(a.line-line)-Math.abs(b.line-line));const snippet=candidates[0];
      $("source-meta").textContent="快照片段 · 完整源码请从本地网页入口查看";
      $("source-content").innerHTML=snippet?codeLines(snippet.text,snippet.start,snippet.line):'<p class="detail-intro">完整源码预览需要本地服务。运行本目录的 node serve.mjs，再打开输出地址。文件和符号索引仍可正常筛选。</p>';
      sourceState=null;
    }
  }
  $("source-close").addEventListener("click",()=>{$("source-dialog").close();sourceRequest++;});
  $("source-dialog").addEventListener("click",event=>{if(event.target===$("source-dialog")){const rect=event.target.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom) event.target.close();}});
  $("source-prev").addEventListener("click",()=>{if(sourceState)openSource(sourceState.path,Math.max(1,sourceState.start-120));});
  $("source-next").addEventListener("click",()=>{if(sourceState)openSource(sourceState.path,sourceState.end+1);});
  const date=new Date(data.snapshotAt).toLocaleString("zh-CN",{timeZone:"Asia/Shanghai",hour12:false});
  $("snapshot-date").textContent=date;$("snapshot-head").textContent=data.head.slice(0,12);$("snapshot-branch").textContent=data.branch;$("footer-snapshot").textContent=`${data.files.length} 个文件 · ${symbolRecords.length.toLocaleString()} 条符号 · ${data.head.slice(0,8)}`;
  const initial=new URLSearchParams(location.hash.slice(1));
  const initialView=initial.get("view");
  navigate(initialView&&initialView!=="issues"?initialView:"features",initialView!=="issues"?initial.get("id"):null);
  window.addEventListener("hashchange",()=>{const next=new URLSearchParams(location.hash.slice(1));navigate(next.get("view")||"features",next.get("id"));});
})();
