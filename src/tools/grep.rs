use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{GrepInput, GrepOutput},
    server::LiteCodeServer,
};

pub struct GrepTool;

impl ToolBase for GrepTool {
    type Parameter = GrepInput;
    type Output = GrepOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Grep".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            r#"A powerful search tool built on ripgrep

  Usage:
  - ALWAYS use Grep for search tasks. NEVER invoke `grep` or `rg` as a Bash command. The Grep tool has been optimized for correct permissions and access.
  - Supports full regex syntax (e.g., "log.*Error", "function\s+\w+")
  - Filter files with glob parameter (e.g., "*.js", "**/*.tsx") or type parameter (e.g., "js", "py", "rust")
  - Output modes: "content" shows matching lines, "files_with_matches" shows only file paths (default), "count" shows match counts
  - Pattern syntax: Uses ripgrep (not grep) - literal braces need escaping (use `interface\{\}` to find `interface{}` in Go code)
  - Multiline matching: By default patterns match within single lines only. For cross-line patterns like `struct \{[\s\S]*?field`, use `multiline: true`
"#
            .into(),
        )
    }
}

impl AsyncTool<LiteCodeServer> for GrepTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .file_service()
            .grep_files(input)
            .await
            .map_err(Into::into)
    }
}
