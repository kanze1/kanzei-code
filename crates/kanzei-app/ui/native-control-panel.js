import { invoke } from "./00-ipc.js";
import { subscribeRuntimeEvent,runtimeEventsReady } from "./00-event-subscriptions.js";
import { t } from "./02-i18n.js";
import { renderNativeControl,renderNativeHighlight } from "./native-control-view.js";

const surface=new URL(location.href).searchParams.get("surface");
document.body.dataset.surface=surface;
let revision=-1;
function update(state) {
  if(state.revision<revision)return;revision=state.revision;
  if(surface==="highlight")renderNativeHighlight(document.getElementById("native-control-highlight"),state);
  else renderNativeControl(document.getElementById("native-control-bar"),state,{compact:true,t,
    stop:controlId=>invoke("native_desktop_stop",{controlId}),dismiss:controlId=>invoke("native_desktop_dismiss",{controlId})});
}
try {
  // Dedicated control surfaces have no conversation pane; use the shared
  // subscription owner without bootstrapping the main application's views.
  const subscription=subscribeRuntimeEvent("kz:native-control",event=>update(event.payload));
  if(!await runtimeEventsReady())throw new Error(subscription.error);
  update(await invoke("native_desktop_status"));
  document.body.dataset.controlReady="true";
  await invoke("native_desktop_surface_ready");
}catch(error){
  document.body.dataset.controlReady="failed";
  const failure=document.createElement("p");failure.className="native-control-error";failure.setAttribute("role","alert");failure.textContent=String(error);document.body.append(failure);
}
