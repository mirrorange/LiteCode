## Stage 1: Foundation
**Goal**: 建立可编译的 LiteCode Rust 项目骨架、CLI、MCP server 基础状态和双传输启动入口。
**Success Criteria**: `cargo test` 通过；支持 `stdio` 与 `http` 两种启动模式；server 正确声明 tools/tasks capability。
**Tests**: CLI 参数解析测试；server info capability 测试。
**Status**: Complete

## Stage 2: Files And Search
**Goal**: 实现文件与搜索服务，以及 `Read`、`Write`、`Edit`、`Glob`、`Grep` 工具。
**Success Criteria**: 支持文件读取/写入/精确替换、glob 匹配和内容搜索；遵守读后写/读后编辑约束；相关测试通过。
**Tests**: 文件读写与编辑行为测试；glob/grep 输出测试；约束失败路径测试。
**Status**: In Progress

## Stage 3: Process And Tasks
**Goal**: 实现命令执行与后台任务管理，交付 `Bash`、`TaskOutput`、`TaskStop` 工具。
**Success Criteria**: 前台/后台命令执行可用；任务状态可查询和停止；工作目录可跨命令持久化。
**Tests**: Bash 前台执行测试；后台任务查询与停止测试；工作目录持久化测试。
**Status**: Not Started

## Stage 4: Notebook And Policy
**Goal**: 实现 `NotebookEdit`，并补齐跨工具状态跟踪与约束校验。
**Success Criteria**: 支持 notebook cell replace/insert/delete；跨工具读写策略一致；错误信息清晰。
**Tests**: notebook 编辑模式测试；未读取即写/编辑失败测试；边界输入测试。
**Status**: Not Started

## Stage 5: Integration And Polish
**Goal**: 完成端到端验证、整理文档与代码，移除计划文件。
**Success Criteria**: 关键集成测试通过；代码格式化完成；计划文件在全部完成后删除。
**Tests**: STDIO/HTTP 冒烟测试；工具 schema/注册验证；完整 `cargo test`。
**Status**: Not Started
