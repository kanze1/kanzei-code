// Run through playwright-cli against the local OC preview origin.
async (page) => {
  const origin = await page.evaluate(() => location.origin);
  await page.route('**/__oc-performance', route => route.fulfill({contentType:'text/html', body:'<!doctype html><html><head><link rel="stylesheet" href="/style.css"></head><body style="display:block;background:#191918"><main class="view active" style="display:block"><div id="probe" style="height:640px;width:800px;display:flex;justify-content:center"><div class="voice-art" style="height:600px;width:400px"></div></div></main></body></html>'}));
  await page.goto(origin + '/__oc-performance');
  await page.evaluate(async () => {
    const nativeCreate = document.createElement.bind(document);
    window.ocMetrics = {videos:[], uploads:0, paints:0, renderMs:[], presented:0, seeks:0};
    document.createElement = function (name, options) {
      const element = nativeCreate(name, options);
      if (name === 'video') {
        window.ocMetrics.videos.push(element);
        element.addEventListener('seeking', () => window.ocMetrics.seeks++);
        const note = () => { window.ocMetrics.presented++; element.requestVideoFrameCallback(note); };
        element.requestVideoFrameCallback(note);
      }
      return element;
    };
    const PIXI = await import('/vendor/pixi/pixi-7.4.3.min.mjs');
    const update = PIXI.VideoResource.prototype.update;
    PIXI.VideoResource.prototype.update = function (...args) { window.ocMetrics.uploads++; return update.apply(this,args); };
    const render = PIXI.Renderer.prototype.render;
    PIXI.Renderer.prototype.render = function (...args) {
      const at = performance.now(); const result = render.apply(this,args);
      window.ocMetrics.paints++; window.ocMetrics.renderMs.push(performance.now()-at); return result;
    };
    const mod = await import('/22-oc-performance.js');
    const root = document.querySelector('#probe');
    root.querySelector('.voice-art').innerHTML = mod.ocPerformanceMarkup();
    window.ocTest = mod.initOcPerformance(root); await window.ocTest.ready();
  });
  await page.waitForTimeout(2000);
  const start = await page.evaluate(() => ({time:performance.now(), uploads:ocMetrics.uploads, paints:ocMetrics.paints, presented:ocMetrics.presented, seeks:ocMetrics.seeks, count:ocTest.snapshot().renderedFrames}));
  await page.waitForTimeout(8000);
  const result = await page.evaluate(start => {
    const s = (performance.now()-start.time)/1000;
    const sorted = ocMetrics.renderMs.slice().sort((a,b)=>a-b);
    const gl = document.querySelector('canvas').getContext('webgl2') || document.querySelector('canvas').getContext('webgl');
    const ext = gl?.getExtension('WEBGL_debug_renderer_info');
    return {duration:s, renderFps:(ocMetrics.paints-start.paints)/s, uploadFps:(ocMetrics.uploads-start.uploads)/s, presentedFps:(ocMetrics.presented-start.presented)/s, seeks:ocMetrics.seeks-start.seeks, renderP95Ms:sorted[Math.floor(sorted.length*.95)], renderer:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):null, videos:ocMetrics.videos.map(v=>({paused:v.paused,time:v.currentTime,quality:v.getVideoPlaybackQuality?.()})), snapshot:ocTest.snapshot()};
  },start);
  await page.evaluate(() => ocTest.destroy());
  return result;
}
