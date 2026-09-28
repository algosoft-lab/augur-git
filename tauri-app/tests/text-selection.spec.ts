import { expect, test, type Page } from "@playwright/test";

import { boot, fixtureRepo } from "./harness";

async function dragAcross(page: Page, selector: string): Promise<void> {
  const box = await page.locator(selector).boundingBox();
  if (!box) {
    throw new Error(`No visible text target: ${selector}`);
  }

  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + 1, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 1, y, { steps: 6 });
  await page.mouse.up();
}

async function expectSelectedInputText(
  page: Page,
  selector: string,
  value: string,
): Promise<void> {
  const field = page.locator(selector);
  await field.fill(value);
  await field.selectText();
  await expect
    .poll(() =>
      field.evaluate((element) => {
        const input = element as HTMLInputElement | HTMLTextAreaElement;
        return [input.selectionStart, input.selectionEnd];
      }),
    )
    .toEqual([0, value.length]);
}

async function expectDiffTextSelectable(
  page: Page,
  layout: "inline" | "side-by-side",
): Promise<void> {
  await page.evaluate((diffLayout) => {
    (window as any).__STUB__.config.view.diff_layout = diffLayout;
  }, layout);
  await page.locator(".graph-row").first().click();

  const layoutClass = layout === "inline" ? ".diff--inline" : ".diff--split";
  await expect(page.locator(layoutClass)).toBeVisible();

  const code = page.locator(".diff__text").filter({ hasText: "count += 2" }).first();
  await expect(code).toBeVisible();
  await code.selectText();
  await expect
    .poll(() => page.evaluate(() => window.getSelection()?.toString() ?? ""))
    .toContain("count += 2");
  await expect(page.locator(".diff__gutter").first()).toHaveCSS(
    "user-select",
    "none",
  );
  if (layout === "inline") {
    await expect(page.locator(".diff__marker").first()).toHaveCSS(
      "user-select",
      "none",
    );
  }
}

test.describe("text selection policy", () => {
  test("prevents selection on ordinary text and buttons", async ({ page }) => {
    await boot(page);
    await expect(page.getByTestId("welcome-open")).toHaveCSS(
      "user-select",
      "none",
    );

    await page.evaluate(() => window.getSelection()?.removeAllRanges());
    await dragAcross(page, ".welcome__title");
    await expect
      .poll(() => page.evaluate(() => window.getSelection()?.toString() ?? ""))
      .toBe("");
  });

  test("keeps Commit Message text selectable", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await expectSelectedInputText(
      page,
      '[data-testid="commit-message"]',
      "commit message text",
    );
  });

  test("keeps other text inputs selectable", async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
    });
    await expectSelectedInputText(
      page,
      '[data-testid="compare-input-Base"]',
      "feature/branch",
    );
  });

  test("keeps inline and side-by-side Diff text selectable", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await expectDiffTextSelectable(page, "inline");

    await page.evaluate(() => window.getSelection()?.removeAllRanges());
    await page.locator(".graph-row").nth(1).click();
    await expectDiffTextSelectable(page, "side-by-side");
  });

  test("keeps Diff text selectable in the comparison window", async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
    });

    await page.getByTestId("compare-toggle-Base").click();
    await page.getByTestId("compare-option-local-refs/heads/master").click();
    await page.getByTestId("compare-toggle-Target").click();
    await page
      .getByTestId("compare-option-local-refs/heads/feature/tauri")
      .click();

    const code = page.locator(".diff__text").first();
    await expect(code).toBeVisible();
    await code.selectText();
    await expect
      .poll(() => page.evaluate(() => window.getSelection()?.toString() ?? ""))
      .not.toBe("");
  });
});
