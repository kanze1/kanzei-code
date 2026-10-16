// Exercise the user-visible menu; tests must not click hidden legacy controls.
export async function openComposerOptions(page) {
  if (!await page.locator("#composer-more-menu").isVisible()) await page.locator("#composer-more").click();
  await page.locator("#composer-more-menu").waitFor({ state: "visible" });
}
export async function closeComposerOptions(page) {
  if (await page.locator("#composer-more-menu").isVisible()) await page.keyboard.press("Escape");
}
