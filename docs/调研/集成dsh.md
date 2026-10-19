# PF 集成 dsh 插件调研

> 调研日期:2026-10-18。对象:dsh(deepseek-harness)`0.2.1-rc.2` @ `687f282e`
> (浅克隆 `D:\github\deepseek-harness`,上游 master 有 force update 属正常)。
> 需求演进:①PF 能否用 dsh 插件 → ②为何不直接用 dsh(否决平行引擎方案)→
> ③**硬前提:用户本机不装 dsh,集成 dsh core**。本文为终版结论,自包含,
> 可独立接续。姊妹篇:[dsh-插件框架调研](dsh-插件框架调研.md)(2026-10-12,
> cordis 机制层)。

## 0. 结论速览

一句话:**可行,dsh 官方桌面端已完整演示"用户零安装"模式**——分发树携带
dsh 生产运行时 + 保留 profile 目录 + 内置 pnpm。PF 照抄该模式:
`vendor/dsh` sidecar(镜像 vendored pi)+ 隔离 `dsh-home` + pi 扩展
`subagent-dsh`(ACP 驱动)。**pi 永远是唯一 agent/转录/UI,dsh 降格为
"插件宿主 + 执行器"**。

| 约束 | 答案 |
|---|---|
| dsh 插件能否直接在 PF/pi 里跑 | ❌ 物理不可能(见 §1) |
| 集成 dsh 插件是否必须装 dsh 宿主 | ✅ 是(无第三种宿主),但可由 PF 分发树携带 |
| UI 型插件(slot React 组件) | ❌ 任何方案都不可见(跨进程+跨 UI 框架物理边界) |
| 逻辑型插件(工具/命令/事件/服务) | ✅ 经 vendored dsh 全量可用 |
| 唯一实质代价 | 发行体积上涨(待实测)+ 钉版维护(dev preview 破坏性变更多) |

## 1. 为什么插件只能活在 dsh 里(已验证)

- dsh 插件 = cordis v4 插件(`@deepseek-ai/cordis 4.0.0-rc.7`),三形态
  `(ctx, config)=>any` / class / `{apply}`;`inject` 声明依赖服务,
  依赖 dsh 服务面(`ctx.tools`/`ctx.llm`/`ctx.sessions`/`ctx.agentLoop`…),
  **缺一个即 PENDING 不启动**。
- 自建最小 cordis 宿主 = 重写 dsh 服务面 = 重写半个 dsh → 否决。
- UI 面:插件 UI 为 Web 端 React slot 组件 + locale/icon 子路径导出
  (实测 `apps/web/tests/fixtures/plugins/fixture-bundle`),GPUI 无 React
  运行时;且 dsh acp profile **刻意不输出** DSH 专用呈现数据(原文:
  "刻意不提供 DSH 专用呈现数据与交互式 UI 功能")。

## 2. 被否决的方案(防止重走弯路)

| 方案 | 否决理由 |
|---|---|
| MCP 桥 | 不是跑插件本体,只是功能等价替代;保留为生态互济通道 |
| 按需移植单插件 → pi 扩展 | 可行的补充路径,但不满足"直接集成"需求 |
| PF 内嵌 cordis 自建宿主 | 服务面缺失,等于重写 dsh |
| **dsh 平行引擎**(PF 渲染 dsh 会话) | 残血壳;acp 刻意阉割呈现数据,官方桌面端全量更优 → "为何不直接用 dsh" 质疑成立,已否 |
| PF 换 cordis 宿主 | 推翻 vendored pi 铁律,PF 自杀 |

**关键转折**:平行引擎错在主从关系。正确形态 = **主从调换**:pi 主引擎
不换,dsh 以"进程外 subagent/工具执行器"身份接入——这正是 dsh 官方集成
Claude Code / Codex 的同一方式(subagent 家族原文:"由 ACP、Codex、
Claude Code 或另一 Harness 运行时支持的进程外子级"),互为镜像,双方协议
面都有预留接口。

## 3. 官方先例:dsh 桌面端的"零安装"模式(逐条核实)

来源:`apps/desktop/README.zh.md`(0.2.1-rc.2):

| 做法 | 原文证据 |
|---|---|
| 分发树携带运行时 | "在签名资源中**携带精确匹配的 dsh 生产运行时**" |
| 不依赖系统 Node/pnpm | "携带独立的 **Node.js 和 pnpm 分发包**…应用必须能够在**没有系统 Node.js 或 pnpm** 的机器上运行"(ELECTRON_RUN_AS_NODE) |
| 插件安装零 PATH 要求 | "在 `resources/runtime/primary-runtime/dependencies/pnpm` 中携带一份 pnpm 分发包…**包操作不要求 PATH 上存在 pnpm**" |
| home 隔离 | "拥有保留的 `$DSH_HOME/profiles/desktop`…与 npm CLI 共享产品数据,但**包、启用选择与锁文件保持独立**" |
| 许可 | MIT(根 LICENSE,Copyright (c) 2026 DeepSeek),vendored 合规 |

DSH_HOME 语义(`docs/cli-help.md`、`docs/config-catalog.md`):
`--profile <name>` 从 `$DSH_HOME/profiles` 加载;`DSH_HOME` 缺省 `~/.dsh`,
**可显式指定**——PF 隔离 home 的抓手。

## 4. PF 终版方案:vendored dsh sidecar + subagent-dsh

```
pi-flash 安装目录/
  vendor/pi/…          ← 现有 pi RPC 引擎(不动)
  vendor/dsh/          ← 新增:@deepseek-ai/dsh 钉版自包含树(npm ci/pnpm deploy)
  vendor/pnpm/         ← 新增:pnpm 分发包(官方桌面同款,插件安装用)
  vendor/subagent-dsh/ ← 新增:pi 扩展,spawn 指向本地 vendor/dsh

PI_FLASH_DIR/
  dsh-home/            ← DSH_HOME 注入(镜像 PI_FLASH_DIR 隔离思路)
    profiles/pf-acp/   ← PF 保留 profile(dsh 插件装这里,不碰用户 ~/.dsh)
    credentials.json   ← 由 pf_auth(051)注入,用户只填一次 key
```

运行链路(pi 唯一转录,体验 100% 保留):

```
pi 会话 ──工具 delegate_dsh(prompt)──▶ pi 扩展 subagent-dsh
    ──ACP v1 (JSON-RPC/stdio)──▶ dsh --profile pf-acp(vendored)
        ├─ dsh 插件 A(工具)      ✓ 执行
        ├─ dsh 插件 B(事件/服务)  ✓ 生效
        └─ cordis 宿主 + 服务面   (dsh 自带,PF 零实现)
    ◀──session/update 流式回流,权限请求透传 PF 审批对话框──
```

### 工程落点

| # | 落点 | 内容 | 量级 |
|---|---|---|---|
| 1 | `pi-link/vendor_dsh.rs` | 运行时定位(镜像 `vendor.rs`);`PI_FLASH_DSH_BIN` 覆盖(镜像 `PI_FLASH_PI_BIN`) | 小 |
| 2 | 首启 profile 初始化 | `dsh-home/profiles/pf-acp`(以 acp 为基)写入 | 小 |
| 3 | `dsh plugin` 封装 | `pnpmCommand` 指向内置 pnpm → 插件安装链路对用户零要求 | 小 |
| 4 | 凭据注入 | pf_auth → dsh credentials(dsh 支持 env/.env;headless 默认 env/.env 不读已存凭据,注入方式按 profile 定) | 小 |
| 5 | pi 扩展 subagent-dsh | ~300-500 行 TS:注册 `delegate_dsh`、ACP 客户端、update 流转发、权限透传 | 中 |
| 6 | 升级机制 | 照抄 pi:bump `vendor/dsh` + VERSION + pi-link 符合性测试 | 流程 |

### 阶段验收

| 阶段 | 内容 | 验收 |
|---|---|---|
| 0 | 实测 `npm install @deepseek-ai/dsh` 体积;headless 冒烟 | 体积判定分发可行性;装插件实例任务跑通 |
| 1 | `pi-link` ACP 协议层 + 测试 | initialize→prompt→update 单测全通 |
| 2 | subagent-dsh 扩展 + PF 审批透传 | PF 会话内 dsh 多轮对话、插件工具出卡 |
| 3 | 插件管理薄 UI(`dsh plugin` 回显)+ 引导页 | 装官方插件 → PF 会话立即可用 |
| 4(远期) | session/resume、多会话复用、Streamable HTTP MCP | 按需 |

## 5. 风险与边界

- **体积**(最大未知数):dsh 运行时 + 依赖树待实测;官方桌面连 Python
  都敢带,量级大概率可控,但这是立项第一关。
- **dev preview**:dsh 自述"未来将出现破坏兼容性的变更"→ 钉版隔离必须,
  升级走符合性测试(同 vendor pi 纪律)。
- **UI 型插件不可见**:物理边界,不在任何阶段承诺;远期若 PF 引入内嵌
  WebView 渲染才可能解锁(大工程,不立项)。
- **dsh 凭据独立**:PF 不代管 dsh 凭据体系,只做启动注入,职责切干净。
- **Node 前提**:PF 现架构已依赖系统 Node(vendored pi),vendored dsh
  不新增前提;官方桌面"免 Node"方案(ELECTRON_RUN_AS_NODE)PF 用不上
  也无需用。

## 6. 待办

- [ ] 阶段 0:体积实测 + headless 冒烟(下一步)
- [ ] bd 立项(挂本文为依据)
- [ ] AGENTS.md 增补 vendored dsh 铁律条目(立项后)
