use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{EditInput, EditOutput},
    server::LiteCodeServer,
};

pub struct EditTool;

impl ToolBase for EditTool {
    type Parameter = EditInput;
    type Output = EditOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Edit".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            r#"Performs exact string replacements in files.

Usage:
- You must use your `Read` tool at least once in the conversation before editing. This tool will error if you attempt an edit without reading the file. 
- When editing text from `Read` output, match the exact file content you received, including indentation and whitespace.
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless explicitly required.
- Only use emojis if the user explicitly requests it. Avoid adding emojis to files unless asked.
- The edit will FAIL if `old_string` is not unique in the file. Either provide a larger string with more surrounding context to make it unique or use `replace_all` to change every instance of `old_string`.
- Use `replace_all` for replacing and renaming strings across the file. This parameter is useful if you want to rename a variable for instance."#
                .into(),
        )
    }
}

impl AsyncTool<LiteCodeServer> for EditTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .file_service()
            .edit_file(input)
            .await
            .map_err(Into::into)
    }
}
