// Open the local studio first, then: playwright-cli run-code --filename=scripts/oc-browser-qa.cjs
async (page) => {
  const origin=await page.evaluate(()=>location.origin);
  const errors=[];
  page.on("pageerror",error=>errors.push(String(error)));
  page.on("console",message=>{if(message.type()==="error")errors.push(message.text());});
  await page.route("**/__oc-qa",route=>route.fulfill({status:200,contentType:"text/html",body:`<!doctype html><html lang="zh-CN"><head><link rel="stylesheet" href="/style.css"><title>OC browser verification</title></head><body style="display:block;padding:30px"><main class="view active" style="display:block"><div id="probe" style="width:800px;height:540px;display:flex;justify-content:center"><div id="first" class="empty-art" style="display:block;width:360px;height:540px"></div><div id="second" class="voice-art" style="display:none;width:360px;height:540px"></div></div></main></body></html>`}));
  await page.goto(origin+"/__oc-qa");
  await page.evaluate(async()=>{
    const mod=await import("/22-oc-performance.js");
    const root=document.querySelector("#probe");
    for(const host of root.children)host.innerHTML=mod.ocPerformanceMarkup();
    window.ocTest=mod.initOcPerformance(root);await window.ocTest.ready();
  });
  const check=(value,message)=>{if(!value)throw new Error(message);};
  await page.waitForFunction(()=>document.querySelector("#probe").dataset.ocRenderer==="pixi");
  await page.evaluate(()=>{window.ocTest.setState("replying");window.ocTest.setSpeaking(true);window.ocTest.setMouthLevel(.9);});
  await page.waitForFunction(()=>window.ocTest.snapshot().sample.clip==="replying"&&window.ocTest.snapshot().sample.sourceTime>2.1&&Number(document.querySelector("canvas").dataset.ocMouth)>.4);
  check(await page.evaluate(()=>window.ocTest.snapshot().sample.sourceTime<4.55),"whole-frame arm gesture");
  await page.evaluate(()=>window.ocTest.setState("interrupted"));
  check(await page.evaluate(()=>document.querySelector("#probe").dataset.ocSpeaking==="false"&&document.querySelector("canvas").dataset.ocMouth==="0"),"interrupt closes actual canvas mouth immediately");
  await page.waitForFunction(()=>window.ocTest.snapshot().sample.state==="idle");
  await page.evaluate(()=>window.ocTest.setState("idle",true));
  const frozen=await page.evaluate(()=>window.ocTest.snapshot().renderedFrames);
  await page.waitForTimeout(220);
  check(await page.evaluate(()=>window.ocTest.snapshot().renderedFrames)===frozen,"hidden view keeps frame counter frozen");
  await page.evaluate(()=>{document.querySelector("#first").style.display="none";document.querySelector("#second").style.display="block";window.ocTest.setState("listening",false);});
  check(await page.locator("#second canvas").count()===1,"shared canvas follows visible voice host");
  await page.emulateMedia({reducedMotion:"reduce"});
  await page.waitForTimeout(100);
  const reducedFrames=await page.evaluate(()=>window.ocTest.snapshot().renderedFrames);
  await page.waitForTimeout(220);
  check(await page.evaluate(()=>window.ocTest.snapshot().renderedFrames)===reducedFrames,"reduced motion stops idle frame loop");
  await page.evaluate(()=>{window.ocTest.setSpeaking(true);window.ocTest.setMouthLevel(.7);});
  await page.waitForFunction(()=>Number(document.querySelector("canvas").dataset.ocMouth)>.5);
  await page.evaluate(()=>window.ocTest.setSpeaking(false));
  check(await page.evaluate(()=>document.querySelector("canvas").dataset.ocMouth)==="0","reduced motion retains speech and immediate closure");
  await page.emulateMedia({reducedMotion:"no-preference"});
  await page.evaluate(()=>window.ocTest.setState("thinking"));
  const before=await page.evaluate(()=>window.ocTest.snapshot().renderedFrames);
  await page.waitForTimeout(1000);
  const fps=await page.evaluate(()=>window.ocTest.snapshot().renderedFrames)-before;
  check(fps>0&&fps<36,"render cadence within 30fps budget: "+fps);
  await page.screenshot({path:"output/playwright/oc-production-component.png"});
  await page.evaluate(()=>{Object.defineProperty(document,"hidden",{configurable:true,get:()=>true});document.dispatchEvent(new Event("visibilitychange"));});
  const hiddenFrames=await page.evaluate(()=>window.ocTest.snapshot().renderedFrames);
  await page.waitForTimeout(250);
  check(await page.evaluate(()=>window.ocTest.snapshot().renderedFrames)===hiddenFrames,"hidden document pauses rendering");
  await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event("visibilitychange"));});
  await page.evaluate(()=>window.ocTest.destroy());
  check(await page.locator("canvas").count()===0,"destroy releases canvases");
  await page.evaluate(async()=>{
    const {initOcCompanion}=await import("/22-oc-companion.js");
    window.session="a";window.runtimes={a:{phase:"running",running:true},b:{phase:"idle",running:false}};
    window.companion=initOcCompanion(document.querySelector("#probe"),{getSessionId:()=>window.session,getRuntime:id=>window.runtimes[id]});
    window.companion.emit("run_started",{session_id:"a"});
  });
  await page.waitForFunction(()=>document.querySelector("#probe").dataset.ocRenderer==="pixi");
  await page.waitForFunction(()=>document.querySelector("#probe").dataset.ocState==="thinking");
  await page.evaluate(()=>{window.session="b";window.companion.voice("a","speaking",.9);});
  check(await page.evaluate(()=>document.querySelector("#probe").dataset.ocState==="idle"&&document.querySelector("#probe").dataset.ocSpeaking==="false"),"session switch resets previous gesture and ignores old voice");
  await page.evaluate(()=>window.companion.voice("b","speaking",.8));
  await page.waitForFunction(()=>Number(document.querySelector("canvas").dataset.ocMouth)>.4);
  await page.evaluate(()=>window.companion.voice("b",null));
  check(await page.evaluate(()=>document.querySelector("canvas").dataset.ocMouth)==="0","companion audio completion closes mouth");
  await page.evaluate(()=>window.companion.destroy());
  await page.locator("body").click({position:{x:10,y:10}});
  await page.evaluate(async()=>{
    const {initOcPerformance}=await import("/22-oc-performance.js");
    const {VoicePlayer}=await import("/23-voice-audio.js");
    window.audioMotion=initOcPerformance(document.querySelector("#probe"));await window.audioMotion.ready();
    window.audioContext=new AudioContext();await window.audioContext.resume();
    window.audioPlayer=new VoicePlayer(window.audioContext,(speaking,level)=>{window.audioMotion.setSpeaking(speaking);window.audioMotion.setMouthLevel(level);});
    const decoded=await window.audioContext.decodeAudioData(await (await fetch("/assets/oc/demo-speech-c-v7.wav")).arrayBuffer());
    window.audioPlayer.push(decoded.getChannelData(0),decoded.sampleRate);
  });
  check(await page.evaluate(()=>document.querySelector("canvas").dataset.ocMouth)==="0","audio preroll stays closed");
  await page.waitForFunction(()=>Number(document.querySelector("canvas").dataset.ocMouth)>.2);
  await page.evaluate(()=>window.audioPlayer.clear());
  check(await page.evaluate(()=>window.audioPlayer.nodes.size===0&&document.querySelector("canvas").dataset.ocMouth==="0"),"real WebAudio cancellation clears nodes and closes mouth");
  await page.evaluate(async()=>{window.audioPlayer.destroy();window.audioMotion.destroy();await window.audioContext.close();});
  const videoChecks=await page.evaluate(async()=>{
    const {loadOcResources,createOcRenderer}=await import('/22-oc-renderer.js');
    const {createOcDirector}=await import('/22-oc-director.js');
    const {OC_CHARACTER_PACK_SRC}=await import('/22-oc-config.js');
    const resources=await loadOcResources(OC_CHARACTER_PACK_SRC);
    const host=document.createElement('div');host.style.cssText='width:384px;height:576px';document.body.append(host);
    const renderer=createOcRenderer(host,resources,{export:true}),base=createOcDirector(resources.pack).sample();
    const readback=document.createElement('canvas');readback.width=64;readback.height=96;
    const pixels=readback.getContext('2d',{willReadFrequently:true});
    renderer.pause();let frames=0,maxDecoders=0;
    try{
      for(const [id,clip] of Object.entries(resources.pack.clips))for(const time of [clip.start,(clip.start+clip.end)/2,clip.end-1/24]){
        await renderer.seek({...base,clip:id,serial:0,sourceTime:time,previous:null,blend:1,to:[base.state,Math.floor(time*24)]},{speaking:true,level:.6});
        pixels.clearRect(0,0,64,96);pixels.drawImage(renderer.canvas,0,0,64,96);
        const data=pixels.getImageData(0,0,64,96).data;
        let visible=0;for(let p=3;p<data.length;p+=4)if(data[p]>128)visible++;
        if(visible<1500||visible>5000)throw new Error('Invalid visible silhouette: '+id+' at '+time+', pixels='+visible);
        maxDecoders=Math.max(maxDecoders,renderer.snapshot().decodedSources);frames++;
      }
      renderer.render(base,{reduced:true});
      if(renderer.snapshot().playingSources!==0)throw new Error('reduced motion left video decoders playing');
    }finally{renderer.destroy();host.remove();}
    return {frames,maxDecoders};
  });
  check(videoChecks.maxDecoders<=4,'bounded decoded video cache');
  check(errors.length===0,"browser errors: "+errors.join("; "));
  return {passed:true,fps,videoChecks,checks:["continuous entrance","immediate mouth closure","view pause","visible host migration","reduced motion","visibility event pause","destroy","session isolation","real WebAudio RMS and cancellation","all clip endpoints and midpoints decode", "bounded decoder cache", "reduced motion pauses videos"],errors};
}
