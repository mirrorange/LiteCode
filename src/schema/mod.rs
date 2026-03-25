use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// Decode Unicode escape sequences like \u4F60 before writing (default false)
    #[serde(default)]
    pub decode_unicode_escapes: bool,
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
    /// Decode Unicode escape sequences like \u4F60 in old_string and new_string (default false)
    #[serde(default)]
    pub decode_unicode_escapes: bool,
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
pub struct WriteOutput {
    /// Whether the write operation succeeded
    pub success: bool,
    /// Summary of what was written
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditOutput {
    /// Whether the edit operation succeeded
    pub success: bool,
    /// Summary of what was edited
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobOutput {
    /// Total number of files found
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    /// Array of file paths that match the pattern
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrepOutput {
    /// Total number of files found
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    /// Array of file paths that match the pattern
    pub filenames: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Total number of matches found
    #[serde(rename = "numMatches", skip_serializing_if = "Option::is_none")]
    pub num_matches: Option<usize>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// The standard error output of the command
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Whether the command was interrupted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<bool>,
    /// ID of the background task if command is running in background
    #[serde(rename = "backgroundTaskId", skip_serializing_if = "Option::is_none")]
    pub background_task_id: Option<String>,
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
    pub status: String,
    pub stdout: String,
    pub stderr: String,
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
    /// Whether the notebook edit operation succeeded
    pub success: bool,
    /// The ID of a newly inserted cell when one is created
    #[serde(rename = "cellId", skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<String>,
}

const fn default_true() -> bool {
    true
}

const fn default_task_timeout() -> u64 {
    30_000
}
