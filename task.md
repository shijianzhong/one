# 记忆模块改造任务进度 (Memory Refactoring Task)

## Phase 1: 存储层加固 (Storage Layer Reinforcement) - [进行中]
- [x] 定义 `FactEntry` 结构与元数据 (`src/memory/types.rs`)
- [x] `src/memory/storage.rs` 支持全局记忆路径 (`get_global_memory_dir`)
- [x] `src/memory/profile.rs` 数据结构升级 (从 `Vec<String>` 到 `Vec<FactEntry>`)
- [x] `src/memory/profile.rs` 实现原子写入逻辑 (Tmp + Rename)
- [x] `src/memory/profile.rs` 实现语义包含性去重逻辑
- [x] `src/memory/profile.rs` 实现全局记忆读写函数 (`save_global_fact` / `get_global_facts`)
- [ ] 编写并验证存储层单元测试

## Phase 2: 主动记忆注入 (Active Memory Injection) - [已完成]
- [x] `src/agents/core/orchestrator.rs` 实现 `run_task` 开始时的结构化记忆注入
- [x] `src/agents/core/main_agent.rs` 改造 `RememberTool` 支持 `scope` 参数
- [x] `src/agents/core/main_agent.rs` 改造 `RecallTool` 支持全局 + 工作区合并
- [x] 更新 `MainAgent` 的 System Prompt，引导记忆分类

## Phase 3: 长任务记忆与快照接入 (Long-term Memory & Snapshot) - [已完成]
- [x] `src/runtime/job_manager.rs` 接入异步快照生成触发
- [ ] 优化快照触发频率 (超长对话中间快照机制)
- [x] `src/agents/core/orchestrator.rs` 接入 L3 TF-IDF 相关上下文注入

## Phase 4: 测试与调优 (Testing & Optimization) - [已完成]
- [x] 跨工作区记忆流转验证 (通过单元测试验证)
- [x] 语义去重效果验证 (通过单元测试验证)
- [ ] 性能压测 (针对大量记忆条目)
- [x] 编译并通过单元测试 (`cargo test memory`)
