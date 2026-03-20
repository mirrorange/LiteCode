use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{GlobInput, GlobOutput},
    server::LiteCodeServer,
};

pub struct GlobTool;

impl ToolBase for GlobTool {
    type Parameter = GlobInput;
    type Output = GlobOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Glob".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            r#"- Fast file pattern matching tool that works with any codebase size
- Supports glob patterns like "**/*.js" or "src/**/*.ts"
- Returns matching file paths sorted by modification time
- Use this tool when you need to find files by name patterns
- You can call multiple tools in a single response. It is always better to speculatively perform multiple searches in parallel if they are potentially useful."#
                .into(),
        )
    }
}

impl AsyncTool<LiteCodeServer> for GlobTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service.file_service().glob_files(input).map_err(Into::into)
    }
}
