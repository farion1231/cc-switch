//! 兼容期残留清理：旧版整份写入时留在 live 里、而且留下来有害的窗口值。
//!
//! 窗口类的键是供应商独有字段，正常情况下切走时按「值相同才删」清掉。有两类残留证明
//! 不了是上一家带进来的，那条规则删不掉：
//! - 旧版在写入时注入的默认值（早期的 Kimi、Codex OAuth 行里没有这些键）；
//! - 兼容期内旧 CLI、Lite、旧 GUI 整份写入的值，新版记录的上一家未必是它们切到的那家。
//!
//! 这些值比下一家的真实窗口大，留着就会超窗。这里冻结的是 CC Switch 自己下发过的
//! （键，值）对：重构时刻的预设值和注入常量。每次直连投影时删掉 live 里精确命中的项，
//! 目标供应商自己要写的键除外（由目标值原位覆盖）。
//!
//! 超时、遥测开关不收：残留下来无害，而且用户很可能照厂商文档把它们设成全局。

use serde_json::Value;

/// Claude Code `env` 里的残留（键，CC Switch 下发过的值）。
pub const CLAUDE_RESIDUE_ENV: &[(&str, &[&str])] = &[
    (
        "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        &["262144", "372000", "983616"],
    ),
    (
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
        &["262144", "372000", "1000000"],
    ),
    ("CLAUDE_CODE_MAX_OUTPUT_TOKENS", &["131072"]),
];

/// 一个残留值在 JSON 里可能的写法：预设写字符串，个别旧行写成数字。
pub fn residue_values(values: &[&str]) -> Vec<Value> {
    values
        .iter()
        .flat_map(|value| {
            let number = value.parse::<u64>().ok().map(Value::from);
            std::iter::once(Value::String((*value).to_string())).chain(number)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::floor;
    use serde_json::json;

    #[test]
    fn residue_keys_are_provider_exclusive_fields() {
        for (key, _) in CLAUDE_RESIDUE_ENV {
            assert!(floor::CLAUDE_EXCLUSIVE_ENV.contains(key), "{key}");
        }
        let pairs: usize = CLAUDE_RESIDUE_ENV
            .iter()
            .map(|(_, values)| values.len())
            .sum();
        assert_eq!(pairs, 7, "the list is frozen");
    }

    #[test]
    fn residue_matches_string_and_number_spellings() {
        assert_eq!(
            residue_values(&["131072"]),
            vec![json!("131072"), json!(131072)]
        );
    }
}
