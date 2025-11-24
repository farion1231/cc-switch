import { DeepLinkImportRequest } from "../../lib/api/deeplink";

export function SkillConfirmation({
  request,
}: {
  request: DeepLinkImportRequest;
}) {
  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">添加 Claude Skill 仓库</h3>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          GitHub 仓库
        </label>
        <div className="mt-1 text-sm font-mono bg-muted/50 p-2 rounded border">
          {request.repo}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          目标目录
        </label>
        <div className="mt-1 text-sm font-mono bg-muted/50 p-2 rounded border">
          {request.directory}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <div>
          <label className="block text-sm font-medium text-muted-foreground">
            分支
          </label>
          <div className="mt-1 text-sm">{request.branch || "main"}</div>
        </div>

        {request.skillsPath && (
          <div>
            <label className="block text-sm font-medium text-muted-foreground">
              Skills 路径
            </label>
            <div className="mt-1 text-sm">{request.skillsPath}</div>
          </div>
        )}
      </div>

      <div className="text-blue-600 dark:text-blue-400 text-sm bg-blue-50 dark:bg-blue-950/30 p-3 rounded border border-blue-200 dark:border-blue-800">
        <p>ℹ️ 此操作将添加 Skill 仓库到列表。</p>
        <p className="mt-1">
          添加后，您可以在 Skills 管理界面中选择安装具体的 Skill。
        </p>
      </div>
    </div>
  );
}
