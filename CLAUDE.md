# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**one** is a Rust GUI application built on GPUI (Zed editor's GUI framework) for the ONE project. It provides a chat-based UI with workspace/task management, terminal integration, and model service configuration.

## Build & Run Commands

```bash
# Build the project
cargo build

# Run the application
cargo run

# Build a specific crate
cargo build -p one_components

# Clean and rebuild
cargo clean && cargo build
```

## Architecture

### GPUI Framework
This project depends on GPUI from the Zed editor (`/Users/shijianzhong/sking/zed/crates/gpui`). GPUI is a Rust-native immediate mode GUI framework with:
- **Stateful components**: Use `cx.listener()` for event handlers that mutate state
- **Render trait**: Components implement `Render` to produce UI elements
- **Context system**: `Context<AppState>` provides access to app state and notifications
- **Entity system**: GPUI uses `Entity<T>` for managed state lifetimes
- **Event handling**: `on_mouse_down`, `on_click`, `on_drag` patterns for user input

### Key Traits
- `Render` - Produces UI elements via `into_element()`
- `Styled` - CSS-like styling via builder pattern (`.bg()`, `.text_color()`, etc.)
- `InteractiveElement` - Mouse/keyboard interaction via `on_click`, `on_mouse_down`, etc.
- `Focusable` - Keyboard focus management
- `ParentElement` - Adding child elements via `.child()`

### Application Structure

```
AppState (main struct)
├── workspaces: Vec<Workspace>     # Folder-based workspaces
│   └── tasks: Vec<TaskItem>      # Tasks within workspaces
├── active_workspace_id          # Currently selected workspace
├── active_task_id                # Currently selected task
├── sidebar_visible               # Left panel toggle
├── terminal_visible              # Bottom terminal toggle
├── terminal_width               # Terminal resizable width
└── model_config                # Model service settings (base_url, api_key, model_name)
```

### UI Layout

```
┌─────────┬──────────────────────────────────┬────────────┐
│         │           Chat Header           │  Terminal  │
│  Nav    ├──────────────────────────────────│   Toggle   │
│  Panel  │                                  ├────────────┤
│         │         Chat Messages            │            │
│ Workspaces │                                │  Terminal  │
│ ─────────  │                                  │  (alacritty)│
│ Tasks     │                                  │            │
│           ├──────────────────────────────────│            │
│           │         Composer Input          │            │
└─────────┴──────────────────────────────────┴────────────┘
```

## Project Structure

```
one_gpui/
├── Cargo.toml              # Main application manifest
├── src/
│   ├── main.rs           # AppState, all render methods, event handlers (~1270 lines)
│   ├── memory/           # Session/memory management (types, storage, search, snapshots)
│   ├── sandbox/         # Terminal backends (Pty, optional Docker)
│   └── services/        # Config loading and chat API calls
└── components/           # Reusable GPUI components crate
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── text_input.rs   # Entity-based text input with keyboard handling
        ├── button.rs       # Button with interaction states
        ├── checkbox.rs    # Checkbox with checked/indeterminate
        └── traits/        # Component state and trait definitions
```

## Important Patterns

### Event Handlers with State Mutation
```rust
.on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, event, window, cx| {
    this.some_field = new_value;
    cx.notify();  // Trigger re-render
}))
```

### Conditional Rendering
```rust
.when(condition, |this| this.child(...))
```

### Adding Children Iteratively
```rust
let mut result = div()...;
for item in items {
    result = result.child(...)
}
```

### GPUI Styled API
GPUI uses a builder pattern for styling. Methods like `.bg()`, `.text_color()`, `.px()`, `.flex()` are available via the `Styled` trait. Use `gpui::prelude::*` for common traits.

### Overflow/Scroll Behavior
- `overflow_scroll()` requires `StatefulInteractiveElement` via `.id()` wrapper
- Scroll methods are defined in `StatefulInteractiveElement` trait

## Cross-Platform Considerations

- **macOS**: Uses `osascript` for native dialogs
- **Windows**: Uses PowerShell `InputBox`
- **Linux**: Uses `zenity` for native dialogs
- Platform-specific code uses `#[cfg(target_os = "...")]` conditionals

## GPUI Dependency

GPUI is located at `/Users/shijianzhong/sking/zed/crates/gpui`. When GPUI methods are not found, ensure:
1. The correct trait is in scope (e.g., `Styled`, `InteractiveElement`, `ParentElement`)
2. For `StatefulInteractiveElement` methods like `overflow_scroll()`, use `.id("name")` first to get a `Stateful` wrapper
3. Check the prelude: `use gpui::prelude::*`

## Working with the Zed GPUI Codebase

When exploring GPUI for APIs:
- Main element implementations: `/zed/crates/gpui/src/elements/div.rs`
- Styled trait: `/zed/crates/gpui/src/styled.rs`
- Interactive elements: `/zed/crates/gpui/src/interactive.rs`
- Example usage: `/zed/crates/gpui/examples/` (scrollable.rs, input.rs, etc.)

## Memory Management

### 生产推荐架构（三层）

| 层 | 存什么 | 技术选型 | 读写时机 |
|---|---|---|---|
| L1 工作层 | 当前会话全文 | Context window | 实时 |
| L2 摘要层 | 结构化 YAML/MD 快照 | SQLite / 文件 | 会话结束后写入 |
| L3 语义层 | 历史片段 embedding | Qdrant / Weaviate | 异步批量写入 |

**读取流程**：新会话开始 → L3 向量召回 Top-K 相关片段 → L2 用户画像 → 合并注入 system prompt → L1 开始对话。

这是 Mem0、LangMem 等主流开源记忆框架的核心思路，已经过大量工程验证，比 MemPalace 更实用，比纯 MD 文件更可扩展。