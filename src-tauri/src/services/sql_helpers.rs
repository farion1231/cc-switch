//! 桌面 Usage SQL 语义由 cc-switch-core 统一维护；本模块仅保留既有 crate 内导入路径。

#[cfg(test)]
pub(crate) use cc_switch_core::INPUT_TOKEN_SEMANTICS_LEGACY;
pub(crate) use cc_switch_core::{
    fresh_input_sql, is_cache_inclusive_app, INPUT_TOKEN_SEMANTICS_FRESH,
    INPUT_TOKEN_SEMANTICS_TOTAL,
};

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn shared_fresh_input_sql_keeps_desktop_semantics() {
        let connection = Connection::open_in_memory().expect("创建内存数据库");
        connection
            .execute_batch(
                "CREATE TABLE proxy_request_logs (
                    app_type TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL,
                    cache_read_tokens INTEGER NOT NULL,
                    cache_creation_tokens INTEGER NOT NULL,
                    input_token_semantics INTEGER NOT NULL
                 );
                 INSERT INTO proxy_request_logs VALUES ('codex', 1000, 300, 200, 1);
                 INSERT INTO proxy_request_logs VALUES ('claude', 100, 900, 0, 2);",
            )
            .expect("写入 token fixture");
        let expression = fresh_input_sql("l");
        let sql = format!("SELECT SUM({expression}) FROM proxy_request_logs l");
        let total: i64 = connection
            .query_row(&sql, [], |row| row.get(0))
            .expect("计算 fresh input");
        assert_eq!(total, 600);
        assert!(is_cache_inclusive_app("codex"));
        assert!(!is_cache_inclusive_app("claude"));
        assert_eq!(INPUT_TOKEN_SEMANTICS_LEGACY, 0);
    }
}
