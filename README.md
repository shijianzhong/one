# ONE - 轻量级 AI 智能体

**ONE** 是一个基于 Rust + [GPUI](https://github.com/zed-industries/zed) 构建的轻量级 AI 智能体。不同于市面上的 Electron/VSCode 二开方案，它从零开始拥抱 Rust，给你一个真正轻快、无负担的编程体验。

---

## 核心理念

**ONE = 一 = 连接万物的那个点。**

我们相信，未来不会有一堆乱七八糟的 AI 应用。真正的智能体应该是：
- **一个入口** — 通过 `1` 连接一切，而不是在无数应用间切换
- **万物归一** — 所有工具、数据、记忆都围绕同一个「1」运转
- **轻装上阵** — 不是在笨重的 Electron 上套壳，而是从零打造真正轻快的体验

当你需要任何 AI 能力时，你只打开 ONE。它会帮你连接一切，完成一切。

---

## 设计哲学

```
        ┌─────────┐
        │   ONE   │  ← 万物归一的起点
        └────┬────┘
             │
    ┌────────┼────────┐
    │        │        │
    ▼        ▼        ▼
┌─────┐  ┌─────┐  ┌─────┐
│Chat │  │Work │  │Term │
│     │  │Space│  │inal │
└─────┘  └─────┘  └─────┘
    │        │        │
    └────────┼────────┘
             │
        ┌────▼────┐
        │ Memory  │
        │ (三层)  │
        └─────────┘
```

不是做另一个应用，而是做**所有应用的连接器**。你只需要一个 ONE。

---

## 与主流方案对比

| 维度 | Electron/VSCode 二开方案 | ONE |
|------|-------------------------|-----|
| **技术栈** | JavaScript + Node.js runtime | Rust + GPUI (原生) |
| **内存占用** | 300MB - 1GB+ | 预计 50-100MB |
| **启动速度** | 5-15 秒 | < 1 秒 |
| **包体积** | 数百 MB | ~10MB |
| **定制成本** | 低，但受限于 Electron 架构 | 高，但完全可控 |
| **UI 渲染** | WebView + CSS | GPU 加速原生渲染 |
| **跨平台** | 一套代码，多平台打包 | 原生跨平台 (macOS/Linux/Win) |

---

## 核心优势

### 1. 真正的原生体验
基于 GPUI (Zed 编辑器的 GUI 框架)，UI 响应即时，没有 WebView 的渲染开销。

### 2. 三层记忆架构
```
┌─────────────────────────────────────────────────────┐
│  L1 工作层 ── 当前会话全文 (Context window)         │
│       ↓                                             │
│  L2 摘要层 ── 结构化 YAML/MD 快照 (会话结束写入)     │
│       ↓                                             │
│  L3 语义层 ── 历史片段 embedding (异步批量写入)      │
└─────────────────────────────────────────────────────┘
```
新会话开始时：L3 向量召回 → L2 用户画像 → 合并注入 system prompt → L1 开始对话。参考 Mem0、LangMem 等主流记忆框架的生产级架构。

### 3. Chat + Workspace + Terminal 三位一体
- **Workspace 管理**：以文件夹为单位组织任务
- **Task 追踪**：结构化任务管理
- **内置终端**：集成 alacritty，支持 Docker sandbox（可选）

### 4. 可插拔的模型服务
支持配置任意 OpenAI-compatible API endpoint，灵活切换模型。

---

## 功能一览

- [x] Chat 界面与消息流
- [x] Workspace / Task 管理（SQLite 持久化）
- [x] 内置终端（alacritty）
- [x] 模型配置对话框
- [x] 三层记忆架构（L1/L2/L3）
- [x] 可选 Docker sandbox 后端
- [x] 跨平台支持（macOS/Linux）

---

## 构建与运行

```bash
# 构建
cargo build

# 运行
cargo run

# 构建特定 crate
cargo build -p one_components

# 清量级重建
cargo clean && cargo build
```

---

## 项目结构

```
one_gpui/
├── Cargo.toml
├── src/
│   ├── main.rs          # AppState、渲染、事件处理
│   ├── memory/          # 三层记忆管理 (types, storage, search, snapshot)
│   ├── sandbox/         # 终端后端 (Pty + Docker 可选)
│   └── services/        # API 调用、配置加载
└── components/          # 可复用 GPUI 组件
    └── src/
        ├── text_input.rs  # 实体文本输入
        ├── button.rs      # 按钮组件
        ├── checkbox.rs    # 复选框
        └── traits/        # 组件状态与 Trait 定义
```

---

## 技术选型

| 组件 | 技术 |
|------|------|
| GUI 框架 | GPUI (Zed) |
| 语言 | Rust |
| 数据库 | SQLite (via sqlez) |
| 终端 | alacritty_terminal + portable-pty |
| HTTP | reqwest |
| 异步 | tokio |

---

## Roadmap

- [ ] Windows 支持
- [ ] 向量数据库集成（L3 语义层）
- [ ] 更多记忆召回策略
- [ ] 插件系统
- [ ] 主题定制

---

## License

MIT