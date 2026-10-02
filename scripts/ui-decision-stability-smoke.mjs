// D-771: real browser DOM + injected snapshots, including concurrent revision changes.
/* global window, CompositionEvent */
import assert from "node:assert/strict";

export async function verifyDecisionStability(browser, origin) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  try {
    await page.goto(`${origin}/?scene=workspace&theme=dark`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    await page.getByRole("button", { name: "决策复核", exact: true }).click();
    await page.locator(".console-decision").first().waitFor();
    await page.evaluate(async () => {
      const api = await import("/12-decision-console.js");
      const pages = await import("/12-docs-pages.js");
      const snapshot = structuredClone(pages.lastWorkspaceSnapshot);
      const callbacks = { refresh: async () => {}, beforeMutation: () => {}, openProject: async () => {} };
      // Deliberately duplicate an ID in another project: ownership must be part of every DOM key.
      snapshot.projects[1].decisions[0].id = snapshot.projects[0].decisions[0].id;
      api.renderDecisionConsole(snapshot, callbacks);
      window.__decisionStability = { api, snapshot, callbacks };
    });
    const row = page.locator(".console-decision").filter({ hasText: "普通选择需要停下来问用户吗？" });
    await row.getByRole("checkbox").check();
    await row.getByRole("button", { name: "纠正决定", exact: true }).click();
    const input = row.getByRole("textbox", { name: "希望如何修改" });
    await input.fill("先保留输入法中的文字和草稿");
    await row.getByRole("combobox", { name: "纠正适用范围" }).selectOption("project");
    await input.focus();
    await row.evaluate((el) => Promise.all(el.getAnimations().map((animation) => animation.finished)));
    await page.evaluate(() => {
      const state = window.__decisionStability;
      const row = [...document.querySelectorAll(".console-decision")].find((el) => el.textContent.includes("普通选择需要停下来问用户吗？"));
      const input = row.querySelector("textarea");
      input.setSelectionRange(3, 7);
      input.dispatchEvent(new CompositionEvent("compositionstart", { data: "中的", bubbles: true }));
      state.row = row; state.input = input; state.title = row.querySelector("strong");
      state.scrollTop = document.getElementById("workspace-scroll").scrollTop;
      state.revision = state.snapshot.projects[0].decisions[0].revision;
      for (let i = 0; i < 8; i += 1) {
        state.snapshot = structuredClone(state.snapshot);
        state.snapshot.observed_at += 1000;
        state.snapshot.projects[1].running_lines += 1;
        state.snapshot.projects[1].updated_at += 1000;
        state.api.renderDecisionConsole(state.snapshot, state.callbacks);
      }
    });
    assert.deepEqual(await page.evaluate(() => {
      const s = window.__decisionStability;
      return { row: s.row.isConnected, input: s.row.querySelector("textarea") === s.input,
        title: s.row.querySelector("strong") === s.title, focus: document.activeElement === s.input,
        caret: [s.input.selectionStart, s.input.selectionEnd], text: s.input.value,
        selected: s.row.querySelector("input").checked, scope: s.row.querySelector("select").value,
        animation: s.row.getAnimations().length, busy: s.api.workspaceConsoleBusy(),
        scroll: document.getElementById("workspace-scroll").scrollTop === s.scrollTop };
    }), { row: true, input: true, title: true, focus: true, caret: [3, 7], text: "先保留输入法中的文字和草稿",
      selected: true, scope: "project", animation: 0, busy: false, scroll: true });
    // Same project/revision also preserves a real text selection outside the editor.
    await page.evaluate(() => {
      const s = window.__decisionStability;
      s.input.dispatchEvent(new CompositionEvent("compositionend", { data: "中的", bubbles: true }));
      const range = document.createRange(); range.selectNodeContents(s.title);
      const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
      s.selection = selection.toString(); s.api.renderDecisionConsole(structuredClone(s.snapshot), s.callbacks);
    });
    assert.equal(await page.evaluate(() => window.getSelection().toString() === window.__decisionStability.selection), true);
    // Changed revision invalidates selected approval; draft stays bound to the old visible revision.
    await page.evaluate(() => {
      const s = window.__decisionStability;
      window.getSelection().removeAllRanges();
      s.input.focus(); s.input.setSelectionRange(4, 4);
      s.input.dispatchEvent(new CompositionEvent("compositionstart", { data: "继续", bubbles: true }));
      s.snapshot = structuredClone(s.snapshot);
      s.snapshot.projects[0].decisions[0].revision += 1;
      s.snapshot.projects[0].decisions[0].resolution.answer = "这是尚未查看的新决定";
      s.api.renderDecisionConsole(s.snapshot, s.callbacks);
      window.__kzPreview.setCommand("decision_review", () => { throw new Error("decision changed; refresh before reviewing"); });
    });
    assert.equal(await row.getByRole("checkbox").isChecked(), false);
    assert.equal(await row.getByRole("checkbox").isDisabled(), true);
    assert.equal(await row.getByRole("button", { name: "本次通过", exact: true }).isDisabled(), true);
    await row.getByText("决策已有更新，请查看后重新复核。", { exact: true }).waitFor();
    assert.equal(await row.getByText("这是尚未查看的新决定", { exact: true }).count(), 0);
    assert.equal(await page.evaluate(() => document.activeElement === window.__decisionStability.input && window.__decisionStability.input.selectionStart === 4), true);
    await page.evaluate(() => window.__decisionStability.input.dispatchEvent(new CompositionEvent("compositionend", { data: "继续", bubbles: true })));
    await row.getByRole("button", { name: "保存纠正并排入原对话", exact: true }).click();
    await page.waitForFunction(() => window.__kzPreview.calls.some((call) => call.cmd === "decision_review"));
    assert.equal(await page.evaluate(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").at(-1).args.review.expected_revision === window.__decisionStability.revision), true);
    await page.waitForFunction(() => !window.__decisionStability.api.workspaceConsoleBusy());
    assert.equal(await input.inputValue(), "先保留输入法中的文字和草稿");
    await row.getByRole("button", { name: "查看更新", exact: true }).click();
    await row.getByText("这是尚未查看的新决定", { exact: true }).waitFor();
    assert.equal(await row.getByRole("checkbox").isChecked(), false);
    assert.equal(await input.inputValue(), "先保留输入法中的文字和草稿");
    await row.getByRole("button", { name: "保存纠正并排入原对话", exact: true }).click();
    await page.waitForFunction(() => window.__kzPreview.calls.filter((call) => call.cmd === "decision_review").length === 2);
    assert.equal(await page.evaluate(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").at(-1).args.review.expected_revision === window.__decisionStability.revision + 1), true);
    await page.waitForFunction(() => !window.__decisionStability.api.workspaceConsoleBusy());
    // Verify the duplicate-ID row kept its own content and unselected state.
    const other = page.locator(".console-decision").filter({ hasText: "搜索结果用分页还是连续滚动？" });
    assert.equal(await other.getByRole("checkbox").isChecked(), false);
    // Refresh before the native click default and during the queued toggle event: preserve the same details.
    await page.evaluate(() => window.__decisionStability.api.setWorkspaceConsoleState({ tab: "deliveries" }));
    const deliveryIdentity = await page.evaluate(() => {
      const s = window.__decisionStability;
      const project = s.snapshot.projects[0];
      const unit = project.work_units[0];
      const el = [...document.querySelectorAll(".console-delivery")].find((row) => row.dataset.projectPath === project.path && row.dataset.unitId === unit.unit_id);
      s.delivery = el;
      el.querySelector("summary").addEventListener("click", () => {
        s.snapshot.projects[1].updated_at += 1000;
        s.api.renderDecisionConsole(structuredClone(s.snapshot), s.callbacks);
        s.deliveryRetainedBeforeToggle = el.isConnected;
      }, { once: true });
      el.addEventListener("toggle", () => {
        s.snapshot.projects[0].verification_jobs[0].status = "passed";
        s.api.renderDecisionConsole(structuredClone(s.snapshot), s.callbacks);
        s.deliveryToggleObserved = true;
      }, { once: true });
      return { path: project.path, unitId: unit.unit_id };
    });
    const delivery = page.locator(`.console-delivery[data-project-path=${JSON.stringify(deliveryIdentity.path)}][data-unit-id=${JSON.stringify(deliveryIdentity.unitId)}]`);
    await delivery.locator("summary").click();
    await page.waitForFunction(() => window.__decisionStability.deliveryToggleObserved);
    assert.equal(await delivery.evaluate((el) => el === window.__decisionStability.delivery && el.open && window.__decisionStability.deliveryRetainedBeforeToggle), true);
    await delivery.getByText("冻结版本: example-source-fingerprint", { exact: true }).waitFor();
    await delivery.getByText("验证通过 · Preview only", { exact: true }).waitFor();
    console.log("D-771 browser verification passed: keyed rows, duplicate IDs, caret/IME, drafts, scroll/text selection, revision conflict and expanded delivery preservation.");
  } finally { await page.close(); }
}
