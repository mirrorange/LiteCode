use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    /// The absolute path to the file to read
    pub file_path: String,
    /// The line number to start reading from. Only provide if the file is too large to read at once
    pub offset: Option<usize>,
    /// The number of lines to read. Only provide if the file is too large to read at once.
    pub limit: Option<usize>,
    /// Page range for PDF files (e.g., "1-5", "3", "10-20"). Only applicable to PDF files. Maximum 20 pages per request.
    pub pages: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteInput {
    /// The absolute path to the file to write (must be absolute, not relative)
    pub file_path: String,
    /// The content to write to the file
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    /// The absolute path to the file to modify
    pub file_path: String,
    /// The text to replace
    pub old_string: String,
    /// The text to replace it with (must be different from old_string)
    pub new_string: String,
    /// Replace all occurrences of old_string (default false)
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobInput {
    /// The glob pattern to match files against
    pub pattern: String,
    /// The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter "undefined" or "null" - simply omit it for the default behavior. Must be a valid directory path if provided.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutputMode {
    Content,
    FilesWithMatches,
    Count,
}

impl Default for GrepOutputMode {
    fn default() -> Self {
        Self::FilesWithMatches
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrepInput {
    /// The regular expression pattern to search for in file contents
    pub pattern: String,
    /// File or directory to search in (rg PATH). Defaults to current working directory.
    pub path: Option<String>,
    /// Glob pattern to filter files (e.g. "*.js", "*.{ts,tsx}") - maps to rg --glob
    pub glob: Option<String>,
    /// Output mode: "content" shows matching lines (supports -A/-B/-C context, -n line numbers, head_limit), "files_with_matches" shows file paths (supports head_limit), "count" shows match counts (supports head_limit). Defaults to "files_with_matches".
    #[serde(default)]
    pub output_mode: GrepOutputMode,
    /// Number of lines to show before each match (rg -B). Requires output_mode: "content", ignored otherwise.
    #[serde(default, rename = "-B")]
    pub before: Option<usize>,
    /// Number of lines to show after each match (rg -A). Requires output_mode: "content", ignored otherwise.
    #[serde(default, rename = "-A")]
    pub after: Option<usize>,
    /// Alias for context.
    #[serde(default, rename = "-C")]
    pub context_alias: Option<usize>,
    /// Number of lines to show before and after each match (rg -C). Requires output_mode: "content", ignored otherwise.
    pub context: Option<usize>,
    /// Show line numbers in output (rg -n). Requires output_mode: "content", ignored otherwise. Defaults to true.
    #[serde(default, rename = "-n")]
    pub line_numbers: Option<bool>,
    /// Case insensitive search (rg -i)
    #[serde(default, rename = "-i")]
    pub case_insensitive: Option<bool>,
    /// File type to search (rg --type). Common types: js, py, rust, go, java, etc. More efficient than include for standard file types.
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    /// Limit output to first N lines/entries, equivalent to "| head -N". Works across all output modes: content (limits output lines), files_with_matches (limits file paths), count (limits count entries). Defaults to 0 (unlimited).
    pub head_limit: Option<usize>,
    /// Skip first N lines/entries before applying head_limit, equivalent to "| tail -n +N | head -N". Works across all output modes. Defaults to 0.
    pub offset: Option<usize>,
    /// Enable multiline mode where . matches newlines and patterns can span lines (rg -U --multiline-dotall). Default: false.
    #[serde(default)]
    pub multiline: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredPatchHunk {
    /// The starting line number in the original file
    #[serde(rename = "oldStart")]
    pub old_start: usize,
    /// The number of lines affected in the original file
    #[serde(rename = "oldLines")]
    pub old_lines: usize,
    /// The starting line number in the updated file
    #[serde(rename = "newStart")]
    pub new_start: usize,
    /// The number of lines affected in the updated file
    #[serde(rename = "newLines")]
    pub new_lines: usize,
    /// The patch lines for this hunk
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitDiff {
    /// The filename relative to the repository root
    pub filename: String,
    /// The git status for the file
    pub status: GitDiffStatus,
    /// Number of added lines
    pub additions: usize,
    /// Number of deleted lines
    pub deletions: usize,
    /// Total number of changed lines
    pub changes: usize,
    /// Unified diff patch for the file
    pub patch: String,
    /// GitHub owner/repo when available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffStatus {
    Modified,
    Added,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WriteResultType {
    Create,
    Update,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteOutput {
    /// Whether a new file was created or an existing file was updated
    pub r#type: WriteResultType,
    /// The path to the file that was written
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// The content that was written to the file
    pub content: String,
    /// Diff patch showing the changes
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    /// The original file content before the write (null for new files)
    #[schemars(required)]
    #[serde(rename = "originalFile")]
    pub original_file: Option<String>,
    #[serde(rename = "gitDiff", skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<GitDiff>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditOutput {
    /// The file path that was edited
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// The original string that was replaced
    #[serde(rename = "oldString")]
    pub old_string: String,
    /// The new string that replaced it
    #[serde(rename = "newString")]
    pub new_string: String,
    /// The original file contents before editing
    #[serde(rename = "originalFile")]
    pub original_file: String,
    /// Diff patch showing the changes
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    /// Whether the user modified the proposed changes
    #[serde(rename = "userModified")]
    pub user_modified: bool,
    /// Whether all occurrences were replaced
    #[serde(rename = "replaceAll")]
    pub replace_all: bool,
    #[serde(rename = "gitDiff", skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<GitDiff>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobOutput {
    /// Time taken to execute the search in milliseconds
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
    /// Total number of files found
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    /// Array of file paths that match the pattern
    pub filenames: Vec<String>,
    /// Whether results were truncated (limited to 100 files)
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrepOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<GrepOutputMode>,
    /// Total number of files found
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    /// Array of file paths that match the pattern
    pub filenames: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Number of lines returned in content
    #[serde(rename = "numLines", skip_serializing_if = "Option::is_none")]
    pub num_lines: Option<usize>,
    /// Total number of matches found
    #[serde(rename = "numMatches", skip_serializing_if = "Option::is_none")]
    pub num_matches: Option<usize>,
    /// The effective head_limit applied
    #[serde(rename = "appliedLimit", skip_serializing_if = "Option::is_none")]
    pub applied_limit: Option<usize>,
    /// The effective offset applied
    #[serde(rename = "appliedOffset", skip_serializing_if = "Option::is_none")]
    pub applied_offset: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BashInput {
    /// The command to execute
    pub command: String,
    /// Optional timeout in milliseconds (max 600000)
    pub timeout: Option<u64>,
    /// Clear, concise description of what this command does in active voice. Never use words like "complex" or "risk" in the description - just describe what it does.
    ///
    /// For simple commands (git, npm, standard CLI tools), keep it brief (5-10 words):
    /// - ls → "List files in current directory"
    /// - git status → "Show working tree status"
    /// - npm install → "Install package dependencies"
    ///
    /// For commands that are harder to parse at a glance (piped commands, obscure flags, etc.), add enough context to clarify what it does:
    /// - find . -name "*.tmp" -exec rm {} \; → "Find and delete all .tmp files recursively"
    /// - git reset --hard origin/main → "Discard all local changes and match remote main"
    /// - curl -s url | jq '.data[]' → "Fetch JSON from URL and extract data array elements"
    pub description: Option<String>,
    /// Set to true to run this command in the background. Use TaskOutput to read the output later.
    pub run_in_background: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BashOutput {
    /// The standard output of the command
    pub stdout: String,
    /// The standard error output of the command
    pub stderr: String,
    /// Whether the command was interrupted
    pub interrupted: bool,
    /// Path to raw output file for large MCP tool outputs
    #[serde(rename = "rawOutputPath", skip_serializing_if = "Option::is_none")]
    pub raw_output_path: Option<String>,
    /// Flag to indicate if stdout contains image data
    #[serde(rename = "isImage", skip_serializing_if = "Option::is_none")]
    pub is_image: Option<bool>,
    /// ID of the background task if command is running in background
    #[serde(rename = "backgroundTaskId", skip_serializing_if = "Option::is_none")]
    pub background_task_id: Option<String>,
    /// True if the user manually backgrounded the command with Ctrl+B
    #[serde(rename = "backgroundedByUser", skip_serializing_if = "Option::is_none")]
    pub backgrounded_by_user: Option<bool>,
    /// True if assistant-mode auto-backgrounded a long-running blocking command
    #[serde(
        rename = "assistantAutoBackgrounded",
        skip_serializing_if = "Option::is_none"
    )]
    pub assistant_auto_backgrounded: Option<bool>,
    /// Semantic interpretation for non-error exit codes with special meaning
    #[serde(
        rename = "returnCodeInterpretation",
        skip_serializing_if = "Option::is_none"
    )]
    pub return_code_interpretation: Option<String>,
    /// Whether the command is expected to produce no output on success
    #[serde(rename = "noOutputExpected", skip_serializing_if = "Option::is_none")]
    pub no_output_expected: Option<bool>,
    /// Structured content blocks
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Vec<Value>>,
    /// Path to the persisted full output in tool-results dir (set when output is too large for inline)
    #[serde(
        rename = "persistedOutputPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub persisted_output_path: Option<String>,
    /// Total size of the output in bytes (set when output is too large for inline)
    #[serde(
        rename = "persistedOutputSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub persisted_output_size: Option<u64>,
    /// Compressed output sent to model when token-saver is active (UI still uses stdout)
    #[serde(rename = "tokenSaverOutput", skip_serializing_if = "Option::is_none")]
    pub token_saver_output: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputInput {
    /// The task ID to get output from
    pub task_id: String,
    /// Whether to wait for completion
    #[serde(default = "default_true")]
    #[schemars(required)]
    pub block: bool,
    /// Max wait time in ms
    #[serde(default = "default_task_timeout")]
    #[schemars(range(min = 0, max = 600000), required)]
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputResponse {
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub status: String,
    #[serde(rename = "taskType")]
    pub task_type: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub interrupted: bool,
    pub completed: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskStopInput {
    /// The ID of the background task to stop
    pub task_id: Option<String>,
    /// Deprecated: use task_id instead
    pub shell_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskStopOutput {
    /// Status message about the operation
    pub message: String,
    /// The ID of the task that was stopped
    pub task_id: String,
    /// The type of the task that was stopped
    pub task_type: String,
    /// The command or description of the stopped task
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotebookCellType {
    Code,
    Markdown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotebookEditMode {
    Replace,
    Insert,
    Delete,
}

impl Default for NotebookEditMode {
    fn default() -> Self {
        Self::Replace
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotebookEditInput {
    /// The absolute path to the Jupyter notebook file to edit (must be absolute, not relative)
    pub notebook_path: String,
    /// The ID of the target cell. Use this or cell_number. If both are provided, they must refer to the same cell, or the same insertion point in insert mode.
    pub cell_id: Option<String>,
    /// The 0-indexed number of the target cell. Use this or cell_id. In insert mode, the new cell is inserted at this index.
    pub cell_number: Option<usize>,
    /// The new source for the cell
    pub new_source: String,
    /// The type of the cell (code or markdown). If not specified, it defaults to the current cell type. If using edit_mode=insert, this is required.
    pub cell_type: Option<NotebookCellType>,
    /// The type of edit to make (replace, insert, delete). Defaults to replace.
    #[serde(default)]
    pub edit_mode: NotebookEditMode,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotebookEditOutput {
    /// The new source code that was written to the cell
    pub new_source: String,
    /// The ID of the cell that was edited
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<String>,
    /// The type of the cell
    pub cell_type: NotebookCellType,
    /// The programming language of the notebook
    pub language: String,
    /// The edit mode that was used
    pub edit_mode: NotebookEditMode,
    /// Error message if the operation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The path to the notebook file
    pub notebook_path: String,
}

const fn default_true() -> bool {
    true
}

const fn default_task_timeout() -> u64 {
    30_000
}
