// Run from the local studio with playwright-cli run-code --filename=...
async (page) => {
  const origin = await page.evaluate(() => location.origin);
  const state = await page.evaluate(() => window.qaState || "replying");
  await page.route("**/__oc-material-qa", route => route.fulfill({contentType:"text/html",body:`<!doctype html><html><head><title>OC material review</title></head><body style="margin:0;background:#eee;display:flex;gap:8px"><div id="dark" style="position:relative;width:384px;height:576px;background:#191e28"></div><div id="light" style="position:relative;width:384px;height:576px;background:#fff"></div><div id="check" style="position:relative;width:384px;height:576px;background:repeating-conic-gradient(#ddd 0% 25%,white 0% 50%) 0/24px 24px"></div></body></html>`}));
  await page.goto(origin+"/__oc-material-qa");
  await page.setViewportSize({width:1168,height:600});
  await page.evaluate(async state => {
    const {loadOcResources,createOcRenderer}=await import('/22-oc-renderer.js');
    const {createOcDirector}=await import('/22-oc-director.js');
    const resources=await loadOcResources('./assets/oc/character-v6.json');
    window.materialRenders=[];
    for(const [id,level] of [['dark',0],['light',.55],['check',1]]){
      const renderer=createOcRenderer(document.getElementById(id),resources);
      const model=createOcDirector(resources.pack);model.setState(state);model.advance(state==='thinking'?3900:3400);
      renderer.pause();await renderer.seek(model.sample(),{speaking:level>0,level});
      window.materialRenders.push(renderer);
    }
  },state);
  await page.screenshot({path:'output/playwright/oc-material-'+state+'-v6.png'});
  await page.evaluate(()=>{
    for(const host of document.querySelectorAll('body>div')){
      host.style.width='384px';host.style.height='360px';host.style.overflow='hidden';
      const canvas=host.querySelector('canvas');canvas.style.position='absolute';canvas.style.width='768px';canvas.style.height='1152px';canvas.style.left='-192px';canvas.style.top='-90px';
    }
  });
  await page.screenshot({path:'output/playwright/oc-mouth-'+state+'-v6.png'});
  return {renderers:await page.evaluate(()=>window.materialRenders.map(renderer=>renderer.snapshot()))};
}
