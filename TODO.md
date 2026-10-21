# TODO

实现级待办，不带行号（位置自己搜）。设计见 `AGENTS.md`。

## 上下文管理

- [ ] 协议：`Fork { from: Genesis | Window(id), seed }` 原语；`UISend`/`UIReceive` 补 fork 变体
- [ ] core：`context_window` 自动探测（现只读配置，未配置即不刹车）
- [ ] core：Session 演进为「活动窗口 + rpds 存档窗口表」
- [ ] 压缩编排 plugin（head 摘要 / tail 原文；超限过多分块 map-reduce）——目前无插件声明 `ContextManager`，刹车后会纯卡
- [ ] tui：从历史窗口 fork 的 UI
- [ ] UI 主动发起压缩（斜杠命令）：**待定**命令归属——放 UI 则 core 全程不知情（UI 请 compose 走能力路由，摘要由 compose 推 `EventToCore::Compact`，core 需能接受无约而至的摘要）；放 core 则复用现有 `CompactRequest` 路径，代价是 core 要解析用户输入

## 插件管理器（已接线）

- [ ] github 插件更新：命中已安装目录即复用，不 re-clone、不跟踪上游（update 语义未定）
- [ ] 本地插件的配置级 manifest 路径语法：现必须装进 XDG 数据目录或走 builtin，配置无法直接指 manifest

## 配置编辑与 CLI（方向已定，未实现）

- [ ] 配置编辑协议：compose 是唯一写者（UI→compose 转发，不给 UI 直接写盘、不拆 config-manager），三消息 `UIReceive::Config`（compose 把配置降维成通用可编辑视图：section → key-value/字符串列表）/ `UISend::ConfigEdit`（SetValue / AddItem / RemoveItem）/ `UIReceive::ConfigResult { ok, restart_required }`；schema 只在 compose，UI 哑渲染，不 import 配置类型
- [ ] 编辑面只有 `agents` 和 `plugins` 两项；`interface` 改动必须重启，不进编辑面
- [ ] 写盘用 `toml_edit`，保用户注释
- [ ] compose 引入 `dirs`，走 XDG：配置在 `$XDG_CONFIG_HOME/ula/config.toml`，运行期目录（socket、日志）在 `$XDG_RUNTIME_DIR/ula/`
- [ ] 默认配置首次自动安装：模板随二进制带（同 builtin 工具的 rust_embed 玩法），首次启动铺到配置目录，之后运行期只读磁盘文件——不做 `include_str!` 内存兜底，磁盘文件即唯一事实源
- [ ] CLI wrapper 二进制（暂名 `ula`）：子命令选 interface，如 `ula tui` = 生成配置（`interface=tui`）+ exec compose；不做 daemon 自动拉起、不让用户选后端。**未定**：配置缺失时才生成 vs 每次覆盖 `interface` 键（覆盖会冲掉用户手改的 agents/plugins）

## 功能缺口

- [ ] 错误栈收敛为 thiserror + eyre 两套：compose 的 snafu → thiserror、ptc 手写 `Error{Lua}` → thiserror、全仓 anyhow → eyre（当前 core/compose/tui/ptc 四套并存）
- [ ] compose 日志文件挪到与 UDS 同一 run 目录（消掉硬编码 `/tmp/ula.log`）；tui 保持不写 stderr
- [ ] 请求未带 `stream_options.include_usage`：usage 目前依赖端点无条件回传，不回传就没法刹车
- [ ] `finish_reason` 字段从未读取，`ModelResponse::End` 只 `warn!("TODO: end")`
- [ ] `Session::save` 是桩（`warn + Ok`），持久化格式未定（依赖窗口表结构先定）
- [ ] `FullConversation` 预留未消费：等 session 恢复（协议已注释）
- [ ] 动态提示词：只在开机首帧注入一次，运行期改提示词没有通道
- [ ] TUI 内容零样式区分：代码块 / `ula-ptc` 块与正文同样式，无高亮
- [ ] TUI 中断 leader 只认双 Esc（800ms 窗口），其余组合无
- [ ] Interrupt 不打断插件调用：工具执行中/权限等待中的 Esc 只到 core，compose 不转发给插件（需协议：compose 拦截 Interrupt 并给活跃插件发 `PluginRequestResponse::Interrupt` 或新的下行变体）
- [ ] 接近稳定后补测试：重点是 pipe 协议的集成面（握手、权限、fork、超时）；不加 CI

## 设计方向（未实现，方向已定）

- 砍掉原生 bash 工具，bash 降级为 Lua 内核内的一个效果原语。命令面 = 原语语法（`pty "command"`：首词原语名 + 空格参数，无括号）；计算内核 = Lua subset（不加载 io/os/package/debug，实现语言级无 IO 的能力剥夺；rlimit 防资源炸弹；seccomp 退为可选保险而非主防线）。yield 协议：宿主将模型 chunk 包进协程执行，原语函数内部 `coroutine.yield`，对模型完全透明；宿主在 yield 边界做审批——能力剥夺优先于静态分析。
- 文件修改走声明式补丁原语 `apply_patch`：内容寻址（锚点 = 唯一文本片段/符号，不用行号）；接受 search/replace 块与 sed 方言（`s/old/new/g`）两种表达——sed 形态是模型的表达层，宿主解析后统一走「锚点匹配 + 审批 + 失败反馈（找不到锚点 → 返回相近内容）」；行号与 unified diff 不进模型接口（仅作宿主与 git 的内部格式）。随 Lua 内核落地。
- 多行文本主通道是 Lua 长字符串 `[[...]]`：内容含 `]]` 需升级层级 `[=[...]=]`（模型易忘，报错隐晦）；5.2+ 忽略开头的换行但保留内容缩进（无 heredoc 的 `<<-` 剥离）。插值缺失（字符串不能展开变量）由"模型先算后写"纪律弥补。
