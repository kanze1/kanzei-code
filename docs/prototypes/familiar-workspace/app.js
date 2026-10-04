(async () => {
  'use strict';
  const KEY = 'kanzei-familiar-prototype-v1';
  const FEEDBACK_KEY = `${KEY}-feedback`;
  // BEGIN GENERATED PROJECT INDEX
  const PROJECT_INDEX = {"head":"7b2b8096","manifestFingerprint":"d7a37e211112e4bb","source":"Cargo.toml + crates/*/Cargo.toml","nodes":[{"id":"kanzei-base","path":"crates/kanzei-base","manifest":"crates/kanzei-base/Cargo.toml","level":0},{"id":"kanzei-harness","path":"crates/kanzei-harness","manifest":"crates/kanzei-harness/Cargo.toml","level":1},{"id":"kanzei-llm","path":"crates/kanzei-llm","manifest":"crates/kanzei-llm/Cargo.toml","level":1},{"id":"kanzei-core","path":"crates/kanzei-core","manifest":"crates/kanzei-core/Cargo.toml","level":2},{"id":"kanzei-memory","path":"crates/kanzei-memory","manifest":"crates/kanzei-memory/Cargo.toml","level":3},{"id":"kanzei-tools","path":"crates/kanzei-tools","manifest":"crates/kanzei-tools/Cargo.toml","level":4},{"id":"kanzei","path":"crates/kanzei","manifest":"crates/kanzei/Cargo.toml","level":5},{"id":"kanzei-app","path":"crates/kanzei-app","manifest":"crates/kanzei-app/Cargo.toml","level":5}],"edges":[{"from":"kanzei-harness","to":"kanzei-base","kinds":["dependencies"],"source":"crates/kanzei-harness/Cargo.toml"},{"from":"kanzei-llm","to":"kanzei-base","kinds":["dependencies"],"source":"crates/kanzei-llm/Cargo.toml"},{"from":"kanzei-llm","to":"kanzei-harness","kinds":["dev-dependencies"],"source":"crates/kanzei-llm/Cargo.toml"},{"from":"kanzei-core","to":"kanzei-llm","kinds":["dependencies"],"source":"crates/kanzei-core/Cargo.toml"},{"from":"kanzei-core","to":"kanzei-harness","kinds":["dependencies"],"source":"crates/kanzei-core/Cargo.toml"},{"from":"kanzei-core","to":"kanzei-base","kinds":["dependencies"],"source":"crates/kanzei-core/Cargo.toml"},{"from":"kanzei-memory","to":"kanzei-base","kinds":["dependencies"],"source":"crates/kanzei-memory/Cargo.toml"},{"from":"kanzei-memory","to":"kanzei-harness","kinds":["dependencies"],"source":"crates/kanzei-memory/Cargo.toml"},{"from":"kanzei-memory","to":"kanzei-llm","kinds":["dependencies"],"source":"crates/kanzei-memory/Cargo.toml"},{"from":"kanzei-memory","to":"kanzei-core","kinds":["dependencies"],"source":"crates/kanzei-memory/Cargo.toml"},{"from":"kanzei-tools","to":"kanzei-base","kinds":["dependencies"],"source":"crates/kanzei-tools/Cargo.toml"},{"from":"kanzei-tools","to":"kanzei-harness","kinds":["dependencies"],"source":"crates/kanzei-tools/Cargo.toml"},{"from":"kanzei-tools","to":"kanzei-llm","kinds":["dependencies"],"source":"crates/kanzei-tools/Cargo.toml"},{"from":"kanzei-tools","to":"kanzei-memory","kinds":["dependencies"],"source":"crates/kanzei-tools/Cargo.toml"},{"from":"kanzei-tools","to":"kanzei-core","kinds":["dependencies","dev-dependencies"],"source":"crates/kanzei-tools/Cargo.toml"},{"from":"kanzei","to":"kanzei-core","kinds":["dependencies"],"source":"crates/kanzei/Cargo.toml"},{"from":"kanzei","to":"kanzei-llm","kinds":["dependencies"],"source":"crates/kanzei/Cargo.toml"},{"from":"kanzei","to":"kanzei-tools","kinds":["dependencies"],"source":"crates/kanzei/Cargo.toml"},{"from":"kanzei","to":"kanzei-harness","kinds":["dependencies"],"source":"crates/kanzei/Cargo.toml"},{"from":"kanzei-app","to":"kanzei-core","kinds":["dependencies"],"source":"crates/kanzei-app/Cargo.toml"},{"from":"kanzei-app","to":"kanzei-harness","kinds":["dependencies"],"source":"crates/kanzei-app/Cargo.toml"},{"from":"kanzei-app","to":"kanzei-llm","kinds":["dependencies"],"source":"crates/kanzei-app/Cargo.toml"},{"from":"kanzei-app","to":"kanzei-tools","kinds":["dependencies"],"source":"crates/kanzei-app/Cargo.toml"}]};
  // END GENERATED PROJECT INDEX
  const readTrialKey=key=>{try{return localStorage.getItem(key);}catch{return null;}};
  // Keep the v1 trial data when moving from the original preview server.
  // Only this prototype's two keys cross between its two loopback origins.
  if(location.origin==='http://127.0.0.1:14104'&&location.hash==='#prototype-state-transfer'&&window.parent!==window) {
    window.parent.postMessage({type:'kanzei-prototype-state',state:readTrialKey(KEY),feedback:readTrialKey(FEEDBACK_KEY)},'http://127.0.0.1:14105');return;
  }
  let importedTrial=false;
  if(location.origin==='http://127.0.0.1:14105'&&!readTrialKey(KEY)) {
    await new Promise(resolve=>{
      const frame=document.createElement('iframe');frame.hidden=true;frame.title='导入旧版试用数据';
      let timeout;
      const finish=()=>{clearTimeout(timeout);window.removeEventListener('message',receive);frame.remove();resolve();};
      const receive=event=>{
        if(event.origin!=='http://127.0.0.1:14104'||event.source!==frame.contentWindow||event.data?.type!=='kanzei-prototype-state')return;
        try {if(event.data.state){const prior=JSON.parse(event.data.state);if(prior.version===1&&Array.isArray(prior.conversations)){localStorage.setItem(KEY,event.data.state);importedTrial=true;}}
          if(event.data.feedback&&!localStorage.getItem(FEEDBACK_KEY)){const prior=JSON.parse(event.data.feedback);if(prior&&typeof prior==='object')localStorage.setItem(FEEDBACK_KEY,event.data.feedback);}
        } catch {} finish();
      };
      window.addEventListener('message',receive);frame.src='http://127.0.0.1:14104/#prototype-state-transfer';
      timeout=setTimeout(finish,1800);document.body.append(frame);
    });
  }
  const $ = selector => document.querySelector(selector);
  const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const icon = (name, small = false) => `<svg class="icon${small ? ' small' : ''}" aria-hidden="true"><use href="#i-${name}"/></svg>`;
  const uid = prefix => `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
  const defaults = () => ({ model:'GPT-6 Luna', thinking:'超高', mode:'自主推进', whip:false, subagents:false });
  const makeConversation = (id, pid, title, messages = []) => ({id,pid,title,messages,draft:'',attachments:[],queue:[],config:defaults(),phase:'idle',run:null,delivery:'queue',sideMessages:[]});
  const statuses = {draft:'草稿',doing:'进行中',pending:'待开始',blocked:'已阻塞',review:'待验收',done:'已完成'};
  const seed = () => ({version:1,theme:'light',permission:'auto',current:{view:'chat',pid:'kanzei',cid:'front'},
    projects:[{id:'kanzei',name:'kanzei code',branch:'dev'},{id:'reader',name:'阅读器',branch:'main'}],
    conversations:[
      makeConversation('front','kanzei','整理前端交互',[
        {id:'m1',role:'user',text:'先把输入区和项目管理页整理一下。常用操作保持原来的位置，需求列表只看工作条目。'},
        {id:'m2',role:'assistant',text:'先保留你已经习惯的操作位置。模型、思考、模式和权限仍放在输入附近，发送与停止也留在右下角。\n\n输入框上方的项目、分支和改动移到页头；鞭挞、子代理与旁路提问收进更多菜单。只有运行时，才出现进度与排队入口。\n\n“概览”改为“管理”，集中看工作和交付。右侧工作清单只列需求与缺陷，负责对话放进条目详情。',tools:true}
      ]),
      makeConversation('search','kanzei','搜索与抓取升级',[
        {id:'s1',role:'user',text:'把模型搜索和网页抓取的结果说明统一一下。'},
        {id:'s2',role:'assistant',text:'当前示例工作 R-365 还剩两个批次。可以从工作清单查看目标、批次和验收条件。'}
      ]),
      makeConversation('reader-chat','reader','阅读进度与书签',[
        {id:'r1',role:'user',text:'让阅读进度和书签在侧栏里更容易找到。'},
        {id:'r2',role:'assistant',text:'阅读器使用自己的工作清单与对话设置。切换项目时，另一边的草稿和演示运行会保留。'}
      ]),
      makeConversation('general',null,'随手聊聊')
    ],
    tasks:[
      {id:'R-381',pid:'kanzei',kind:'requirement',title:'工作台与项目管理整理',status:'doing',priority:'P1',batch:3,total:4,cid:'front',goal:'保持熟悉的对话操作，整理项目入口、管理页和输入区。',acceptance:'无需重新学习常用操作；切页保留草稿；工作清单只展示工作条目。'},
      {id:'R-365',pid:'kanzei',kind:'requirement',title:'搜索与网页抓取升级',status:'doing',priority:'P1',batch:2,total:5,cid:'search',goal:'统一模型搜索、网页抓取和来源说明。',acceptance:'结果可追溯；失败原因可读；搜索与抓取状态不混淆。'},
      {id:'D-568',pid:'kanzei',kind:'defect',title:'修正记忆索引描述串号',status:'doing',priority:'P2',batch:0,total:1,cid:null,goal:'修正条目描述与编号的错误关联。',acceptance:'原条目和检索结果的编号、描述一致。'},
      {id:'R-372',pid:'kanzei',kind:'requirement',title:'交付结果与验收入口',status:'review',priority:'P1',batch:3,total:3,cid:'front',goal:'将完成说明、文件和人工验收放到同一个交付入口。',acceptance:'区分自动检查和人工验收；可标记通过或退回。'},
      {id:'R-389',pid:'kanzei',kind:'requirement',title:'统一深色主题状态颜色',status:'pending',priority:'P2',batch:0,total:2,cid:null,goal:'运行、等待和错误能通过文字与颜色区分。',acceptance:'关键状态在深浅主题下都可辨识。'},
      {id:'D-601',pid:'kanzei',kind:'defect',title:'修复切换项目后的草稿归属',status:'pending',priority:'P1',batch:0,total:2,cid:null,goal:'草稿保存在所属对话，切换时不串内容。',acceptance:'两个项目的输入、附件和模型配置保持独立。'},
      {id:'R-358',pid:'kanzei',kind:'requirement',title:'远程执行连接反馈',status:'blocked',priority:'P2',batch:1,total:3,cid:null,goal:'明确展示远程连接阶段与错误原因。',acceptance:'需要远端测试环境提供连接样本。'},
      {id:'R-340',pid:'kanzei',kind:'requirement',title:'从项目直接打开 VS Code',status:'done',priority:'P2',batch:2,total:2,cid:'front',goal:'在熟悉的编辑器里浏览、搜索和编辑项目文件。',acceptance:'打开正确的项目目录，保留当前对话和草稿。'},
      {id:'R-12',pid:'reader',kind:'requirement',title:'阅读进度与书签',status:'doing',priority:'P1',batch:1,total:3,cid:'reader-chat',goal:'将最近阅读位置与书签放到容易找到的位置。',acceptance:'打开书籍后能直接回到上次阅读位置。'},
      {id:'D-08',pid:'reader',kind:'defect',title:'长标题换行',status:'pending',priority:'P2',batch:0,total:1,cid:null,goal:'长书名在窄窗口中保持可读。',acceptance:'窄屏没有横向溢出。'}
    ],
    deliveries:[
      {id:'delivery-ui',pid:'kanzei',task:'R-372',title:'交付与验收入口',description:'工作条目可以直接打开交付结果，查看检查结论并记录人工验收。',state:'pending',files:['ui/work-delivery.js','ui/app.css']},
      {id:'delivery-files',pid:'kanzei',task:'R-340',title:'VS Code 打开入口',description:'项目可以直接在 VS Code 打开，对话和草稿保留。',state:'accepted',files:['ui/project-actions.js']}
    ]
  });
  const load = (key, fallback) => { try { return JSON.parse(localStorage.getItem(key)) || fallback; } catch { return fallback; } };
  let state = load(KEY, seed());
  if (state.version !== 1 || !Array.isArray(state.conversations) || !state.conversations.some(c => c.id === state.current?.cid)) state = seed();
  const extensions = () => ({
    memories:[
      {key:'global:M-001',id:'M-001',scope:'global',pid:null,title:'常用交互习惯',category:'偏好',status:'active',text:'保留熟悉的模型、思考、模式、权限、发送和停止位置。特色功能按场景展开。',source:'用户明确偏好 · 示例',updated:'2026-10-04'},
      {key:'global:M-002',id:'M-002',scope:'global',pid:null,title:'先给出结论，再说明细节',category:'偏好',status:'active',text:'用简单直接的语言说明结论。需要用户决策时，把影响说清楚。',source:'用户明确偏好 · 示例',updated:'2026-10-04'},
      {key:'kanzei:M-001',id:'M-001',scope:'project',pid:'kanzei',title:'需求、缺陷、交付平级',category:'项目约定',status:'active',text:'管理页中三类对象使用同一级导航。需求与缺陷打开完整详情；批次使用格子进度。',source:'项目试用反馈 · 示例',updated:'2026-10-04'},
      {key:'kanzei:M-002',id:'M-002',scope:'project',pid:'kanzei',title:'差异使用红绿颜色',category:'项目约定',status:'candidate',text:'代码增加使用绿色，删除使用红色，和通用状态颜色独立。',source:'待整理笔记 · 示例',updated:'2026-10-04'},
      {key:'reader:M-001',id:'M-001',scope:'project',pid:'reader',title:'保留阅读位置',category:'项目约定',status:'active',text:'重新打开书籍时回到上次阅读位置，书签与阅读进度分别记录。',source:'项目验收 · 示例',updated:'2026-10-03'},
      {key:'reader:M-002',id:'M-002',scope:'project',pid:'reader',title:'旧版书签呈现',category:'项目约定',status:'deprecated',text:'旧约定已被新的书签列表替代，保留历史用于追溯。',source:'历史约定 · 示例',updated:'2026-10-02'}
    ],
    memoryNotes:[{id:'note-color',scope:'project',pid:'kanzei',text:'Diff 保持红绿，别跟普通状态色混在一起。',state:'pending'}],
    memoryChat:[],
    schedules:[{id:'daily',pid:'kanzei',name:'检查待验收工作',when:'工作日 09:00',enabled:true,host:'本机',prompt:'汇总需要人工验收的工作。',history:['示例运行：发现 1 项待验收交付。']}],
    preferences:{defaultModel:'GPT-6 Luna',defaultThinking:'超高',background:true,voiceLanguage:'自动识别',fontSize:'标准'},
    management:{},
    memoryView:{scope:'all',project:'all',status:'all',query:'',tab:'entries',selected:null,chatDrafts:{}},
    settingsTab:'models',settingsProject:'kanzei',mapView:{selected:'kanzei-app',query:''}
  });
  function upgrade() {
    const extra=extensions();for(const [key,value] of Object.entries(extra))if(state[key]===undefined)state[key]=value;
    if(!state.revision || state.revision<2) {
      const old=state.tasks.find(t=>t.id==='R-340');if(old?.title==='独立文件浏览入口')Object.assign(old,seed().tasks.find(t=>t.id==='R-340'));
      const delivered=state.deliveries.find(d=>d.id==='delivery-files');if(delivered?.title==='独立文件浏览页')Object.assign(delivered,seed().deliveries.find(d=>d.id==='delivery-files'));
      if(state.current.view==='files')state.current.view='manage';
      state.revision=2;
    }
    const message=state.conversations.find(c=>c.id==='front')?.messages.find(m=>m.id==='m2');
    if(message)message.text='保留熟悉的对话操作。\n\n工作清单只显示当前进行和下一候选，可手动调整。子代理与终端共用右侧活动栏，网页预览使用可以拖动、调整大小和缩放的独立窗口。\n\n项目地图先看主干结构，点击模块再看直接依赖。下方可以查看运行时长、工具调用与记忆使用观察。';
  }
  upgrade();
  upgradeV3();
  upgradeV4();
  const feedback = load(FEEDBACK_KEY, {checks:{},notes:''});
  let panel = null, compact = false, menuTrigger = null, menuName = null, guideTab = 'design';
  let toastTimer, voiceTimer, recordingCid = null, sideTimer;
  const timers = new Map();
  let storageWarning = false;
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(state)); } catch { if (!storageWarning) {storageWarning=true; toast('浏览器存储不可用；本次操作仍可试用，刷新不会保留。');} } };
  const saveFeedback = () => { try { localStorage.setItem(FEEDBACK_KEY,JSON.stringify(feedback)); } catch {toast('反馈暂时无法保存，请导出反馈。');} };
  for (const c of state.conversations){activityState(c);for(const a of c.activities)if(a.status==='running')a.status='stopped';}
  for (const c of state.conversations) if (c.phase === 'running' || c.phase === 'waiting') {recordRuntime(c,'interrupted');const record=state.runtimeHistory.find(r=>r.id===c.run?.token);if(record)record.durationMs=null;c.phase='stopped';c.reloaded=true;}
  const convo = id => state.conversations.find(c => c.id === (id || state.current.cid));
  const project = () => state.projects.find(p => p.id === state.current.pid);
  const active = c => ['running','waiting'].includes(c.phase);
  const tasks = () => state.tasks.filter(t => t.pid === state.current.pid);
  const pendingDeliveries = pid => state.deliveries.filter(d => (!pid || d.pid === pid) && d.state === 'pending');
  const statusHTML = task => `<span class="status ${task.status === 'review' ? 'waiting' : task.status}"><span class="dot" style="background:currentColor"></span>${task.kind === 'defect' && task.status === 'doing' ? '修复中' : statuses[task.status]}</span>`;
  const textHTML = value => esc(value).split('\n\n').map(p => `<p>${p.replace(/\n/g,'<br>')}</p>`).join('');
  const button = (action, text, cls='button', extra='') => `<button class="${cls}" data-action="${action}" ${extra}>${text}</button>`;
  function toast(text) { $('#toast').textContent=text; $('#toast').hidden=false; clearTimeout(toastTimer); toastTimer=setTimeout(() => $('#toast').hidden=true,3500); }
  const globalViews=new Set(['workspace','attention','memory','settings','schedules','coverage']);
  function manageState() {return state.management[state.current.pid] ||= {tab:'requirements',query:'',status:'all'};}
  function navigate(view, pid = state.current.pid, cid = state.current.cid, itemId=null) {
    closeMenu(); panel=null; compact=view !== 'chat';
    if (pid !== convo(cid)?.pid && !globalViews.has(view)) cid=state.conversations.find(c => c.pid === pid)?.id || cid;
    state.current={view,pid,cid,itemId};if(view==='chat'&&convo(cid)?.dock?.open)panel={type:'activity',id:cid}; $('#shell').classList.remove('sidebar-open'); save(); render(true);
  }
  function renderSidebar() {
    const c = state.current;
    const convRows = pid => state.conversations.filter(x => x.pid === pid).map(x => `<button class="conversation-row ${c.view==='chat' && c.cid===x.id ? 'active':''}" data-action="conversation" data-id="${esc(x.id)}" title="${esc(x.title)}"><span>${esc(x.title)}</span>${active(x) ? '<span class="dot running" title="运行中"></span>':''}</button>`).join('');
    $('#sidebar').innerHTML=`<div class="brand-row"><img class="brand-icon" src="brand.svg" alt=""><span class="brand-title">Kanzei</span><span class="grow"></span>${button('toggle-sidebar',icon('panel'),'icon-button','aria-label="收起侧栏" title="收起侧栏"')}</div>
      <div class="sidebar-main">
        ${button('new-chat',`${icon('plus')}新对话<span class="shortcut">Ctrl ⇧ N</span>`,'nav-row')}
        ${button('search',`${icon('search')}搜索<span class="shortcut">Ctrl P</span>`,'nav-row')}
        ${button('workspace',`${icon('grid')}工作台`,'nav-row'+(c.view==='workspace'?' active':''))}
        ${button('attention',`${icon('clock')}待我处理<span class="grow"></span><span class="muted">${pendingDeliveries().length}</span>`,'nav-row'+(c.view==='attention'?' active':''))}
        ${button('schedules',`${icon('clock')}定时任务`,'nav-row'+(c.view==='schedules'?' active':''))}
        <div class="sidebar-label">项目${button('new-project',icon('plus'),'icon-button','aria-label="新建示例项目" title="新建示例项目"')}</div>
        ${state.projects.map(p => `${button('project',`${icon('folder')}<span>${esc(p.name)}</span>${icon('right')}`,'nav-row project-row'+(c.pid===p.id && c.view==='manage'?' active':''),`data-id="${esc(p.id)}" title="打开项目管理 · 右键更多操作" aria-haspopup="menu"`)}<div class="project-chats">${convRows(p.id)}</div>`).join('')}
        <div class="sidebar-label">独立对话</div><div class="project-chats">${convRows(null)}</div>
      </div>
      <div class="sidebar-bottom">
        ${button('memory',`${icon('memory')}记忆管理`,'nav-row'+(c.view==='memory'?' active':''))}
        ${button('settings',`${icon('settings')}设置`,'nav-row'+(c.view==='settings'?' active':''))}
        <div class="sidebar-bottom-row"><span class="prototype-label">交互原型 v4 · 示例</span><span class="grow"></span>${button('theme',icon(state.theme==='light'?'moon':'sun'),'icon-button',`aria-label="切换到${state.theme==='light'?'深':'浅'}色" title="切换主题"`)}${button('guide',icon('help'),'icon-button','aria-label="试用说明与验收" title="试用说明与验收"')}</div>
      </div>`;
  }
  function renderTopbar() {
    const {view}=state.current, p=project(), c=convo();
    const titles={manage:'项目管理',memory:'记忆管理',workspace:'工作台',attention:'待我处理',settings:'设置',schedules:'定时任务',coverage:'功能对照','work-detail':'工作详情','delivery-detail':'交付详情'};
    $('#topbar').innerHTML=`${button('toggle-sidebar',icon('panel'),'icon-button','aria-label="展开或收起侧栏" title="展开或收起侧栏"')}
      <div class="breadcrumb">${p && !globalViews.has(view)?`<span class="project-name">${esc(p.name)}</span><span class="slash">/</span>`:''}<span class="title">${esc(titles[view] || c.title)}</span></div>
      ${p && !globalViews.has(view)?`<nav class="view-tabs" aria-label="项目视图">${button('chat','对话','view-tab'+(view==='chat'?' active':''),`aria-current="${view==='chat'?'page':'false'}"`)}${button('manage','管理','view-tab'+(['manage','work-detail','delivery-detail'].includes(view)?' active':''))}</nav>`:''}
      <div class="top-actions">${p && !globalViews.has(view)?`${view==='chat'?button('activity-open',`${icon('panel')}<span class="activity-label">活动</span>`,'top-text','aria-label="打开对话活动" title="子代理与终端"')+((c.activities||[]).some(a=>a.kind==='preview')?button('preview-open',`${icon('grid')}<span class="preview-label">预览</span>`,'top-text','aria-label="打开网页预览"'):''):''}${button('changes',`3 个文件 <span class="diff-count diff-add">+57</span><span class="diff-count diff-del">−7</span>`,'top-text changes','title="查看示例改动"')}${button('work-list',`${icon('list')}<span class="work-label">工作清单</span>`,'top-text work-toggle','aria-label="工作清单" title="工作清单"')}`:`${button('return-chat','返回对话','top-text')}${button('guide',icon('help'),'icon-button','aria-label="试用说明与验收"')}`}</div>`;
  }
  function renderMessages(resetScroll = false) {
    const page=$('#page'), old=page.scrollTop, atEnd=page.scrollHeight-page.clientHeight-old<65;
    const c=convo();
    page.innerHTML=`<div class="chat-reader">${c.messages.length?`<div class="chat-date">今天 · ${project()?esc(project().name):'独立对话'}</div>${c.messages.map(m => `<article class="message ${m.role}" data-message-id="${esc(m.id)}">${m.role==='assistant'?`<div class="assistant-label"><img src="brand.svg" alt="">Kanzei <span>· ${m.demo?'演示回复':'示例对话'}</span></div>`:''}${m.attachments?.length?m.attachments.map(f=>`<span class="message-attachment">${icon('file',true)}${esc(f.name)}</span>`).join(''):''}<div class="message-text">${m.activityId?button('activity-result',`${icon(m.kind==='terminal'?'code':m.kind==='preview'?'grid':'chat',true)}${esc(m.text)} ${icon('right',true)}`,'tool-result-link',`data-id="${m.activityId}"`):textHTML(m.text)}</div>${m.tools?`<details class="tool-group"><summary>${icon('right',true)}已查看 3 个文件 · 8 次工具调用</summary><div class="tool-lines"><div class="tool-line">${icon('check',true)}读取 index.html</div><div class="tool-line">${icon('check',true)}检查 composer 布局与运行入口</div><div class="tool-line">${icon('check',true)}核对工作清单数据来源</div></div></details><div class="message-actions">${button('changes',`${icon('code',true)}查看改动 · 3 个文件`,'inline-link')}</div>`:''}</article>`).join('')}`:`<div class="empty-chat"><img class="brand-icon" src="brand.svg" alt=""><h1>从这里开始</h1><p>${project()?`在 ${esc(project().name)} 中开始一个新任务`:'随手提问，也可以稍后归入项目'}</p><div class="empty-actions">${button('suggestion','梳理下一步工作','button small','data-value="帮我梳理当前项目下一步要做的工作。"')}${button('suggestion','检查页面交互','button small','data-value="检查页面交互，保留熟悉的常用操作。"')}${button('demo-tools','演示工具调用','button small')}</div></div>`}</div>`;
    page.scrollTop=resetScroll || atEnd ? page.scrollHeight : old;
  }
  function batchGrid(t) {
    const total=Math.max(1,Number(t.total)||1), completed=Math.min(total,Math.max(0,Number(t.batch)||0));
    return `<span class="batch-grid" role="img" aria-label="已完成 ${completed} 个批次，共 ${total} 个批次" style="--steps:${total}">${Array.from({length:total},(_,i)=>`<span class="batch-cell ${i<completed?'complete':i===completed&&t.status==='doing'?'current':''}" title="${i<completed?'已完成':i===completed&&t.status==='doing'?'进行中':'待开始'}"></span>`).join('')}</span>`;
  }
  function workRows() {
    const m=manageState(), kind=m.tab==='defects'?'defect':'requirement';
    const list=tasks().filter(t=>t.kind===kind&&(m.status==='all'||t.status===m.status)&&`${t.id} ${t.title}`.toLowerCase().includes(m.query.toLowerCase()));
    return `<div class="table-head"><span>编号</span><span>${kind==='defect'?'缺陷':'需求'}</span><span>批次进度</span><span>状态</span><span>优先级</span></div>${list.length?list.map(t=>`<button class="work-table-row" data-action="task" data-id="${esc(t.id)}"><span class="work-id">${esc(t.id)}</span><span class="work-title">${esc(t.title)}</span>${batchGrid(t)}${statusHTML(t)}<span class="priority">${esc(t.priority)}</span></button>`).join(''):'<div class="table-empty">没有符合条件的工作</div>'}`;
  }
  function deliveryRow(d) {
    const labels={pending:'待你验收',accepted:'已验收',changes:'待修改'};
    return `<button class="delivery-row" data-action="delivery" data-id="${d.id}">${icon('file')}<span class="delivery-copy"><strong>${esc(d.title)}</strong><small>${d.task} · ${d.files.length} 个文件 · 示例交付</small></span><span class="status ${d.state==='accepted'?'done':d.state==='changes'?'doing':'waiting'}">${labels[d.state]}</span>${icon('right',true)}</button>`;
  }
  function renderManage() {
    const t=tasks(),d=state.deliveries.filter(x=>x.pid===state.current.pid),m=manageState();
    const tabs=[['requirements','需求',t.filter(x=>x.kind==='requirement').length],['defects','缺陷',t.filter(x=>x.kind==='defect').length],['deliveries','交付',d.length],['map','项目地图',null]];
    const content=m.tab==='map'?mapMarkup():m.tab==='deliveries'?`<div class="section-heading"><h2>交付</h2><span class="muted">自动检查与人工验收分别记录</span></div><div class="delivery-list">${d.length?d.map(deliveryRow).join(''):'<div class="table-empty">这里还没有交付</div>'}</div>`:`<div class="work-toolbar">${button('new-work',`${icon('plus',true)}新建${m.tab==='defects'?'缺陷':'需求'}`,'button')}<label class="search-field">${icon('search')}<input id="work-search" placeholder="搜索编号或标题" aria-label="搜索工作" value="${esc(m.query)}"></label><select id="work-state" class="work-state-filter" aria-label="按工作状态筛选"><option value="all">全部状态</option>${Object.entries(statuses).map(([id,label])=>`<option value="${id}" ${m.status===id?'selected':''}>${label}</option>`).join('')}</select></div><div class="work-table" id="work-table">${workRows()}</div>`;
    $('#page').innerHTML=`<div class="manage-page"><div class="page-heading"><div><h1>项目管理</h1><p class="subtitle">${esc(project()?.name||'项目')}</p></div></div><nav class="management-tabs" aria-label="管理类别">${tabs.map(([id,label,count])=>button('manage-tab',`${label}${count===null?'':`<span>${count}</span>`}`,'management-tab'+(m.tab===id?' active':''),`data-value="${id}" aria-current="${m.tab===id?'page':'false'}"`)).join('')}</nav>${content}</div>`;
  }

  function openTask(id) {
    const t=state.tasks.find(x=>x.id===id);if(!t)return;
    const current=convo();navigate('work-detail',t.pid,current?.pid===t.pid?current.id:t.cid||state.conversations.find(c=>c.pid===t.pid)?.id,id);
    manageState().tab=t.kind==='defect'?'defects':'requirements';save();
  }
  function openDelivery(id) {
    const d=state.deliveries.find(x=>x.id===id);if(!d)return;
    navigate('delivery-detail',d.pid,convo()?.pid===d.pid?state.current.cid:state.conversations.find(c=>c.pid===d.pid)?.id,id);manageState().tab='deliveries';save();
  }
  // UI adapter: unknown fields remain visible; the underlying tracker schema is not fixed here.
  function workPresentation(t) {
    const known=new Set(['id','pid','kind','title','status','priority','batch','total','cid','goal','acceptance','sections','fields','body','batches','requirement']);
    const extra=Object.entries(t).filter(([key])=>!known.has(key));
    const pairs=value=>Array.isArray(value)?value:Object.entries(value||{});
    const fields=[['类型',t.kind==='defect'?'缺陷':'需求'],['优先级',t.priority],['所属项目',state.projects.find(p=>p.id===t.pid)?.name],...pairs(t.fields),...extra];
    const sections=t.sections?pairs(t.sections):[
      ['原始说明',t.goal||'见原始字段'],['目标与范围',`${t.goal||''}\n\n${t.kind==='defect'?'修复可复现的问题，并保留原有功能。':'完成本条目的目标，与相关工作保持一致。'}`],
      ['验收条件',t.acceptance],['执行进展',t.status==='done'?'实施已完成，结果已进入交付。':t.status==='blocked'?'当前工作存在外部依赖，解除阻塞后继续。':'按批次推进，完成一批后记录结果与验收证据。'],
      ['依赖与关联',t.id==='R-381'?'关联：R-372 交付入口、D-601 草稿归属。':'暂无额外依赖。'],['验证与证据','示例条目用于试用完整信息呈现。正式页面应呈现原条目的测试、证据、来源与变更记录。']
    ];
    for(const [key,value] of fields)if(/边界|迁移|回滚/.test(key)&&!sections.some(([title])=>title===key))sections.push([key,typeof value==='object'?JSON.stringify(value,null,2):value]);
    return {fields,sections,body:t.body||`# ${t.id} ${t.title}\n\n${fields.map(([key,value])=>`- ${key}: ${typeof value==='object'?JSON.stringify(value):value}`).join('\n')}\n\n${sections.map(([title,text])=>`## ${title}\n\n${text}`).join('\n\n')}`};
  }
  function requirementGaps(spec) {
    return [...(!spec.statement?.trim()?['补充需求正文']:[]),...(!spec.acceptance?.length?['补充验收标准']:[]),...(!spec.source?.reference?.trim()&&!spec.source?.quote?.trim()?['保留原始来源']:[]),...(spec.questions||[])];
  }
  function renderWorkDetail() {
    const t=state.tasks.find(x=>x.id===state.current.itemId);if(!t){navigate('manage');return;}
    const view=workPresentation(t),contract=t.requirement,spec=contract?.spec;
    const batch=`<h3>批次进度</h3>${batchGrid(t)}<div class="batch-plan">${Array.from({length:t.total},(_,i)=>`<div>${icon(i<t.batch?'check':'clock',true)}<span>${esc(t.batches?.[i]?.title||['梳理目标与范围','完成核心修改','检查已有功能','人工试用与收尾'][i]||'后续实施')}</span><span class="muted">${i<t.batch?'已完成':i===t.batch&&t.status==='doing'?'进行中':'待开始'}</span></div>`).join('')}</div>`;
    let body;
    if(spec) {
      const gaps=requirementGaps(spec),evidence=contract.evidence||[];
      body=`<section class="full-section requirement-body"><h2>需求正文</h2><div class="long-copy">${textHTML(spec.statement||'待补充需求正文')}</div></section><section class="full-section"><h2>验收标准</h2><ol class="requirement-criteria">${spec.acceptance.map(ac=>{const e=evidence.find(e=>e.criterion_id===ac.id&&e.revision===contract.revision)||evidence.find(e=>e.criterion_id===ac.id);return `<li data-criterion-id="${esc(ac.id)}"><code>${esc(ac.id)}</code><div><p>${esc(ac.text)}</p><small class="${e&&e.revision!==contract.revision?'stale-evidence':''}">${!e?'待验证':e.revision===contract.revision?'已有示例证据':'要求已变更，待复核'}</small></div></li>`;}).join('')||'<li>待补充验收标准</li>'}</ol></section>${gaps.length?`<section class="requirement-gaps"><h2>待解决事项</h2><ul>${gaps.map(g=>`<li>${esc(g)}</li>`).join('')}</ul></section>`:''}<details class="requirement-supplement"><summary>来源与说明</summary>${spec.source.quote?`<blockquote>${esc(spec.source.quote)}</blockquote>`:''}<p>${esc(spec.source.reference||'原型中记录的用户原文')}</p></details><details class="requirement-supplement"><summary>关联与设计</summary>${(spec.links||[]).map(l=>`<p>${({related:'关联',depends_on:'依赖',parent:'上级目标',design:'设计文档'})[l.relation]||esc(l.relation)}：${state.tasks.some(t=>t.id===l.target)?button('task',esc(l.target),'inline-link',`data-id="${esc(l.target)}"`):esc(l.target)}</p>`).join('')||'<p>暂无关联。</p>'}</details><details class="requirement-supplement"><summary>验收证据</summary>${evidence.map(e=>`<p><code>${esc(e.criterion_id)}</code> · ${e.revision===contract.revision?'当前版本':'旧版本证据'}<br>${esc(e.reference)}</p>`).join('')||'<p>暂无证据，尚未验证。</p>'}</details><details class="requirement-supplement"><summary>执行记录</summary>${batch}<dl class="full-facts">${view.fields.map(([key,value])=>`<div><dt>${esc(key)}</dt><dd>${esc(typeof value==='object'?JSON.stringify(value,null,2):value)}</dd></div>`).join('')}</dl></details>`;
    } else body=`${view.sections.map(([title,text])=>`<section class="full-section"><h2>${esc(title)}</h2><div class="long-copy">${textHTML(text)}</div></section>`).join('')}<section class="full-section">${batch}</section><details class="requirement-supplement"><summary>完整字段与执行记录</summary><dl class="full-facts">${view.fields.map(([key,value])=>`<div><dt>${esc(key)}</dt><dd>${esc(typeof value==='object'?JSON.stringify(value,null,2):value)}</dd></div>`).join('')}</dl></details>`;
    const ready=spec&&requirementGaps(spec).length===0;
    $('#page').innerHTML=`<article class="full-detail"><div class="detail-navigation">${button('back-management',`${icon('back',true)}返回${t.kind==='defect'?'缺陷':'需求'}列表`,'text-button')}<span class="grow"></span>${spec?button('edit-requirement','编辑需求','button',`data-id="${t.id}"`):''}${t.status==='draft'?(ready?button('ready-requirement','转为待开始','button primary',`data-id="${t.id}"`):''):button(t.cid?'conversation':'prepare-work',t.cid?'继续负责对话':'准备执行','button primary',`data-id="${t.cid||t.id}"`)}</div><div class="detail-heading"><p class="detail-eyebrow">${esc(t.id)} ${statusHTML(t)}<span>${esc(t.priority||'')}</span></p><h1>${esc(t.title)}</h1><p class="detail-owner">${esc(state.projects.find(p=>p.id===t.pid)?.name)}${t.cid?` · 负责对话：${button('conversation',esc(convo(t.cid)?.title||'打开对话'),'inline-link',`data-id="${t.cid}"`)}`:''}</p></div>${body}<details class="source-document"><summary>完整原文与字段</summary><pre>${esc(t.body||JSON.stringify(t,null,2))}</pre></details></article>`;
  }
  function requirementForm(task=null) {
    const spec=task?.requirement?.spec;
    openDialog(task?'编辑需求':'登记需求',`<p class="dialog-subtitle">正文、验收、来源分开记录。未确定的内容先保存为草稿。</p><form id="requirement-form" data-id="${task?.id||''}"><label class="field"><span>标题${task?'':' · 可选'}</span><input name="title" aria-label="需求标题" value="${esc(task?.title||'')}"></label><label class="field"><span>原始描述</span><textarea name="original" required aria-label="需求原始描述" ${task?'readonly':''}>${esc(spec?.source?.quote||spec?.source?.reference||'')}</textarea></label><label class="field"><span>需求正文</span><textarea name="statement" aria-label="需求正文" placeholder="描述系统在什么情况下应做什么…">${esc(spec?.statement||'')}</textarea></label><label class="field"><span>验收标准 · 一行一项${task?'，保留已有 AC 编号':''}</span><textarea name="criteria" aria-label="需求验收标准">${esc(spec?.acceptance?.map(a=>`${a.id} | ${a.text}`).join('\n')||'')}</textarea></label><label class="field"><span>开放问题</span><textarea name="questions" aria-label="需求开放问题" placeholder="还缺少哪些明确的信息？">${esc(spec?.questions?.join('\n')||'')}</textarea></label><div class="dialog-actions"><button type="submit" class="button primary">保存示例需求</button>${button('close-dialog','取消','button','type="button"')}</div></form>`);
  }
  function saveRequirement(form,data) {
    const old=state.tasks.find(t=>t.id===form.dataset.id),prior=old?.requirement,original=data.get('original');if(!original.trim())return;
    const reserved=new Set([...(prior?.retiredIds||[]),...(prior?.spec.acceptance||[]).map(a=>a.id)]),used=new Set();let next=Math.max(0,...[...reserved].map(id=>Number(id.slice(3))))+1;
    const acceptance=data.get('criteria').split('\n').map(x=>x.trim()).filter(Boolean).map(line=>{const match=line.match(/^(AC-\d+)\s*\|\s*(.+)$/);const id=match?match[1]:prior?.spec.acceptance.find(a=>a.text===line)?.id||`AC-${next++}`;return {id,text:match?match[2]:line};});
    for(const a of acceptance){if(used.has(a.id)){form.elements.criteria.setCustomValidity('验收编号重复，请保留独立编号。');form.elements.criteria.reportValidity();return;}used.add(a.id);}
    const spec={...(prior?.spec||{}),statement:data.get('statement').trim(),acceptance,source:prior?.spec.source||{quote:original},questions:data.get('questions').split('\n').map(x=>x.trim()).filter(Boolean),links:prior?.spec.links||[]};
    const gaps=requirementGaps(spec),same=prior&&JSON.stringify({...spec,source:null})===JSON.stringify({...prior.spec,source:null});
    const contract={format:2,revision:same?prior.revision:uid('prototype-spec'),spec,gaps,evidence:prior?.evidence||[],retiredIds:[...new Set([...reserved,...used])]};
    const title=data.get('title').trim()||original.trim().slice(0,36);
    if(old){old.title=title;old.requirement=contract;if(gaps.length)old.status='draft';else if(old.status==='done'&&!same)old.status='review';}
    else {const last=Math.max(0,...state.tasks.filter(t=>t.id.startsWith('R-')).map(t=>Number(t.id.slice(2))));state.tasks.push({id:`R-${last+1}`,pid:state.current.pid,kind:'requirement',title,status:gaps.length?'draft':'pending',priority:'',batch:0,total:1,cid:null,requirement:contract});}
    const item=old||state.tasks.at(-1);closeDialog();save();openTask(item.id);toast(item.status==='draft'?'已保存草稿，补齐后可转为待开始。':'示例需求已保存。');
  }
  function renderDeliveryDetail() {
    const d=state.deliveries.find(x=>x.id===state.current.itemId);if(!d){navigate('manage');return;}
    $('#page').innerHTML=`<article class="full-detail"><div class="detail-navigation">${button('back-management',`${icon('back',true)}返回交付列表`,'text-button')}<span class="grow"></span><span class="tag">示例交付</span></div><div class="detail-heading"><p class="detail-eyebrow">交付 · ${d.task}</p><h1>${esc(d.title)}</h1></div><section class="full-section"><h2>交付说明</h2><div class="long-copy">${textHTML(d.description)}</div></section><section class="full-section"><h2>关联工作</h2>${button('task',`${d.task} ${esc(state.tasks.find(t=>t.id===d.task)?.title||'')}`,'inline-link',`data-id="${d.task}"`)}</section><section class="full-section"><h2>交付文件</h2>${d.files.map(f=>`<div class="artifact-row">${icon('file',true)}<span class="mono">${esc(f)}</span></div>`).join('')}<div class="detail-actions">${button('changes','查看红绿差异','button')}</div></section><section class="full-section"><h2>自动检查</h2><p class="status done">${icon('check',true)}示例检查通过</p><p class="muted">正式检查结论和证据将在这里展示。</p></section><section class="full-section"><h2>人工验收</h2><p>${d.state==='accepted'?'已验收通过':d.state==='changes'?'已退回修改':'等待你确认使用体验'}</p><textarea class="review-note" id="delivery-note" aria-label="交付验收意见" placeholder="记录问题或验收意见…">${esc(d.note||'')}</textarea><div class="detail-actions">${button('accept-delivery','验收通过','button primary',`data-id="${d.id}"`)}${button('reject-delivery','需要修改','button',`data-id="${d.id}"`)}</div></section></article>`;
  }

  function renderWorkspace(attention = false) {
    $('#page').innerHTML=`<div class="workspace-page"><div class="page-heading"><div><h1>${attention?'待我处理':'工作台'}</h1><p class="subtitle">${attention?'需要你确认的交付集中在这里。':'继续项目里的工作，或开始一个新对话。'}</p></div>${button(attention?'workspace':'new-chat',attention?'返回工作台':`${icon('plus',true)}新对话`,'button')}</div>
      ${attention ? (pendingDeliveries().length?pendingDeliveries().map(deliveryRow).join(''):'<p class="workspace-note">没有待验收交付，可以继续工作。</p>') : `<p class="workspace-summary">${state.projects.length} 个项目 · ${state.tasks.filter(t=>t.status==='doing').length} 项进行中 · ${pendingDeliveries().length} 项待你验收</p>${state.projects.map(p=>{const work=state.tasks.filter(t=>t.pid===p.id), first=work.find(t=>t.status==='doing');return `<button class="project-overview-row" data-action="project" data-id="${p.id}"><span><strong>${esc(p.name)}</strong><small>${work.filter(t=>t.status!=='done').length} 项未完成 · ${state.conversations.filter(c=>c.pid===p.id).length} 个对话</small></span><span class="project-work">${first?esc(first.title):'还没有进行中的工作'}<small>${first?`${first.id} · ${statuses[first.status]}`:'打开项目添加第一项工作'}</small></span>${first?statusHTML(first):'<span class="status">待开始</span>'}${icon('right',true)}</button>`;}).join('')}<p class="workspace-note">选中项目查看管理页；左侧对话可以直接继续工作。</p>`}</div>`;
  }
  const memoryStatus={active:'有效',candidate:'候选',shadow:'观察中',deprecated:'已归档',invalid:'无效'};
  const ownerName=m=>m.scope==='global'?'全局':state.projects.find(p=>p.id===m.pid)?.name||'项目';
  function filteredMemories() {
    const v=state.memoryView;
    return state.memories.filter(m=>(v.scope==='all'||m.scope===v.scope)&&(v.scope!=='project'||v.project==='all'||m.pid===v.project)&&(v.status==='all'||m.status===v.status)&&`${m.id} ${m.title} ${m.text}`.toLowerCase().includes(v.query.toLowerCase()));
  }
  function memoryRows() {
    const list=filteredMemories();
    return list.length?list.map(m=>`<button class="memory-row" data-action="memory-entry" data-id="${esc(m.key)}"><span><strong>${esc(m.title)}</strong><small>${m.id} · ${esc(m.category)}</small></span><span>${esc(ownerName(m))}</span><span class="status ${m.status==='active'?'done':m.status==='candidate'?'waiting':''}">${memoryStatus[m.status]}</span><span class="muted memory-updated">${m.updated}</span>${icon('right',true)}</button>`).join(''):'<div class="table-empty">没有符合条件的记忆</div>';
  }
  function renderMemory() {
    const v=state.memoryView, selected=state.memories.find(m=>m.key===v.selected);
    if(selected) {
      $('#page').innerHTML=`<article class="full-detail"><div class="detail-navigation">${button('memory-back',`${icon('back',true)}返回记忆管理`,'text-button')}<span class="grow"></span>${button('edit-memory','编辑','button',`data-id="${esc(selected.key)}"`)}</div><div class="detail-heading"><p class="detail-eyebrow">${esc(ownerName(selected))} · ${selected.id}</p><h1>${esc(selected.title)}</h1></div><dl class="full-facts"><div><dt>范围</dt><dd>${selected.scope==='global'?'全局 · 跨项目适用':`项目 · ${esc(ownerName(selected))}`}</dd></div><div><dt>状态</dt><dd>${memoryStatus[selected.status]}</dd></div><div><dt>来源</dt><dd>${esc(selected.source)}</dd></div></dl><section class="full-section"><h2>记忆内容</h2><div class="long-copy">${textHTML(selected.text)}</div></section><section class="full-section"><h2>管理</h2><div class="detail-actions">${button('memory-chat-entry','在管理对话中处理','button',`data-id="${esc(selected.key)}"`)}${button('archive-memory',selected.status==='deprecated'?'恢复此记忆':'归档此记忆','button',`data-id="${esc(selected.key)}"`)}</div><p class="muted">此处修改的是原型中的示例记忆。</p></section></article>`;
      return;
    }
    const tabs=[['entries','记忆条目'],['pending','待整理'],['chat','管理对话'],['usage','使用记录']];
    let content='';
    if(v.tab==='entries') content=`<div class="memory-scope-bar" aria-label="记忆范围">${[['all','全部记忆'],['global','全局记忆'],['project','项目记忆']].map(([id,label])=>button('memory-scope',label,'filter-button'+(v.scope===id?' active':''),`data-value="${id}"`)).join('')}${v.scope==='project'?`<select id="memory-project-filter" aria-label="选择记忆所属项目"><option value="all">所有项目</option>${state.projects.map(p=>`<option value="${p.id}" ${v.project===p.id?'selected':''}>${esc(p.name)}</option>`).join('')}</select>`:''}</div><div class="work-toolbar"><span class="muted" id="memory-count">${filteredMemories().length} 条记忆</span><label class="search-field">${icon('search')}<input id="memory-search" placeholder="搜索标题或正文" aria-label="搜索记忆" value="${esc(v.query)}"></label><select id="memory-status-filter" class="work-state-filter" aria-label="按记忆状态筛选"><option value="all">全部状态</option>${Object.entries(memoryStatus).map(([id,label])=>`<option value="${id}" ${v.status===id?'selected':''}>${label}</option>`).join('')}</select></div><div id="memory-rows" class="memory-table">${memoryRows()}</div>`;
    else if(v.tab==='pending') content=`<div class="section-heading"><h2>待整理笔记</h2><span class="muted">保留来源与所属范围</span></div>${state.memoryNotes.length?state.memoryNotes.map(n=>`<div class="pending-note"><span class="tag">${esc(ownerName(n))}</span><p>${esc(n.text)}</p>${n.state==='pending'?button('organize-memory','整理为候选','button small',`data-id="${n.id}"`):'<span class="status done">已整理为候选</span>'}</div>`).join(''):'<div class="table-empty">没有待整理笔记</div>'}`;
    else if(v.tab==='chat') content=`<div class="memory-chat-target"><label for="memory-chat-target">这段对话管理</label><select id="memory-chat-target"><option value="global" ${v.chatTarget==='global'?'selected':''}>全局记忆</option>${state.projects.map(p=>`<option value="${p.id}" ${v.chatTarget===p.id?'selected':''}>${esc(p.name)} 的项目记忆</option>`).join('')}</select></div><div class="memory-conversation">${state.memoryChat.filter(m=>m.target===(v.chatTarget||'global')).map(m=>`<div class="side-answer"><strong>${esc(m.text)}</strong><p>${esc(m.reply)}</p></div>`).join('')||'<p class="muted">在这里整理或修订记忆，主对话继续保留。</p>'}</div><form id="memory-chat-form"><textarea class="review-note" name="message" required aria-label="记忆管理消息" placeholder="例如：合并重复条目，保留适用条件和来源。">${esc(v.chatDraft||'')}</textarea><div class="detail-actions"><button type="submit" class="button primary">发送</button></div></form>`;
    else content=`<div class="section-heading"><h2>最近使用</h2><span class="muted">示例记录</span></div>${state.memories.filter(m=>m.status==='active').map(m=>`<button class="usage-row" data-action="memory-entry" data-id="${esc(m.key)}"><span><strong>${esc(m.title)}</strong><small>${esc(ownerName(m))} · ${m.id}</small></span><span>已召回</span><span>已注入</span><span class="muted">效果待验证</span>${icon('right',true)}</button>`).join('')}<p class="workspace-note">召回、注入与实际效果分别记录；不把被使用等同于有效。</p>`;
    $('#page').innerHTML=`<div class="memory-page"><div class="page-heading"><div><h1>记忆管理</h1><p class="subtitle">全局与所有项目 · 管理范围独立于当前对话</p></div>${button('new-memory',`${icon('plus',true)}新建记忆`,'button')}</div><nav class="management-tabs" aria-label="记忆管理内容">${tabs.map(([id,label])=>button('memory-tab',label,'management-tab'+(v.tab===id?' active':''),`data-value="${id}"`)).join('')}</nav>${content}</div>`;
  }
  function memoryForm(key=null) {
    const m=state.memories.find(x=>x.key===key),v=state.memoryView;
    const scope=m?.scope|| (v.scope==='project'?'project':'global'),pid=m?.pid|| (v.project!=='all'?v.project:state.projects[0]?.id);
    openDialog(m?'编辑记忆':'新建记忆',`<p class="dialog-subtitle">请确认所属范围。内容保存在原型中，不会修改实际记忆文件。</p><form id="memory-edit-form" data-key="${esc(key||'')}"><label class="field"><span>范围</span><select name="scope" id="memory-edit-scope" ${m?'disabled':''}><option value="global" ${scope==='global'?'selected':''}>全局记忆</option><option value="project" ${scope==='project'?'selected':''}>项目记忆</option></select></label><label class="field" id="memory-edit-project-field" ${scope==='global'?'hidden':''}><span>所属项目</span><select name="project" aria-label="记忆所属项目" ${m?'disabled':''}>${state.projects.map(p=>`<option value="${p.id}" ${pid===p.id?'selected':''}>${esc(p.name)}</option>`).join('')}</select></label><label class="field"><span>标题</span><input name="title" aria-label="记忆标题" required maxlength="120" value="${esc(m?.title||'')}"></label><label class="field"><span>内容</span><textarea name="body" aria-label="记忆内容" required>${esc(m?.text||'')}</textarea></label><div class="dialog-actions"><button type="submit" class="button primary">保存示例记忆</button>${button('close-dialog','取消','button','type="button"')}</div></form>`);
  }
  async function openEditor(pid=state.current.pid) {
    if(pid!=='kanzei') {openDialog('在 VS Code 打开',`<p class="dialog-subtitle">${esc(state.projects.find(p=>p.id===pid)?.name||'当前对话')} 是示例项目，没有绑定本机目录。已绑定的 kanzei code 可以直接打开。</p>`);return;}
    try {
      const response=await fetch('/api/open-editor',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({project:pid})});
      const result=await response.json();if(!response.ok)throw new Error(result.error||'无法打开 VS Code');toast('已请求 VS Code 打开 kanzei code。');
    } catch(error) {openDialog('VS Code 打开失败',`<p class="dialog-subtitle">${esc(error.message)}</p><p>请使用本地预览入口，并确认 VS Code 已安装。</p>`);}
  }

  const settingNames={models:'模型',providers:'服务商',execution:'权限与运行',runs:'运行记录',conventions:'开发规范',tests:'测试记录',appearance:'外观',tools:'工具与代理',voice:'语音',connections:'连接与远程',about:'数据与更新'};
  function projectSettingsMarkup(tab) {
    const pid=state.settingsProject||state.projects[0]?.id,list=state.conversations.filter(c=>c.pid===pid);
    let content='';
    if(tab==='runs')content=list.map(c=>`<div class="run-record"><span><strong>${esc(c.title)}</strong><small>${esc(c.config.model)} · ${c.activities?.length||0} 个工具记录</small></span><span class="status">${({running:'运行中',done:'已完成',stopped:'已停止',waiting:'待确认',error:'失败'})[c.phase]||'空闲'}</span>${button('open-run','查看','button small',`data-id="${c.id}"`)}</div>`).join('')||'<p class="workspace-note">暂无运行记录。</p>';
    else if(tab==='conventions')content=`<form id="conventions-form"><label class="field"><span>项目约定</span><textarea name="body" class="conventions-editor" aria-label="项目开发规范">${esc(state.conventions?.[pid]||'保留熟悉的通用操作。\n需求与验收证据分开，保留原始来源。\n改动后记录验证结果。')}</textarea></label><button type="submit" class="button primary">保存示例规范</button></form>`;
    else content=`<details class="record-disclosure"><summary><strong>对话与草稿归属</strong><span class="status done">示例通过</span></summary><p>切换对话后，运行记录和草稿仍属于原对话。</p></details><details class="record-disclosure"><summary><strong>原生桌面试用</strong><span class="status waiting">待试用</span></summary><p>当前原型没有代替正式桌面应用的验收。</p></details>`;
    return `<h2>${settingNames[tab]}</h2><label class="settings-project">项目<select id="settings-project" aria-label="设置所属项目">${state.projects.map(p=>`<option value="${p.id}" ${p.id===pid?'selected':''}>${esc(p.name)}</option>`).join('')}</select></label>${content}`;
  }

  function renderSettings() {
    const tab=state.settingsTab,p=state.preferences;let content='';
    if(['runs','conventions','tests'].includes(tab))content=projectSettingsMarkup(tab);
    else if(tab==='models')content=`<h2>默认模型</h2><p class="settings-intro">用于新对话；已有对话保留各自的选择。</p><form id="preferences-form"><label class="field"><span>主模型</span><select name="defaultModel">${options.model.map(([v])=>`<option ${p.defaultModel===v?'selected':''}>${v}</option>`).join('')}</select></label><label class="field"><span>思考强度</span><select name="defaultThinking">${options.thinking.map(([v])=>`<option ${p.defaultThinking===v?'selected':''}>${v}</option>`).join('')}</select></label><button type="submit" class="button primary">保存默认值</button></form><p class="workspace-note">正式应用还支持快速模型、压缩模型和项目覆盖。</p>`;
    else if(tab==='providers')content='<h2>服务商</h2><div class="setting-row"><span><strong>Codex</strong><small>示例服务商 · 由本机登录连接</small></span><span class="status done">已配置</span></div><div class="setting-row"><span><strong>OpenAI 兼容接口</strong><small>端点、模型探测与网络代理归在这里</small></span><span class="tag">布局示例</span></div><p class="workspace-note">此原型不收集密钥、不连接服务商。</p>';
    else if(tab==='execution')content=`<h2>权限与运行</h2><div class="setting-row"><span><strong>默认权限</strong><small>应用范围，所有对话共享</small></span><select id="settings-permission" aria-label="应用权限"><option value="ask" ${state.permission==='ask'?'selected':''}>按需询问</option><option value="auto" ${state.permission==='auto'?'selected':''}>自动放行</option></select></div><div class="setting-row"><span><strong>后台常驻</strong><small>运行状态与退出控制放在应用层</small></span><button class="button small" data-action="background-toggle">${p.background?'已开启':'已关闭'}</button></div><p class="workspace-note">示例开关不改变本机服务。正式功能包括执行预算、超时、并发及断线恢复。</p>`;
    else if(tab==='appearance')content=`<h2>外观</h2><div class="setting-row"><span><strong>主题</strong><small>跟随你的阅读习惯</small></span>${button('theme',state.theme==='light'?'切换深色':'切换浅色','button')}</div><p class="workspace-note">普通操作使用中性色。文件差异的增加为绿色，删除为红色。</p>`;
    else if(tab==='tools')content=`<h2>工具与代理</h2><div class="setting-row"><span><strong>打开方式</strong><small>VS Code · 项目文件与编辑</small></span><span class="muted">项目右键菜单</span></div><div class="setting-row"><span><strong>代理目录</strong><small>开发、只读、记忆管理、需求登记与代码审核</small></span><span class="tag">示例目录</span></div><p class="workspace-note">代理定义、工具权限与项目覆盖在同一处查看；活跃子代理在对话右侧查看。</p>`;
    else if(tab==='voice')content=`<h2>语音</h2><div class="setting-row"><span><strong>识别语言</strong><small>原型语音仍使用示例输入</small></span><select id="voice-language-setting" aria-label="识别语言">${['自动识别','中文','English'].map(v=>`<option ${p.voiceLanguage===v?'selected':''}>${v}</option>`).join('')}</select></div><p class="workspace-note">正式入口包含语音服务、录音设备与识别状态。</p>`;
    else if(tab==='connections')content='<h2>连接与远程</h2><div class="setting-row"><span><strong>手机连接</strong><small>服务状态、设备配对与连接管理</small></span><span class="tag">入口示例</span></div><div class="setting-row"><span><strong>远程执行</strong><small>登记服务器、运行环境和任务归属</small></span><span class="tag">入口示例</span></div><p class="workspace-note">原型没有启动手机或远程服务。</p>';
    else content=`<h2>数据与更新</h2><div class="setting-row"><span><strong>项目数据</strong><small>存储位置、数据导出与日志</small></span><span class="tag">正式应用能力</span></div><div class="setting-row"><span><strong>应用更新</strong><small>检查版本与更新记录</small></span><span class="tag">原型 v4</span></div>${button('coverage','查看功能对照','button')}`;
    $('#page').innerHTML=`<div class="settings-page"><div class="page-heading"><div><h1>设置</h1><p class="subtitle">应用默认值与连接管理</p></div></div><div class="settings-layout"><nav aria-label="设置分类">${Object.entries(settingNames).map(([id,label])=>button('settings-tab',label,'resource-item'+(tab===id?' active':''),`data-value="${id}"`)).join('')}</nav><section class="settings-content">${content}</section></div></div>`;
  }
  function renderSchedules() {
    $('#page').innerHTML=`<div class="utility-page"><div class="page-heading"><div><h1>定时任务</h1><p class="subtitle">所有项目 · Asia/Shanghai · 示例计划</p></div>${button('new-schedule',`${icon('plus',true)}新建任务`,'button')}</div>${state.schedules.map(s=>`<section class="schedule-row"><div><h2>${esc(s.name)}</h2><p>${esc(state.projects.find(p=>p.id===s.pid)?.name||'独立对话')} · ${esc(s.when)} · ${esc(s.host)}</p><span class="status ${s.enabled?'done':''}">${s.enabled?'已启用':'已暂停'}</span></div><div class="detail-actions">${button('schedule-toggle',s.enabled?'暂停':'启用','button small',`data-id="${s.id}"`)}${button('schedule-run','立即演示','button small',`data-id="${s.id}"`)}${button('schedule-history','运行历史','button small',`data-id="${s.id}"`)}</div></section>`).join('')||'<div class="table-empty">没有定时任务</div>'}<p class="workspace-note">计划只保存在原型中，不会在后台自动执行。</p></div>`;
  }
  function scheduleForm() {openDialog('新建定时任务',`<form id="schedule-form"><label class="field"><span>任务名称</span><input name="name" required aria-label="定时任务名称"></label><label class="field"><span>项目</span><select name="pid">${state.projects.map(p=>`<option value="${p.id}">${esc(p.name)}</option>`).join('')}</select></label><label class="field"><span>频率</span><select name="frequency"><option>每天</option><option>工作日</option><option>每周一</option></select></label><label class="field"><span>时间 · Asia/Shanghai</span><input name="time" type="time" value="09:00" required></label><label class="field"><span>任务内容</span><textarea name="prompt" required aria-label="定时任务内容"></textarea></label><label class="field"><span>运行模式</span><select name="mode"><option>只读检查</option><option>开发任务</option></select></label><button class="button primary" type="submit">保存示例计划</button></form>`);}
  const coverage=[
    ['对话与输入','模型、模式、权限、附件、草稿','可交互','chat'],
    ['运行控制','排队、插入、停止、继续、失败重试','可交互','chat'],
    ['需求与缺陷','列表、筛选、格子进度、完整信息','可交互','manage'],
    ['交付','独立列表、完整结果与人工验收','可交互','deliveries'],
    ['编辑器与文件差异','项目右键打开 VS Code；页头看红绿差异','VS Code 实际跳转','project-menu'],
    ['记忆管理','全局、所有项目、详情、管理对话','可交互','memory'],
    ['记忆整理与效果','待整理、使用记录、图谱、追溯','整理与记录；图谱待补','memory'],
    ['定时任务','新建、暂停、历史与项目归属','演示操作','schedules'],
    ['设置与模型连接','默认值、服务商、权限、网络','部分可交互，其余入口示例','settings'],
    ['子代理','调用自动展开、结果与补充要求','可交互；工作树采纳待接','agent'],
    ['运行画像与历史','设置中查看，再回到所属对话','记录示例；诊断筛选待接','runs'],
    ['后台终端','对话右侧输出、单独停止、复制','可交互示例；进程服务待接','terminal'],
    ['网页预览','独立浮窗、拖动、调整尺寸与页面缩放','示例页面可交互；浏览器服务待接','preview'],
    ['测试记录','设置中查看检查与证据','示例记录','tests'],
    ['开发规范','设置中选择项目、查看与编辑','可交互；建议稿对照待接','conventions'],
    ['项目地图与运行','横向主干、直接依赖、运行与记忆统计','真实依赖；运行数据为示例与本地演示','map'],
    ['当前工作与候选','对话清单仅两项，可手动切换；运行中下轮生效','可交互；正式调度待接','chat'],
    ['手机、远程与语音','连接设置、识别、运行环境','入口示例；无真实服务','connections'],
    ['会话高级操作','重命名、置顶、回退与分叉、交接、上下文','搜索已做，其余待补','runs'],
    ['工作依赖与决策','条目依赖、阻塞、提问与决定复核','仍待原型覆盖','manage']
  ];
  function renderCoverage() {$('#page').innerHTML=`<div class="utility-page"><div class="page-heading"><div><h1>功能对照</h1><p class="subtitle">正式功能的归属与本轮原型覆盖</p></div></div><p class="workspace-note">已有功能不需要每项都占一个一级页面。常用动作直接可达，专业功能放在所属页面。</p><div class="coverage-table">${coverage.map(([title,desc,status,target])=>`<div><span><strong>${title}</strong><small>${desc}</small></span><span>${status}</span>${button('coverage-target','查看入口','text-button',`data-value="${target}"`)}</div>`).join('')}</div></div>`;}


  function upgradeV3() {
    if(state.revision>=3)return;
    if(state.current.view==='tools') {
      if(['runs','conventions','tests'].includes(state.toolTab)){state.settingsTab=state.toolTab;state.current.view='settings';}
      else state.current.view='chat';
    }
    state.settingsProject||=state.current.pid||'kanzei';state.mapView||={selected:'kanzei-app',query:''};
    for(const c of state.conversations){c.activities||=[];c.dock||={open:false,kind:'agent',selected:null,follow:true};}
    const t=state.tasks.find(t=>t.id==='R-381');
    if(t&&!t.requirement)t.requirement={format:2,revision:'prototype-spec-2',gaps:[],spec:{kind:'functional',statement:'项目管理应分别展示需求、缺陷、交付与项目索引地图。用户点击需求时，可以在完整页面阅读正文和验收标准，并返回之前的列表位置。',acceptance:[{id:'AC-1',text:'需求、缺陷、交付和项目地图使用同级导航，各自展示对应内容。'},{id:'AC-2',text:'点击需求后展示正文、稳定编号的验收标准、来源及执行记录；返回后保留搜索和筛选。'},{id:'AC-3',text:'子代理、终端和网页调用在所属对话右侧展开；其他对话更新不切换当前预览。'}],source:{quote:'子代理后台和网页预览终端应该在对话右侧边栏，agent 调用就弹出。项目架构生成项目依赖索引地图，放在交付旁边。',reference:'第三轮原型试用反馈 · 示例'},questions:[],links:[{relation:'related',target:'R-372'},{relation:'related',target:'D-601'}]},evidence:[{criterion_id:'AC-1',revision:'prototype-spec-2',reference:'示例：管理分类交互检查'},{criterion_id:'AC-2',revision:'prototype-spec-1',reference:'示例：上一版侧栏详情检查'}]};
    const draft=state.tasks.find(t=>t.id==='R-389');
    if(draft&&!draft.requirement){draft.status='draft';draft.requirement={format:2,revision:'prototype-draft-1',gaps:['需要确认系统主题是否优先于应用选择'],spec:{statement:'应用应在深浅主题中使用可辨识的状态颜色。',acceptance:[{id:'AC-1',text:'深浅主题下，增加为绿色、删除为红色，状态保留文字。'}],source:{quote:'颜色要清楚，红绿差异保持熟悉的方式。'},questions:['需要确认系统主题是否优先于应用选择'],links:[]},evidence:[]};}
    state.revision=3;
  }

  function upgradeV4() {
    if(state.revision>=4)return;
    state.mapView={selected:'kanzei-app',mode:'structure'};
    state.runtimeView={range:'7',cid:'all'};
    state.runtimeHistory||=runtimeFixtures();
    const example=state.tasks.find(t=>t.id==='R-381')?.requirement;
    if(example?.revision==='prototype-spec-2'){
      example.revision='prototype-spec-3';
      example.spec.statement='项目管理分别展示需求、缺陷、交付与项目地图。地图先展示横向依赖主干，再查看直接关系与项目运行统计。对话工作清单聚焦当前工作和下一候选，允许手动调整。';
      example.spec.acceptance.find(a=>a.id==='AC-3').text='子代理和终端共用所属对话的活动栏；网页独立打开，支持移动、尺寸调整与页面缩放。运行中手动调整当前工作到下一轮生效。';
    }
    for(const c of state.conversations){
      c.work={currentId:state.tasks.find(t=>t.cid===c.id&&t.status==='doing')?.id||null,nextId:null,pendingId:null};
      c.preview={open:c.dock?.open&&c.dock?.kind==='preview',selected:c.activities?.filter(a=>a.kind==='preview').at(-1)?.id||null,zoom:100,maximized:false,rect:null};
      if(c.dock?.kind==='preview'){c.dock.open=false;c.dock.kind='agent';c.dock.selected=null;}
      if(['running','waiting'].includes(c.phase)&&c.run)c.run.taskId=c.work.currentId;
    }
    state.revision=4;
  }
  function runtimeFixtures() {
    const now=Date.now(),day=86400000;
    const tools=(prefix,kind,statuses,duration)=>statuses.map((status,i)=>({id:`${prefix}-${kind}-${i}`,kind,status,durationMs:Number.isFinite(duration)?duration+i*100:null}));
    return [
      {id:'sample-front-1',cid:'front',pid:'kanzei',startedAt:now-day*.1,outcome:'done',durationMs:120000,tools:[...tools('sf1','terminal',['done','done','done','failed'],2000),...tools('sf1','agent',['done'],12000)],memory:{retrieved:['global:M-001','kanzei:M-001','kanzei:M-002'],injected:['global:M-001','kanzei:M-001'],read:['kanzei:M-001']},sample:true},
      {id:'sample-search-1',cid:'search',pid:'kanzei',startedAt:now-day*.4,outcome:'done',durationMs:60000,tools:[...tools('ss1','search',['done','done'],900),...tools('ss1','fetch',['done','failed'],1800)],memory:{retrieved:['global:M-001','kanzei:M-001'],injected:['global:M-001'],read:[]},sample:true},
      {id:'sample-search-2',cid:'search',pid:'kanzei',startedAt:now-day*1.2,outcome:'error',durationMs:45000,tools:tools('ss2','terminal',['failed','rejected'],2300),memory:{retrieved:['kanzei:M-002'],injected:['kanzei:M-002'],read:null},sample:true},
      {id:'sample-front-2',cid:'front',pid:'kanzei',startedAt:now-day*2,outcome:'stopped',durationMs:20000,tools:tools('sf2','terminal',['stopped'],null),memory:null,sample:true},
      {id:'sample-reader-1',cid:'reader-chat',pid:'reader',startedAt:now-day*.2,outcome:'done',durationMs:180000,tools:tools('sr1','terminal',['done','done'],3000),memory:{retrieved:['reader:M-001'],injected:['reader:M-001'],read:['reader:M-001']},sample:true},
      {id:'sample-front-old',cid:'front',pid:'kanzei',startedAt:now-day*9,outcome:'done',durationMs:300000,tools:tools('sfo','agent',['done'],20000),memory:{retrieved:['global:M-001'],injected:['global:M-001'],read:['global:M-001']},sample:true}
    ];
  }
  function workState(c=convo()) {return c.work||=( {currentId:null,nextId:null,pendingId:null} );}
  function executableTask(t,c=convo()) {return !!t&&t.pid===c.pid&&['doing','pending'].includes(t.status)&&!t.requirement?.gaps?.length&&(!t.requirement?.spec||!requirementGaps(t.requirement.spec).length);}
  function ownerBusy(t,c=convo()){const owner=t?.cid&&convo(t.cid);return owner&&owner.id!==c.id&&active(owner);}
  function currentWork(c=convo()) {const id=active(c)?c.run?.taskId:workState(c).currentId,t=state.tasks.find(t=>t.id===id);return t&&['doing','pending'].includes(t.status)?t:null;}
  function nextWork(c=convo()) {
    const w=workState(c),eligible=t=>executableTask(t,c)&&t.status==='pending'&&t.id!==w.currentId&&!ownerBusy(t,c);
    return state.tasks.find(t=>t.id===w.nextId&&eligible(t))||state.tasks.filter(eligible).sort((a,b)=>(a.priority||'P9').localeCompare(b.priority||'P9'))[0]||null;
  }
  function applyCurrentWork(c,id) {
    const t=state.tasks.find(t=>t.id===id);if(!executableTask(t,c)||ownerBusy(t,c))return false;
    const w=workState(c),old=state.tasks.find(t=>t.id===w.currentId);
    if(old&&old.id!==id&&old.cid===c.id&&old.status==='doing')old.status='pending';
    for(const other of state.conversations)if(other.id!==c.id&&workState(other).currentId===id)other.work.currentId=null;
    w.currentId=id;w.pendingId=null;if(w.nextId===id)w.nextId=null;t.cid=c.id;t.status='doing';return true;
  }
  function selectWork(id,slot) {
    const c=convo(),t=state.tasks.find(t=>t.id===id);if(!executableTask(t,c)||ownerBusy(t,c)){toast('这项工作当前不可选，请重新选择。');return;}
    const w=workState(c);
    if(slot==='next'){if(t.status!=='pending')return;w.nextId=id;}
    else if(active(c))w.pendingId=id;
    else applyCurrentWork(c,id);
    closeDialog();save();renderSidebar();renderComposer();renderInspector();toast(slot==='next'?'已调整下一候选。':active(c)?'已安排下一轮切换，本轮工作保持不变。':'已切换当前工作，原进度保留。');
  }
  function workPicker(slot='current') {
    const c=convo();openDialog(slot==='next'?'选择下一候选':'调整当前工作',`<p class="dialog-subtitle">${esc(c.title)}${active(c)&&slot==='current'?' · 选择将在下一轮生效':''}</p><input class="search-dialog-input" id="work-picker-query" data-slot="${slot}" aria-label="搜索可选工作" placeholder="搜索需求或缺陷…"><div id="work-picker-results">${workPickerRows(slot)}</div><p class="dialog-subtitle">草稿、阻塞和待验收项保留在项目管理中。</p>`);
  }
  function workPickerRows(slot,query='') {
    const c=convo(),list=state.tasks.filter(t=>executableTask(t,c)&&(slot!=='next'||t.status==='pending')&&`${t.id} ${t.title}`.toLowerCase().includes(query.toLowerCase()));
    return list.map(t=>`<div class="work-choice"><div><small>${t.id} · ${t.kind==='defect'?'缺陷':'需求'} · ${statuses[t.status]}</small><strong>${esc(t.title)}</strong>${t.cid&&t.cid!==c.id?`<span>${esc(convo(t.cid)?.title||'其他对话')}</span>`:''}</div>${button('select-work',ownerBusy(t,c)?'对话运行中':slot==='next'?'选为下一候选':t.cid&&t.cid!==c.id?'转交并选为当前':'选为当前','button small',`data-id="${t.id}" data-value="${slot}" ${ownerBusy(t,c)?'disabled':''}`)}</div>`).join('')||'<p class="workspace-note">没有符合条件的工作。</p>';
  }
  function workQueueMarkup() {
    const c=convo(),w=workState(c),current=currentWork(c),next=nextWork(c),pending=state.tasks.find(t=>t.id===w.pendingId);
    const row=(t,label,slot)=>`<section class="focus-work-slot" data-work-slot="${slot}"><div class="focus-slot-heading"><h3>${label}</h3>${button('pick-work',slot==='current'?'调整':'更换','text-button',`data-value="${slot}"`)}</div>${t?`<button class="work-list-item" data-action="task" data-id="${t.id}"><span class="work-list-meta"><span class="mono">${t.id}</span><span>${t.kind==='defect'?'缺陷':'需求'}</span></span><strong>${esc(t.title)}</strong>${batchGrid(t)}</button>${slot==='next'?button('select-work',active(c)?'下一轮改做这项':'设为当前','text-button',`data-id="${t.id}" data-value="current"`):''}`:`<p class="focus-work-empty">${slot==='current'?'尚未选取当前工作':'暂无可执行候选'}</p>`}</section>`;
    return inspectorHead('工作清单')+`<div class="inspector-content"><p class="inspector-subtitle">${esc(c.title)}</p>${row(current,'正在进行','current')}${pending?`<div class="pending-work"><span>下轮切换 · ${pending.id}</span><strong>${esc(pending.title)}</strong>${button('cancel-work-switch','取消切换','text-button')}</div>`:''}${row(next,'下一候选','next')}<div class="focus-work-footer">${button('all-work','全部工作','button')}<span>查看项目完整台账</span></div></div>`;
  }
  function recordRuntime(c,outcome) {
    const run=c.run;if(!run?.started)return;
    const endedAt=Date.now();const record={id:run.token,cid:c.id,pid:c.pid,startedAt:run.started,endedAt,outcome,durationMs:endedAt-run.started,tools:(c.activities||[]).filter(a=>a.run===run.token).map(a=>({id:a.id,kind:a.kind,status:a.status,durationMs:typeof a.startedAt==='number'&&typeof a.endedAt==='number'?a.endedAt-a.startedAt:null})),memory:null,sample:false};
    state.runtimeHistory||=[];const index=state.runtimeHistory.findIndex(r=>r.id===record.id);if(index<0)state.runtimeHistory.push(record);else state.runtimeHistory[index]=record;
  }
  function runtimeRecords() {
    const v=state.runtimeView,cutoff=v.range==='all'?0:Date.now()-Number(v.range)*86400000;
    return (state.runtimeHistory||[]).filter(r=>r.pid===state.current.pid&&r.startedAt>=cutoff&&(v.cid==='all'||r.cid===v.cid)).sort((a,b)=>b.startedAt-a.startedAt);
  }
  function durationText(ms) {if(!Number.isFinite(ms))return '未记录';const sec=Math.round(ms/1000);return sec<60?`${sec} 秒`:`${Math.floor(sec/60)} 分 ${sec%60} 秒`;}
  function runtimeMarkup() {
    const v=state.runtimeView,records=runtimeRecords(),completed=records.filter(r=>r.outcome==='done'&&Number.isFinite(r.durationMs)),calls=[...new Map(records.flatMap(r=>r.tools||[]).map(t=>[t.id,t])).values()],failed=calls.filter(t=>t.status==='failed').length,rejected=calls.filter(t=>t.status==='rejected').length,stopped=calls.filter(t=>t.status==='stopped').length;
    const average=completed.length?completed.reduce((n,r)=>n+r.durationMs,0)/completed.length:null;
    const memoryRecords=records.filter(r=>r.memory),reads=memoryRecords.filter(r=>Array.isArray(r.memory.read));
    const unique=a=>[...new Set(a||[])],count=(list,key)=>list.reduce((n,r)=>n+unique(r.memory[key]).length,0),metrics=[['平均运行时长',durationText(average),`已完成 ${completed.length} 轮`,'duration'],['工具调用',String(calls.length),'按调用记录计数','calls'],['工具失败',String(failed),`规则拒绝 ${rejected} · 停止 ${stopped}`,'failures'],['记忆读取',reads.length?`${count(reads,'read')} 条次`:'未记录',`${reads.length} 轮有读取观察`,'memory']];
    const toolRows=[...new Set(calls.map(t=>t.kind))].map(kind=>{const group=calls.filter(t=>t.kind===kind),timed=group.filter(t=>t.status==='done'&&Number.isFinite(t.durationMs));return `<tr><th>${({agent:'子代理',terminal:'终端',preview:'网页预览',search:'搜索',fetch:'网页抓取'})[kind]||esc(kind)}</th><td>${group.length}</td><td>${group.filter(t=>t.status==='failed').length}</td><td>${durationText(timed.length?timed.reduce((n,t)=>n+t.durationMs,0)/timed.length:null)}</td></tr>`;}).join('');
    const memoryRows=['global','project'].map(scope=>{const scopeCount=(list,key)=>list.reduce((n,r)=>n+unique(r.memory[key]).filter(id=>scope==='global'?id.startsWith('global:'):!id.startsWith('global:')).length,0);return `<tr><th>${scope==='global'?'全局记忆':'项目记忆'}</th><td>${memoryRecords.length?scopeCount(memoryRecords,'retrieved'):'未记录'}</td><td>${memoryRecords.length?scopeCount(memoryRecords,'injected'):'未记录'}</td><td>${reads.length?scopeCount(reads,'read'):'未记录'}</td></tr>`;}).join('');
    return `<section class="runtime-section"><div class="runtime-heading"><div><h2>项目运行</h2><p>示例运行数据 · ${records.length} 条记录</p></div><div class="runtime-filters">${button('runtime-refresh','刷新','text-button','aria-label="刷新运行数据"')}<select id="runtime-conversation" aria-label="运行数据对话"><option value="all">全部对话</option>${state.conversations.filter(c=>c.pid===state.current.pid).map(c=>`<option value="${c.id}" ${v.cid===c.id?'selected':''}>${esc(c.title)}</option>`).join('')}</select><select id="runtime-range" aria-label="运行数据时间范围">${[['1','最近 24 小时'],['7','最近 7 天'],['all','全部时间']].map(([id,label])=>`<option value="${id}" ${v.range===id?'selected':''}>${label}</option>`).join('')}</select></div></div><div class="runtime-metrics">${metrics.map(([label,value,note,id])=>`<div data-metric="${id}"><span>${label}</span><strong>${value}</strong><small>${note}</small></div>`).join('')}</div><div class="runtime-tables"><section><h3>工具调用</h3><table><thead><tr><th>工具</th><th>调用</th><th>失败</th><th>成功均时</th></tr></thead><tbody>${toolRows||'<tr><td colspan="4">暂无调用记录</td></tr>'}</tbody></table></section><section><h3>记忆使用观察 <span>条次</span></h3><table><thead><tr><th>范围</th><th>召回</th><th>注入</th><th>读取</th></tr></thead><tbody>${memoryRows}</tbody></table><p class="runtime-note">${records.length-memoryRecords.length} 轮未记录记忆事件。读取与注入分别统计。</p></section></div><details class="runtime-method"><summary>统计说明</summary><p>平均运行时长只含有完整计时的已完成轮次；失败、取消和中断单独记录。工具失败不包含规则拒绝和手动停止。记忆按每轮独立条目累计，读取记录不代表效果证明。这里由示例数据与本地演示记录计算，尚未连接正式运行服务。</p></details><h3 class="runtime-history-title">最近运行</h3><div class="runtime-history">${records.slice(0,6).map(r=>`<button data-action="runtime-record" data-id="${esc(r.id)}" class="runtime-history-row"><span><strong>${esc(convo(r.cid)?.title||'历史对话')}</strong><small>${new Date(r.startedAt).toLocaleString('zh-CN',{month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit'})} · ${r.sample?'示例记录':'本地演示'}</small></span><span>${({done:'已完成',error:'执行失败',stopped:'已停止',interrupted:'刷新中断'})[r.outcome]||esc(r.outcome)}</span><span>${durationText(r.durationMs)}</span>${icon('right',true)}</button>`).join('')||'<p class="workspace-note">此范围内没有运行记录。</p>'}</div></section>`;
  }
  function directEdges() {return PROJECT_INDEX.edges.filter(e=>e.kinds.some(k=>!k.startsWith('dev-')));}
  function dependencyBackbone(edges) {
    const reachable=(start,target,skip)=>{const seen=new Set([start]),queue=[start];while(queue.length){const id=queue.shift();for(const edge of edges){if(edge===skip||edge.from!==id)continue;if(edge.to===target)return true;if(!seen.has(edge.to)){seen.add(edge.to);queue.push(edge.to);}}}return false;};
    if(edges.some(e=>reachable(e.to,e.from,null)))return edges;
    return edges.filter(e=>!reachable(e.from,e.to,e));
  }
  function dependencyDiagram(mode) {
    const selected=PROJECT_INDEX.nodes.find(n=>n.id===state.mapView.selected)||PROJECT_INDEX.nodes[0],all=directEdges(),focus=mode==='focus';
    const edges=focus?all.filter(e=>e.from===selected.id||e.to===selected.id):dependencyBackbone(all),nodeIds=new Set(edges.flatMap(e=>[e.from,e.to]));nodeIds.add(selected.id);
    const nodes=focus?PROJECT_INDEX.nodes.filter(n=>nodeIds.has(n.id)):PROJECT_INDEX.nodes,max=focus?2:Math.max(...nodes.map(n=>n.level));
    const layer=n=>focus?(n.id===selected.id?1:edges.some(e=>e.from===n.id&&e.to===selected.id)?0:2):max-n.level;
    const groups=Array.from({length:max+1},(_,i)=>nodes.filter(n=>layer(n)===i)),rows=Math.max(...groups.map(g=>g.length)),height=Math.max(235,rows*78+62),width=(max+1)*180+10,positions=new Map();
    groups.forEach((group,i)=>group.forEach((n,j)=>positions.set(n.id,{x:88+i*180,y:height/2+(j-(group.length-1)/2)*78})));
    const line=e=>{const a=positions.get(e.from),b=positions.get(e.to),mid=(a.x+b.x)/2;return `<path data-edge-from="${e.from}" data-edge-to="${e.to}" class="tree-edge" d="M${a.x+72} ${a.y}H${mid}V${b.y}H${b.x-72}"><title>${e.from} 依赖 ${e.to}</title></path>`;};
    return `<div class="project-map ${focus?'focus-map':''}"><svg class="dependency-map" viewBox="0 0 ${width} ${height}" aria-label="${focus?'模块直接依赖':'横向主干依赖图'}">${focus?['依赖此模块','当前模块','此模块依赖'].map((text,i)=>`<text class="tree-column-label" x="${88+i*180}" y="23" text-anchor="middle">${text}</text>`).join(''):''}${edges.map(line).join('')}${nodes.map(n=>{const a=positions.get(n.id);return `<g class="dependency-node ${n.id===selected.id?'selected':''}" role="button" tabindex="0" data-map-id="${n.id}" aria-label="查看模块 ${n.id}"><rect x="${a.x-72}" y="${a.y-22}" width="144" height="44" rx="7"/><text x="${a.x}" y="${a.y+4}" text-anchor="middle">${n.id}</text></g>`;}).join('')}</svg></div>`;
  }
  const activityNames={agent:'子代理',terminal:'终端'};
  function activityState(c=convo()) {c.activities||=[];c.dock||={open:false,kind:'agent',selected:null,follow:true};return c.dock;}
  function openActivity(kind,id=null) {
    if(kind==='preview'){openPreview(id);return;}
    const c=convo(),d=activityState(c);d.open=true;d.follow=false;d.kind=kind||d.kind;d.selected=id||c.activities.filter(a=>a.kind===d.kind).at(-1)?.id||null;
    if(state.current.view!=='chat')navigate('chat',c.pid,c.id);openPanel('activity',c.id);save();
  }

  function addActivity(c,kind) {
    const d=activityState(c),a={id:uid(kind),kind,run:c.run.token,status:'running',startedAt:Date.now(),title:({agent:'交互检查',terminal:'检查项目',preview:'阅读器预览'})[kind],logs:kind==='terminal'?['$ npm run check','正在检查示例工作区…']:[],messages:[],draft:'',bookmarks:0};
    c.activities.push(a);
    if(state.current.cid===c.id&&state.current.view==='chat') {
      if(kind==='preview'){const p=previewState(c);if(p.dismissedRun!==c.run.token){if(p.follow||!p.open)p.selected=a.id;p.open=true;renderPreview();}}
      else if(d.dismissedRun!==c.run.token&&(!panel||panel.type==='activity')) {d.open=true;if(d.follow||!panel){d.kind=kind;d.selected=a.id;}panel={type:'activity',id:c.id};renderActivity();}
    }
    c.messages.push({id:uid('call'),role:'tool',activityId:a.id,kind,text:`${activityNames[kind]||'网页预览'} · ${a.title}`});refreshRun(c,true);return a;
  }

  function previewDocument(a) {
    return `<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>*{box-sizing:border-box}body{margin:0;background:#faf9f6;color:#2b302d;font:14px/1.7 "Segoe UI","Microsoft YaHei",sans-serif}header{padding:20px 24px;border-bottom:1px solid #dfdfd8;display:flex;justify-content:space-between;font-size:12px}main{padding:32px 24px}small{color:#737b72}h1{font-size:28px;font-weight:500;margin:22px 0}p{line-height:2}button{margin-top:25px;background:#2c4136;color:white;border:0;border-radius:6px;padding:10px 18px;cursor:pointer}footer{margin-top:36px;color:#83897f;font-size:12px}progress{width:100%;height:4px;accent-color:#476551}</style><header><strong>阅读器</strong><span>我的书架</span></header><main><small>正在阅读 · 第三章</small><h1>留一点时间给阅读</h1><p>可以拖动浮窗、调整尺寸和页面缩放。添加书签后，切换活动仍保留当前记录。</p><progress value="38" max="100"></progress><small>阅读进度 38%</small><br><button id="bookmark">添加书签</button><p id="count">已保存 ${Number(a.bookmarks)||0} 个书签</p><footer>用于独立预览验收的示例页面</footer></main><script>let bookmarks=${Number(a.bookmarks)||0};document.getElementById('bookmark').onclick=()=>{bookmarks++;document.getElementById('count').textContent='已保存 '+bookmarks+' 个书签';parent.postMessage({type:'prototype-bookmark',id:${JSON.stringify(a.id)}},'*');};<\/script></html>`;
  }
  function renderActivity() {
    const el=$('#inspector'),c=convo(panel?.id);if(!c)return;
    const d=activityState(c),items=c.activities.filter(a=>a.kind===d.kind),a=items.find(x=>x.id===d.selected)||items.at(-1);
    const focused=el.contains(document.activeElement)?document.activeElement:null,focusId=focused?.id,selection=focused?.selectionStart,scroll=el.scrollTop;
    let content='';
    if(!a)content=`<div class="activity-empty">${icon('panel')}<h3>暂无${activityNames[d.kind]}调用</h3><p>当前对话调用后会在这里展开。</p>${button('demo-tools','演示工具调用','button')}</div>`;
    else if(d.kind==='agent')content=`<div class="activity-title"><h2>${esc(a.title)}</h2><span class="status ${a.status==='running'?'doing':a.status==='done'?'done':''}">${a.status==='running'?'执行中':a.status==='done'?'已完成':'已停止'}</span></div><p class="activity-meta">所属对话 · ${esc(c.title)}</p><ol class="agent-steps"><li>检查对话和输入区</li><li>核对页面与按钮归属</li><li>${a.status==='done'?'已返回检查结果':'整理结果与建议'}</li></ol><div class="agent-result"><strong>${a.status==='done'?'结果摘要':'当前工作'}</strong><p>${a.status==='done'?'示例检查完成。需求正文与验收应优先展示，右侧面板按对话分别保存。':'正在检查示例界面，主对话仍可继续补充。'}</p></div><div class="activity-messages">${a.messages.map(m=>`<p><strong>补充要求</strong>${esc(m)}</p>`).join('')}</div><form id="activity-message-form" data-id="${a.id}"><label class="field"><span>发给「${esc(a.title)}」</span><textarea id="agent-message-draft" name="message" required aria-label="子代理补充要求" placeholder="补充要求…">${esc(a.draft)}</textarea></label><button class="button" type="submit">发送</button></form>`;
    else if(d.kind==='terminal')content=`<div class="activity-title"><h2>终端输出</h2><span class="status ${a.status==='running'?'doing':''}">${a.status==='running'?'运行中':a.status==='done'?'退出码 0':'已停止'}</span></div><p class="activity-meta">${esc(c.title)} · 示例命令</p><pre class="terminal-output">${esc(a.logs.join('\n'))}</pre><div class="detail-actions">${a.status==='running'?button('stop-activity','停止此命令','button',`data-id="${a.id}"`):''}${button('copy-terminal','复制输出','button',`data-id="${a.id}"`)}</div>`;
    el.classList.add('activity-inspector');el.hidden=false;
    el.innerHTML=inspectorHead('活动')+`<nav class="activity-tabs" aria-label="对话活动分类">${Object.entries(activityNames).map(([id,label])=>button('activity-tab',`${label}${c.activities.some(a=>a.kind===id&&a.status==='running')?'<span class="dot running"></span>':''}`,'activity-tab'+(d.kind===id?' active':''),`data-value="${id}" aria-selected="${d.kind===id}"`)).join('')}</nav>${items.length>1?`<select id="activity-instance" aria-label="选择调用记录">${items.map(x=>`<option value="${x.id}" ${x.id===a?.id?'selected':''}>${esc(x.title)} · ${x.status==='running'?'运行中':x.status==='done'?'已完成':'已停止'}</option>`).join('')}</select>`:''}<div class="activity-content">${content}</div>`;
    el.scrollTop=scroll;
    if(focusId){const next=document.getElementById(focusId);if(next){next.focus({preventScroll:true});if(typeof selection==='number')next.setSelectionRange?.(selection,selection);}}
  }

  function previewState(c=convo()) {return c.preview||={open:false,selected:null,zoom:100,rect:null,maximized:false,follow:true};}
  function openPreview(id=null) {
    const c=convo(),p=previewState(c);p.open=true;p.follow=false;p.selected=id||c.activities.filter(a=>a.kind==='preview').at(-1)?.id||null;
    if(state.current.view!=='chat')navigate('chat',c.pid,c.id);renderPreview();save();
  }
  function previewGeometry(rect) {
    const margin=8,maxW=Math.max(280,innerWidth-margin*2),maxH=Math.max(220,innerHeight-margin*2),w=Math.min(maxW,Math.max(Math.min(320,maxW),rect?.w||Math.min(640,innerWidth*.57))),h=Math.min(maxH,Math.max(Math.min(280,maxH),rect?.h||Math.min(620,innerHeight-110)));
    return {w,h,x:Math.min(innerWidth-w-margin,Math.max(margin,rect?.x??innerWidth-w-24)),y:Math.min(innerHeight-h-margin,Math.max(margin,rect?.y??80))};
  }
  function applyPreviewGeometry() {
    const el=$('#preview-window');if(el.hidden)return;const p=previewState(),r=p.maximized?{x:8,y:8,w:innerWidth-16,h:innerHeight-16}:previewGeometry(p.rect);if(!p.maximized)p.rect=r;
    Object.assign(el.style,{left:r.x+'px',top:r.y+'px',width:r.w+'px',height:r.h+'px'});el.classList.toggle('maximized',p.maximized);
    const frame=$('#preview-frame'),scale=(p.zoom||100)/100;if(frame)Object.assign(frame.style,{width:(100/scale)+'%',height:(100/scale)+'%',transform:`scale(${scale})`});
    const zoom=$('#preview-zoom');if(zoom)zoom.value=String(p.zoom||100);
    const max=el.querySelector('[data-action="preview-maximize"]');if(max){max.textContent=p.maximized?'恢复':'最大化';max.setAttribute('aria-label',p.maximized?'恢复预览窗口':'最大化预览窗口');}
  }
  function renderPreview(force=false) {
    const el=$('#preview-window'),c=convo(),p=previewState(c);el.hidden=state.current.view!=='chat'||!p.open;if(el.hidden)return;
    const records=c.activities.filter(a=>a.kind==='preview'),a=records.find(a=>a.id===p.selected)||records.at(-1),binding=`${c.id}:${a?.id||'empty'}`;
    if(force||el.dataset.binding!==binding) {
      el.dataset.binding=binding;el.dataset.owner=c.id;
      el.innerHTML=`<header class="preview-window-head"><div class="preview-drag" tabindex="0" role="button" aria-label="移动网页预览，方向键调整位置" title="拖动标题栏移动窗口">${icon('grid',true)}<strong>网页预览</strong><span>${esc(c.title)}</span></div><div class="preview-window-actions">${button('preview-maximize',p.maximized?'恢复':'最大化','text-button','aria-label="最大化预览窗口"')}${button('preview-close',icon('close'),'icon-button','aria-label="关闭网页预览"')}</div></header><div class="preview-window-toolbar">${records.length>1?`<select id="preview-instance" aria-label="选择网页预览记录">${records.map((r,i)=>`<option value="${r.id}" ${r.id===a?.id?'selected':''}>${esc(r.title)} · ${i+1}</option>`).join('')}</select>`:`<span>${a?'示例 · 阅读器':'暂无网页调用'}</span>`}<span class="grow"></span><label>缩放 <select id="preview-zoom" aria-label="网页缩放">${[50,75,100,125,150].map(v=>`<option value="${v}" ${v===p.zoom?'selected':''}>${v}%</option>`).join('')}</select></label>${button('preview-reset','重置窗口','text-button')}${button('preview-refresh','刷新','text-button')}</div><div class="preview-window-content">${a?`<iframe id="preview-frame" title="独立网页预览" sandbox="allow-scripts" srcdoc="${esc(previewDocument(a))}"></iframe>`:`<div class="activity-empty"><p>网页调用后会自动打开独立预览。</p>${button('demo-tools','演示工具调用','button')}</div>`}</div><button class="preview-resize" aria-label="调整预览窗口大小，方向键调整" title="拖动调整大小"></button>`;
    }
    // New calls update the history selector without recreating the current iframe.
    if(records.length>1){
      let picker=$('#preview-instance');
      if(!picker){picker=document.createElement('select');picker.id='preview-instance';picker.setAttribute('aria-label','选择网页预览记录');el.querySelector('.preview-window-toolbar>span').replaceWith(picker);}
      if(picker.options.length!==records.length)picker.innerHTML=records.map((r,i)=>`<option value="${r.id}" ${r.id===a?.id?'selected':''}>${esc(r.title)} · ${i+1}</option>`).join('');
    }
    applyPreviewGeometry();
  }
  function closePreview() {const p=previewState();p.open=false;p.dismissedRun=convo().run?.token;save();renderPreview();}
  let previewGesture=null;
  function startPreviewGesture(event) {
    const handle=event.target.closest('.preview-drag,.preview-resize');if(!handle||event.button!==0)return;
    event.preventDefault();const p=previewState(),bounds=$('#preview-window').getBoundingClientRect();p.maximized=false;p.rect={x:bounds.x,y:bounds.y,w:bounds.width,h:bounds.height};
    previewGesture={cid:convo().id,pointer:event.pointerId,kind:handle.classList.contains('preview-resize')?'resize':'move',x:event.clientX,y:event.clientY,rect:{...p.rect},handle};handle.setPointerCapture(event.pointerId);$('#preview-window').classList.add('adjusting');
  }
  function movePreviewGesture(event) {
    const g=previewGesture;if(!g||event.pointerId!==g.pointer||convo().id!==g.cid)return;const dx=event.clientX-g.x,dy=event.clientY-g.y,p=previewState();p.rect=previewGeometry(g.kind==='resize'?{...g.rect,w:g.rect.w+dx,h:g.rect.h+dy}:{...g.rect,x:g.rect.x+dx,y:g.rect.y+dy});applyPreviewGeometry();
  }
  function endPreviewGesture(event) {if(!previewGesture||event.pointerId!==previewGesture.pointer)return;$('#preview-window').classList.remove('adjusting');previewGesture=null;save();}
  function closeInspector() {if(panel?.type==='activity'){const c=convo(panel.id),d=activityState(c);d.open=false;d.dismissedRun=c.run?.token;save();}panel=null;renderInspector();}
  function projectContextMenu(target,x,y) {
    closeMenu();menuTrigger=target;menuName='project-context';target.setAttribute('aria-expanded','true');
    const p=state.projects.find(p=>p.id===target.dataset.id),pop=$('#popover');
    pop.innerHTML=`<div class="menu-heading">${esc(p.name)}</div>${button('project','打开项目管理','menu-item',`data-id="${p.id}" role="menuitem"`)}${button('editor','在 VS Code 打开','menu-item',`data-id="${p.id}" role="menuitem"`)}${button('project-settings','项目设置','menu-item',`data-id="${p.id}" role="menuitem"`)}`;pop.hidden=false;
    pop.style.left=`${Math.max(8,Math.min(x,innerWidth-pop.offsetWidth-8))}px`;pop.style.top=`${Math.max(8,Math.min(y,innerHeight-pop.offsetHeight-8))}px`;pop.querySelector('button')?.focus();
  }

  function mapMarkup() {
    state.runtimeView||={range:'7',cid:'all'};if(state.runtimeView.cid!=='all'&&!state.conversations.some(c=>c.id===state.runtimeView.cid&&c.pid===state.current.pid))state.runtimeView.cid='all';
    if(state.current.pid!=='kanzei')return `<p class="workspace-note">这个示例项目尚未生成架构关系。</p>${runtimeMarkup()}`;
    const v=state.mapView,selected=PROJECT_INDEX.nodes.find(n=>n.id===v.selected)||PROJECT_INDEX.nodes[0],edges=directEdges();
    return `<div class="architecture-heading"><div><h2>项目架构</h2><p>从左向右为依赖方向 · ${v.mode==='focus'?'当前模块的全部直接关系':'主干保留层级，省略重复路径'}</p></div><div class="architecture-switch">${button('map-mode','主干结构',v.mode==='structure'?'active':'',`data-value="structure"`)}${button('map-mode','直接依赖',v.mode==='focus'?'active':'',`data-value="focus"`)}</div></div>${dependencyDiagram(v.mode)}<div class="architecture-selection"><span class="mono">${esc(selected.id)}</span><span class="muted">${edges.filter(e=>e.from===selected.id).length} 个直接依赖 · ${edges.filter(e=>e.to===selected.id).length} 个使用方</span><details><summary>查看来源</summary><p>${esc(selected.manifest)} · ${PROJECT_INDEX.head}</p>${PROJECT_INDEX.edges.filter(e=>e.from===selected.id).map(e=>`<p>${esc(e.to)} · ${esc(e.kinds.join('、'))}</p>`).join('')}</details></div>${runtimeMarkup()}`;
  }

  function renderPage(reset = false) {
    const view=state.current.view;
    if(view==='chat') renderMessages(reset);
    else if(view==='manage') renderManage();
    else if(view==='work-detail')renderWorkDetail();
    else if(view==='delivery-detail')renderDeliveryDetail();
    else if(view==='memory')renderMemory();
    else if(view==='settings')renderSettings();
    else if(view==='schedules')renderSchedules();

    else if(view==='coverage')renderCoverage();
    else renderWorkspace(view==='attention');
  }
  function render(reset = false) {
    document.documentElement.dataset.theme=state.theme;
    renderSidebar();renderTopbar();renderPage(reset);renderComposer();renderInspector();renderPreview();
  }
  function autoHeight() {const el=$('#prompt');el.style.height='auto';el.style.height=`${Math.min(190,Math.max(65,el.scrollHeight))}px`;}
  function renderComposer() {
    const c=convo(), view=state.current.view, busy=active(c);
    $('#compose-zone').hidden=!['chat','manage'].includes(view);
    $('#compose-zone').classList.toggle('compact',compact && view!=='chat');
    $('#compose-expand').innerHTML=`${icon('chat')}<span>给「${esc(c.title)}」补充消息${c.draft?' · 有草稿':''}</span><span class="grow"></span>${icon('plus',true)}`;
    $('#recipient-line').innerHTML=view==='manage'?`发给 ${icon('chat',true)}<strong>${esc(c.title)}</strong>${button('collapse-compose','收起','text-button')}`:'';
    if ($('#prompt').value!==c.draft) $('#prompt').value=c.draft;
    $('#prompt').placeholder=c.messages.length?'继续这个任务…':'想做什么？';
    $('#attachment-list').innerHTML=c.attachments.map(f=>`<span class="attachment-chip">${icon('file',true)}<span title="${esc(f.name)}">${esc(f.name)}</span>${button('remove-attachment',icon('close',true),'',`data-id="${f.id}" aria-label="移除附件 ${esc(f.name)}"`)}</span>`).join('');
    $('#model-control').innerHTML=`${esc(c.config.model)} ${icon('chevron',true)}`;
    $('#model-control').setAttribute('aria-label',`模型 ${c.config.model}`);
    $('#thinking-control').innerHTML=`思考 ${c.config.thinking} ${icon('chevron',true)}`;
    $('#thinking-control').setAttribute('aria-label',`思考 ${c.config.thinking}`);
    $('#mode-control').innerHTML=`${c.config.mode} ${icon('chevron',true)}`;
    $('#mode-control').setAttribute('aria-label',`模式 ${c.config.mode}`);
    $('#permission-control').innerHTML=`${icon('shield',true)}${state.permission==='auto'?'自动放行':'按需询问'}`;
    $('#permission-control').dataset.full=String(state.permission==='auto');
    $('#permission-control').setAttribute('aria-label',`权限 ${state.permission==='auto'?'自动放行':'按需询问'}`);
    $('#permission-control').title='应用级权限设置';
    $('#delivery-control').hidden=!busy;
    $('#delivery-control').innerHTML=`${c.delivery==='queue'?'排队':'插入'} ${icon('chevron',true)}`;
    $('#delivery-control').setAttribute('aria-label','运行中消息发送方式');
    $('#stop').hidden=!busy;
    $('#send').disabled=!c.draft.trim() && !c.attachments.length;
    const sendLabel=busy?(c.delivery==='queue'?'加入队列':'插入当前运行'):'发送消息';
    $('#send').setAttribute('aria-label',sendLabel);$('#send').title=sendLabel;
    $('#voice').classList.toggle('recording',recordingCid===c.id);
    $('#more-control').innerHTML=`${icon('more')}${c.config.whip||c.config.subagents?'<span class="dot" style="width:4px;height:4px"></span>':''}`;
    const extras=[c.config.whip?'鞭挞开启':'',c.config.subagents?'子代理开启':''].filter(Boolean).join(' · ');
    const changed=busy && c.run && JSON.stringify(c.run.config)!==JSON.stringify(c.config);
    $('#config-hint').textContent=changed?'设置已更新 · 下一轮生效':extras;
    $('#compose-hint').textContent=busy?`Enter ${c.delivery==='queue'?'排队':'插入'} · Shift+Enter 换行`:'Enter 发送 · Shift+Enter 换行';
    $('#queue-list').innerHTML=c.queue.map((q,i)=>`<div class="queue-item">${icon('clock',true)}<span class="queue-text">${i+1}. ${esc(q.text || q.attachments.map(f=>f.name).join('、'))}</span><span>待发送</span>${button('remove-queue',icon('close',true),'icon-button',`data-id="${q.id}" aria-label="移除排队消息 ${i+1}"`)}</div>`).join('');
    const status=$('#run-status');status.dataset.state=c.phase;
    if(c.phase==='running') status.innerHTML=`<span class="dot running"></span><span>${c.run?.taskId?`${esc(c.run.taskId)} · `:''}${esc(c.run?.stage||'准备执行')} <span class="muted">· 演示</span></span>`;
    else if(c.phase==='waiting') status.innerHTML=`${icon('clock',true)}<span>等待你确认 · 示例文件写入</span><span class="status-actions">${button('approve-run','允许这次','status-action')}${button('stop','拒绝','status-action')}</span>`;
    else if(c.phase==='error') status.innerHTML=`<span class="dot error"></span><span>执行失败 · 示例连接中断</span><span class="status-actions">${button('retry','重试','status-action')}</span>`;
    else if(c.phase==='stopped') status.innerHTML=`${icon('stop',true)}<span>${c.reloaded?'重载后已暂停': '已停止'}${c.queue.length?` · ${c.queue.length} 条排队保留`:''}</span><span class="status-actions">${button('resume',c.queue.length?'继续队列':'继续运行','status-action')}</span>`;
    else status.innerHTML='';
    autoHeight();
  }
  const options = {
    model:[['GPT-6 Luna','日常工作 · 示例模型'],['GPT-6 Sol','复杂任务 · 示例模型'],['Claude Sonnet','示例模型']],
    thinking:[['低',''],['中',''],['高',''],['超高','']],
    mode:[['结伴开发','逐步协作，需要时向你确认'],['自主推进','围绕当前任务连续推进']],
    permission:[['ask','按需询问'],['auto','自动放行']],
    delivery:[['queue','排队 · 当前运行完成后发送'],['steer','插入 · 现在补充给当前运行']]
  };
  function closeMenu(restore = false) {
    $('#popover').hidden=true;
    if(menuTrigger) {menuTrigger.setAttribute('aria-expanded','false');if(restore&&menuTrigger.isConnected)menuTrigger.focus();}
    menuTrigger=null;menuName=null;
  }
  function openMenu(name, trigger) {
    if(menuName===name) {closeMenu(true);return;}
    closeMenu();menuName=name;menuTrigger=trigger;trigger.setAttribute('aria-expanded','true');
    const c=convo(), pop=$('#popover');
    if(name==='more') pop.innerHTML=`<div class="menu-heading">当前对话 · 下一轮执行选项</div>${[['whip','鞭挞','自动继续处理当前任务的剩余项'],['subagents','子代理','允许委派并行子任务']].map(([id,label,desc])=>`<button class="menu-item" role="menuitemcheckbox" aria-checked="${c.config[id]}" data-setting="${id}" data-value="${!c.config[id]}"><span class="menu-item-copy">${label}<small>${desc}</small></span><span class="switch-track"></span></button>`).join('')}<div class="menu-separator"></div>${button('side-question',`${icon('chat',true)}<span class="menu-item-copy">旁路提问<small>单独问一句，保留当前运行</small></span>`,'menu-item','role="menuitem"')}`;
    else {
      const current=name==='permission'?state.permission:name==='delivery'?c.delivery:c.config[name];
      const heading={model:'当前对话的模型',thinking:'当前对话的思考强度',mode:'当前对话的执行模式',permission:'应用级权限 · 影响所有对话',delivery:'运行中的消息'}[name];
      pop.innerHTML=`<div class="menu-heading">${heading}${active(c)&&name!=='delivery'&&name!=='permission'?' · 下一轮生效':''}</div>${options[name].map(([value,desc])=>{const special=['permission','delivery'].includes(name);return `<button class="menu-item" role="menuitemradio" aria-checked="${current===value}" data-setting="${name}" data-value="${esc(value)}"><span class="menu-item-copy">${esc(special?desc:value)}${!special&&desc?`<small>${esc(desc)}</small>`:''}</span>${current===value?icon('check',true):''}</button>`;}).join('')}${name==='permission'?'<div class="menu-heading">原型只演示设置范围，不执行真实操作。</div>':''}`;
    }
    pop.hidden=false;
    const r=trigger.getBoundingClientRect(), w=pop.offsetWidth, h=pop.offsetHeight;
    pop.style.left=`${Math.max(10,Math.min(r.left,innerWidth-w-10))}px`;
    pop.style.top=`${Math.max(10,r.top>h+18?r.top-h-9:Math.min(innerHeight-h-10,r.bottom+9))}px`;
    pop.querySelector('[aria-checked="true"],button')?.focus({preventScroll:true});
  }
  function chooseSetting(name,value) {
    const c=convo();
    if(name==='permission') {state.permission=value;toast(`应用权限已设为${value==='auto'?'自动放行':'按需询问'}，所有对话共享。`);}
    else if(name==='delivery') c.delivery=value;
    else {c.config[name]=['whip','subagents'].includes(name)?value==='true':value;if(active(c))toast('设置已保存，下一轮执行生效。');}
    closeMenu(true);save();renderComposer();
  }
  function openPanel(type,id) {closeMenu();panel={type,id};renderInspector();}
  function inspectorHead(title,back=false) {return `<div class="inspector-head">${back?button('work-list',icon('back'),'icon-button','aria-label="返回工作清单"'):''}<strong>${title}</strong>${button('close-panel',icon('close'),'icon-button','aria-label="关闭详情面板"')}</div>`;}
  function renderInspector() {
    const el=$('#inspector');el.hidden=!panel;el.classList.toggle('activity-inspector',panel?.type==='activity');if(!panel)return;if(panel.type==='activity'){renderActivity();return;}
    if(panel.type==='work-list') {
      el.innerHTML=workQueueMarkup();
    } else if(panel.type==='changes') {
      el.innerHTML=inspectorHead('改动')+`<div class="inspector-content"><p class="inspector-subtitle">3 个文件 · <span class="diff-add">+57</span> <span class="diff-del">−7</span> · 示例</p>${[
        ['ui/index.html','− 输入区顶部：对话 / 概览 / 需求','+ 页头：对话 / 管理 / 工作清单'],
        ['ui/work-list.js','− 工作条目 + 未关联运行中的对话','+ 需求与缺陷；对话关联放入详情'],
        ['ui/app.css','− 品牌、运行和错误共用橙色','+ 错误使用独立红色，并保留状态文字']
      ].map(([name,del,add])=>`<details class="diff-file" open><summary class="mono">${name}</summary><div class="diff-code"><span class="deletion">${esc(del)}</span>\n<span class="addition">${esc(add)}</span></div></details>`).join('')}</div>`;
    } else if(panel.type==='side-question') {
      const c=convo(panel.id);
      el.innerHTML=inspectorHead('旁路提问')+`<div class="inspector-content side-question"><p class="inspector-subtitle">关于「${esc(c.title)}」· 不进入主消息队列</p><div id="side-messages">${c.sideMessages.map(m=>`<div class="side-answer"><strong>${esc(m.question)}</strong>${esc(m.answer)}</div>`).join('')}</div><textarea id="side-prompt" aria-label="旁路问题" placeholder="单独问一句…"></textarea>${button('send-side','提问','button primary')}</div>`;
    }
  }
  function clearRun(id) {for(const handle of timers.get(id)||[])clearTimeout(handle);timers.delete(id);}
  function refreshRun(c, messages=false) {
    save();renderSidebar();
    if(state.current.cid===c.id) {if(messages&&state.current.view==='chat')renderMessages();renderComposer();if(panel?.type==='activity')renderActivity();else if(panel?.type==='work-list')renderInspector();renderTopbar();renderPreview();}
  }
  function startRun(c, input, attachments=[], append=true, demoTools=false) {
    if(active(c)){for(const a of c.activities||[])if(a.run===c.run?.token&&a.status==='running'){a.status='stopped';a.endedAt=Date.now();}recordRuntime(c,'stopped');}
    clearRun(c.id);c.reloaded=false;const work=workState(c);
    if(work.pendingId&&!applyCurrentWork(c,work.pendingId)){
      c.phase='stopped';c.queue.unshift({id:uid('queue'),text:input,attachments});refreshRun(c);
      toast('待切换工作已不可执行。消息保留在队列，请调整工作或取消切换后继续。');return false;
    }
    if(append) c.messages.push({id:uid('message'),role:'user',text:input,attachments});
    if(c.title==='新对话'&&input) {c.title=input.slice(0,20);if(state.current.cid===c.id)renderTopbar();}
    const token=uid('run');activityState(c).follow=true;previewState(c).follow=true;const toolPlan=demoTools?['agent','terminal','preview']:[...(c.config.subagents?['agent']:[]),...(/终端|命令|测试|构建/.test(input)?['terminal']:[]),...(/网页|浏览器|预览/.test(input)?['preview']:[])];
    c.run={token,input,taskId:workState(c).currentId,config:{...c.config},permission:state.permission,stage:'准备执行',started:Date.now(),toolPlan};c.phase='running';
    const schedule=(ms,fn)=>setTimeout(()=>{if(c.run?.token===token && c.phase==='running')fn();},ms);
    timers.set(c.id,[
      schedule(900,()=>{c.run.stage='读取当前任务';if(toolPlan[0])addActivity(c,toolPlan[0]);else refreshRun(c);}),
      schedule(2400,()=>{c.run.stage='执行工具';if(toolPlan[1])addActivity(c,toolPlan[1]);else refreshRun(c);}),
      schedule(4300,()=>{c.run.stage='检查结果';if(toolPlan[2])addActivity(c,toolPlan[2]);else refreshRun(c);}),
      schedule(6800,()=>finishRun(c))
    ]);
    refreshRun(c,true);
  }
  function finishRun(c) {
    clearRun(c.id);c.phase='done';for(const a of c.activities||[])if(a.run===c.run?.token&&a.status==='running'){a.status='done';a.endedAt=Date.now();if(a.kind==='terminal')a.logs.push('✓ 示例检查通过','Process exited with code 0');}
    recordRuntime(c,'done');
    const text=c.run?.input || '继续当前任务';
    c.messages.push({id:uid('reply'),role:'assistant',demo:true,text:`已收到「${text.length>75?text.slice(0,75)+'…':text}」。\n\n这轮演示已完成。你可以继续补充消息，或到管理页查看工作与交付。实际接入时，这里会展示模型的执行结果。`});
    if(c.queue.length) {const next=c.queue.shift();startRun(c,next.text,next.attachments);}
    else {refreshRun(c,true);if(state.current.cid!==c.id)toast(`「${c.title}」的演示运行已完成。`);}
  }
  function send() {
    const c=convo(), text=c.draft.trim(), attachments=[...c.attachments];
    if(!text&&!attachments.length)return;
    c.draft='';c.attachments=[];
    if(active(c)) {
      if(c.delivery==='queue') {c.queue.push({id:uid('queue'),text,attachments});refreshRun(c);toast('已排队；当前运行完成后发送。');}
      else {c.messages.push({id:uid('message'),role:'user',text,attachments},{id:uid('note'),role:'note',text:'已插入当前演示运行'});refreshRun(c,true);toast('已补充给当前运行。');}
    } else startRun(c,text,attachments);
    $('#prompt').focus();
  }
  function stopRun() {const c=convo();if(!active(c))return;clearRun(c.id);c.phase='stopped';for(const a of c.activities||[])if(a.run===c.run?.token&&a.status==='running'){a.status='stopped';a.endedAt=Date.now();if(a.kind==='terminal')a.logs.push('Process stopped');}recordRuntime(c,'stopped');refreshRun(c);}
  function resumeRun() {
    const c=convo();
    if(c.queue.length) {const q=c.queue.shift();startRun(c,q.text,q.attachments);}
    else startRun(c,c.run?.input || '继续整理前端交互',[],false);
  }
  function scenario(name) {
    const c=convo();clearRun(c.id);closeDialog();navigate('chat',c.pid,c.id);
    if(name==='running') startRun(c,'演示一次执行流程',[],true);
    else {
      c.reloaded=false;c.phase=name;
      c.run={token:uid('scene'),input:'演示一次执行流程',taskId:workState(c).currentId,config:{...c.config},stage:'等待结果'};
      if(name==='done') {c.messages.push({id:uid('reply'),role:'assistant',demo:true,text:'演示任务已完成。交付结果放在管理页，需要你确认的内容会出现在“待我处理”。'});}
      refreshRun(c,true);
    }
  }
  function addAttachments(files, cid=state.current.cid) {
    const c=convo(cid);if(!c)return;
    for(const f of files)c.attachments.push({id:uid('file'),name:f.name || '粘贴的图片.png',size:f.size});
    save();if(c.id===state.current.cid)renderComposer();toast('已添加附件信息；原型不读取或上传文件内容。');
  }
  function newChat(pid=state.current.pid) {
    const c=makeConversation(uid('conversation'),pid,'新对话');c.config.model=state.preferences.defaultModel;c.config.thinking=state.preferences.defaultThinking;state.conversations.push(c);navigate('chat',pid,c.id);$('#prompt').focus();
    return c;
  }
  function openDialog(title,body) {
    closeMenu();$('#dialog-content').innerHTML=`<div class="dialog-head"><h2>${title}</h2>${button('close-dialog',icon('close'),'icon-button','aria-label="关闭弹窗"')}</div><div class="dialog-content">${body}</div>`;
    if(!$('#dialog').open)$('#dialog').showModal();
  }
  function closeDialog() {$('#dialog').close();}
  const checks=[
    ['familiar','常用操作是否熟悉','切换模型、思考、模式、权限；发送后找到停止按钮。'],
    ['density','输入框是否清爽','常用设置能直接找到；更多菜单承接鞭挞、子代理和旁路提问。'],
    ['queue','发送与停止是否明确','运行时排队或插入；停止后等待，确认队列不会自行执行。'],
    ['worklist','当前工作和候选是否清楚','工作清单只显示正在进行与下一候选；手动调整，运行中切换应到下一轮生效。'],
    ['manage','管理导航是否清楚','切换需求、缺陷、交付、项目地图；打开条目后返回原列表。'],
    ['batches','格子进度是否好读','各条目不同批次数量能看清，列表不出现批次数字。'],
    ['memory','全局记忆管理是否清楚','切换全局和所有项目，编辑同编号条目时确认所属范围。'],
    ['activity','活动栏是否好用','演示工具调用；子代理和终端共用右栏。手动切标签后，后续调用保持当前阅读。'],
    ['preview-window','独立网页窗口是否顺手','拖动标题栏、调整右下角、缩放和最大化；开关网页不影响活动栏。'],
    ['activity-focus','后台更新是否打扰','手动选择标签或关闭面板，再切换对话；当前阅读和草稿应保持。'],
    ['contract','需求是否完整且好读','打开 R-381 查看正文、AC 编号和旧版证据；登记不完整需求应成为草稿。'],
    ['map','横向架构是否清楚','在交付旁打开项目地图，先读主干，再点模块看直接关系与清单来源。'],
    ['runtime','运行统计是否有用','筛选时间与对话，核对均时、调用、失败和记忆召回/注入/读取，再打开所属对话。'],
    ['editor','项目右键是否顺手','右键左侧项目打开 VS Code，或进入项目设置；页头仍可看红绿差异。'],
    ['coverage','低频入口是否清楚','设置里找运行记录和开发规范；左下统一管理记忆；没有项目工具菜单。'],
    ['ownership','草稿与归属是否可靠','在两个对话中分别输入，切换项目、管理页并刷新；检查草稿。'],
    ['color','颜色与信息层级是否舒服','切换深浅主题，通过场景按钮查看等待和失败状态。'],
    ['space','尺寸与可达性是否合适','缩窄窗口、收起侧栏、展开管理页底部输入；检查关键操作。']
  ];
  function openGuide(tab=guideTab) {
    guideTab=tab;
    const tabs=`<div class="dialog-tabs" role="tablist" aria-label="试用说明">${[['design','设计说明'],['checklist','验收与反馈'],['before','原截图对照']].map(([id,label])=>button('guide-tab',label,id===tab?'active':'',`role="tab" aria-selected="${id===tab}" data-value="${id}"`)).join('')}</div>`;
    let content='';
    if(tab==='design') content=`<div class="guide-points">
      <div class="guide-point"><strong>01 · 保留已经熟悉的操作</strong><p>侧栏找项目与对话，输入附近切模型、思考、模式和权限，右下发送与停止。通用操作沿用已有习惯。</p></div>
      <div class="guide-point"><strong>02 · 输入区只放这轮需要的东西</strong><p>项目、分支和改动归到页头。没有运行时不显示排队和停止；鞭挞、子代理、旁路提问在更多菜单里。</p></div>
      <div class="guide-point"><strong>03 · 活动共用右栏，网页独立打开</strong><p>子代理和终端共用右侧活动栏。网页预览单独弹出，可拖动、调整大小与缩放页面。手动切换后保持当前阅读；活动栏和网页独立开关，按对话保留。</p></div>
      <div class="guide-point"><strong>04 · 只盯当前工作和下一候选</strong><p>工作清单只显示两项，可以手动调整。运行中更换工作会在下一轮生效。全部需求、缺陷与交付仍在管理页，点击条目阅读完整信息。</p></div>
      <div class="guide-point"><strong>05 · 先看架构，再看运行</strong><p>项目地图与交付并列：横向主干先说明层级，点击模块看直接依赖。下方展示均时、工具调用、失败与记忆召回/注入/读取，支持时间和对话筛选。</p></div>
      <div class="guide-point"><strong>06 · 低频操作各归其位</strong><p>运行记录、开发规范和测试记录放在设置。记忆统一从左下管理。VS Code 从左侧项目右键菜单打开。想法与研究入口退出本方案，红绿差异保留。</p></div></div><div class="guide-section"><h3>试一遍自动展开</h3><p class="dialog-subtitle">演示一次子代理、终端和网页调用，约 7 秒。网页浮窗可以添加书签；主输入框的草稿会保留。</p>${button('demo-tools','演示工具调用','button primary')}</div><div class="guide-section"><h3>其他运行状态</h3><div class="guide-scenes">${[['idle','待输入'],['running','运行中'],['waiting','等待确认'],['error','执行失败'],['done','已完成']].map(([id,label])=>button('scenario',label,'button small',`data-value="${id}"`)).join('')}</div></div>
      <div class="guide-section"><h3>这一版的范围</h3><p class="dialog-subtitle">这是独立的本地交互原型。项目、回复、改动与执行是示例；没有接入正式应用。附件仅保留文件名，语音仅演示录音状态。草稿、设置与验收反馈保存在当前浏览器。</p><p class="dialog-subtitle">Enter 发送 · Shift+Enter 换行 · Ctrl+K 聚焦输入 · Ctrl+P 搜索 · Ctrl+Shift+N 新对话 · Ctrl+Shift+C 停止</p></div><div class="dialog-actions">${button('guide-tab','开始验收','button primary','data-value="checklist"')}${button('coverage','查看功能对照','button')}${button('reset-demo','重置演示数据','text-button')}<span class="muted" style="font-size:11px">重置会保留验收反馈</span></div>`;
    else if(tab==='checklist') content=`<p class="dialog-subtitle">逐项试用后标记结果。可以直接告诉我哪里不顺手，也可以导出这份反馈。</p>${checks.map(([id,title,desc],i)=>`<div class="review-row"><div><strong>${i+1}. ${title}</strong><p>${desc}</p></div><select aria-label="验收 ${esc(title)}" data-check="${id}">${[['untried','未试'],['pass','符合'],['change','需调整']].map(([value,label])=>`<option value="${value}" ${(feedback.checks[id]||'untried')===value?'selected':''}>${label}</option>`).join('')}</select></div>`).join('')}<textarea class="review-note" id="review-note" aria-label="试用反馈" placeholder="哪里不顺手？希望保留或恢复哪些操作？">${esc(feedback.notes)}</textarea><div class="dialog-actions">${button('export-feedback',`${icon('download',true)}导出验收反馈`,'button primary')}<span class="muted" style="font-size:11px">修改后自动保存</span></div>`;
    else content=`<p class="dialog-subtitle">以下是你提供的两张原截图。此版保留熟悉的常用控件，调整信息归属与密度。</p><figure class="before-figure"><figcaption>原输入区：导航、状态与多组运行设置集中在一处。</figcaption><img src="composer-before.png" alt="用户提供的原输入框截图"></figure><figure class="before-figure long"><figcaption>原需求列表：底部混入了一段运行中的对话。</figcaption><img src="work-list-before.png" alt="用户提供的需求列表截图，底部包含对话文字"></figure>${button('guide-tab','回到验收','button','data-value="checklist"')}`;
    openDialog('试用说明',tabs+content);
  }
  function exportFeedback() {
    const names={untried:'未试',pass:'符合',change:'需调整'};
    const text=`# Kanzei 交互原型 v4 验收反馈\n\n时间：${new Date().toLocaleString('zh-CN')}\n主题：${state.theme}\n窗口：${innerWidth} × ${innerHeight}\n\n${checks.map(([id,title])=>`- ${title}：${names[feedback.checks[id]||'untried']}`).join('\n')}\n\n## 具体意见\n\n${feedback.notes || '尚未填写'}\n\n原型路径：docs/prototypes/familiar-workspace\n`;
    const url=URL.createObjectURL(new Blob([text],{type:'text/markdown;charset=utf-8'})), a=document.createElement('a');
    a.href=url;a.download='Kanzei-原型验收反馈.md';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);toast('验收反馈已导出。');
  }
  function openNewWork() {
    if(manageState().tab!=='defects'){requirementForm();return;}
    openDialog('新建缺陷',`<p class="dialog-subtitle">添加到 ${esc(project()?.name)} 的示例缺陷列表。</p><form id="new-work-form"><label class="field"><span>标题</span><input name="title" aria-label="工作标题" required maxlength="80" placeholder="清楚地描述遇到的问题"></label><input type="hidden" name="kind" value="defect"><label class="field"><span>目标与验收</span><textarea name="goal" aria-label="目标与验收" placeholder="做到什么程度可以验收？"></textarea></label><label class="field"><span>优先级</span><select name="priority" aria-label="优先级"><option>P1</option><option selected>P2</option><option>P3</option></select></label><div class="dialog-actions"><button class="button primary" type="submit">创建缺陷</button>${button('close-dialog','取消','button','type="button"')}</div></form>`);
    $('#new-work-form input[name="title"]').focus();
  }
  function openNewProject() {
    openDialog('新建示例项目',`<p class="dialog-subtitle">体验项目切换与草稿隔离，不会创建实际目录。</p><form id="new-project-form"><label class="field"><span>项目名称</span><input name="name" aria-label="项目名称" required maxlength="40" placeholder="例如：个人网站"></label><div class="dialog-actions"><button class="button primary" type="submit">创建项目</button>${button('close-dialog','取消','button','type="button"')}</div></form>`);$('#new-project-form input').focus();
  }
  function searchResults(query='') {
    const results=[...state.projects.map(p=>({action:'project',id:p.id,title:p.name,desc:'项目',icon:'folder'})),...state.conversations.map(c=>({action:'conversation',id:c.id,title:c.title,desc:`对话 · ${state.projects.find(p=>p.id===c.pid)?.name||'独立对话'}`,icon:'chat'})),...state.tasks.map(t=>({action:'search-task',id:t.id,title:`${t.id} ${t.title}`,desc:`${t.kind==='defect'?'缺陷':'需求'} · ${statuses[t.status]}`,icon:'list'})),...['memory','settings','schedules','coverage'].map(id=>({action:id,id,title:({memory:'记忆管理',settings:'设置',schedules:'定时任务',coverage:'功能对照'})[id],desc:'全局页面',icon:'grid'})),...Object.entries(settingNames).map(([id,title])=>({action:'search-setting',id,title,desc:'设置',icon:'settings'})),{action:'map',id:'map',title:'项目地图',desc:'项目管理',icon:'grid'}].filter(r=>`${r.title} ${r.desc}`.toLowerCase().includes(query.toLowerCase()));
    $('#search-results').innerHTML=results.length?results.slice(0,16).map(r=>button(r.action,`${icon(r.icon)}<span>${esc(r.title)}<small>${esc(r.desc)}</small></span>`,'search-result',`data-id="${esc(r.id)}"`)).join(''):'<p class="muted">没有找到匹配内容。</p>';
  }
  function openSearch() {openDialog('搜索',`<input class="search-dialog-input" id="global-search" aria-label="搜索项目、对话、工作或功能" placeholder="搜索项目、对话、工作或功能…"><div id="search-results"></div>`);searchResults();$('#global-search').focus();}
  function resetDemo() {
    for(const id of timers.keys())clearRun(id);clearTimeout(voiceTimer);clearTimeout(sideTimer);recordingCid=null;
    state=seed();upgrade();upgradeV3();upgradeV4();panel=null;compact=false;closeDialog();save();render(true);toast('演示数据已重置，验收反馈仍然保留。');
  }
  function voice() {
    if(recordingCid) {clearTimeout(voiceTimer);recordingCid=null;renderComposer();toast('已取消语音演示。');return;}
    const c=convo();recordingCid=c.id;renderComposer();toast('语音演示：两秒后填入示例文字，不访问麦克风。');
    voiceTimer=setTimeout(()=>{c.draft+=(c.draft?'\n':'')+'请继续检查输入区的交互。';recordingCid=null;save();if(state.current.cid===c.id)renderComposer();},2000);
  }
  function sendSide() {
    const c=convo(panel?.id), prompt=$('#side-prompt'), question=prompt?.value.trim();if(!c||!question)return;
    c.sideMessages.push({question,answer:'这是旁路提问的示例回复。它单独展示，不进入主对话的执行队列，也不清空主输入框的草稿。'});save();renderInspector();$('#side-prompt')?.focus();
  }
  document.addEventListener('click', event => {
    const target=event.target.closest('button');if(!target || target.disabled)return;
    if(target.dataset.menu) {openMenu(target.dataset.menu,target);return;}
    if(target.dataset.setting) {chooseSetting(target.dataset.setting,target.dataset.value);return;}
    const {action,id,value,index}=target.dataset;
    if(!action)return;
    if(target.closest('#search-results'))closeDialog();
    const c=convo();
    switch(action) {
      case 'toggle-sidebar': $('#shell').classList.toggle(innerWidth<=860?'sidebar-open':'sidebar-collapsed');break;
      case 'conversation': {const next=convo(id);if(next)navigate('chat',next.pid,next.id);break;}
      case 'project': {const next=state.conversations.find(x=>x.pid===id);navigate('manage',id,next?.id || c.id);break;}
      case 'chat': navigate('chat');break;
      case 'manage': navigate('manage');break;
      case 'workspace': navigate('workspace');break;
      case 'attention': navigate('attention');break;
      case 'return-chat': navigate('chat',c.pid,c.id);break;
      case 'settings': navigate('settings');break;
      case 'settings-tab': state.settingsTab=value;save();renderSettings();break;
      case 'background-toggle': state.preferences.background=!state.preferences.background;save();renderSettings();break;
      case 'editor': closeMenu();openEditor(id||state.current.pid);break;
      case 'schedules': navigate('schedules');break;
      case 'new-schedule': scheduleForm();break;
      case 'schedule-toggle': {const s=state.schedules.find(s=>s.id===id);s.enabled=!s.enabled;save();renderSchedules();break;}
      case 'schedule-run': {const s=state.schedules.find(s=>s.id===id);s.history.unshift(`${new Date().toLocaleString('zh-CN')} · 手动演示：${s.prompt}`);save();toast('已添加一条示例运行记录，没有执行实际任务。');break;}
      case 'schedule-history': {const s=state.schedules.find(s=>s.id===id);openDialog('运行历史',`<p class="dialog-subtitle">${esc(s.name)} · 示例记录</p>${s.history.map(h=>`<p class="history-line">${esc(h)}</p>`).join('')||'<p>尚无运行记录。</p>'}`);break;}
      case 'coverage': closeDialog();navigate('coverage');break;
      case 'coverage-target': {
        if(value==='preview')openPreview();
        else if(activityNames[value])openActivity(value);
        else if(settingNames[value]){state.settingsTab=value;navigate('settings');}
        else if(['map','deliveries'].includes(value)){navigate('manage',state.current.pid||state.projects[0].id);manageState().tab=value;save();renderManage();}
        else if(value==='project-menu'){const row=$(`#sidebar .project-row[data-id="${state.current.pid||state.projects[0].id}"]`);$('#shell').classList.add('sidebar-open');const r=row.getBoundingClientRect();projectContextMenu(row,r.right,r.bottom);}
        else navigate(value);break;
      }
      case 'activity-open': openActivity();break;
      case 'activity-tab': openActivity(value);break;
      case 'activity-result': {const a=c.activities.find(a=>a.id===id);if(a)openActivity(a.kind,id);break;}
      case 'demo-tools': closeDialog();if(state.current.view!=='chat')navigate('chat',c.pid,c.id);startRun(c,'演示子代理检查、终端测试与网页预览',[],true,true);break;
      case 'stop-activity': {const a=c.activities.find(a=>a.id===id);a.status='stopped';a.endedAt=Date.now();a.logs.push('Process stopped');save();renderActivity();break;}
      case 'copy-terminal': navigator.clipboard.writeText(c.activities.find(a=>a.id===id).logs.join('\n')).then(()=>toast('输出已复制。'),()=>toast('无法访问剪贴板。'));break;
      case 'preview-open': openPreview();break;
      case 'preview-close': closePreview();break;
      case 'preview-maximize': previewState().maximized=!previewState().maximized;applyPreviewGeometry();save();break;
      case 'preview-reset': Object.assign(previewState(),{rect:null,maximized:false,zoom:100});applyPreviewGeometry();save();break;
      case 'preview-refresh': renderPreview(true);break;
      case 'open-run': {const next=convo(id);navigate('chat',next.pid,next.id);openActivity();break;}
      case 'project-settings': state.settingsProject=id;state.settingsTab='conventions';navigate('settings');break;
      case 'map': navigate('manage',state.current.pid||state.projects[0].id);manageState().tab='map';save();renderManage();break;
      case 'map-mode': state.mapView.mode=value;save();renderManage();break;
      case 'pick-work': workPicker(value);break;
      case 'select-work': selectWork(id,value);break;
      case 'cancel-work-switch': workState().pendingId=null;save();renderInspector();break;
      case 'all-work': {navigate('manage',c.pid,c.id);Object.assign(manageState(),{tab:'requirements',query:'',status:'all'});save();renderManage();break;}
      case 'runtime-refresh': renderManage();break;
      case 'runtime-record': {const r=state.runtimeHistory.find(r=>r.id===id),owner=convo(r.cid);navigate('chat',owner.pid,owner.id);if(!r.sample){const a=owner.activities.find(a=>a.run===r.id);if(a)openActivity(a.kind,a.id);}else toast('示例历史记录，已打开所属对话。');break;}
      case 'map-node': state.mapView.selected=id;state.mapView.mode='focus';save();renderManage();break;
      case 'edit-requirement': requirementForm(state.tasks.find(t=>t.id===id));break;
      case 'ready-requirement': {const t=state.tasks.find(t=>t.id===id);if(!requirementGaps(t.requirement.spec).length){t.status='pending';save();renderWorkDetail();}break;}
      case 'memory': state.memoryView.selected=null;navigate('memory');break;
      case 'memory-tab': state.memoryView.tab=value;save();renderMemory();break;
      case 'memory-scope': state.memoryView.scope=value;state.memoryView.project='all';save();renderMemory();break;
      case 'memory-entry': state.memoryView.selected=id;save();renderMemory();$('#page').scrollTop=0;break;
      case 'memory-back': state.memoryView.selected=null;save();renderMemory();break;
      case 'new-memory': memoryForm();break;
      case 'edit-memory': memoryForm(id);break;
      case 'archive-memory': {const m=state.memories.find(m=>m.key===id);m.status=m.status==='deprecated'?(m.previousStatus||'active'):(m.previousStatus=m.status,'deprecated');save();renderMemory();break;}
      case 'memory-chat-entry': {const m=state.memories.find(m=>m.key===id);Object.assign(state.memoryView,{selected:null,tab:'chat',chatTarget:m.pid||'global',chatDraft:`整理 ${m.id}「${m.title}」：`});save();renderMemory();break;}
      case 'organize-memory': {const n=state.memoryNotes.find(n=>n.id===id);n.state='organized';const existing=state.memories.find(m=>m.key==='kanzei:M-002');if(existing&&n.id==='note-color'){state.memoryView.selected=existing.key;save();renderMemory();}break;}
      case 'new-chat': newChat();break;
      case 'new-project': openNewProject();break;
      case 'new-work': openNewWork();break;
      case 'search': openSearch();break;
      case 'search-task': openTask(id);break;
      case 'search-setting': state.settingsTab=id;navigate('settings');break;
      case 'manage-tab': manageState().tab=value;save();renderManage();break;
      case 'back-management': navigate('manage');break;
      case 'work-list': if(panel?.type==='work-list') {panel=null;renderInspector();}else openPanel('work-list');break;
      case 'task': openTask(id);break;
      case 'delivery': openDelivery(id);break;
      case 'changes': openPanel('changes');break;
      case 'close-panel': closeInspector();break;
      case 'side-question': openPanel('side-question',c.id);$('#side-prompt').focus();break;
      case 'send-side': sendSide();break;
      case 'prepare-work': {
        const t=state.tasks.find(x=>x.id===id);if(t.status==='draft'){requirementForm(t);break;}const next=newChat(t.pid);next.title=t.title;applyCurrentWork(next,t.id);next.draft=`处理 ${t.id}：${t.title}\n\n目标：${t.requirement?.spec?.statement||t.goal}\n验收：${t.requirement?.spec?.acceptance?.map(a=>a.id+' '+a.text).join('；')||t.acceptance}`;t.cid=next.id;save();render(true);$('#prompt').focus();break;
      }
      case 'accept-delivery': case 'reject-delivery': {
        const d=state.deliveries.find(x=>x.id===id);d.state=action==='accept-delivery'?'accepted':'changes';
        const task=state.tasks.find(t=>t.id===d.task);if(task)task.status=d.state==='accepted'?'done':'doing';
        save();render();toast(d.state==='accepted'?'示例交付已标记验收通过。':'示例交付已退回修改。');break;
      }
      case 'expand-compose': compact=false;renderComposer();$('#prompt').focus();break;
      case 'collapse-compose': compact=true;renderComposer();break;
      case 'suggestion': c.draft=value;save();renderComposer();$('#prompt').focus();break;
      case 'attach': $('#file-input').dataset.cid=c.id;$('#file-input').click();break;
      case 'remove-attachment': c.attachments=c.attachments.filter(f=>f.id!==id);save();renderComposer();break;
      case 'remove-queue': c.queue=c.queue.filter(q=>q.id!==id);save();renderComposer();break;
      case 'send': send();break;
      case 'stop': stopRun();break;
      case 'resume': resumeRun();break;
      case 'retry': startRun(c,c.run?.input || '重试当前任务',[],false);break;
      case 'approve-run': startRun(c,c.run?.input || '演示一次执行流程',[],false);break;
      case 'voice': voice();break;
      case 'theme': state.theme=state.theme==='light'?'dark':'light';save();render();break;
      case 'guide': openGuide();break;
      case 'guide-tab': openGuide(value);break;
      case 'close-dialog': closeDialog();break;
      case 'scenario': scenario(value);break;
      case 'export-feedback': exportFeedback();break;
      case 'reset-demo': resetDemo();break;
    }
  });
  document.addEventListener('input', event => {
    const target=event.target;
    if(['title','criteria'].includes(target.name))target.setCustomValidity('');
    if(target.id==='prompt') {const c=convo();c.draft=target.value;save();autoHeight();$('#send').disabled=!c.draft.trim()&&!c.attachments.length;}
    else if(target.id==='agent-message-draft') {const a=convo().activities.find(a=>a.id===target.closest('form').dataset.id);a.draft=target.value;save();}
    else if(target.id==='work-picker-query')$('#work-picker-results').innerHTML=workPickerRows(target.dataset.slot,target.value);
    else if(target.id==='work-search') {manageState().query=target.value;save();$('#work-table').innerHTML=workRows();}
    else if(target.id==='memory-search') {state.memoryView.query=target.value;save();$('#memory-rows').innerHTML=memoryRows();$('#memory-count').textContent=`${filteredMemories().length} 条记忆`;}
    else if(target.closest('#memory-chat-form')) {state.memoryView.chatDraft=target.value;(state.memoryView.chatDrafts||={})[state.memoryView.chatTarget||'global']=target.value;save();}
    else if(target.id==='delivery-note') {state.deliveries.find(d=>d.id===state.current.itemId).note=target.value;save();}
    else if(target.id==='global-search') searchResults(target.value);
    else if(target.id==='review-note') {feedback.notes=target.value;saveFeedback();}
  });
  document.addEventListener('change', event => {
    const t=event.target;
    if(t.id==='runtime-range'||t.id==='runtime-conversation'){state.runtimeView[t.id==='runtime-range'?'range':'cid']=t.value;save();renderManage();}
    else if(t.id==='settings-project'){state.settingsProject=t.value;save();renderSettings();}
    else if(t.id==='preview-zoom'){previewState().zoom=Number(t.value);applyPreviewGeometry();save();}
    else if(t.id==='preview-instance'){previewState().selected=t.value;previewState().follow=false;renderPreview();save();}
    else if(t.id==='activity-instance'){activityState().selected=t.value;save();renderActivity();}
    else if(t.id==='work-state') {manageState().status=t.value;save();$('#work-table').innerHTML=workRows();}
    else if(t.id==='memory-project-filter') {state.memoryView.project=t.value;save();renderMemory();}
    else if(t.id==='memory-status-filter') {state.memoryView.status=t.value;save();renderMemory();}
    else if(t.id==='memory-edit-scope') $('#memory-edit-project-field').hidden=t.value==='global';
    else if(t.id==='memory-chat-target') {const v=state.memoryView;v.chatDrafts||={};v.chatDrafts[v.chatTarget||'global']=v.chatDraft||'';v.chatTarget=t.value;v.chatDraft=v.chatDrafts[t.value]||'';save();renderMemory();}
    else if(t.id==='settings-permission') {state.permission=t.value;save();}
    else if(t.id==='voice-language-setting') {state.preferences.voiceLanguage=t.value;save();}
    else if(t.dataset.check) {feedback.checks[t.dataset.check]=t.value;saveFeedback();}
    else if(t.id==='file-input') {if(t.files.length)addAttachments(t.files,t.dataset.cid);t.value='';}
  });
  document.addEventListener('submit', event => {
    event.preventDefault();const form=event.target, data=new FormData(form);
    if(form.id==='requirement-form') {saveRequirement(form,data);
    } else if(form.id==='new-work-form') {
      const title=data.get('title').trim();if(!title){form.elements.title.setCustomValidity('请输入工作标题。');form.elements.title.reportValidity();return;}
      const kind=data.get('kind'), prefix=kind==='defect'?'D':'R';
      const last=Math.max(0,...state.tasks.filter(t=>t.id.startsWith(prefix+'-')).map(t=>Number(t.id.split('-')[1])));
      const task={id:`${prefix}-${last+1}`,pid:state.current.pid,kind,title,status:'pending',priority:data.get('priority'),batch:0,total:1,cid:null,goal:data.get('goal').trim()||title,acceptance:data.get('goal').trim()||'执行前补充具体的验收条件。'};
      state.tasks.push(task);Object.assign(manageState(),{tab:kind==='defect'?'defects':'requirements',query:'',status:'all'});closeDialog();save();openTask(task.id);toast(`${task.id} 已加入示例工作清单。`);
    } else if(form.id==='new-project-form') {
      const name=data.get('name').trim();if(!name)return;
      const p={id:uid('project'),name,branch:'main'};state.projects.push(p);const c=makeConversation(uid('conversation'),p.id,'新对话');state.conversations.push(c);closeDialog();navigate('manage',p.id,c.id);toast('示例项目已创建。');
    } else if(form.id==='memory-edit-form') {
      const old=state.memories.find(m=>m.key===form.dataset.key), scope=old?.scope||data.get('scope'),pid=old?.pid||(scope==='project'?data.get('project'):null);
      const title=data.get('title').trim(),body=data.get('body').trim();if(!title||!body)return;
      const last=Math.max(0,...state.memories.filter(m=>m.scope===scope&&m.pid===pid).map(m=>Number(m.id.slice(2))));
      const id=old?.id||`M-${String(last+1).padStart(3,'0')}`,key=old?.key||`${pid||'global'}:${id}`;
      const value={key,id,scope,pid,title,text:body,status:old?.status||'active',category:old?.category||'手动记录',source:old?.source||'原型试用中手动记录',updated:new Date().toLocaleDateString('sv-SE')};
      if(old)Object.assign(old,value);else state.memories.push(value);state.memoryView.selected=key;closeDialog();save();renderMemory();toast('示例记忆已保存。');
    } else if(form.id==='memory-chat-form') {
      const message=data.get('message').trim();if(!message)return;
      const target=state.memoryView.chatTarget||'global';state.memoryChat.push({target,text:message,reply:`已收到。这段示例回复仅针对${target==='global'?'全局记忆':state.projects.find(p=>p.id===target)?.name+' 的项目记忆'}。正式接入后，在这里核对修改建议及写入的范围。`});state.memoryView.chatDraft='';(state.memoryView.chatDrafts||={})[target]='';save();renderMemory();
    } else if(form.id==='preferences-form') {Object.assign(state.preferences,Object.fromEntries(data));save();toast('默认值已保存，将用于新对话。');
    } else if(form.id==='conventions-form') {state.conventions||={};state.conventions[state.settingsProject||state.projects[0].id]=data.get('body');save();toast('示例规范已保存。');
    } else if(form.id==='schedule-form') {state.schedules.push({id:uid('schedule'),pid:data.get('pid'),name:data.get('name').trim(),when:`${data.get('frequency')} ${data.get('time')}`,enabled:true,host:'本机',mode:data.get('mode'),prompt:data.get('prompt').trim(),history:[]});closeDialog();save();renderSchedules();
    } else if(form.id==='activity-message-form') {const a=convo().activities.find(a=>a.id===form.dataset.id),message=data.get('message').trim();if(!message)return;a.messages.push(message);a.draft='';save();renderActivity();}
  });


  $('#preview-window').addEventListener('pointerdown',startPreviewGesture);
  document.addEventListener('pointermove',movePreviewGesture);
  document.addEventListener('pointerup',endPreviewGesture);
  document.addEventListener('pointercancel',endPreviewGesture);
  $('#preview-window').addEventListener('keydown',event=>{const handle=event.target.closest('.preview-drag,.preview-resize');if(!handle||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key))return;event.preventDefault();const p=previewState(),r=previewGeometry(p.rect),amount=event.shiftKey?40:12,dx=event.key==='ArrowRight'?amount:event.key==='ArrowLeft'?-amount:0,dy=event.key==='ArrowDown'?amount:event.key==='ArrowUp'?-amount:0;p.maximized=false;p.rect=previewGeometry(handle.classList.contains('preview-resize')?{...r,w:r.w+dx,h:r.h+dy}:{...r,x:r.x+dx,y:r.y+dy});applyPreviewGeometry();save();});
  document.addEventListener('contextmenu',event=>{const target=event.target.closest('#sidebar .project-row');if(target){event.preventDefault();projectContextMenu(target,event.clientX,event.clientY);}});
  document.addEventListener('keydown',event=>{const target=event.target.closest('#sidebar .project-row');if(target&&(event.key==='ContextMenu'||event.shiftKey&&event.key==='F10')){event.preventDefault();const r=target.getBoundingClientRect();projectContextMenu(target,r.right,r.bottom);}const node=event.target.closest('[data-map-id]');if(node&&['Enter',' '].includes(event.key)){event.preventDefault();state.mapView.selected=node.dataset.mapId;state.mapView.mode='focus';save();renderManage();}});
  document.addEventListener('click',event=>{const node=event.target.closest('[data-map-id]');if(node){state.mapView.selected=node.dataset.mapId;state.mapView.mode='focus';save();renderManage();}});
  window.addEventListener('message',event=>{const frame=$('#preview-frame');if(!frame||event.source!==frame.contentWindow||event.data?.type!=='prototype-bookmark')return;const a=convo().activities.find(a=>a.id===event.data.id&&a.kind==='preview');if(a){a.bookmarks++;save();}});

  let composing=false;
  $('#prompt').addEventListener('compositionstart',()=>composing=true);
  $('#prompt').addEventListener('compositionend',()=>composing=false);
  document.addEventListener('keydown', event => {
    if(event.isComposing || event.keyCode===229 || composing)return;
    const mod=event.ctrlKey||event.metaKey, key=event.key.toLowerCase();
    if(!$('#popover').hidden) {
      const items=[...$('#popover').querySelectorAll('button')], current=items.indexOf(document.activeElement);
      if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {event.preventDefault();items[event.key==='Home'?0:event.key==='End'?items.length-1:(current+(event.key==='ArrowDown'?1:-1)+items.length)%items.length]?.focus();return;}
      if(event.key==='Escape') {event.preventDefault();event.stopPropagation();closeMenu(true);return;}
    }
    if(event.key==='Escape'&&!$('#dialog').open&&!$('#preview-window').hidden){closePreview();return;}
    if(event.key==='Escape'&&!$('#dialog').open&&panel) {closeInspector();return;}
    if(mod&&key==='p') {event.preventDefault();openSearch();return;}
    if($('#dialog').open) {
      if(event.key==='Enter'&&event.target.id==='global-search') {event.preventDefault();$('#search-results button')?.click();}
      return;
    }
    if(mod&&key==='k') {event.preventDefault();if(state.current.view!=='chat')navigate('chat');$('#prompt').focus();return;}
    if(mod&&event.shiftKey&&key==='n') {event.preventDefault();newChat();return;}
    if(mod&&event.shiftKey&&key==='c') {event.preventDefault();stopRun();return;}
    if(event.target.id==='prompt'&&event.key==='Enter'&&!event.shiftKey) {event.preventDefault();send();}
  });
  document.addEventListener('pointerdown',event=>{if(!event.target.closest('#popover,[data-menu]'))closeMenu();});
  $('#dialog').addEventListener('click',event=>{if(event.target!==$('#dialog'))return;const r=$('#dialog').getBoundingClientRect();if(event.clientX<r.left||event.clientX>r.right||event.clientY<r.top||event.clientY>r.bottom)closeDialog();});
  $('#composer').addEventListener('dragover',event=>{event.preventDefault();$('#composer').classList.add('drag-over');});
  $('#composer').addEventListener('dragleave',event=>{if(!$('#composer').contains(event.relatedTarget))$('#composer').classList.remove('drag-over');});
  $('#composer').addEventListener('drop',event=>{event.preventDefault();$('#composer').classList.remove('drag-over');if(event.dataTransfer.files.length)addAttachments(event.dataTransfer.files);});
  $('#prompt').addEventListener('paste',event=>{const files=[...(event.clipboardData?.files||[])];if(files.length){event.preventDefault();addAttachments(files);}});
  window.addEventListener('resize',()=>{closeMenu();autoHeight();applyPreviewGeometry();});
  window.addEventListener('beforeunload',save);
  compact=state.current.view!=='chat';if(state.current.view==='chat'&&convo().dock?.open)panel={type:'activity',id:convo().id};save();render(true);
  if(importedTrial)toast('旧版试用的草稿、设置和反馈已带到新版。');
})();
