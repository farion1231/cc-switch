# CC Switch 工程经验

1. **测试写到哪里，就从哪里读。** 涉及本机配置时，夹具写入、配置读取和快照检查使用同一个显式指定的临时目录，并行测试进程各用独立目录。 [#8567](https://github.com/farion1231/cc-switch/pull/8567) · [#10023](https://github.com/farion1231/cc-switch/pull/10023) · [#9752](https://github.com/farion1231/cc-switch/pull/9752)
2. **写入失败时，原数据还要在。** 替换配置前先验证新内容能读回，旧配置读不出来时保留原件，需要回读确认的数据库更新把写入和回读放在同一事务里。 [#10002](https://github.com/farion1231/cc-switch/pull/10002) · [#10020](https://github.com/farion1231/cc-switch/pull/10020) · [#9767](https://github.com/farion1231/cc-switch/pull/9767)
3. **协议转换按接收方的格式检查。** 转换工具定义和消息时，按目标协议校验字段与顺序，并保留工具调用与结果的对应关系。 [#8659](https://github.com/farion1231/cc-switch/pull/8659) · [#7023](https://github.com/farion1231/cc-switch/pull/7023) · [#8825](https://github.com/farion1231/cc-switch/pull/8825)
4. **同一请求只记一次用量。** 合并用量时用可追踪的记录标识核对去重，并验证新旧格式并存和归档重读时的汇总。 [#9249](https://github.com/farion1231/cc-switch/pull/9249) · [#9527](https://github.com/farion1231/cc-switch/pull/9527) · [#8363](https://github.com/farion1231/cc-switch/pull/8363)
5. **旧任务完成时，先看状态有没有变。** 排队任务开始执行或写回结果前，重新核对当前配置、会话或版本，避免旧任务覆盖用户的新操作。 [#9528](https://github.com/farion1231/cc-switch/pull/9528) · [#9593](https://github.com/farion1231/cc-switch/pull/9593) · [#9946](https://github.com/farion1231/cc-switch/pull/9946)
