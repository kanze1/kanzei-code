/* global document, window */
import assert from "node:assert/strict";
import http from "node:http";
import path from "node:path";
import { writeFile } from "node:fs/promises";
import { WebSocketServer } from "ws";
import { withTask } from "./artifact-lifecycle.mjs";
import { nativeApp, until, textReply, toolReply } from "./fixtures/native-app.mjs";
import clipAudit from "./ui-preview/clip-audit.mjs";

await withTask({ suite: "ui-initiative-native-smoke" }, async task => {
  assert.equal(process.platform, "win32", "Initiative acceptance requires Windows/WebView2");
  const checks = [], deliveries = [], sockets = new Set(), feedCalls = [];
  const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
  let phase = "seed", decision = "silent", heldReview = null, heldGeneration = null, sendMode = "success";
  const server = task.server(http.createServer((req, res) => {
    feedCalls.push(req.url); res.setHeader("Content-Type", "application/json");
    if (req.url === "/x/web-interface/nav") res.end(JSON.stringify({ code: 0, data: { isLogin: true, mid: 1000, uname: "本地测试" } }));
    else if (req.url.startsWith("/x/web-interface/popular")) res.end(JSON.stringify({ code: 0, data: { list: [{ bvid: "BV1234567890", title: "SYNTH_DISCOVERY", owner: { name: "测试 UP" } }] } }));
    else { res.statusCode = 404; res.end(JSON.stringify({ code: -404 })); }
  }));
  const onebot = new WebSocketServer({ server });
  onebot.on("connection", socket => {
    sockets.add(socket); socket.on("close", () => sockets.delete(socket));
    socket.on("message", raw => {
      const call = JSON.parse(String(raw)); let data;
      switch (call.action) {
        case "get_login_info": data = { user_id: 100, nickname: "本地测试" }; break;
        case "get_friend_list": data = [{ user_id: 200, nickname: "测试好友" }]; break;
        case "get_group_list": data = [{ group_id: 300, group_name: "测试群" }]; break;
        case "send_private_msg": case "send_group_msg":
          deliveries.push(call);
          if (sendMode === "unknown") { socket.terminate(); return; }
          data = { message_id: deliveries.length }; break;
        default: throw new Error(`Unexpected OneBot action ${call.action}`);
      }
      socket.send(JSON.stringify({ status: "ok", retcode: 0, echo: call.echo, data }));
    });
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${server.address().port}`;
  const plan = label => ({ kind: "discovery", topic: `SYNTH_TOPIC_${label}`, context: "延续原聊天对合成器的兴趣，先读取真实 B站资料再分享", not_before: null, expires: null });
  const app = await nativeApp(path.resolve("target/debug/kzapp.exe"), task.evidence, "Initiative fixture", (body, res) => {
    for (const tool of body.tools || []) {
      assert.equal(tool.function.parameters.type, "object", `${tool.function.name} parameters require an object root`);
      assert(tool.function.parameters.properties && typeof tool.function.parameters.properties === "object", `${tool.function.name} properties must be an object`);
    }
    if (body.tools?.some(tool => tool.function.name === "initiative_decision")) {
      if (decision === "hold") { heldReview = { res, closed: false }; res.on("close", () => { heldReview.closed = true; }); return; }
      const input = decision === "speak" ? { action: "speak", topic: { selection: "new", plan: plan(phase) }, reason: `REASON_${phase}：有一个与原聊天有关的新发现方向` }
        : decision === "defer" ? { action: "defer", after_minutes: 30, reason: "稍晚再接着聊" } : { action: "silent", reason: "现在没有值得打断对方的内容" };
      toolReply(res, "initiative_decision", { decision: input }, phase); return;
    }
    if (!body.tools?.some(tool => tool.function.name === "social_plan")) { textReply(res, "MEMORY_REVIEW_PENDING"); return; }
    const user = body.messages.findLastIndex(message => message.role === "user"), tail = body.messages.slice(user);
    const automatic = JSON.stringify(body.messages[user]).includes("主动聊天：");
    const used = tail.flatMap(message => message.tool_calls || []).map(call => call.function.name);
    if (automatic) {
      if (phase === "cancel-generation") { heldGeneration = { res, closed: false }; res.on("close", () => { heldGeneration.closed = true; }); return; }
      if (!used.includes("bili_feed")) { toolReply(res, "bili_feed", {}, `${phase}-feed`); return; }
      if (body.tools.some(tool => tool.function.name === "qq_reply") && !used.includes("qq_reply")) { toolReply(res, "qq_reply", { text: `AUTO_${phase}：刚查到一个合成器视频，要接着聊上次那个音色吗？` }, `${phase}-reply`); return; }
      textReply(res, `AUTO_${phase}：刚查到一个合成器视频，要接着聊上次那个音色吗？`); return;
    }
    if (phase === "seed" && !used.includes("social_plan")) { toolReply(res, "social_plan", { operation: { action: "remember", plan: { ...plan("seed"), kind: "follow_up" } } }, "original-topic"); return; }
    if (body.tools.some(tool => tool.function.name === "qq_reply") && !used.includes("qq_reply")) { toolReply(res, "qq_reply", { text: `HUMAN_REPLY_${phase}` }, `${phase}-human-reply`); return; }
    textReply(res, `HUMAN_REPLY_${phase}`);
  }, "", { KANZEI_E2E_BILI_ENDPOINT: endpoint });
  const { page, invoke } = app;
  const args = target => ({ projectDir: target.root, processId: target.process_id });
  const status = target => invoke("initiative_status", args(target));
  const idle = target => until(async () => !(await invoke("process_list", { projectDir: target.root })).some(process => process.running), "Original conversation becomes idle");
  const save = async (target, extra = {}) => {
    const before = await status(target);
    return invoke("initiative_settings_save", { ...args(target), revision: before.target.revision, settings: { ...before.target.settings, enabled: true, idle_minutes: 0, review_minutes: 1, daily_messages: 20, unanswered_messages: 20, quiet_start: 0, quiet_end: 0, ...extra } });
  };
  const waitState = async (target, expected) => { let result; await until(async () => { result = await status(target); return result.reviews[0]?.state === expected; }, `Review settles as ${expected}`); return result; };
  const incoming = (kind, peer, text, id) => {
    const event = { post_type: "message", self_id: 100, message_type: kind === "friend" ? "private" : "group", user_id: kind === "friend" ? Number(peer) : 201, message_id: id, sender: { nickname: "测试成员" }, message: [{ type: "text", data: { text } }] };
    if (kind === "group") { event.group_id = Number(peer); event.message.unshift({ type: "at", data: { qq: "100" } }); }
    for (const socket of sockets) socket.send(JSON.stringify(event));
  };
  try {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.locator("#social-nav").click();
    const personal = await invoke("social_conversation_open");
    await until(() => page.evaluate(async target => {
      const shell = await import("./03-shell.js");
      return shell.activeProcessId === target.process_id && shell.currentProject?.toLowerCase() === target.root.toLowerCase();
    }, personal), "Social navigation has selected the original personal conversation");
    await invoke("run_prompt", { ...args(personal), prompt: "ORIGINAL_SYNTH_CONTEXT：继续聊上次合成器音色，我喜欢短包络", commandId: "seed" });
    await idle(personal);
    let result = await status(personal);
    check(!result.target.settings.enabled && result.topics.some(topic => topic.plan.topic === "SYNTH_TOPIC_seed"), "Normal original conversation saves an intention while initiative remains opt-in");
    await page.locator("#social-manage").click(); await page.locator("#social-platform-initiative").click();
    await page.locator("#initiative-settings-form").waitFor();
    const input = page.locator('#initiative-settings-form [name="idle_minutes"]');
    await input.fill("37"); await page.locator("#initiative-refresh").click();
    check(await input.inputValue() === "37", "Background refresh preserves the unsaved policy and focused input");
    const initial = result.target;
    phase = "silent";
    for (const [name, value] of [["idle_minutes", "0"], ["review_minutes", "1"], ["daily_messages", "20"], ["unanswered_messages", "20"], ["quiet_start", "0"], ["quiet_end", "0"]]) await page.locator(`#initiative-settings-form [name="${name}"]`).fill(value);
    await page.locator('#initiative-settings-form [name="enabled"]').check();
    await page.locator('#initiative-settings-form button[type="submit"]').click();
    result = await waitState(personal, "silent");
    check(result.reviews[0].decision.action === "silent" && deliveries.length === 0, "Silent decision is durable without any QQ message");
    const reviewRequest = app.requests.find(body => body.tools?.some(tool => tool.function.name === "initiative_decision"));
    check(JSON.stringify(reviewRequest).includes("ORIGINAL_SYNTH_CONTEXT") && JSON.stringify(reviewRequest).includes("SYNTH_TOPIC_seed") && JSON.stringify(reviewRequest).includes("大肥鱼"), "Review receives real original history, intention and the same persona");
    phase = "defer"; decision = "defer"; await save(personal); result = await waitState(personal, "deferred");
    check(result.target.next_review > Date.now() + 25 * 60000, "Contextual deferral sets a real future decision time");
    phase = "desktop"; decision = "speak"; await save(personal); result = await waitState(personal, "sent"); await idle(personal);
    check(result.target.identity.session_id === initial.identity.session_id && feedCalls.some(url => url.startsWith("/x/web-interface/popular")), "A newly discovered topic researches Bilibili and speaks in the same original session");
    await until(async () => (await page.locator("#messages").innerText()).includes("AUTO_desktop"), "Confirmed desktop output reaches original chat");
    check(!(await page.locator("#messages").innerText()).includes("主动聊天："), "Engine opening instructions never appear as a user message");
    const display = await invoke("conversation_display_get", args(personal));
    const modelHistory = await invoke("conversation_get", args(personal));
    check(!JSON.stringify(display).includes("主动聊天：") && JSON.stringify(modelHistory).includes("主动聊天：") && JSON.stringify(display).includes("AUTO_desktop"), "Reload projection preserves actual assistant history and hides internal opening input");
    phase = "cancel-review"; decision = "hold"; await save(personal); await until(() => heldReview, "Planner request is in flight");
    const reviewId = (await status(personal)).reviews[0].id;
    phase = "human-during-review";
    await invoke("run_prompt", { ...args(personal), prompt: "NEW_HUMAN_INPUT_DURING_REVIEW", commandId: "human-review" }); await idle(personal);
    await until(() => heldReview.closed, "New human input cancels the actual planner HTTP request");
    result = await status(personal);
    check(result.reviews.find(review => review.id === reviewId).state === "cancelled", "New input cancels the durable old review");
    phase = "cancel-generation"; decision = "speak"; await save(personal); await until(() => heldGeneration, "Original runner is generating the chosen opening");
    const generationId = (await status(personal)).reviews[0].id;
    phase = "human-during-generation";
    await invoke("run_prompt", { ...args(personal), prompt: "NEW_HUMAN_INPUT_DURING_GENERATION", commandId: "human-generation" }); await idle(personal);
    await until(() => heldGeneration.closed, "New input stops actual original runner HTTP generation");
    result = await status(personal);
    check(result.reviews.find(review => review.id === generationId).state === "cancelled" && !(await page.locator("#messages").innerText()).includes("AUTO_cancel-generation"), "A cancelled opening leaves no phantom assistant message");
    phase = "stop-review"; decision = "hold"; heldReview = null; await save(personal); await until(() => heldReview, "Second planner request is in flight");
    await invoke("stop_run", args(personal)); await until(() => heldReview.closed, "Stop cancels planner HTTP");
    result = await status(personal); check(result.target.state === "stopped" && result.reviews[0].state === "cancelled", "Stopping durably pauses automatic conversations");
    // No real external account is involved: all transport endpoints belong to this task.
    const qq = (await invoke("qq_status")).settings;
    Object.assign(qq, { enabled: true, friends_enabled: true, url: endpoint.replace("http", "ws"), account: "100", model: "stub:test", groups: [{ id: "300", name: "主动测试群", enabled: true, interval_seconds: 1, proactive_minutes: 0, quiet_start: 0, quiet_end: 0, persona: "本群只聊合成器" }] });
    await invoke("qq_config_save", { settings: qq, token: "local-test" }); await until(async () => (await invoke("qq_status")).connected, "Local OneBot connects");
    phase = "group-human"; decision = "silent"; incoming("group", "300", "GROUP_SYNTH_CONTEXT", 101);
    await until(() => deliveries.some(call => JSON.stringify(call).includes("HUMAN_REPLY_group-human")), "Original group chat receives a real reply");
    const group = (await invoke("qq_status")).peers.find(peer => peer.identity.kind === "group"); await idle(group);
    phase = "group"; decision = "speak"; const beforeGroup = deliveries.length; await save(group); result = await waitState(group, "sent"); await idle(group);
    check(deliveries.length === beforeGroup + 1 && deliveries.at(-1).action === "send_group_msg" && deliveries.at(-1).params.group_id === 300 && JSON.stringify(deliveries.at(-1)).includes("AUTO_group"), "Group initiative uses the same research tools and sends one authored reply to the original group");
    const groupReview = app.requests.find(body => body.tools?.some(tool => tool.function.name === "initiative_decision") && JSON.stringify(body).includes("GROUP_SYNTH_CONTEXT"));
    check(groupReview && !JSON.stringify(groupReview).includes("ORIGINAL_SYNTH_CONTEXT") && (await status(group)).topics.every(topic => topic.plan.topic !== "SYNTH_TOPIC_seed"), "Group initiative isolates private conversation history and topics");
    await page.locator('#initiative-settings-form [name="idle_minutes"]').fill("29");
    await page.locator("#social-target").click(); await page.getByRole("menuitemcheckbox", { name: group.title, exact: false }).click();
    await until(async () => (await page.locator("#initiative-target").innerText()).includes(group.title), "Management follows selected group");
    check(await page.locator('#initiative-settings-form [name="idle_minutes"]').inputValue() === "0", "Group policy does not inherit private unsaved settings");
    await page.locator('#initiative-settings-form [name="idle_minutes"]').fill("45");
    await page.locator("#social-nav").click();
    await until(async () => (await page.locator('#initiative-settings-form [name="idle_minutes"]').inputValue()) === "29", "Private draft survives switching conversations");
    await page.locator("#social-target").click(); await page.getByRole("menuitemcheckbox", { name: group.title, exact: false }).click();
    await until(async () => (await page.locator('#initiative-settings-form [name="idle_minutes"]').inputValue()) === "45", "Group draft survives switching conversations");
    check(true, "Unsaved policy drafts stay isolated across conversation switches");
    phase = "friend-human"; decision = "silent"; incoming("friend", "200", "FRIEND_SYNTH_CONTEXT", 102);
    await until(() => deliveries.some(call => JSON.stringify(call).includes("HUMAN_REPLY_friend-human")), "Friend normal conversation replies");
    const friend = (await invoke("qq_status")).peers.find(peer => peer.identity.kind === "friend"); await idle(friend);
    phase = "friend"; decision = "speak"; await save(friend); await waitState(friend, "sent"); await idle(friend);
    check(deliveries.at(-1).action === "send_private_msg" && JSON.stringify(deliveries.at(-1)).includes("AUTO_friend"), "Friend initiative sends only the authored final reply to the actual friend");
    await save(friend, { enabled: false });
    await invoke("bili_cookie_save", { cookie: "SESSDATA=local-fixture; bili_jct=csrf" });
    const bili = await invoke("bili_conversation_open");
    phase = "bili-human"; await invoke("run_prompt", { ...args(bili), prompt: "BILI_SYNTH_CONTEXT", commandId: "bili-human" }); await idle(bili);
    phase = "bilibili"; decision = "speak"; await save(bili); result = await waitState(bili, "sent"); await idle(bili);
    check(result.target.identity.channel === "bilibili" && JSON.stringify(await invoke("conversation_display_get", args(bili))).includes("AUTO_bilibili"), "Bilibili initiative continues its actual browser conversation with shared research tools");
    await save(bili, { enabled: false });
    const quietGroup = { ...qq.groups[0], quiet_start: new Date().getHours(), quiet_end: (new Date().getHours() + 1) % 24 };
    await invoke("qq_group_save", { group: quietGroup, existingId: "300" });
    result = await status(group); check(result.channelQuiet && result.available, "Common initiative reads current group quiet hours separately from connection state");
    await invoke("qq_group_save", { group: qq.groups[0], existingId: "300" });
    phase = "unknown"; sendMode = "unknown"; await save(group); result = await waitState(group, "unknown");
    check(result.topics.find(topic => topic.id === result.reviews[0].topic_id).state === "uncertain", "Lost QQ acknowledgment keeps an unknown receipt and blocks replay of its topic");
    check(deliveries.every(call => !JSON.stringify(call).includes("tool_calls") && !JSON.stringify(call).includes("REASON_") && !JSON.stringify(call).includes("sessionId")), "QQ receives no planner JSON, tool trace or internal session metadata");
    await page.locator("#social-nav").click(); await page.locator("#social-toolbar").waitFor({ state: "visible" });
    await page.locator("#social-target").click(); await page.getByRole("menuitemcheckbox", { name: group.title, exact: false }).click();
    await until(async () => (await page.locator("#social-target-name").innerText()) === group.title, "Selected group remains accessible after unknown delivery");
    if (!await page.locator("#social-manager").isVisible()) await page.locator("#social-manage").click();
    await page.locator("#social-platform-initiative").click();
    await page.locator("#initiative-refresh").waitFor({ state: "visible" });
    for (const theme of ["dark", "light"]) {
      await page.evaluate(theme => document.documentElement.setAttribute("data-theme", theme), theme);
      for (const width of [1440, 420]) {
        await page.setViewportSize({ width, height: 900 }); await page.locator("#initiative-refresh").hover();
        const audit = await page.evaluate(clipAudit, { viewport: true });
        check(audit.cut === 0 && await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `${theme}/${width} no clipped shadow or horizontal overflow`);
        await page.screenshot({ path: path.join(task.evidence, `${theme}-${width}.png`) });
      }
    }
    check(app.errors.length === 0, `Native runtime and fixtures have no errors: ${app.errors.join("; ")}`);
    await writeFile(path.join(task.evidence, "receipt.json"), JSON.stringify({ boundary: "Real WebView2, Rust runtime, original session stores and QQ/Bilibili adapters; local model and transport fixtures; no external recipients", checks, deliveries, feedCalls }, null, 2));
  } catch (error) {
    await page.screenshot({ path: path.join(task.evidence, "failure.png") });
    await writeFile(path.join(task.evidence, "failure.json"), JSON.stringify({ error: String(error), phase, active: await page.evaluate(async () => { const shell = await import("./03-shell.js"); return { root: shell.currentProject, process: shell.activeProcessId, session: shell.activeSessionId }; }), requests: app.requests, ui: await page.locator("body").innerText(), deliveries }, null, 2)); throw error;
  } finally { await app.close(); for (const socket of sockets) socket.terminate(); await new Promise(resolve => onebot.close(resolve)); }
});
