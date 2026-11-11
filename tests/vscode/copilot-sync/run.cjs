const vscode = require("vscode");
const fs = require("node:fs");
const path = require("node:path");
const assert = require("node:assert/strict");

exports.run = async function () {
  const fixturePath = process.env.CC_SWITCH_VSCODE_SMOKE_FIXTURE;
  const reportPath = process.env.CC_SWITCH_VSCODE_SMOKE_REPORT;
  const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"));
  const report = { vscodeVersion: vscode.version, results: [], success: false };
  const save = () =>
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
  try {
    const extension = vscode.extensions.getExtension(
      "GitHub.copilot-chat",
      true,
    );
    assert.ok(extension, "Installed Copilot extension is unavailable");
    report.copilotVersion = extension.packageJSON.version;
    await extension.activate();
    let models = [];
    for (let attempt = 0; attempt < 90; attempt++) {
      models = await vscode.lm.selectChatModels({ vendor: "customendpoint" });
      if (
        fixture.cases.every((testCase) =>
          models.some((model) => model.id === testCase.wire),
        )
      )
        break;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    report.discovered = models.map((model) => ({
      id: model.id,
      name: model.name,
    }));
    assert.ok(
      models.some((model) => model.id === "ccswitch-existing"),
      "Existing model was lost",
    );
    for (const testCase of fixture.cases) {
      const model = models.find((candidate) => candidate.id === testCase.wire);
      assert.ok(model, "Projected model missing: " + testCase.name);
      const cancellation = new vscode.CancellationTokenSource();
      const timer = setTimeout(() => cancellation.cancel(), 30000);
      try {
        const response = await model.sendRequest(
          [
            vscode.LanguageModelChatMessage.User(
              "Return a short confirmation.",
            ),
          ],
          {},
          cancellation.token,
        );
        let text = "";
        for await (const part of response.text) text += part;
        assert.ok(
          text.includes(testCase.expected),
          "Unexpected reply for " + testCase.name,
        );
        report.results.push({
          name: testCase.name,
          model: model.id,
          reply: text,
        });
        save();
      } finally {
        clearTimeout(timer);
        cancellation.dispose();
      }
    }
    report.success = true;
    save();
  } catch (error) {
    report.error = error.message;
    save();
    throw error;
  }
};
