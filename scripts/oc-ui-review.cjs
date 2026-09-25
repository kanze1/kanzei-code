// Exercise the actual UI markup/styles and character modules with a fixed session.
async (page) => {
  const origin = await page.evaluate(() => location.origin);
  const source = await (await page.request.get(origin+'/index.html')).text();
  const html = source.replace(/<script\b[^>]*>[\s\S]*?<\/script>/g,'');
  await page.route('**/__oc-layout', route => route.fulfill({contentType:'text/html',body:html}));
  const errors=[], requests=[];
  page.on('pageerror',e=>errors.push(String(e)));
  page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
  page.on('request',r=>{if(/assets\/oc\/|vendor\/pixi/.test(r.url()))requests.push(r.url());});
  async function hydrate(fresh=false) {
    await page.goto(origin+'/__oc-layout');
    await page.evaluate(async fresh => {
      if(fresh)localStorage.removeItem('kanzei.character.enabled');
      const {initOcPreference}=await import('/22-oc-preference.js');
      const {initOcCompanion}=await import('/22-oc-companion.js');
      const {voicePresenceLine}=await import('/23-voice-copy.js');
      for(const view of document.querySelectorAll('.view'))view.classList.remove('active');
      const view=document.querySelector('#view-chat');view.classList.add('active','voice-mode');
      for(const id of ['voice-stage','voice-panel'])document.getElementById(id).classList.remove('hidden');
      document.getElementById('voice-stage-status').textContent='聆听';
      document.getElementById('voice-status').textContent='聆听';
      document.getElementById('voice-toggle').textContent='结束语音';
      const presence=()=>document.getElementById('voice-live-caption').textContent=voicePresenceLine('listening',document.documentElement.dataset.ocEnabled==='true');
      document.addEventListener('kz:oc-preference',presence);
      initOcPreference(document.getElementById('oc-toggle'));
      window.ocUi=initOcCompanion(document.getElementById('chat-area'),{getSessionId:()=> 'preview',getRuntime:()=>({phase:'idle',running:false})});
      ocUi.voice('preview','listening');presence();
    },fresh);
  }
  const check=(value,why)=>{if(!value)throw new Error(why);};
  await page.setViewportSize({width:1440,height:900});
  await hydrate(true);
  await page.waitForTimeout(350);
  check(requests.length===0,'default off requested character assets: '+requests.join(','));
  check(await page.locator('.oc-canvas').count()===0,'default off created a canvas');
  await page.screenshot({path:'output/playwright/oc-v7-voice-off.png'});
  await page.locator('#oc-toggle').click();
  await page.waitForFunction(()=>ocUi.snapshot().renderer==='pixi');
  check(await page.locator('#voice-live-caption').textContent()==='说吧，今天折腾什么。','character presence line');
  await page.waitForTimeout(1800);
  const active=await page.evaluate(()=>ocUi.snapshot());
  check(active.renderedFrames>20,'character did not play');
  check(active.media.playingSources===1,'steady state kept a spare decoder running');
  await page.screenshot({path:'output/playwright/oc-v7-voice-on.png'});
  await hydrate();
  await page.waitForFunction(()=>ocUi.snapshot().renderer==='pixi');
  check(await page.locator('#oc-toggle').getAttribute('aria-pressed')==='true','choice did not persist');
  await page.locator('#oc-toggle').click();
  check(await page.locator('.oc-canvas').count()===0,'off did not release canvas');
  check(await page.evaluate(()=>ocUi.snapshot().renderer)==='off','off kept renderer');
  check(await page.locator('#voice-live-caption').textContent()==='开始语音对话','plain voice copy');
  await page.setViewportSize({width:920,height:680});
  await page.screenshot({path:'output/playwright/oc-v7-voice-off-narrow.png'});
  check(await page.locator('#oc-toggle').isVisible(),'rail control is inaccessible');
  await page.locator('#oc-toggle').click();
  await page.locator('#oc-toggle').click();
  await page.locator('#oc-toggle').click();
  await page.waitForFunction(()=>ocUi.snapshot().renderer==='pixi');
  check(await page.locator('.oc-canvas').count()===1,'rapid toggles left duplicate or missing canvas');
  await page.locator('#oc-toggle').click();
  await page.reload();await hydrate();
  check(await page.locator('.oc-canvas').count()===0,'off preference did not persist');
  check(errors.length===0,errors.join(';'));
  return {passed:true,checks:['default off: no asset requests','left toggle','character and plain copy','live animation','on persists','off releases renderer','off persists','narrow layout','rapid toggles'],active,errors};
}
