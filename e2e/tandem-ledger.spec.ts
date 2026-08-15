import { expect, test, type Locator, type Page } from "@playwright/test";
import { isWithinVerticalViewport } from "./viewportGeometry";
const fixtureRows = [
  "Resolve foundation alert",
  "Review foundation acceptance",
  "Continue foundation build",
  "Resume foundation task",
];

const interactiveSelector = [
  "button",
  "input",
  "textarea",
  "select",
  "a[href]",
  "[role=button]",
  "[role=checkbox]",
  "[role=combobox]",
  "[role=link]",
  "[role=menuitem]",
  "[role=menuitemcheckbox]",
  "[role=menuitemradio]",
  "[role=option]",
  "[role=radio]",
  "[role=searchbox]",
  "[role=slider]",
  "[role=spinbutton]",
  "[role=switch]",
  "[role=tab]",
  "[role=textbox]",
].join(",");

async function expectAccessibleInteractiveControls(page: Page) {
  const unnamed = await page
    .locator(interactiveSelector)
    .evaluateAll((controls) =>
      controls
        .filter((control) => {
          const element = control as HTMLElement;
          const rect = element.getBoundingClientRect();
          const style = getComputedStyle(element);
          return (
            style.visibility !== "hidden" &&
            style.display !== "none" &&
            rect.width > 0 &&
            rect.height > 0
          );
        })
        .filter((control) => {
          const element = control as HTMLElement;
          const labelledBy = element.getAttribute("aria-labelledby");
          const labels =
            element instanceof HTMLInputElement ||
            element instanceof HTMLTextAreaElement ||
            element instanceof HTMLSelectElement
              ? Array.from(element.labels ?? [])
                  .map((label) => label.textContent?.trim())
                  .join(" ")
              : "";
          const labelledByText = labelledBy
            ? labelledBy
                .split(/\s+/)
                .map((id) => document.getElementById(id)?.textContent?.trim())
                .join(" ")
            : "";
          return ![
            element.getAttribute("aria-label"),
            labelledByText,
            labels,
            element.textContent?.trim(),
            element.getAttribute("title"),
          ].some((name) => name && /\S/.test(name));
        })
        .map((control) => control.outerHTML.slice(0, 200)),
    );
  expect(
    unnamed,
    "every visible interactive control must have an accessible name or label",
  ).toEqual([]);
}

async function expectWithinVerticalViewport(locator: Locator) {
  const box = await locator.boundingBox();
  const viewport = locator.page().viewportSize();
  expect(box, "expected element must have rendered geometry").not.toBeNull();
  expect(viewport, "page must have an explicit viewport").not.toBeNull();
  expect(
    isWithinVerticalViewport(
      { top: box!.y, bottom: box!.y + box!.height },
      viewport!.height,
    ),
    "expected element must be fully within the vertical viewport",
  ).toBe(true);
}

async function expectLedgerSectionWithinViewport(
  page: Page,
  sectionName: string,
  headingName: string,
  rowName: string,
) {
  const section = page.getByRole("region", { name: sectionName });
  await section.scrollIntoViewIfNeeded();
  const expected = [
    section.getByRole("heading", { name: headingName }),
    section.getByText(rowName),
  ];
  for (const locator of expected) {
    await expect(locator).toBeVisible();
    await expectWithinVerticalViewport(locator);
  }
  for (const control of await section.locator(interactiveSelector).all()) {
    if (await control.isVisible()) await expectWithinVerticalViewport(control);
  }
  await expectCoherentVisibleGeometry(page);
}

async function expectCoherentVisibleGeometry(page: Page) {
  const result = await page.evaluate((selector) => {
    const activeDialog = document.querySelector(
      '[role="dialog"], [role="alertdialog"]',
    );
    const root = activeDialog ?? document.body;
    const independentSelector = activeDialog
      ? selector
      : [
          selector,
          "section > div:first-child",
          "section > div:nth-child(2) > div",
        ].join(",");
    const elements = Array.from(
      root.querySelectorAll<HTMLElement>(independentSelector),
    )
      .filter((element) => {
        const style = getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        return (
          style.visibility !== "hidden" &&
          style.display !== "none" &&
          rect.width > 0 &&
          rect.height > 0
        );
      })
      .map((element, index) => {
        const rect = element.getBoundingClientRect();
        return {
          element,
          label:
            element.getAttribute("aria-label") ||
            element.textContent?.trim().replace(/\s+/g, " ").slice(0, 80) ||
            element.tagName.toLowerCase() + " " + index,
          rect: {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
          },
        };
      });
    const clipped = elements
      .filter(({ rect }) => {
        const verticallyExpected =
          rect.bottom > 0 && rect.top < window.innerHeight;
        return (
          rect.left < -1 ||
          rect.right > window.innerWidth + 1 ||
          (verticallyExpected &&
            (rect.top < -1 || rect.bottom > window.innerHeight + 1))
        );
      })
      .map(({ label }) => label);
    const overlaps: string[] = [];
    for (let leftIndex = 0; leftIndex < elements.length; leftIndex += 1) {
      for (
        let rightIndex = leftIndex + 1;
        rightIndex < elements.length;
        rightIndex += 1
      ) {
        const left = elements[leftIndex];
        const right = elements[rightIndex];
        if (
          left.element.contains(right.element) ||
          right.element.contains(left.element)
        )
          continue;
        const overlapWidth =
          Math.min(left.rect.right, right.rect.right) -
          Math.max(left.rect.left, right.rect.left);
        const overlapHeight =
          Math.min(left.rect.bottom, right.rect.bottom) -
          Math.max(left.rect.top, right.rect.top);
        if (overlapWidth > 1 && overlapHeight > 1)
          overlaps.push(left.label + " <> " + right.label);
      }
    }
    return {
      overflow:
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
      clipped,
      overlaps,
    };
  }, interactiveSelector);
  expect(
    result,
    "visible controls and independent regions must fit coherently",
  ).toEqual({
    overflow: 0,
    clipped: [],
    overlaps: [],
  });
}

async function expectAutomatedLayoutChecks(page: Page) {
  await expectAccessibleInteractiveControls(page);
  await expectCoherentVisibleGeometry(page);
}

async function expectTopConsoleDimensions(page: Page, width: number) {
  const box = await page.locator("header").boundingBox();
  expect(box).not.toBeNull();
  expect(box).toMatchObject({ x: 0, y: 0, width, height: 52 });
}

for (const viewport of [
  {
    name: "desktop",
    width: 1200,
    height: 800,
    screenshot: "ledger-desktop.png",
  },
  { name: "narrow", width: 390, height: 844, screenshot: "ledger-narrow.png" },
]) {
  test(
    viewport.name + " production-component ledger journey",
    async ({ page }) => {
      await page.setViewportSize(viewport);
      await page.goto("");
      await expect(
        page.getByRole("button", { name: "任务", exact: true }),
      ).toHaveAttribute("aria-current", "page");
      await expectTopConsoleDimensions(page, viewport.width);
      await expectAutomatedLayoutChecks(page);
      for (const [section, heading, row] of [
        ["需要你处理", "需要你处理 1", fixtureRows[0]],
        ["待验收", "待验收 1", fixtureRows[1]],
        ["正在推进", "正在推进 1", fixtureRows[2]],
        ["最近可继续", "最近可继续 1", fixtureRows[3]],
      ] as const) {
        await expectLedgerSectionWithinViewport(page, section, heading, row);
      }

      await page.getByRole("button", { name: "新建任务" }).click();
      const createDialog = page.getByRole("dialog", { name: "新建任务" });
      await expect(createDialog).toBeVisible();
      await expectWithinVerticalViewport(createDialog);
      await expectAutomatedLayoutChecks(page);
      await createDialog.getByLabel("项目名称").fill("Tandem Demo");
      await createDialog.getByLabel("项目路径").fill("/tmp/tandem-demo");
      await createDialog.getByLabel("任务标题").fill("修复恢复流程");
      await createDialog
        .getByLabel("原始指令")
        .fill("Disposable browser instruction");
      await createDialog.getByRole("button", { name: "创建任务" }).click();
      await expect(createDialog).toHaveCount(0);
      const createdRow = page
        .getByRole("region", { name: "正在推进" })
        .getByText("修复恢复流程");
      await createdRow.scrollIntoViewIfNeeded();
      await expect(createdRow).toBeVisible();
      await expectWithinVerticalViewport(createdRow);

      await page
        .getByRole("button", { name: "确认完成 Review foundation acceptance" })
        .click();
      const confirmDialog = page.getByRole("alertdialog", {
        name: "确认任务完成",
      });
      await expect(confirmDialog).toBeVisible();
      await expectWithinVerticalViewport(confirmDialog);
      await expectAutomatedLayoutChecks(page);
      await confirmDialog.getByRole("button", { name: "确认完成" }).click();
      await expect(confirmDialog).toHaveCount(0);
      await expect(page.getByText("Review foundation acceptance")).toHaveCount(
        0,
      );

      await page
        .getByRole("button", { name: "Continue foundation build 操作" })
        .click();
      await expect(
        page.getByRole("menuitem", { name: "确认完成" }),
      ).toBeVisible();
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: "Agent 配置" }).click();
      const configHeading = page.getByRole("heading", {
        name: "Agent Configuration",
      });
      await configHeading.scrollIntoViewIfNeeded();
      await expect(configHeading).toBeVisible();
      await expectWithinVerticalViewport(configHeading);
      const legacyRoot = page.getByText("Demo legacy provider root");
      await legacyRoot.scrollIntoViewIfNeeded();
      await expect(legacyRoot).toBeVisible();
      await expectWithinVerticalViewport(legacyRoot);
      await page.getByRole("button", { name: "任务", exact: true }).click();
      await createdRow.scrollIntoViewIfNeeded();
      await expect(createdRow).toBeVisible();
      await expectWithinVerticalViewport(createdRow);
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByRole("alertdialog")).toHaveCount(0);
      await expectTopConsoleDimensions(page, viewport.width);
      await expectAutomatedLayoutChecks(page);
      const screenshot = await page.screenshot({
        path: "e2e/__screenshots__/" + viewport.screenshot,
        fullPage: false,
      });
      const pixels = await page.evaluate(async (png) => {
        const image = new Image();
        image.src = "data:image/png;base64," + png;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.width;
        canvas.height = image.height;
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (!context) throw new Error("2D canvas unavailable");
        context.drawImage(image, 0, 0);
        const data = context.getImageData(0, 0, image.width, image.height).data;
        const corner = Array.from(data.slice(0, 4));
        let opaque = 0;
        let differsFromCorner = 0;
        for (let index = 0; index < data.length; index += 16) {
          if (data[index + 3] > 0) opaque += 1;
          if (
            data[index] !== corner[0] ||
            data[index + 1] !== corner[1] ||
            data[index + 2] !== corner[2] ||
            data[index + 3] !== corner[3]
          )
            differsFromCorner += 1;
        }
        return {
          width: image.width,
          height: image.height,
          opaque,
          differsFromCorner,
        };
      }, screenshot.toString("base64"));
      expect(pixels.width).toBe(viewport.width);
      expect(pixels.height).toBe(viewport.height);
      expect(pixels.opaque).toBeGreaterThan(1000);
      expect(pixels.differsFromCorner).toBeGreaterThan(1000);
    },
  );
}
