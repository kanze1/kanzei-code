/* global window, document */

export default async function run({ browser, server, check }) {
  const page = await browser.newPage({ viewport: { width: 2000, height: 1000 } });
  const errors = [];
  page.on('pageerror', error => errors.push(String(error)));

  await page.goto(`${server.origin}/?scene=chat`); await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const shell = await import('/03-shell.js'), panel = await import('/06-agent-panel.js'), agents = await import('/05-subagents.js');
    window.__sideTest = { shell, panel, agents, now: 1800000000000 };
    panel.setTasksPanelClock(() => window.__sideTest.now);
    panel.closeTasksPanel();
    for (const id of ['wb6-b', 'wb6-a']) {
      shell.setActiveSessionId(id); panel.tasksPanelUserRun(id);
      agents.subagentStart({ sessionId: id, id, input: { prompt: id, description: id } }); panel.agentPanelSync();
    }
  });
  await page.locator('#tasks-panel').waitFor({ state: 'visible' });
  await page.locator('#tasks-panel').hover();
  const held = await page.evaluate(() => {
    const { shell, panel, agents } = window.__sideTest;
    // Same owner transition as sessions.switchProcess after selecting another process.
    shell.setActiveSessionId('wb6-b'); panel.agentPanelSync();
    agents.subagentEnd({ sessionId: 'wb6-b', id: 'wb6-b', ok: true, outcome: 'success', content: 'done', durationMs: 1000 });
    panel.reconcileTasksPanel(); window.__sideTest.now += 7000;
    return panel.reconcileTasksPanel();
  });
  check('Actual panel stays open under the pointer after switching sessions', held.visible, true);
  await page.mouse.move(5, 5);
  const released = await page.evaluate(() => { window.__sideTest.now += 6100; return window.__sideTest.panel.reconcileTasksPanel(); });
  check('Leaving the panel resumes automatic closing', released.visible, false);
  const controls = await page.evaluate(async () => {
    const p = await import('/06-side-policy.js');
    const m = p.createSideModel(), prefs = { autoOpen: true, autoClose: true };
    const env = (sid, now, extra = {}) => ({ sid, now, active: 0, view: 'chat', dock: 'side', prefs, ...extra });
    for (const sid of ['a', 'b']) { p.sideEvent(m, { type: 'user-run', sid }); p.sideEvent(m, { type: 'work-start', sid }, prefs); }
    p.sideDecide(m, env('a', 1000));
    p.sideEvent(m, { type: 'interact', sid: 'a', on: true, now: 1000 });
    p.sideDecide(m, env('b', 2000));
    const hoverB = p.sideDecide(m, env('b', 9000)).visible;
    p.sideEvent(m, { type: 'interact', sid: 'b', on: false, now: 9000 });
    const closesB = p.sideDecide(m, env('b', 15000)).visible;
    const closesA = p.sideDecide(m, env('a', 15000)).visible;
    p.sideEvent(m, { type: 'user-open', sid: 'a' });
    const pinned = p.sideDecide(m, env('a', 99999)).reason;
    p.sideEvent(m, { type: 'user-close', sid: 'a' }); p.sideEvent(m, { type: 'work-start', sid: 'a' }, prefs);
    const suppressed = p.sideDecide(m, env('a', 100000, { active: 1 })).visible;
    p.sideEvent(m, { type: 'user-run', sid: 'a' }); p.sideEvent(m, { type: 'work-start', sid: 'a' }, prefs);
    const fresh = p.sideDecide(m, env('a', 100001, { active: 1 })).visible;
    p.sideEvent(m, { type: 'failure', sid: 'a', key: 'failure', hold: true });
    const failure = p.sideDecide(m, env('a', 110000));
    const drawer = p.sideDecide(m, env('a', 110000, { dock: 'drawer' })).visible;
    const outside = p.sideDecide(m, env('a', 110000, { view: 'files' })).visible;
    return { hoverB, closesB, closesA, pinned, suppressed, fresh, failure: failure.badge, drawer, outside };
  });
  for (const [key, expected] of Object.entries({ hoverB: true, closesB: false, closesA: false, pinned: 'pinned', suppressed: false, fresh: true, failure: { count: 1, tone: 'err' }, drawer: false, outside: false })) check(`Policy transition: ${key}`, controls[key], expected);
  check('No browser runtime errors', errors, []);

}
