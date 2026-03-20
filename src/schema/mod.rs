use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub file_path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub pages: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteInput {
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobInput {
    pub pattern: String,
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
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    #[serde(default)]
    pub output_mode: GrepOutputMode,
    #[serde(default, rename = "-B")]
    pub before: Option<usize>,
    #[serde(default, rename = "-A")]
    pub after: Option<usize>,
    #[serde(default, rename = "-C")]
    pub context_alias: Option<usize>,
    pub context: Option<usize>,
    #[serde(default, rename = "-n")]
    pub line_numbers: Option<bool>,
    #[serde(default, rename = "-i")]
    pub case_insensitive: Option<bool>,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub head_limit: Option<usize>,
    pub offset: Option<usize>,
    #[serde(default)]
    pub multiline: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredPatchHunk {
    #[serde(rename = "oldStart")]
    pub old_start: usize,
    #[serde(rename = "oldLines")]
    pub old_lines: usize,
    #[serde(rename = "newStart")]
    pub new_start: usize,
    #[serde(rename = "newLines")]
    pub new_lines: usize,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitDiff {
    pub filename: String,
    pub status: GitDiffStatus,
    pub additions: usize,
    pub deletions: usize,
    pub changes: usize,
    pub patch: String,
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
    pub r#type: WriteResultType,
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub content: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "originalFile")]
    pub original_file: Option<String>,
    #[serde(rename = "gitDiff", skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<GitDiff>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditOutput {
    #[serde(rename = "filePath")]
    pub file_path: String,
    #[serde(rename = "oldString")]
    pub old_string: String,
    #[serde(rename = "newString")]
    pub new_string: String,
    #[serde(rename = "originalFile")]
    pub original_file: String,
    #[serde(rename = "structuredPatch")]
    pub structured_patch: Vec<StructuredPatchHunk>,
    #[serde(rename = "userModified")]
    pub user_modified: bool,
    #[serde(rename = "replaceAll")]
    pub replace_all: bool,
    #[serde(rename = "gitDiff", skip_serializing_if = "Option::is_none")]
    pub git_diff: Option<GitDiff>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobOutput {
    #[serde(rename = "durationMs")]
    pub duration_ms: u128,
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    pub filenames: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrepOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<GrepOutputMode>,
    #[serde(rename = "numFiles")]
    pub num_files: usize,
    pub filenames: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(rename = "numLines", skip_serializing_if = "Option::is_none")]
    pub num_lines: Option<usize>,
    #[serde(rename = "numMatches", skip_serializing_if = "Option::is_none")]
    pub num_matches: Option<usize>,
    #[serde(rename = "appliedLimit", skip_serializing_if = "Option::is_none")]
    pub applied_limit: Option<usize>,
    #[serde(rename = "appliedOffset", skip_serializing_if = "Option::is_none")]
    pub applied_offset: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BashInput {
    pub command: String,
    pub timeout: Option<u64>,
    pub description: Option<String>,
    pub run_in_background: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BashOutput {
    pub stdout: String,
    pub stderr: String,
    pub interrupted: bool,
    #[serde(rename = "rawOutputPath", skip_serializing_if = "Option::is_none")]
    pub raw_output_path: Option<String>,
    #[serde(rename = "isImage", skip_serializing_if = "Option::is_none")]
    pub is_image: Option<bool>,
    #[serde(rename = "backgroundTaskId", skip_serializing_if = "Option::is_none")]
    pub background_task_id: Option<String>,
    #[serde(rename = "backgroundedByUser", skip_serializing_if = "Option::is_none")]
    pub backgrounded_by_user: Option<bool>,
    #[serde(
        rename = "assistantAutoBackgrounded",
        skip_serializing_if = "Option::is_none"
    )]
    pub assistant_auto_backgrounded: Option<bool>,
    #[serde(
        rename = "returnCodeInterpretation",
        skip_serializing_if = "Option::is_none"
    )]
    pub return_code_interpretation: Option<String>,
    #[serde(rename = "noOutputExpected", skip_serializing_if = "Option::is_none")]
    pub no_output_expected: Option<bool>,
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Vec<Value>>,
    #[serde(
        rename = "persistedOutputPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub persisted_output_path: Option<String>,
    #[serde(
        rename = "persistedOutputSize",
        skip_serializing_if = "Option::is_none"
    )]
    pub persisted_output_size: Option<u64>,
    #[serde(rename = "tokenSaverOutput", skip_serializing_if = "Option::is_none")]
    pub token_saver_output: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputInput {
    pub task_id: String,
    #[serde(default = "default_true")]
    pub block: bool,
    #[serde(default = "default_task_timeout")]
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
    pub task_id: Option<String>,
    pub shell_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskStopOutput {
    pub message: String,
    pub task_id: String,
    pub task_type: String,
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
    pub notebook_path: String,
    pub cell_id: Option<String>,
    pub new_source: String,
    pub cell_type: Option<NotebookCellType>,
    #[serde(default)]
    pub edit_mode: NotebookEditMode,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotebookEditOutput {
    pub new_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<String>,
    pub cell_type: NotebookCellType,
    pub language: String,
    pub edit_mode: NotebookEditMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub notebook_path: String,
    pub original_file: String,
    pub updated_file: String,
}

const fn default_true() -> bool {
    true
}

const fn default_task_timeout() -> u64 {
    30_000
}
