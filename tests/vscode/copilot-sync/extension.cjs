const vscode = require("vscode");

exports.activate = function () {
  // Copilot deliberately skips model-provider activation in ExtensionMode.Test.
  // A development host exercises the installed extension's normal activation.
  setTimeout(() => {
    require("./run.cjs")
      .run()
      .catch(() => {})
      .finally(() => vscode.commands.executeCommand("workbench.action.quit"));
  }, 1500);
};
