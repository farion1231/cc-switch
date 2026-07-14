//! 手工验证工具：从命令行参数生成 catalog JSON。
//! 用法：
//!   cargo run -p codex-plus-core --example generate_model_catalog -- \
//!       "deepseek-v4-pro[1M]" "claude-sonnet-4[200K]" > catalog.json

use codex_plus_core::model_suffix::{
    build_model_catalog_json, collect_catalog_entries, migrate_model_list_with_suffixes,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let raw_list = args.join("\n");
    let (model_list, model_windows) = migrate_model_list_with_suffixes(&raw_list);
    let entries = collect_catalog_entries(&model_list, &model_windows, "");
    print!("{}", build_model_catalog_json(&entries, None));
}
