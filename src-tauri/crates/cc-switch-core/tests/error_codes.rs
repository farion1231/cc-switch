use std::io::ErrorKind;
use std::path::PathBuf;

use cc_switch_core::CoreError;

#[test]
fn permission_denied_io_uses_remote_permission_code() {
    // 远端 live 文件与会话目录都可能受 Unix 权限限制；协议层需要可操作的稳定码，
    // 不能让 UI 依赖平台相关的原始 IO 文案。
    let error = CoreError::Io {
        path: PathBuf::from("/root/.claude/settings.json"),
        source: std::io::Error::new(ErrorKind::PermissionDenied, "fixture denied"),
    };

    assert_eq!(error.code(), "REMOTE_PERMISSION_DENIED");
}
