# Coding Assistant Skill 设计方案

## 一、背景

当前编码工作流的逻辑分散在：
- `main_agent.rs` — system prompt 中塞了编码工作流的三步走说明
- `orchestrator.rs` — coding 分支硬编码了 Claude Code 的调用逻辑  
- `job_manager.rs` — 增加了多轮交互的通道

这种耦合方式导致：
1. MainAgent 的 system prompt 不断膨胀，影响其他能力
2. 编码工作流的逻辑修改需要改多个文件
3. 无法独立测试和升级编码能力

## 二、目标架构

将编码工作流封装为独立的 **Skill**，与 `system.cleaner`、`desktop.organizer` 等保持一致架构。

```
用户 ──→ MainAgent ──→ run_system_task("coding_assistant", args)
                              │
                              ▼
                    Coding Assistant Skill
                              │
                    ┌─────────┴─────────┐
                    │  阶段1：需求分析   │
                    │  调 Claude Code   │
                    │  输出结构化的 JSON │
                    └─────────┬─────────┘
                              │ 返回 JSON 需求清单
                              ▼
                    MainAgent 收到 JSON
                              │
                    ┌─────────┴─────────┐
                    │  传递到 UI 层      │
                    │  渲染为交互式表单   │
                    │  （主工程负责）     │
                    └─────────┬─────────┘
                              │ 用户填写提交
                              ▼
                    MainAgent 收到用户回答
                              │
                    ┌─────────┴─────────┐
                    │  阶段2：编码执行   │
                    │  调 Claude Code   │
                    │  （完整任务单）    │
                    └─────────┬─────────┘
                              │ 返回编码结果
                              ▼
                    MainAgent 展示给用户
```

## 三、Skill 职责

Skill 只负责两件事：

1. **需求分析**：接收用户原始需求，调 Claude Code 分析需要哪些信息，输出**结构化 JSON**
2. **编码执行**：接收用户确认后的完整需求信息，调 Claude Code 执行编码，返回编码结果

Skill **不负责**：
- UI 渲染（表单、按钮、弹窗等由主工程负责）
- 用户交互（等待用户输入由主工程的多轮交互机制负责）
- 记忆管理（profile facts 的读写由主工程负责）

## 四、JSON 格式规范

### 需求分析阶段输出

Skill 调 Claude Code 时，指令中要求输出以下格式的 JSON：

```json
{
  "fields": [
    {
      "key": "tech_stack",
      "label": "选择技术栈",
      "type": "select",
      "options": ["原生 HTML/CSS/JS", "React", "Vue", "其他"],
      "default": "原生 HTML/CSS/JS",
      "required": true
    },
    {
      "key": "project_name",
      "label": "项目名称",
      "type": "text",
      "placeholder": "例如：login-page",
      "default": "",
      "required": false
    },
    {
      "key": "api_endpoint",
      "label": "后端 API 地址（如有）",
      "type": "text",
      "placeholder": "例如：http://localhost:3000/api",
      "default": "",
      "required": false
    },
    {
      "key": "design_style",
      "label": "设计风格偏好",
      "type": "select",
      "options": ["现代简洁", "毛玻璃效果", "深色模式", "无特定偏好"],
      "default": "现代简洁",
      "required": false
    }
  ],
  "summary": "需要确认以上信息后才能开始编码"
}
```

### 字段类型定义

| type | 含义 | UI 渲染 | 提交值类型 |
|------|------|---------|-----------|
| `select` | 单选 | 选项按钮列表 | `String` |
| `multi_select` | 多选 | 勾选框列表 | `Vec<String>` |
| `text` | 单行文本 | 输入框 | `String` |
| `textarea` | 多行文本 | 多行输入框 | `String` |

## 五、数据流

### 5.1 需求分析阶段

```
1. MainAgent 收到用户编码需求
2. MainAgent 调 run_system_task("coding_assistant", {task: "写一个登录页面"})
3. Skill 被调用
4. Skill 构造指令调 Claude Code：
   "分析以下编码需求需要哪些信息才能完整执行。
    请以 JSON 格式输出需要确认的信息清单。
    格式要求：{{JSON 格式规范}}
    用户需求：写一个登录页面
    只输出 JSON，不要输出其他内容。"
5. Claude Code 返回 JSON 需求清单
6. Skill 解析 JSON，返回给 MainAgent
7. MainAgent 收到结构化的表单数据，渲染 UI
```

### 5.2 需求确认阶段

```
8. UI 根据 JSON 渲染表单（选项按钮、输入框）
9. 用户填写后提交
10. 表单数据通过 orchestrator 多轮交互通道发回 MainAgent
11. MainAgent 将用户回答传给 Skill（调 run_system_task 第二轮）
```

### 5.3 编码执行阶段

```
12. Skill 收到完整需求信息
13. Skill 构造完整任务单调 Claude Code：
    "根据以下完整需求执行编码：
     用户原始需求：写一个登录页面
     已确认信息：{用户填写的内容 JSON}
     请直接编码，不需要确认任何信息。"
14. Claude Code 执行编码，返回结果
15. Skill 返回编码结果给 MainAgent
16. MainAgent 展示给用户
```

## 六、Skill 实现

### 文件结构

```
skills/
  coding_assistant/
    mod.rs       — Skill 注册、preview、execute
    Cargo.toml   — 依赖
```

### mod.rs 主要接口

```rust
impl Skill for CodingAssistantSkill {
    fn id(&self) -> &str { "coding_assistant" }
    
    async fn preview(&self, args: Value) -> Result<SkillPreview> {
        // 返回预览信息（需要确认的信息摘要）
    }
    
    async fn execute(&self, args: Value) -> Result<SkillOutput> {
        // 根据 args 中的阶段标识执行不同操作
        // stage: "analyze" → 调 Claude Code 做需求分析
        // stage: "execute" → 调 Claude Code 执行编码
    }
}
```

### 是否需要 stage 参数

有两种方式区分阶段：

**方案 A：通过 stage 参数**
```json
// 需求分析
run_system_task("coding_assistant", {stage: "analyze", task: "写一个登录页面"})

// 编码执行
run_system_task("coding_assistant", {stage: "execute", task: "写一个登录页面", confirmed: {...}})
```

**方案 B：根据是否有 confirmed 字段自动判断**
```json
// 需求分析（无 confirmed）
run_system_task("coding_assistant", {task: "写一个登录页面"})

// 编码执行（有 confirmed）
run_system_task("coding_assistant", {task: "写一个登录页面", confirmed: {...}})
```

推荐方案 B，对 MainAgent 更友好。

## 七、主工程改动

| 文件 | 改动 |
|------|------|
| `src/agents/core/main_agent.rs` | 从 system prompt 中移除编码工作流的三步走说明；在 `run_system_task` 的 tool description 中加入 `coding_assistant` |
| `src/agents/core/orchestrator.rs` | 移除或简化 coding 分支中的任务指令增强逻辑（由 Skill 内部处理） |
| `src/ui/chat.rs` / `src/ui/components.rs` | 新增 `ContentPart::RequirementForm` 变体；渲染交互式表单（选项按钮、输入框、提交按钮） |
| `src/memory/types.rs` / `src/ui/components.rs` | 新增表单提交的数据结构 |

## 八、实施步骤

### Phase 1：创建 Skill（独立可测）

1. 创建 `skills/coding_assistant/` 目录
2. 实现需求分析阶段：调 Claude Code，输出 JSON
3. 实现编码执行阶段：接收完整需求，调 Claude Code 执行
4. 注册到技能系统

### Phase 2：UI 表单渲染

1. 新增 `ContentPart::RequirementForm` 变体
2. `parse_think_content` 解析 JSON 表单定义
3. 渲染选项按钮、输入框、提交按钮
4. 用户提交后，表单数据发回 MainAgent

### Phase 3：集成

1. MainAgent 的 `run_system_task` 工具描述中加入 `coding_assistant`
2. 测试完整流程：需求分析 → 表单确认 → 编码执行
3. 清理旧代码

## 九、风险

- Skill 内部调 Claude Code 需要访问 `Config`（模型配置）— 当前 Skill 系统是否支持？需要确认 `Skill::execute` 的参数结构
- `preview` 阶段可能需要调 Claude Code（分析需求），这可能导致 preview 耗时较长
- 多轮交互中，MainAgent 如何缓存用户的确认结果？需要在 MainAgent 的 context 中保留
- UI 层的 RequirementForm 渲染需要考虑 GPUI 的实体生命周期管理