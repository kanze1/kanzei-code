(() => {
  const $ = id => document.getElementById(id);
  const params = new URLSearchParams(location.search);
  let version = params.get('version') === 'before' ? 'before' : 'after';
  let theme = params.get('theme') === 'dark' ? 'dark' : 'light';
  const symbols = {doing:'▶',awaiting_external:'!',todo:'○'};
  const node = (tag, cls, text) => {const e=document.createElement(tag);if(cls)e.className=cls;if(text!==undefined)e.textContent=text;return e;};
  function applyTheme() { document.documentElement.dataset.theme=theme; $('case-theme').textContent=theme==='light'?'深色':'浅色';$('case-theme').setAttribute('aria-label',`切换到${theme==='light'?'深':'浅'}色主题`); }
  function render() {
    document.body.classList.toggle('before',version==='before');
    $('before').setAttribute('aria-pressed',String(version==='before'));$('after').setAttribute('aria-pressed',String(version==='after'));
    const query=$('case-search').value.trim().toLocaleLowerCase(), filter=$('case-filter').value;
    const rows=window.caseRows.filter(r=>(filter==='all'||r.state===filter)&&`${r.id} ${r.title}`.toLocaleLowerCase().includes(query));
    $('case-list').replaceChildren();
    $('case-count').textContent=`显示 ${rows.length} / ${window.caseRows.length} 条需求${query||filter!=='all'?' · 已应用筛选':''}`;
    for(const r of rows) {
      const row=node('details','work-row'), summary=node('summary','work-summary');
      row.dataset.id=r.id;
      const identity=node('span','identity'); identity.append(node('small','',version==='before'?`${r.id} · ${r.label}`:r.id),node('strong','',r.title));
      const status=node('span','status');status.dataset.state=r.state; const glyph=node('span','state-glyph',symbols[r.state]);glyph.setAttribute('aria-hidden','true');status.append(glyph,node('span','',r.label));
      let progress;
      if(version==='before') {progress=node('span','legacy-cells');progress.setAttribute('role','img');progress.setAttribute('aria-label',`批次进度 ${r.done}/${r.total}`);progress.title=Array.from({length:r.total},(_,i)=>`${i+1}: ${i<r.done?'done':r.state==='doing'&&i===r.done?'active':'pending'}`).join(' · ');for(let i=0;i<r.total;i++)progress.append(node('i',i<r.done?'done':r.state==='doing'&&i===r.done?'active':''));}
      else {progress=node('span','progress');const p=node('span','progress-text');p.append(node('small','progress-label','已完成批次 '),node('span','',`${r.done} / ${r.total}`));p.setAttribute('aria-label',`已完成批次 ${r.done}，共 ${r.total} 批`);const meter=node('span','meter');meter.setAttribute('aria-hidden','true');const fill=node('i');fill.style.setProperty('--progress',`${r.done/r.total*100}%`);meter.append(fill);progress.append(p,meter);if(r.done===r.total)progress.append(node('span','progress-note','批次完成，整体未关闭'));}
      const priority=node('span','priority',r.priority);priority.dataset.priority=r.priority;priority.setAttribute('aria-label',`优先级 ${r.priority}`);
      const expand=node('span','expand','›');expand.setAttribute('aria-hidden','true');summary.append(identity,status,progress,priority,expand);
      const detail=node('div','row-detail'),dl=node('dl');
      for(const [k,v] of [['整体状态',r.label],['已完成批次',`${r.done}/${r.total}；批次完成不等于需求完成`],...(r.external?[['外部验收',r.external]]:[])]) dl.append(node('dt','',k),node('dd','',v));
      detail.append(dl);if(r.done===r.total)detail.append(node('p','detail-tip','批次已全部完成；整体仍按记录显示为“进行中”。请查看验收信息，再决定是否迁移状态。'));
      row.append(summary,detail);$('case-list').append(row);
    }
    if(!rows.length){const empty=node('div','empty');empty.append(node('strong','','没有符合条件的需求'),node('p','',version==='after'?'试试清除搜索和状态筛选。':'当前搜索与状态筛选没有结果。'));if(version==='after'){const reset=node('button','','清除全部筛选');reset.type='button';reset.addEventListener('click',resetFilters);empty.append(reset);}$('case-list').append(empty);}
    $('version-note').textContent=version==='before'?'调整前：按当前源码重现批次方格、弱状态和统一优先级样式。':'调整后：常驻批次数量；整体状态独立；P0 单次强化；展开可读真实验收信息。';
  }
  function resetFilters(){ $('case-search').value='';$('case-filter').value='all';render();$('case-search').focus(); }
  $('before').addEventListener('click',()=>{version='before';render();});$('after').addEventListener('click',()=>{version='after';render();});
  $('case-search').addEventListener('input',render);$('case-filter').addEventListener('change',render);$('case-reset').addEventListener('click',resetFilters);
  $('case-theme').addEventListener('click',()=>{theme=theme==='light'?'dark':'light';applyTheme();});$('grayscale').addEventListener('change',e=>document.body.classList.toggle('is-gray',e.target.checked));
  applyTheme();render();
})();
