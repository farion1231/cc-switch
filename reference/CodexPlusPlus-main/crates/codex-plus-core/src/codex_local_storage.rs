use std::time::Duration;

use anyhow::Context;
use serde_json::json;

pub(crate) const SANITIZE_LOCAL_STORAGE_SCRIPT: &str = r#"
(function() {
  const key = '__codexDailyTokenUsageV1';
  const raw = localStorage.getItem(key);
  if (!raw) return { changed: false, reason: 'not found' };
  let data;
  try {
    data = JSON.parse(raw);
  } catch (e) {
    return { changed: false, reason: 'parse error' };
  }
  let changed = false;
  let cleanedTurns = 0;
  if (data.days) {
    for (const day of Object.values(data.days)) {
      if (!day.turns) continue;
      for (const turn of Object.values(day.turns)) {
        if (turn.model && typeof turn.model === 'string' && turn.model.includes('[')) {
          const original = turn.model;
          // 去掉尾部 [suffix]，保留 slug
          turn.model = turn.model.replace(/\[[^\]]+\]$/, '');
          if (turn.model !== original) {
            changed = true;
            cleanedTurns += 1;
          }
        }
      }
    }
  }
  if (changed) {
    localStorage.setItem(key, JSON.stringify(data));
  }
  return { changed, cleanedTurns };
})()
"#;

const CDP_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);

/// 通过 CDP 清理 Codex 页面 Local Storage 中残留的带后缀模型名。
/// 目前只处理 `__codexDailyTokenUsageV1`，这是 Electron 端记录每日
/// token 用量并回显到模型选择器的历史数据来源。
pub async fn sanitize_local_storage_model_suffixes(debug_port: u16) -> anyhow::Result<bool> {
    let targets = crate::cdp::list_targets(debug_port)
        .await
        .context("failed to list CDP targets")?;
    let target = crate::cdp::pick_page_target(&targets)?;
    let websocket_url = target
        .web_socket_debugger_url
        .as_deref()
        .context("CDP target missing websocket debugger URL")?;

    let result = tokio::time::timeout(
        CDP_OPERATION_TIMEOUT,
        crate::bridge::evaluate_script_with_await_promise(
            websocket_url,
            SANITIZE_LOCAL_STORAGE_SCRIPT,
            false,
        ),
    )
    .await
    .context("CDP localStorage sanitize timed out")?
    .context("CDP Runtime.evaluate failed")?;

    let changed = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("changed"))
        .and_then(|c| c.as_bool())
        .unwrap_or(false);

    let cleaned_turns = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("cleanedTurns"))
        .and_then(|c| c.as_i64())
        .unwrap_or(0);

    let _ = crate::diagnostic_log::append_diagnostic_log(
        "codex_local_storage.sanitize_model_suffixes",
        json!({
            "debug_port": debug_port,
            "changed": changed,
            "cleaned_turns": cleaned_turns,
        }),
    );

    Ok(changed)
}

/// 尝试通过 CDP 清理 Local Storage；失败时不抛出错误，仅记录诊断日志。
/// 适合在启动流程中作为最佳-effort 步骤调用。
pub async fn sanitize_local_storage_model_suffixes_nonfatal(debug_port: u16) {
    if let Err(error) = sanitize_local_storage_model_suffixes(debug_port).await {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "codex_local_storage.sanitize_model_suffixes_failed",
            json!({
                "debug_port": debug_port,
                "error": error.to_string(),
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::SANITIZE_LOCAL_STORAGE_SCRIPT;

    #[test]
    fn sanitize_script_targets_daily_token_usage_key() {
        assert!(SANITIZE_LOCAL_STORAGE_SCRIPT.contains("__codexDailyTokenUsageV1"));
        assert!(SANITIZE_LOCAL_STORAGE_SCRIPT.contains("localStorage.getItem"));
        assert!(SANITIZE_LOCAL_STORAGE_SCRIPT.contains("localStorage.setItem"));
    }

    #[test]
    fn sanitize_script_strips_trailing_suffix_from_model() {
        assert!(SANITIZE_LOCAL_STORAGE_SCRIPT.contains("turn.model"));
        assert!(SANITIZE_LOCAL_STORAGE_SCRIPT.contains("replace(/\\[[^\\]]+\\]$/"));
    }
}
