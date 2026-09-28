import { expect, test, type Page } from "@playwright/test";

import { boot, fixtureRepo, rightClick } from "./harness";

async function observeNextContextMenu(page: Page, selector: string): Promise<boolean> {
  await page.evaluate(() => {
    delete document.documentElement.dataset.contextMenuPrevented;
    document.addEventListener(
      "contextmenu",
      (event) => {
        document.documentElement.dataset.contextMenuPrevented = String(
          event.defaultPrevented,
        );
      },
      { once: true },
    );
  });

  await rightClick(page, selector);
  await expect
    .poll(() =>
      page.locator("html").getAttribute("data-context-menu-prevented"),
    )
    .not.toBeNull();
  return (
    (await page.locator("html").getAttribute("data-context-menu-prevented")) ===
    "true"
  );
}

test.describe("browser context menu policy", () => {
  test("blocks the browser menu in the main window", async ({ page }) => {
    await boot(page);
    expect(await observeNextContextMenu(page, '[data-testid="welcome"]')).toBe(true);
  });

  test("blocks the browser menu in the comparison window", async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
    });
    expect(await observeNextContextMenu(page, '[data-testid="compare-window"]')).toBe(
      true,
    );
  });

  test("blocks the browser menu in the About window", async ({ page }) => {
    await boot(page, { window: "about" });
    expect(await observeNextContextMenu(page, '[data-testid="about"]')).toBe(true);
  });

  test("keeps the browser menu available in text inputs and text areas", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()] });
    expect(
      await observeNextContextMenu(page, '[data-testid="commit-message"]'),
    ).toBe(false);

    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
    });
    expect(
      await observeNextContextMenu(page, '[data-testid="compare-input-Base"]'),
    ).toBe(false);
  });

  test("keeps the application context menu working", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await rightClick(page, ".graph-row");

    await expect(page.getByTestId("context-copy-message")).toBeVisible();
  });
});
