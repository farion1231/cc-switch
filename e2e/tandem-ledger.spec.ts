import { expect, test, type Page } from "@playwright/test";
const fixtureRows = [
  "Resolve foundation alert",
  "Review foundation acceptance",
  "Continue foundation build",
  "Resume foundation task",
];

async function expectNoViewportOverflow(page: Page) {
  const overflow = await page.evaluate(() => ({
    width:
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
    clipped: Array.from(
      document.querySelectorAll("button, [role=dialog], [role=menu], section"),
    )
      .filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.right > window.innerWidth + 1 || rect.left < -1;
      })
      .map(
        (element) =>
          element.getAttribute("aria-label") ||
          element.textContent?.trim().slice(0, 80),
      ),
  }));
  expect(overflow, "interactive regions must fit the viewport").toEqual({
    width: 0,
    clipped: [],
  });
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
      await page.goto("/tandem-demo.html");
      await expect(
        page.getByRole("button", { name: "任务", exact: true }),
      ).toHaveAttribute("aria-current", "page");
      for (const heading of [
        "需要你处理 1",
        "待验收 1",
        "正在推进 1",
        "最近可继续 1",
      ])
        await expect(
          page.getByRole("heading", { name: heading }),
        ).toBeVisible();
      for (const row of fixtureRows)
        await expect(page.getByText(row)).toBeVisible();
      await expectNoViewportOverflow(page);

      await page.getByRole("button", { name: "新建任务" }).click();
      const createDialog = page.getByRole("dialog", { name: "新建任务" });
      await expect(createDialog).toBeVisible();
      await createDialog.getByLabel("项目名称").fill("Tandem Demo");
      await createDialog.getByLabel("项目路径").fill("/tmp/tandem-demo");
      await createDialog.getByLabel("任务标题").fill("修复恢复流程");
      await createDialog
        .getByLabel("原始指令")
        .fill("Disposable browser instruction");
      await createDialog.getByRole("button", { name: "创建任务" }).click();
      await expect(
        page
          .getByRole("region", { name: "正在推进" })
          .getByText("修复恢复流程"),
      ).toBeVisible();

      await page
        .getByRole("button", { name: "确认完成 Review foundation acceptance" })
        .click();
      const confirmDialog = page.getByRole("alertdialog", {
        name: "确认任务完成",
      });
      await expect(confirmDialog).toBeVisible();
      await confirmDialog.getByRole("button", { name: "确认完成" }).click();
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
      await expect(
        page.getByRole("heading", { name: "Agent Configuration" }),
      ).toBeVisible();
      await expect(page.getByText("Demo legacy provider root")).toBeVisible();
      await page.getByRole("button", { name: "任务", exact: true }).click();
      await expect(page.getByText("修复恢复流程")).toBeVisible();
      await expectNoViewportOverflow(page);
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
