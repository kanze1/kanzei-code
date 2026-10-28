// One rendering owner for the main conversation and the desktop control bar.
export function controlWords(phase) {
  return ({running:"Kanzei 正在控制电脑",stopping:"正在停止控制",stopped:"控制已停止",ended:"本轮控制结束",stop_failed:"停止尚未确认",idle:"原生控制"})[phase] || "原生控制";
}
export function actionWords(action) {
  return ({inspect:"观察窗口",screenshot:"获取画面",fill:"填写",invoke:"点击",click:"点击",key:"按键",toggle:"切换选项",select:"选择选项",choose_file:"选择文件"})[action] || "原生操作";
}
export function resultWords(status) {
  return ({executing:"执行中",dispatched:"已发送，待核对",observed:"已取得画面",cancelled:"已取消",failed:"失败"})[status] || "等待结果";
}
function element(tag, className, text) {
  const node=document.createElement(tag);node.className=className;
  if(text !== undefined)node.textContent=text;
  return node;
}
export function renderNativeControl(root,state,{compact=false,t=value=>value,stop,dismiss}={}) {
  const detailsOpen=root.querySelector("details")?.open || false;
  root.replaceChildren();root.dataset.phase=state.phase || "idle";
  const owner=state.owner;
  root.hidden=!owner || state.phase==="idle";
  if(root.hidden)return;
  const heading=element("div","native-control-heading");
  const mark=element("span","native-control-dot");mark.setAttribute("aria-hidden","true");
  const text=element("div","native-control-identity");
  const title=element("strong","",t(controlWords(state.phase)));title.setAttribute("role","status");
  text.append(title,element("span","native-control-target",owner.title));
  heading.append(mark,text);
  const active=["running","stopping","stop_failed"].includes(state.phase);
  const button=element("button","native-control-stop",t(active?"停止控制":"收起"));button.type="button";
  button.disabled=state.phase==="stopping";
  if(state.phase==="running")button.append(element("kbd","","Esc"));
  button.addEventListener("click",()=>{
    button.disabled=true;
    void Promise.resolve(active?stop(owner.id):dismiss(owner.id)).catch(error=>{
      button.disabled=false;
      const message=element("p","native-control-error",String(error));message.setAttribute("role","alert");root.append(message);
    });
  });
  heading.append(button);root.append(heading);
  const trace=state.trace || [];
  const list=element("ol","native-control-trace");
  for(const action of compact?trace.slice(-3):trace) {
    const row=element("li","native-control-step");row.dataset.status=action.status;row.dataset.actionId=String(action.id);
    if(action.started_at){const at=new Date(action.started_at),time=element("time","native-control-time",at.toLocaleTimeString([],{hour12:false}));time.dateTime=at.toISOString();row.append(time);}
    row.append(element("span","native-control-action",`${t(actionWords(action.action))}${action.target?`  ${action.target}`:""}`),element("span","native-control-result",t(resultWords(action.status))));
    list.append(row);
  }
  if(compact)root.append(list);
  else {
    const details=element("details","native-control-details");details.open=detailsOpen;
    details.append(element("summary","",`${t("操作轨迹")}  ${trace.length}`),list);root.append(details);
  }
  if(state.error) { const error=element("p","native-control-error",state.error);error.setAttribute("role","alert");root.append(error); }
}

export function renderNativeHighlight(root,state) {
  const action=state.trace?.at(-1), bounds=action?.bounds;
  root.hidden=state.phase!=="running" || !bounds;
  if(root.hidden)return;
  const stamp=`${action.id}:${action.status}`;
  if(root.dataset.stamp===stamp)return;
  root.dataset.stamp=stamp;root.dataset.action=action.action;root.dataset.status=action.status;
  root.replaceChildren(element("div","native-control-outline"));
  if(["invoke","click"].includes(action.action) && action.status==="dispatched") {
    const ring=element("div","native-control-click");
    ring.style.left=`${action.point?100*action.point.x/bounds.width:50}%`;
    ring.style.top=`${action.point?100*action.point.y/bounds.height:50}%`;
    root.append(ring);
  }
}
