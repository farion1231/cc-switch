//! 临时 SSH Agent 的独立进程入口。
//!
//! 入口只负责采集参数并返回稳定退出码，会话与业务逻辑位于可测试的库边界中。

fn main() {
    let exit_code = cc_switch_agent::run_cli(std::env::args().skip(1));
    std::process::exit(exit_code);
}
