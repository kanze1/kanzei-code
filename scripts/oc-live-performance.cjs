// Run through playwright-cli against the local OC preview origin.
async (page) => {
  const origin = await page.evaluate(() => location.origin);
  await page.route('**/__oc-performance', route => route.fulfill({contentType:'text/html', body:'<!doctype html><html><head><link rel="stylesheet" href="/style.css"></head><body style="display:block;background:#191918"><main class="view active" style="display:block"><div id="probe" style="height:640px;width:800px;display:flex;justify-content:center"><div class="voice-art" style="height:600px;width:400px"></div></div></main></body></html>'}));
  await page.goto(origin + '/__oc-performance');
  await page.evaluate(async () => {
    const nativeCreate = document.createElement.bind(document);
    window.ocMetrics = {videos:[], uploads:0, paints:0, renderMs:[], presented:0, seeks:0, frames:[], textureFrames:[]};
    document.createElement = function (name, options) {
      const element = nativeCreate(name, options);
      if (name === 'video') {
        window.ocMetrics.videos.push(element);
        element.addEventListener('seeking', () => window.ocMetrics.seeks++);
        const id=window.ocMetrics.videos.length-1;
        const note = (now,meta) => {
          window.ocMetrics.presented++;
          element.ocFrameTime=meta.mediaTime;
          window.ocMetrics.frames.push({id,now,time:meta.mediaTime});
          element.requestVideoFrameCallback(note);
        };
        element.requestVideoFrameCallback(note);
      }
      return element;
    };
    const PIXI = await import('/vendor/pixi/pixi-7.4.3.min.mjs');
    const update = PIXI.VideoResource.prototype.update;
    PIXI.VideoResource.prototype.update = function (...args) {
      window.ocMetrics.uploads++;
      window.ocMetrics.textureFrames.push({id:ocMetrics.videos.indexOf(this.source),now:performance.now(),time:this.source.ocFrameTime??this.source.currentTime});
      return update.apply(this,args);
    };
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
    const cadence=frames=>{
      const previous=new Map(),gaps=[],mediaSteps=[];
      for(const f of frames.filter(f=>f.now>=start.time)){
        const last=previous.get(f.id);
        if(last){gaps.push(f.now-last.now);mediaSteps.push(f.time-last.time);}
        previous.set(f.id,f);
      }
      gaps.sort((a,b)=>a-b);
      return {samples:frames.filter(f=>f.now>=start.time).length,gapP95Ms:gaps[Math.floor(gaps.length*.95)],maxGapMs:Math.max(...gaps),skippedMediaFrames:mediaSteps.reduce((sum,n)=>sum+Math.max(0,Math.round(n*24)-1),0),duplicateMediaFrames:mediaSteps.filter(n=>Math.abs(n)<.001).length};
    };
    return {duration:s, renderFps:(ocMetrics.paints-start.paints)/s, uploadFps:(ocMetrics.uploads-start.uploads)/s, presentedFps:(ocMetrics.presented-start.presented)/s, seeks:ocMetrics.seeks-start.seeks, renderP95Ms:sorted[Math.floor(sorted.length*.95)], callbackCadence:cadence(ocMetrics.frames),textureCadence:cadence(ocMetrics.textureFrames),renderer:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):null, videos:ocMetrics.videos.map(v=>{const q=v.getVideoPlaybackQuality?.();return {paused:v.paused,time:v.currentTime,quality:q?{total:q.totalVideoFrames,dropped:q.droppedVideoFrames}:null};}), snapshot:ocTest.snapshot()};
  },start);
  await page.evaluate(() => ocTest.destroy());
  return result;
}
