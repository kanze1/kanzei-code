(()=>{
 const $=id=>document.getElementById(id);let mode='accented',theme='light',phase='idle';
 function paint(){document.body.dataset.color=mode;$('baseline').setAttribute('aria-pressed',String(mode==='baseline'));$('accented').setAttribute('aria-pressed',String(mode==='accented'));for(const key of ['completion','resource','saved','panel','reference'])document.body.classList.toggle('accent-'+key,$('opt-'+key).checked);}
 $('baseline').addEventListener('click',()=>{mode='baseline';paint();});$('accented').addEventListener('click',()=>{mode='accented';paint();});
 document.querySelectorAll('.lab-options input').forEach(e=>e.addEventListener('change',paint));
 $('lab-theme').addEventListener('click',()=>{theme=theme==='light'?'dark':'light';document.documentElement.dataset.theme=theme;$('lab-theme').textContent=theme==='light'?'深色':'浅色';$('lab-theme').setAttribute('aria-label',`切换到${theme==='light'?'深':'浅'}色主题`);});
 function panel(open){$('lab-panel').hidden=!open;$('panel-toggle').setAttribute('aria-expanded',String(open));$('panel-toggle').setAttribute('aria-label',open?'关闭活动面板':'打开活动面板');if(!open)$('panel-toggle').focus();}
 $('panel-toggle').addEventListener('click',()=>panel($('lab-panel').hidden));$('panel-close').addEventListener('click',()=>panel(false));
 $('reference-badge').addEventListener('click',()=>{$('reference-detail').hidden=!$('reference-detail').hidden;$('reference-badge').setAttribute('aria-expanded',String(!$('reference-detail').hidden));});
 $('resource-link').addEventListener('click',()=>{$('read-result').open=!$('read-result').open;$('read-result').scrollIntoView({block:'nearest'});});
 $('save-demo').addEventListener('click',()=>{$('saved-feedback').hidden=false;$('save-demo').textContent='再次演示保存';});
 function execution(next){phase=next;document.body.classList.toggle('phase-running',next==='running');$('lab-stop').hidden=next!=='running';$('phase-label').textContent=next==='running'?'运行中':next==='stopped'?'已停止':'空闲';$('status-text').textContent=next==='running'?'● 运行中':next==='stopped'?'□ 已停止':'○ 空闲';$('execution-hint').textContent=next==='running'?'发送后保留橙色运行反馈；完成前不点亮青蓝标记。':next==='stopped'?'已停止，恢复可输入状态。青蓝不用于停止。':'本轮完成，仅终态标记使用少量青蓝。';document.querySelectorAll('.run-receipt,.resource-link').forEach(e=>e.hidden=next==='running');}
 $('lab-send').addEventListener('click',()=>{if(phase==='running'){execution('idle');$('lab-send').setAttribute('aria-label','演示发送');}else{execution('running');$('lab-send').setAttribute('aria-label','演示收到完成回执');}});$('lab-stop').addEventListener('click',()=>{execution('stopped');$('lab-send').setAttribute('aria-label','演示发送');});
 paint();
})();
