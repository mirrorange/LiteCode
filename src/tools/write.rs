use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{WriteInput, WriteOutput},
    server::LiteCodeServer,
};

pub struct WriteTool;

impl ToolBase for WriteTool {
    type Parameter = WriteInput;
    type Output = WriteOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Write".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            r#"Writes a file to the local filesystem.

Usage:
- This tool will overwrite the existing file if there is one at the provided path.
- If this is an existing file, you MUST use the Read tool first to read the file's contents. This tool will fail if you did not read the file first.
- Prefer the Edit tool for modifying existing files — it only sends the diff. Only use this tool to create new files or for complete rewrites.
- NEVER create documentation files (*.md) or README files unless explicitly requested by the User.
- Only use emojis if the user explicitly requests it. Avoid writing emojis to files unless asked."#
                .into(),
        )
    }
}

impl AsyncTool<LiteCodeServer> for WriteTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .file_service()
            .write_file(input)
            .await
            .map_err(Into::into)
    }
}
