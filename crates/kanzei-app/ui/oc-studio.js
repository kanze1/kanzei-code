import { createOcDirector } from "./22-oc-director.js";
import { loadOcResources, createOcRenderer } from "./22-oc-renderer.js";
import { OC_CHARACTER_PACK_SRC } from "./22-oc-config.js";

const labels={idle:"待机",listening:"倾听",thinking:"思考",replying:"说话",executing:"执行",blocked:"疑惑",aside:"吐槽",warm:"会意",complete:"完成"};
const copy={idle:"说吧，今天折腾什么。",listening:"嗯，继续。",thinking:"等一下，我捋捋。",replying:"从这里说起。",executing:"找到毛病了。",blocked:"啧，卡在这里了。",aside:"这弯绕得，还挺有创意。",warm:"你听出来了？",complete:"好了，试试。"};
const host=document.querySelector("#character");
const packSrc=OC_CHARACTER_PACK_SRC;
const timeline=document.querySelector("#timeline");
const playButton=document.querySelector("#play");
const controls=[...document.querySelectorAll("button,input")];
for(const control of controls)control.disabled=true;
const reduced=window.matchMedia("(prefers-reduced-motion: reduce)");
let resources,renderer,director,demo,frame=null,playing=false,time=0,last=null,manual=false;
let context,audio,source,analyser,samples,audioStart=0,audioEnd=0;
const ready=(async()=>{
  resources=await loadOcResources(packSrc);
  renderer=createOcRenderer(host,resources);
  director=createOcDirector(resources.pack);
  demo=resources.pack.demo;
  context=new AudioContext();
  audio=await context.decodeAudioData(await (await fetch(new URL(demo.audio,new URL(packSrc,import.meta.url)))).arrayBuffer());
  analyser=context.createAnalyser();analyser.fftSize=512;
  analyser.connect(context.destination);samples=new Float32Array(analyser.fftSize);
  await seek(0);
  for(const control of controls)control.disabled=false;
  setState("idle");
})().catch(error=>{document.querySelector("#error").textContent="预览加载失败："+error.message;throw error;});

function stopAudio(){
  if(source){source.onended=null;try{source.stop();}catch{}source.disconnect();source=null;}
}
function scheduleAudio(){
  stopAudio();
  const offset=time;
  if(offset>=audio.duration||manual)return;
  source=context.createBufferSource();source.buffer=audio;source.connect(analyser);
  audioStart=context.currentTime;
  audioEnd=audioStart+audio.duration-offset;
  source.start(audioStart,offset);
}
function recordedLevel(seconds){
  const start=Math.floor(seconds*audio.sampleRate),data=audio.getChannelData(0);
  if(start<0||start>=data.length)return 0;
  let energy=0;
  for(let i=0;i<512;i++)energy+=(data[start+i]||0)**2;
  return Math.min(1,Math.sqrt(energy/512)*9);
}
function actualLevel(){
  if(!source||context.currentTime<audioStart||context.currentTime>=audioEnd)return 0;
  analyser.getFloatTimeDomainData(samples);
  let energy=0;for(const v of samples)energy+=v*v;
  return Math.min(1,Math.sqrt(energy/samples.length)*9);
}
function at(seconds){
  const model=createOcDirector(resources.pack);let previous=0;
  for(const [when,state] of demo.events){
    if(when>seconds)break;
    model.advance((when-previous)*1000);model.setState(state);previous=when;
  }
  model.advance((seconds-previous)*1000);
  return model;
}
function captionAt(seconds,sample){
  const cue=demo.cues.find(cue=>seconds>=cue.at&&seconds<cue.at+cue.duration);
  return cue?.text || copy[sample.state];
}
function paint(sample,level=0){
  renderer.render(sample,{speaking:level>0,level,reduced:reduced.matches});
  document.querySelector("#phase").textContent=sample.clipLabel||labels[sample.state]||sample.state;
  document.querySelector("#time").textContent=manual?"单独动作":String(Math.floor(time)).padStart(2,"0")+" / 30";
  document.querySelector("#caption").textContent=manual?copy[sample.state]:captionAt(time,sample);
  timeline.value=String(time);
  for(const button of document.querySelectorAll("[data-state]"))button.setAttribute("aria-pressed",String(button.dataset.state===sample.requested));
}
function pause(){
  playing=false;stopAudio();if(frame!==null)cancelAnimationFrame(frame);frame=null;last=null;
  playButton.textContent="播放样片";
  renderer?.pause?.();
}
async function seek(seconds){
  pause();manual=false;time=Math.max(0,Math.min(30,seconds));
  director=at(time);paint(director.sample(),recordedLevel(time));
  if(renderer?.seek)await renderer.seek(director.sample(),{speaking:recordedLevel(time)>0,level:recordedLevel(time),reduced:reduced.matches});
}
async function play(){
  await ready;await context.resume();
  if(time>=30||manual){time=0;manual=false;director=at(0);}
  playing=true;last=null;scheduleAudio();playButton.textContent="暂停";
  renderer?.resume?.();
  frame=requestAnimationFrame(tick);
}
function tick(now){
  frame=null;if(!playing)return;
  const delta=last===null?0:Math.min(100,now-last);last=now;
  if(manual){director.advance(delta);}
  else{time=Math.min(30,time+delta/1000);director=at(time);}
  paint(director.sample(),actualLevel());
  if(!manual&&time>=30){pause();return;}
  frame=requestAnimationFrame(tick);
}
function setState(state){
  if(!renderer)return;
  pause();manual=true;director.setState(state);playing=true;
  renderer?.resume?.();
  playButton.textContent="播放样片";frame=requestAnimationFrame(tick);
  const cue=demo.cues.find(cue=>cue.state===state);
  if(cue)void context.resume().then(()=>{
    if(!manual||director.requested!==state||!playing)return;
    source=context.createBufferSource();source.buffer=audio;source.connect(analyser);
    audioStart=context.currentTime+.8;audioEnd=audioStart+cue.duration;source.start(audioStart,cue.at,cue.duration);
  });
}
playButton.addEventListener("click",()=>{if(playing&&!manual)pause();else void play();});
function seekFromControl(seconds){
  void seek(seconds).catch(error=>{document.querySelector("#error").textContent="预览定位失败："+error.message;});
}
document.querySelector("#reset").addEventListener("click",()=>seekFromControl(0));
timeline.addEventListener("input",()=>seekFromControl(Number(timeline.value)));
for(const button of document.querySelectorAll("[data-state]"))button.addEventListener("click",()=>setState(button.dataset.state));
document.querySelector("#interrupt").addEventListener("click",()=>setState("interrupted"));
document.addEventListener("visibilitychange",()=>{if(document.hidden)pause();});
window.addEventListener("resize",()=>{if(renderer)paint(director.sample());});
window.addEventListener("pagehide",()=>{pause();renderer?.destroy();void context?.close();});

// Deterministic export uses the same rig and the demo WAV's measured PCM envelope.
async function exportFrames(first=0,count=720,mode="demo"){
  await ready;pause();
  const exportHost=document.createElement("div");
  const exportWidth=resources.pack.size[0]+2*(resources.pack.rig?.canvasPaddingX||0);
  exportHost.style.cssText=`position:fixed;width:${exportWidth}px;height:${resources.pack.size[1]}px;left:-3000px;top:0`;
  document.body.append(exportHost);
  const exportRenderer=createOcRenderer(exportHost,resources,{export:true});
  const canvas=document.createElement("canvas");canvas.width=1920;canvas.height=1080;
  const c=canvas.getContext("2d");
  try{
    for(let i=first;i<Math.min(720,first+count);i++){
      const seconds=i/24;
      const idle=mode==="idle";
      const model=idle?createOcDirector(resources.pack):at(seconds);
      if(idle)model.advance(seconds*1000);
      const sample=model.sample(),level=idle?0:recordedLevel(seconds);
      if(exportRenderer.seek)await exportRenderer.seek(sample,{speaking:level>0,level});
      else exportRenderer.render(sample,{speaking:level>0,level});
      c.fillStyle="#e9e6e0";c.fillRect(0,0,1920,1080);
      const portraitWidth=exportWidth*1035/resources.pack.size[1];
      c.drawImage(exportRenderer.canvas,585-portraitWidth/2,25,portraitWidth,1035);
      c.fillStyle="#717877";c.font="16px Segoe UI";c.fillText("K A N Z E I   /   C H A R A C T E R   S T U D Y",1080,252);
      c.fillStyle="#282b30";c.font="42px Microsoft YaHei";c.fillText("安静地，把事情做好。",1075,345);
      c.fillStyle="#526778";c.font="22px Microsoft YaHei";c.fillText(sample.clipLabel||labels[sample.state],1080,470);
      c.fillStyle="#777b78";c.font="19px Microsoft YaHei";
      const line=idle?"轻呼吸，偶尔移开视线，再回到眼前。":captionAt(seconds,sample);
      let row="",lineY=531;
      for(const char of line){if(c.measureText(row+char).width>520){c.fillText(row,1080,lineY);lineY+=30;row="";}row+=char;}
      c.fillText(row,1080,lineY);
      c.fillStyle="#c7c9c5";c.fillRect(1080,606,525,2);c.fillStyle="#526778";c.fillRect(1080,606,525*seconds/30,2);
      c.font="14px Segoe UI";c.fillText(String(Math.floor(seconds)).padStart(2,"0")+" / 30",1080,646);
      c.fillStyle="#7e827d";c.font="14px Segoe UI";c.fillText("SAME EYES. DIFFERENT TOMORROW.",1080,911);
      const blob=await new Promise(resolve=>canvas.toBlob(resolve,"image/png"));
      const response=await fetch((idle?"/__idle_frame/":"/__frame/")+String(i).padStart(4,"0"),{method:"POST",body:blob});
      if(!response.ok)throw new Error("Export HTTP "+response.status);
    }
  } finally{exportRenderer.destroy();exportHost.remove();}
  return {first,count:Math.min(count,720-first),fps:24,width:1920,height:1080};
}
window.ocStudio={ready,pause,seek,setState,exportFrames,sample:()=>director.sample(),canvas:()=>renderer.canvas,recordedLevel,play,at:seconds=>at(seconds).sample(),demo:()=>demo};
