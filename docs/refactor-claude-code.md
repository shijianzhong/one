# 重构 Claude Code 集成方案 v2

## 一、核心思路

**两步走模式：分析阶段 → 执行阶段**

```
用户：帮我写个登录页面
  │
  ▼
MainAgent：判断需要编码
  │
  ├──→ 阶段一：需求分析
  │      调 Claude Code：`claude -p "分析以下需求需要哪些信息才能编码：{用户需求}"`
  │      → Claude Code 输出信息清单（技术栈、项目结构、API接口等）
  │      → MainAgent 根据清单向用户逐项收集
  │
  ├──→ （用户提供完整信息）
  │
  └──→ 阶段二：编码执行
         调 Claude Code：`claude -p "{结构化完整任务单}"`
         → Claude Code 执行编码
         → 返回修改文件列表 + 执行结果
         → MainAgent 整理后展示给用户
```

## 二、需要改动的核心

### 核心改动 1：去掉 stdin pipe 所有相关代码

不再需要：
- `execute_instruction_stream` 的 `stdin_tx` 参数
- `OrchestratorEvent::SubAgentQuestion` 变体
- oneshot channel 暂停等待机制
- `pending_subagent_answer` 状态
- `continue_subagent_with_answer` 方法
- chat 中的选项按钮
- stdin 保持 `Stdio::null()`

### 核心改动 2：MainAgent 分两步调用 Claude Code

不修改 `run_claude_code` 工具本身，而是修改 **MainAgent 的 system prompt**，指导它在编码场景下执行两步流程：

```
当用户提出编码需求时，请按以下流程操作：

1. 需求分析阶段：
   - 调用 run_claude_code，指令为：
     "分析以下编码需求的完整信息需求。请列出完成此任务需要知道的全部信息，
      包括技术栈、项目结构、API规范、设计约束等。只分析需要什么信息，
      不要执行任何编码操作：{用户需求}"
   - 等待 Claude Code 返回信息清单

2. 信息收集阶段：
   - 根据 Claude Code 返回的信息清单，逐项向用户确认
   - 如果某些信息不在清单中但你觉得重要，也可以补充提问

3. 编码执行阶段：
   - 收集完所有信息后，调用 run_claude_code，指令为：
     "根据以下完整需求执行编码：{用户需求 + 已确认的各项信息}"
   - Claude Code 执行完毕后，将结果（修改的文件列表、总结）展示给用户
```

### 核心改动 3：恢复 Clone derive

Phase B 改动中为了加 `oneshot::Sender` 去掉了 `Clone`，现在恢复。

## 三、改动文件清单

### Phase A：清理无用代码（先做）

| 文件 | 改动 |
|------|------|
| `src/agents/claude_code.rs` | stdin 保持 `null()`；移除 `stdin_tx` 参数；移除 channel 发送逻辑 |
| `src/agents/core/orchestrator.rs` | 移除 `SubAgentQuestion` 变体；从 coding 分支移除 AskUserQuestion 检测 + oneshot channel + stdin 写入；恢复 `#[derive(Clone)]` |
| `src/runtime/job_manager.rs` | 移除 `pending_subagent_answer` 字段 + 方法 + 清理；移除 `SubAgentQuestion` 事件处理分支；恢复 `OrchestratorWrapperEvent` 的 `#[derive(Clone)]` |
| `src/ui/chat.rs` | 移除选项按钮渲染；发送按钮逻辑回退 |

### Phase B：优化 MainAgent 的 system prompt

| 文件 | 改动 |
|------|------|
| `src/agents/core/main_agent.rs` | 更新 system prompt：指导 MainAgent 在编码任务中执行两步流程；更新 `run_claude_code` 工具描述的提示 |
| `src/agents/core/orchestrator.rs` | （可选）coding 分支的任务指令可加入简单的项目上下文 |

## 四、实施顺序

**Phase A（清理）→ Phase B（提示优化）**

Phase A 是纯删除代码，完成后编译检查即可。
Phase B 是 prompt 层面的调整，不需要改代码结构。

## 五、风险

- **两次 Claude Code 调用可能引入额外延迟** — 分析阶段通常很快（输出文字不需要写代码）
- **MainAgent 可能不遵循两步流程** — 依赖 LLM 的指令理解能力，需要通过 prompt 反复强调
- **分析阶段的信息清单可能不完整** — 可让用户在后续补充信息，Claude Code 执行阶段也能自适应
- **与现有代码不冲突** — 所有 subagent 卡片、子代理流、ModifiedFiles 事件都保留