#!/usr/bin/env bash
#
# 把上游 farion1231/cc-switch 的更新合并进当前分支。
#
# 用法：
#   scripts/sync-upstream.sh              # 合并到当前分支
#   scripts/sync-upstream.sh --dry-run    # 只看差距，不动工作区
#
# 设计要点：
#   - upstream-sync 是纯上游镜像分支，永远不对它提交自有代码。
#   - 用 merge 而不是 rebase：自有提交已在 origin 上，rebase 要 force push，
#     而且每次上游更新都要重新解一遍冲突；merge 只解新增的那部分。
#   - 工作区不干净就直接退出，避免把进行中的改动卷进合并。
#
set -euo pipefail

UPSTREAM_REMOTE="upstream"
UPSTREAM_BRANCH="main"
SYNC_BRANCH="upstream-sync"

DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

cd "$(git rev-parse --show-toplevel)"

# 1. 工作区必须干净
if [[ -n "$(git status --porcelain)" ]]; then
  echo "✗ 工作区有未提交改动，先提交或 stash 再同步：" >&2
  git status --short >&2
  exit 1
fi

# 2. 拉取上游并刷新镜像分支
echo "→ 拉取 $UPSTREAM_REMOTE/$UPSTREAM_BRANCH ..."
git fetch "$UPSTREAM_REMOTE" "$UPSTREAM_BRANCH" --quiet
git branch -f "$SYNC_BRANCH" "$UPSTREAM_REMOTE/$UPSTREAM_BRANCH"

BEHIND=$(git rev-list --count "HEAD..$SYNC_BRANCH")
AHEAD=$(git rev-list --count "$SYNC_BRANCH..HEAD")
CURRENT=$(git rev-parse --abbrev-ref HEAD)

echo "  当前分支：$CURRENT"
echo "  上游新增：$BEHIND 个提交"
echo "  自有独有：$AHEAD 个提交"

if [[ "$BEHIND" -eq 0 ]]; then
  echo "✓ 已与上游同步，无需操作。"
  exit 0
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "→ dry-run 结束，未改动工作区。"
  echo "  会合入的上游提交（最近 10 条）："
  git log --oneline -10 "HEAD..$SYNC_BRANCH" | sed 's/^/    /'
  exit 0
fi

# 3. 合并
echo "→ 合并 $SYNC_BRANCH 到 $CURRENT ..."
if git merge "$SYNC_BRANCH" --no-ff --no-edit \
    -m "chore(sync): 合并上游 $UPSTREAM_REMOTE/$UPSTREAM_BRANCH（$(git rev-parse --short "$SYNC_BRANCH")）"; then
  echo "✓ 合并完成，无冲突。"
  echo "  接着跑：pnpm typecheck && cargo check --tests --manifest-path src-tauri/Cargo.toml"
  exit 0
fi

# 4. 冲突提示
echo ""
echo "✗ 出现冲突，需要手工处理："
git diff --name-only --diff-filter=U | sed 's/^/    /'
cat <<'EOF'

处理原则（本项目的固定约定）：
  - 上游删了、我们保留的文件       -> git checkout --ours
  - 我们清理掉、上游改了           -> git rm（保持删除）
  - 品牌名冲突（CC Switch）        -> 保留 XSEC Agent Shield
  - 启动自动导入                  -> 保留我们的显式导入（不恢复自动导入）
  - 上游新增功能                  -> 尽量全收，只做品牌名与 crate 名替换

crate 名检查（上游会带来 cc_switch_lib）：
  grep -rn 'cc_switch_lib' src-tauri --include=*.rs

解完冲突后：
  git add -A && git commit --no-edit
  pnpm typecheck
  cargo check --tests --manifest-path src-tauri/Cargo.toml
EOF
exit 1
