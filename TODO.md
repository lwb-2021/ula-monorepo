# TODO

实现级待办，全部不带行号（随重构移动，位置自己搜）。上下文管理的**设计**已定稿，见 `AGENTS.md`。

## 上下文管理（设计已定，待实现）

- [ ] 协议：握手 `Meta` 加 `capabilities`；同一能力多声明 = 启动报冲突
- [ ] 协议：`EventFromCore::ContextFull`（携带估算 tokens）
- [ ] 协议：`Fork { from: Genesis | Window(id), seed }` 原语；`UISend`/`UIReceive` 补 fork 变体（UI 也要能 fork）
- [ ] core：config 加 `context_limit`（默认 0.8）；`context_window` 走 `{base_url}/models/{model}` 的 `context_length` 自动探测，配置兜底
- [ ] core：请求前 token 估算与刹车；Usage 落地前用字符/4 估算
- [ ] core：Session 演进为「活动窗口 + rpds 存档窗口表」，fork = O(1) Arc clone
- [ ] compose：ContextFull 定向路由给声明 `context` 能力的 plugin（仲裁收敛成一个函数，留给将来的插件管理器）
- [ ] ptc：压缩编排（head 摘要 / tail 原文；超限过多分块 map-reduce；压缩轮是内部轮）
- [ ] tui：从历史窗口 fork 的 UI
- [ ] 无 context 能力的 plugin = 纯卡，不做降级截断（已定，不实现）

## 功能缺口

- [ ] Usage：请求加 `stream_options.include_usage` 并统计；现状 `ModelResponse::Usage` 只 `warn!("TODO: usage")`
- [ ] `finish_reason` 字段从未读取，`ModelResponse::End` 只 `warn!("TODO: end")`
- [ ] `Session::save` 是桩（`warn + Ok`），持久化格式未定（依赖窗口表结构先定）
- [ ] `FullConversation` 预留未消费：等 session 恢复（协议已注释）
- [ ] 动态提示词：只在开机首帧注入一次，运行期改提示词没有通道
- [ ] TUI 内容零样式区分：代码块 / `ula-ptc` 块与正文同样式，无高亮
- [ ] TUI 中断 leader 只认双 Esc（800ms 窗口），其余组合无
- [ ] 接近稳定后补测试：重点是 pipe 协议的集成面（握手、权限、fork、超时）；不加 CI

## 设计方向（未实现，方向已定）

- 砍掉原生 bash 工具，bash 降级为 Lua 内核内的一个效果原语。命令面 = 原语语法（`pty "command"`：首词原语名 + 空格参数，无括号）；计算内核 = Lua subset（不加载 io/os/package/debug，实现语言级无 IO 的能力剥夺；rlimit 防资源炸弹；seccomp 退为可选保险而非主防线）。yield 协议：宿主将模型 chunk 包进协程执行，原语函数内部 `coroutine.yield`，对模型完全透明；宿主在 yield 边界做审批——能力剥夺优先于静态分析。
- 文件修改走声明式补丁原语 `apply_patch`：内容寻址（锚点 = 唯一文本片段/符号，不用行号）；接受 search/replace 块与 sed 方言（`s/old/new/g`）两种表达——sed 形态是模型的表达层，宿主解析后统一走「锚点匹配 + 审批 + 失败反馈（找不到锚点 → 返回相近内容）」；行号与 unified diff 不进模型接口（仅作宿主与 git 的内部格式）。随 Lua 内核落地。
- 多行文本主通道是 Lua 长字符串 `[[...]]`：内容含 `]]` 需升级层级 `[=[...]=]`（模型易忘，报错隐晦）；5.2+ 忽略开头的换行但保留内容缩进（无 heredoc 的 `<<-` 剥离）。插值缺失（字符串不能展开变量）由"模型先算后写"纪律弥补。
