// Run against the local OC preview server with Playwright CLI.
async(page)=>{
  const origin=await page.evaluate(()=>location.origin),errors=[];
  const html=(await (await page.request.get(origin+'/index.html')).text()).replace(/<script\b[^>]*>[\s\S]*?<\/script>/g,'');
  await page.route('**/__oc-layout-settings',route=>route.fulfill({contentType:'text/html',body:html}));
  page.on('pageerror',e=>errors.push(String(e)));
  const check=(ok,message)=>{if(!ok)throw new Error(message);};
  async function hydrate(fresh=false){
    await page.goto(origin+'/__oc-layout-settings');
    await page.evaluate(async fresh=>{
      if(fresh){localStorage.removeItem('kanzei.character.enabled');localStorage.removeItem('kanzei.character.preferences.v1');}
      const pref=await import('/22-oc-preference.js');window.ocPreferences=pref;
      const {initOcCompanion}=await import('/22-oc-companion.js');
      document.querySelector('#view-chat').classList.add('voice-mode');
      document.querySelector('#voice-stage').classList.remove('hidden');
      document.querySelector('#voice-live-caption').textContent='说吧，今天折腾什么。';
      pref.initOcPreference(document.querySelector('#oc-toggle'));
      window.ocUi=initOcCompanion(document.querySelector('#chat-area'),{getSessionId:()=> 'layout',getRuntime:()=>({phase:'idle',running:false})});
    },fresh);
  }
  async function settings(show){await page.evaluate(show=>{
    document.querySelector('#view-chat').classList.toggle('active',!show);
    document.querySelector('#view-settings').classList.toggle('active',show);
  },show);}
  const figure=page.locator('#voice-stage .oc-figure');
  const preferences=()=>page.evaluate(()=>ocPreferences.readOcSettings());
  async function canvasFits(){
    const dimensions=await page.locator('.oc-canvas').evaluate(canvas=>{
      const view=canvas.getBoundingClientRect(),host=canvas.parentElement.getBoundingClientRect();
      return {view:{width:view.width,height:view.height},host:{width:host.width,height:host.height}};
    });
    check(Math.abs(dimensions.view.width-dimensions.host.width)<1&&Math.abs(dimensions.view.height-dimensions.host.height)<1,'scaled canvas overflows its host');
  }
  await page.setViewportSize({width:1440,height:900});await hydrate(true);
  check(await page.locator('.oc-canvas').count()===0,'disabled created a canvas');
  await page.locator('#oc-toggle').click();
  await page.waitForFunction(()=>ocUi.snapshot().renderer==='pixi');
  await page.waitForTimeout(550);
  let box=await figure.boundingBox();
  await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();
  await page.mouse.move(box.x+box.width/2+65,box.y+box.height/2-30,{steps:8});await page.mouse.up();
  let after=await figure.boundingBox();
  check(Math.abs(after.x-box.x-65)<2&&Math.abs(after.y-box.y+30)<2,'drag did not follow pointer');
  const dragged=await preferences();check(dragged.positions.voice.x!==0,'drag not persisted');
  await page.mouse.move(after.x+after.width/2,after.y+after.height/2);await page.mouse.wheel(0,-100);await page.waitForTimeout(100);
  check((await preferences()).scale>1,'wheel did not resize');
  box=await figure.boundingBox();const handle=await figure.locator('.oc-resize').boundingBox();
  await page.mouse.move(handle.x+handle.width/2,handle.y+handle.height/2);await page.mouse.down();
  await page.mouse.move(handle.x+handle.width/2-25,handle.y+handle.height/2-40,{steps:6});await page.mouse.up();
  after=await figure.boundingBox();check(after.height<box.height-20,'corner did not resize');
  await figure.focus();await page.keyboard.press('ArrowLeft');
  check((await figure.boundingBox()).x<after.x-5,'keyboard did not move character');
  const beforeEscape=await preferences();box=await figure.boundingBox();
  await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();
  await page.mouse.move(box.x+box.width/2+40,box.y+box.height/2,{steps:4});await page.keyboard.press('Escape');await page.mouse.up();
  check(JSON.stringify((await preferences()).positions)===JSON.stringify(beforeEscape.positions),'Escape did not cancel drag');
  await settings(true);
  await page.locator('#set-oc-scale').fill('140');await page.locator('#set-oc-opacity').fill('65');
  await page.locator('#set-oc-motion').selectOption('still');await page.locator('#set-oc-locked').check();
  await page.screenshot({path:'output/oc-layout-settings/settings.png'});
  await settings(false);await page.waitForTimeout(350);
  check(await figure.evaluate(n=>getComputedStyle(n).pointerEvents)==='none','locked character intercepts pointer');
  check(await figure.evaluate(n=>getComputedStyle(n).opacity)==='0.65','opacity not applied');
  const frozen=await page.evaluate(()=>ocUi.snapshot());await page.waitForTimeout(350);
  check(frozen.media.playingSources===0&&await page.evaluate(()=>ocUi.snapshot().renderedFrames)===frozen.renderedFrames,'still mode kept animating');
  await page.evaluate(()=>ocUi.voice('layout','speaking',.8));
  await page.waitForFunction(()=>Number(document.querySelector('.oc-canvas').dataset.ocMouth)>.4);
  await page.evaluate(()=>ocUi.voice('layout',null));
  check(await page.locator('.oc-canvas').getAttribute('data-oc-mouth')==='0','still mouth did not close');
  await hydrate();await page.waitForFunction(()=>ocUi.snapshot().renderer==='pixi');
  const restored=await preferences();check(restored.scale===1.4&&restored.opacity===.65&&restored.locked&&restored.motion==='still','settings did not survive reload');
  check(JSON.stringify(restored.positions)===JSON.stringify(beforeEscape.positions),'position did not survive reload');
  await canvasFits();
  await settings(true);await page.locator('#set-oc-locked').uncheck();await page.locator('#set-oc-motion').selectOption('idle');
  await settings(false);await page.evaluate(()=>ocUi.voice('layout','speaking',.6));await page.waitForTimeout(500);
  check((await page.evaluate(()=>ocUi.snapshot())).sample.state==='idle','idle-only mode used task gestures');
  await page.evaluate(()=>ocUi.voice('layout',null));
  await page.setViewportSize({width:1050,height:660});await page.waitForTimeout(400);
  const bounds=await page.locator('#chat-area').boundingBox();box=await figure.boundingBox();
  check(box.x>=bounds.x+10&&box.y>=bounds.y+10&&box.x+box.width<=bounds.x+bounds.width-10&&box.y+box.height<=bounds.y+bounds.height-10,'resized window lost character');
  await canvasFits();
  await page.screenshot({path:'output/oc-layout-settings/voice-adjusted.png'});
  await page.setViewportSize({width:1440,height:900});
  const voicePosition=(await preferences()).positions.voice;
  await page.evaluate(()=>{
    ocPreferences.updateOcSettings({scale:1,opacity:1});
    document.querySelector('#view-chat').classList.remove('voice-mode');
    document.querySelector('#voice-stage').classList.add('hidden');
  });
  await page.waitForSelector('.empty-art .oc-canvas');
  const welcome=page.locator('.empty-art .oc-figure');await welcome.focus();await page.keyboard.press('ArrowLeft');
  check((await preferences()).positions.welcome.x<0&&JSON.stringify((await preferences()).positions.voice)===JSON.stringify(voicePosition),'welcome position changed voice layout');
  box=await welcome.boundingBox();
  await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();
  await page.mouse.move(1440,895,{steps:12});await page.mouse.up();
  const overflow=await page.locator('#messages').evaluate(n=>n.scrollWidth-n.clientWidth);
  check(overflow===0,'drag added a horizontal chat scrollbar');
  await canvasFits();
  const welcomePosition=(await preferences()).positions.welcome;
  await page.evaluate(()=>{
    document.querySelector('.msg-pane[data-active="1"]').innerHTML='<div class="msg"><p>角色调整后，文字仍然可以选择。</p></div>';
  });
  await page.waitForSelector('#oc-companion .oc-canvas');
  const companion=page.locator('#oc-companion .oc-figure');await companion.focus();await page.keyboard.press('ArrowLeft');
  const contexts=await preferences();check(contexts.positions.conversation.x<0&&JSON.stringify(contexts.positions.welcome)===JSON.stringify(welcomePosition)&&JSON.stringify(contexts.positions.voice)===JSON.stringify(voicePosition),'conversation position crossed layouts');
  check(await page.locator('.oc-canvas').count()===1,'layout switching duplicated canvas');
  await settings(true);
  await page.locator('#oc-reset-placement').click();
  check((await preferences()).scale===1&&Object.values((await preferences()).positions).every(p=>p.x===0&&p.y===0),'reset did not restore placement');
  await page.locator('#set-oc-enabled').uncheck();
  check(await page.locator('#oc-toggle').getAttribute('aria-pressed')==='false','settings toggle not synced');
  check(await page.locator('.oc-canvas').count()===0,'settings toggle did not release renderer');
  await page.evaluate(()=>ocUi.destroy());
  check(!errors.length,errors.join(';'));
  return {passed:true,checks:['pointer drag','wheel zoom','corner resize','keyboard position','Escape cancel','immediate settings','lock passes through','static mode with lip sync','reload persistence','idle-only mode','window bounds','canvas scaling','independent layouts','chat overflow','reset','toggle synchronization and disposal'],errors};
}
