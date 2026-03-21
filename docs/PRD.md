# LiteCode — Product Requirements Document

## Overview

**LiteCode** is an open-source, ultra-lightweight **Coding MCP (Model Context Protocol) server** built with Rust. It provides a streamlined set of developer tools consistent with Claude Code, while stripping away non-essential functionality to achieve minimal footprint and maximum performance.

LiteCode supports two transport modes — **STDIO** and **Streamable HTTP** — making it flexible for both local and networked deployments.

---

## Goals

- **Lightweight**: Minimal binary size and resource usage, powered by Rust's zero-cost abstractions
- **Compatible**: Provide a tool interface consistent with Claude Code so that existing workflows transfer seamlessly
- **Focused**: Only ship essential coding tools — no bloat, no unnecessary features
- **Flexible transport**: Support both STDIO (for local/embedded use) and Streamable HTTP (for remote/networked use)
- **Open source**: Community-driven development with transparent roadmap

---

## Architecture

### Transport Layer

| Transport | Description | Use Case |
| --- | --- | --- |
| **STDIO** | Communication via standard input/output streams | Local IDE integrations, CLI tools, embedded agents |
| **Streamable HTTP** | HTTP-based streaming protocol for real-time communication | Remote servers, cloud deployments, multi-client access |

### Tech Stack

- **Language**: Rust
- **Protocol**: MCP (Model Context Protocol)
- **Serialization**: JSON (JSON Schema for tool input/output validation)

---

## Tools Specification

LiteCode implements **9 tools** that mirror Claude Code's core capabilities. Each tool must be implemented **strictly** as specified below.

### Tool 1 · Bash

Executes shell commands and returns output.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `command` | string | Yes | The bash command to execute |
| `timeout` | number | No | Timeout in ms (max 600,000) |
| `description` | string | No | Human-readable description of the command |
| `run_in_background` | boolean | No | Run command in background; use `TaskOutput` to retrieve results later |

**Output fields**: `stdout`, `stderr`, `interrupted` (required); plus optional `rawOutputPath`, `isImage`, `backgroundTaskId`, `backgroundedByUser`, `assistantAutoBackgrounded`, `returnCodeInterpretation`, `noOutputExpected`, `structuredContent`, `persistedOutputPath`, `persistedOutputSize`, `tokenSaverOutput`.

**Key behaviors**:

- Working directory persists between commands; shell state does not
- Shell environment initialized from user profile (bash or zsh)
- Default timeout: 120,000 ms (2 minutes)

---

### Tool 2 · Read

Reads file contents from the local filesystem.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `file_path` | string | Yes | Absolute path to the file |
| `offset` | number | No | Starting line number |
| `limit` | number | No | Number of lines to read |
| `pages` | string | No | Page range for PDFs (e.g. "1-5"). Max 20 pages per request |

**Key behaviors**:

- Default read limit: 2,000 lines from file start
- Output uses `cat -n` format (line numbers starting at 1)
- Supports images (PNG, JPG, etc.), PDFs, and Jupyter notebooks (.ipynb)
- Can only read files, not directories

---

### Tool 3 · Write

Writes (or overwrites) a file on the local filesystem.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `file_path` | string | Yes | Absolute path to write to |
| `content` | string | Yes | Content to write |

**Output fields**: `type` (create/update), `filePath`, `content`, `structuredPatch`, `originalFile` (required); plus optional `gitDiff`.

**Key behaviors**:

- Overwrites existing file if present
- Existing files must be read first (enforced)

---

### Tool 4 · Edit

Performs exact string replacements in files.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `file_path` | string | Yes | Absolute path to the file |
| `old_string` | string | Yes | The text to replace |
| `new_string` | string | Yes | Replacement text (must differ from `old_string`) |
| `replace_all` | boolean | No | Replace all occurrences (default: false) |

**Key behaviors**:

- File must be read at least once before editing (enforced)
- Fails if `old_string` matches multiple locations and `replace_all` is false
- Preserves exact indentation

---

### Tool 5 · Glob

Fast file pattern matching using glob syntax.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `pattern` | string | Yes | Glob pattern (e.g. `**/*.rs`, `src/**/*.ts`) |
| `path` | string | No | Directory to search in (defaults to cwd) |

**Output fields**: `durationMs`, `numFiles`, `filenames`, `truncated` (all required).

**Key behaviors**:

- Results sorted by modification time
- Truncated at 100 files

---

### Tool 6 · Grep

Regex-based content search powered by ripgrep.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `pattern` | string | Yes | Regex pattern to search |
| `path` | string | No | File or directory to search in |
| `glob` | string | No | Glob filter for files |
| `output_mode` | enum | No | `content` | `files_with_matches` (default) | `count` |
| `-B` / `-A` / `-C` / `context` | number | No | Context lines (before/after/both). Requires `content` mode |
| `-n` | boolean | No | Show line numbers (default: true). Requires `content` mode |
| `-i` | boolean | No | Case-insensitive search |
| `type` | string | No | File type filter (e.g. `js`, `py`, `rust`) |
| `head_limit` | number | No | Limit output entries (default: 0 = unlimited) |
| `offset` | number | No | Skip first N entries before applying limit |
| `multiline` | boolean | No | Enable multiline matching (default: false) |

**Key behaviors**:

- Uses ripgrep syntax (not grep)
- Literal braces need escaping
- Multiline mode maps to `rg -U --multiline-dotall`

---

### Tool 7 · NotebookEdit

Edits cells in Jupyter notebooks (.ipynb).

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `notebook_path` | string | Yes | Absolute path to the `.ipynb` file |
| `new_source` | string | Yes | New source content for the cell |
| `cell_id` | string | No | Target cell ID; can be used instead of `cell_number` |
| `cell_number` | number | No | 0-indexed target cell number; can be used instead of `cell_id` |
| `cell_type` | enum | No* | `code` | `markdown` (*required for insert mode) |
| `edit_mode` | enum | No | `replace` (default) | `insert` | `delete` |

If both `cell_id` and `cell_number` are provided, they must resolve to the same cell, or the same insertion point in `insert` mode.

---

### Tool 8 · TaskOutput

Retrieves output from a running or completed background task.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `task_id` | string | Yes | The task ID to retrieve output from |
| `block` | boolean | Yes | Whether to wait for completion (default: true) |
| `timeout` | number | Yes | Max wait time in ms (default: 30,000; max: 600,000) |

**Key behaviors**:

- Works with all task types: background shells, async agents, remote sessions
- Use `block=false` for non-blocking status checks

---

### Tool 9 · TaskStop

Stops a running background task.

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `task_id` | string | No | The background task ID to stop |

**Output fields**: `message`, `task_id`, `task_type` (required); optional `command`.

---

## Tool Summary Matrix

| Tool | Category | Primary Function |
| --- | --- | --- |
| **Bash** | Execution | Run shell commands |
| **Read** | File I/O | Read files (code, images, PDFs, notebooks) |
| **Write** | File I/O | Create or overwrite files |
| **Edit** | File I/O | Exact string replacement in files |
| **Glob** | Search | Find files by pattern |
| **Grep** | Search | Search file contents by regex |
| **NotebookEdit** | File I/O | Edit Jupyter notebook cells |
| **TaskOutput** | Task Mgmt | Retrieve background task output |
| **TaskStop** | Task Mgmt | Terminate background tasks |

---

## Non-Goals

The following Claude Code features are **explicitly excluded** from LiteCode:

- **Agent tool** — no recursive sub-agent orchestration
- **TodoWrite tool** — no built-in task/todo management
- **Remote session management** — server does not manage remote connections itself
- **Built-in permission/approval system** — delegated to the MCP client

---

## Implementation Requirements

### Strict Compliance

All 9 tools must conform **exactly** to the input/output schemas defined on the [Tools](Tools%2032904adbd2cd801abf32c25bac403b75.md) page. This includes:

1. **Input schemas**: Every parameter, type, default, and constraint must be respected
2. **Output schemas**: All required fields must be present; optional fields included when applicable
3. **Behavioral semantics**: Tool descriptions define expected behavior — implementations must match

### Error Handling

- Return structured errors for invalid inputs, file-not-found, permission denied, and timeout scenarios
- Never crash on malformed input — always return a well-formed error response

### Performance Targets

- Binary size: target < 10 MB (release build, stripped)
- Cold start: < 100 ms
- Tool dispatch latency: < 5 ms overhead per call (excluding actual tool execution time)

---

## Success Criteria

- [ ]  All 9 tools pass conformance tests against the Claude Code tool schema
- [ ]  Both STDIO and Streamable HTTP transports are functional
- [ ]  Binary compiles on Linux, macOS, and Windows
- [ ]  End-to-end latency is competitive with or better than Node.js-based MCP servers
