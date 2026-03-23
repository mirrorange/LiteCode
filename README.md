<div align="center">

# ⚡ LiteCode

**An ultra-lightweight Coding MCP server built with Rust**

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![MCP](https://img.shields.io/badge/Protocol-MCP-green)](https://modelcontextprotocol.io/)

[English](#english) · [中文](#中文)

</div>

---

## English

### What is LiteCode?

LiteCode is an open-source, ultra-lightweight **Coding MCP (Model Context Protocol) server** built with Rust. It provides a streamlined set of developer tools consistent with [Claude Code](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/overview), while stripping away non-essential functionality to achieve **minimal footprint** and **maximum performance**.

### ✨ Features

- **🦀 Lightweight** — Minimal binary size and resource usage, powered by Rust's zero-cost abstractions
- **🔌 Compatible** — Tool interface consistent with Claude Code for seamless workflow transfer
- **🎯 Focused** — Only essential coding tools, no bloat
- **🌐 Flexible Transport** — Supports both STDIO and Streamable HTTP
- **🔓 Open Source** — Community-driven development with a transparent roadmap

### 🛠️ Tools

LiteCode ships with **9 core tools**:

| Tool | Category | Description |
|------|----------|-------------|
| **Bash** | Execution | Run shell commands with background task support |
| **Read** | File I/O | Read files (code, images, PDFs, Jupyter notebooks) |
| **Write** | File I/O | Create or overwrite files |
| **Edit** | File I/O | Exact string replacement in files |
| **Glob** | Search | Find files by glob pattern |
| **Grep** | Search | Search file contents by regex (ripgrep-powered) |
| **NotebookEdit** | File I/O | Edit Jupyter notebook cells |
| **TaskOutput** | Task Mgmt | Retrieve background task output |
| **TaskStop** | Task Mgmt | Terminate background tasks |

### 📦 Installation

#### Build from source

```bash
git clone https://github.com/anthropics/litecode.git
cd litecode
cargo build --release
```

The compiled binary will be at `target/release/litecode`.

### 🚀 Usage

#### STDIO Mode (Local)

```bash
litecode stdio
```

Best for local IDE integrations, CLI tools, and embedded agents.

#### Streamable HTTP Mode (Remote)

```bash
litecode http --host 0.0.0.0 --port 8080
```

Best for remote servers, cloud deployments, and multi-client access.

### 🏗️ Project Structure

```
litecode/
├── src/
│   ├── main.rs          # Entry point
│   ├── cli.rs           # CLI argument parsing
│   ├── lib.rs           # Library exports
│   ├── server.rs        # MCP server implementation
│   ├── error.rs         # Error types
│   ├── schema/          # JSON Schema definitions
│   ├── tools/           # Tool implementations
│   ├── services/        # Shared services
│   └── transport/       # STDIO & HTTP transport layers
├── docs/                # Documentation
└── Cargo.toml
```

### 🤝 Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

### 📄 License

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.

---

## 中文

### LiteCode 是什么？

LiteCode 是一个使用 Rust 构建的开源、超轻量级 **Coding MCP（Model Context Protocol）服务器**。它提供了一套与 [Claude Code](https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/overview) 一致的精简开发者工具集，同时去除了非必要功能，以实现**最小体积**和**最高性能**。

### ✨ 特性

- **🦀 轻量级** — 极小的二进制体积和资源占用，得益于 Rust 的零成本抽象
- **🔌 兼容性强** — 工具接口与 Claude Code 保持一致，无缝迁移现有工作流
- **🎯 专注核心** — 仅包含必要的编码工具，无冗余功能
- **🌐 灵活传输** — 同时支持 STDIO 和 Streamable HTTP 两种传输模式
- **🔓 开源开放** — 社区驱动开发，路线图透明公开

### 🛠️ 工具集

LiteCode 提供 **9 个核心工具**：

| 工具 | 分类 | 描述 |
|------|------|------|
| **Bash** | 执行 | 运行 Shell 命令，支持后台任务 |
| **Read** | 文件 I/O | 读取文件（代码、图片、PDF、Jupyter 笔记本） |
| **Write** | 文件 I/O | 创建或覆写文件 |
| **Edit** | 文件 I/O | 文件内精确字符串替换 |
| **Glob** | 搜索 | 按 glob 模式匹配查找文件 |
| **Grep** | 搜索 | 按正则表达式搜索文件内容（基于 ripgrep） |
| **NotebookEdit** | 文件 I/O | 编辑 Jupyter Notebook 单元格 |
| **TaskOutput** | 任务管理 | 获取后台任务输出 |
| **TaskStop** | 任务管理 | 终止后台任务 |

### 📦 安装

#### 从源码构建

```bash
git clone https://github.com/anthropics/litecode.git
cd litecode
cargo build --release
```

编译后的二进制文件位于 `target/release/litecode`。

### 🚀 使用方法

#### STDIO 模式（本地）

```bash
litecode stdio
```

适用于本地 IDE 集成、CLI 工具和嵌入式 Agent。

#### Streamable HTTP 模式（远程）

```bash
litecode http --host 0.0.0.0 --port 8080
```

适用于远程服务器、云部署和多客户端访问。

### 🏗️ 项目结构

```
litecode/
├── src/
│   ├── main.rs          # 程序入口
│   ├── cli.rs           # CLI 参数解析
│   ├── lib.rs           # 库导出
│   ├── server.rs        # MCP 服务器实现
│   ├── error.rs         # 错误类型定义
│   ├── schema/          # JSON Schema 定义
│   ├── tools/           # 工具实现
│   ├── services/        # 共享服务
│   └── transport/       # STDIO 与 HTTP 传输层
├── docs/                # 文档
└── Cargo.toml
```

### 🤝 参与贡献

欢迎贡献代码！请随时提交 Issue 和 Pull Request。

### 📄 许可证

本项目采用 MIT 许可证 — 详见 [LICENSE](LICENSE) 文件。
