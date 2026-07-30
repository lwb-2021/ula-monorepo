# AGENTS.md

## 第一性原理

- **模型是主体，脚手架是配角**：工具面越小，模型越自由、行为越可预测。脚手架只做解析与执行，不替模型决策。
- **顺模型而非逆模型**：协议贴合模型的实际输出分布，而不是让模型适应我们的格式。提示词措辞不搞严格约束。
- **系统简单性优先**：少抽象、代码自解释、不过度设计。能靠自由度解决的问题，不靠机制解决。

## Unix 哲学原则

本项目严格遵循 Unix 哲学：

1. **管道优于 ABI**：组件间通信用 pipe（stdin/stdout）而非 unsafe FFI。子进程天然隔离，崩溃不影响主进程。

2. **文本流优于二进制**：所有 IPC 消息使用 JSON 文本流。便于调试、rg、curl 测试、跨语言调用。

3. **每个模块做好一件事**：Core 只管对话，Plugin 只管工具调用，Compose 只管路由，UI 只管显示和输入。

4. **同步优于异步**：能用 blocking IO 解决的问题不用 async。只有两处例外——Compose 要同时守多条 pipe/UDS，用 `smol` 单线程事件循环；TUI 要同时守 crossterm 事件流与 UDS，用 `tokio`。Core 与 ptc 保持纯 blocking，不引入重型 runtime。

5. **子进程隔离能力**：危险操作（bash 执行等）放在 Plugin 子进程中，用 OS 进程边界做安全隔离。

## 模块职责

| 模块 | 职责 |
|------|------|
| **ula-core** | 负责 LLM 对话。输入用户消息，输出 `EventFromCore`（Streaming/Content/Error）。不处理 UI，不处理 Plugin。 |
| **ula-ptc** | 负责工具调用等流程。接收 Core 的 Content，自行解析 `ula-ptc` 代码块，在 Luau 运行时中执行，通过 `coroutine.yield` 发起工具调用，输出 `PluginOutput`。 |
| **ula-compose** | 负责传输信息。路由 Core 输出到 Plugin，把 Plugin 输出发回给 Core；启动时先收插件开机首帧聚合提示词；串行化权限提示。 |
| **ula-tui** | 负责显示和发送用户消息。通过 UDS 与 Compose 通信，权限请求弹 modal。 |

## 架构概览

```
┌──────────────────────────────────────────────────────────────────┐
│                         ula-compose                               │
│                     (传输信息，路由消息)                          │
│                                                                  │
│  stdin/stdout                     stdin/stdout                     │
│       │                               │                           │
│  ┌────▼────┐                   ┌──────▼──────┐                  │
│  │  Core   │ ◄─────────────────► │   Plugin    │                  │
│  └────▲────┘                     └─────────────┘                  │
│       │                                                     │
│  stderr (日志)                                              │
│       │                                                     │
│  stdout (EventFromCore)                                      │
│       │                                                     │
│  IPC (UDS) ──────────────────────────────────────────────► │
│                                                          │     │
│                                                   ┌───────▼────┐│
│                                                   │   UI       ││
│                                                   │ (WebUI/TUI)││
│                                                   └────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

## 执行流程

```
启动：Compose 读配置 → 拉起 Core → 收插件开机首帧聚合提示词 → 拉起 UI
  Plugin ──首帧 PluginMeta{system_prompt}──► Compose（插件自推，无请求）
  Compose ──UserMessage::SystemPrompt──► Core

对话：
用户输入 → UI ──UDS──► Compose
                              │
                              ▼
                       ┌──────────────┐
                       │   Core       │
                       │ stdin/stdout │
                       └──────┬───────┘
                              │
                              ▼
                       Core 输出 EventFromCore
                              │
                    ┌─────────┼─────────┬──────────────┐
                    ▼         ▼         ▼              ▼
              Streaming    Content     Error      FullConversation
              (UDS → UI)  (整轮原文   (UDS → UI；  (预留：session 恢复/
                           → Plugin)   core 挂起    上下文管理，暂不消费)
                                        等下一条消息)
                                        │
                                        ▼
                                 ┌──────────────┐
                                 │   Plugin      │
                                 │ stdin/stdout │
                                 └──────┬───────┘
                                        │
                                        ▼
                     SendMessage{result} ──► UserMessage::Text 回灌 Core
                     Idle                  ──► 丢弃（不回灌，避免死循环）
                     Request{…}            ──► 权限提示，见下

权限流（同一时刻至多一个提示）：
  Plugin ──PluginOutput::Request──► Compose prompter
  Compose ──UIReceive::Request{id,type,message}──► UI（modal）
  UI ──UISend::RequestResult{id,allow,reason}──► Compose
  Compose ──PluginRequestResponse──► Plugin（Allowed / Rejected{reason}）
```

超时语义：开机首帧（`PluginMeta`）整体 10s；插件调用中每行 IO 120s（插件不再说话才算死）；**权限等待不计时**——用户离开工位不应导致插件被重启。UI 断连走全局 shutdown。

## IPC 设计

| 连接 | 方式 | 协议 |
|------|------|------|
| Core → Compose | pipe | `EventFromCore` (JSON) |
| Compose → Core | pipe | `UserMessage` (JSON) |
| Compose ↔ Plugin | pipe | `PluginMeta`（开机首帧）/ `PluginInput` / `PluginOutput` (JSON) |
| UI ↔ Compose | UDS | `UISend` / `UIReceive` (JSON) |

### 协议类型 (ula-protocol)

- `EventFromCore`: Streaming / Content / Error / FullConversation（预留）
- `UserMessage`: Text / SystemPrompt / Interrupt
- `PluginInput`: Message（首帧之后的下行）
- `PluginOutput`: SendMessage / Idle / Request（首帧不上行，`PluginMeta` 独立于流外）
- `PluginMeta`: 开机首帧，携带 `system_prompt: Option<String>`，插件自推
- `PluginRequestResponse`: Allowed / Rejected{reason} / Interrupt
- `UISend`: UserMessage / RequestResult / Interrupt
- `UIReceive`: Stream / Content / Error / Request

协议变体是封闭枚举，新增变体 = 所有进程必须同步升级（plugin 侧解析失败即退出）。加变体前想清楚广播面：定向能力路由优于广播。

**Wire 约定**：所有协议枚举统一内部标签 `#[serde(tag = "type", rename_all = "snake_case")]`，wire 形如 `{"type":"text","text":"..."}`；struct 变体优先，newtype 变体内必须是 struct（serde 对「内部标签 + 字符串 newtype」在运行期直接报错）。派生统一 `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`。
## 上下文管理（设计定稿，未实现）

1. **刹车在 core**：core 在发请求前估算将发的 messages，达到 `context_limit`（配置，默认 0.8）即停止放行，发 `EventFromCore::ContextFull`（新增）。
2. **分母自动探测**：`context_window` 优先从 OpenAI 兼容端点 `{base_url}/models/{model}` 的 `context_length` 自动识别，配置兜底；未探测到且未配置 = 不刹车。
3. **能力路由，不广播**：plugin 在握手 `Meta` 中声明 `capabilities`；core 的 ContextFull 只路由给声明 `context` 能力的 plugin。**同一能力被多个 plugin 声明 = 启动即报冲突**，不做静默仲裁（仲裁函数留给将来的插件管理器）。
4. **单原语 Fork**：`Fork { from: Genesis | Window(id), seed }`。开新上下文 = `from: Genesis` 的退化情形；`seed` 装压缩指令，旧内容不走 IPC（core 手里的 Conversation 才是源数据）。UI 侧同样需要 fork（从历史窗口分叉），锚点语义进 `UISend`/`UIReceive`。
5. **窗口数据结构**：`Conversation` 基于 rpds 持久化结构，fork = O(1) Arc clone；Session 从单 conversation 演进为「活动窗口 + 存档窗口表」，同一时刻只有一个活动窗口。
6. **系统提示词铁律**：`ConversationItem::System` 是占位符，构建请求时物化 `SYSTEM_PROMPT` 单例——新窗口自动携带当前提示词，协议载荷不带系统提示词。`SYSTEM_PROMPT` 只在启动握手时聚合一次、之后 append-only（`rcu` 追加），**fork 永不重新聚合、永不重新握手**，否则提示词翻倍。
7. **摘要策略**：head 做摘要、tail 保原文（最近若干轮原样保留，模型刚看过、细节最活）；system prompt 与硬规则每窗口重新注入，不进摘要。压缩量过大时分块 map-reduce，循环由 plugin 编排（plugin 的协程状态机天然适合多轮循环），core 只提供 Fork 与刹车。
8. **压缩轮是内部轮**：不以普通聊天形态渲染进 UI（需要事件标记或复用 FullConversation 语义）。
9. **无能力 plugin = 纯卡**：core 刹车后没人回 Fork 就一直等。用户不装上下文管理，假定他不想要（不做降级截断兜底）。
10. **计数**：以真实 `Usage`（`stream_options.include_usage`）为准。字符估算已否决——对中文对话低估约 4 倍，刹车永远不会触发。Usage 落地前不实现计数，只留 `warn!("TODO: usage")` 占位。

## 设计约定

- **单一工具通道，封闭能力原语面**：不新增 edit 等任务专用工具。模型自由度来自封闭能力原语的开放组合。
- **伪标签协议** ```ula-ptc：模型训练集里不存在的标签，只能照着 `prompt.md` 示例逐字模仿。解析在 Plugin 内做，Core/Compose 都不理解标签。
- **插件提示词注入**：Plugin 开机自推首帧 `PluginMeta{system_prompt}`（可为 `None`），Compose 聚合后经 `UserMessage::SystemPrompt` 注入 `SYSTEM_PROMPT`（`ArcSwap`）。Core 自身的 `system-prompt.md` 只写通用规则，不写工具调用文档。注入只在启动时发生一次。
- **不宣传未实现能力**：提示词与文档只写已实现的功能。
- **请求错误不杀进程**：API 错误就地处理——丢弃 provider、发 `EventFromCore::Error` 到 UI、挂起等用户下一条消息；只有真·致命才让 core 退出。
- **错误栈两套封顶**：`thiserror` 写域错误枚举（结构化字段，含 compose 的 plugin 模块），`eyre` 写边界与顶层；不再引入 anyhow/snafu。
- **日志只留一个出口**：stdout 全部留给数据流。compose 写日志文件（与 UDS 同一 run 目录），core/plugin 的 stderr 被 compose 管道收拢后打 tag 进同一文件；tui 在终端里画 inline viewport，不许往 stderr 写——任何直写终端的进程都会把 TUI 画布打穿。
- **插件元数据分层**：能力与启动方式走插件仓库根的 `ula-plugin.toml`（`name`/`provides`/`exec`/可选 `install`，语言无关），compose 配置只回答「装了什么、从哪来」（source 字符串）——单一事实源在插件自身，配置不复述能力；开机首帧只带运行期贡献（system prompt）。compose 配置格式同为 TOML。
- **插件语言无关**：插件 = 一条可执行命令 + stdin/stdout JSON-lines。插件管理器嵌在 compose，不内建任何语言工具链：编译型交二进制，脚本型走 `exec`/shebang，装依赖最多转发一条插件声明的 install 命令。

## 代码风格

- 代码与注释一律使用英文；其余场合使用简体中文。
- 代码自解释，尽量少写注释。
- 优先简洁直接的实现。
- Rust 代码偏向函数式风格：迭代器/链式调用、模式匹配、`Result`/`Option` 组合子。
- **错误 context 分层**：域错误（结构化字段）不加 context，信息已在字段里；IO/解析边界必须加（`eyre::WrapErr` 的 `.context()`——配置文件读取、逐行 JSON 解析、管道 IO，说清楚是哪个输入/哪条报文失败）。unwrap/expect 必须写明不变量。

## 关键文件

| 文件 | 说明 |
|------|------|
| `crates/ula-core/src/` | LLM 调用逻辑 |
| `crates/ula-compose/src/` | 进程编排、消息路由（单线程 smol 事件循环）|
| `crates/ula-tui/src/` | UI 实现 |
| `crates/ula-ptc/` | Plugin 实现（Luau 工具调用，通过 stdin/stdout 连接 Compose）|
| `crates/ula-protocol/src/` | 协议类型定义 |

## Pitfalls

- **提示词是编译期内嵌的**：`system-prompt.md`、插件的 `prompt.md` 均经 `include_str!` 编译进各自二进制；运行期提示词只增不减（`ArcSwap::rcu`）。见上下文管理铁律。
- **`Terminal::insert_before` 不做宽字符感知**：它逐格发单元格，CJK/emoji 后的续格会留下空隙，必须手动抹掉（见 TUI flush 的 hide_wide_trailing 处理）。
- **封闭枚举 = 一损俱损**：给 pipe 上的 JSON 协议加变体，两端必须同步升级；老进程解析未知变体会直接失败。

## 维护要求

- 新增设计决策、协议变更：更新到本文档。
- **文档只写契约与决策**：不写行号、不写文件内的定位、不复述实现细节——这些会随重构腐烂。实现级待办只进 `TODO.md`，且同样不带行号。
- 架构细节以代码为准，本文不承诺描述的实时性。
