use std::borrow::Cow;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rmcp::{
    ErrorData,
    handler::server::{
        tool::{ToolCallContext, ToolRoute, parse_json_object, schema_for_type},
        wrapper::Parameters,
    },
    model::{CallToolResult, Content, Tool},
};

use crate::{
    schema::ReadInput,
    server::LiteCodeServer,
    services::file_service::{ReadContent, ReadFileOutput},
};

const READ_DESCRIPTION: &str = r#"Reads a file from the local filesystem. You can access any file directly by using this tool.
Assume this tool is able to read all files on the machine. If the User provides a path to a file assume that path is valid. It is okay to read a file that does not exist; an error will be returned.

Usage:
- The file_path parameter must be an absolute path, not a relative path
- By default, it reads up to 2000 lines starting from the beginning of the file
- You can optionally specify a line offset and limit (especially handy for long files), but it's recommended to read the whole file by not providing these parameters
- This tool allows Claude Code to read images (eg PNG, JPG, etc). When reading an image file the contents are presented visually as Claude Code is a multimodal LLM.
- This tool can read PDF files (.pdf). For large PDFs (more than 10 pages), you MUST provide the pages parameter to read specific page ranges (e.g., pages: "1-5"). Reading a large PDF without the pages parameter will fail. Maximum 20 pages per request.
- This tool can read Jupyter notebooks (.ipynb files) and returns all cells with their outputs, combining code, text, and visualizations.
- This tool can only read files, not directories. To read a directory, use an ls command via the Bash tool.
- You can call multiple tools in a single response. It is always better to speculatively read multiple potentially useful files in parallel.
- You will regularly be asked to read screenshots. If the user provides a path to a screenshot, ALWAYS use this tool to view the file at the path. This tool will work with all temporary file paths.
- If you read a file that exists but has empty contents you will receive a system reminder warning in place of file contents."#;

pub fn route() -> ToolRoute<LiteCodeServer> {
    let tool = Tool::new(
        "Read",
        Cow::Borrowed(READ_DESCRIPTION),
        schema_for_type::<Parameters<ReadInput>>(),
    );

    ToolRoute::new_dyn(tool, |context: ToolCallContext<'_, LiteCodeServer>| {
        Box::pin(async move {
            let input = parse_json_object::<ReadInput>(context.arguments.unwrap_or_default())?;
            let output = context
                .service
                .file_service()
                .read_file(
                    input.file_path,
                    input.offset,
                    input.limit,
                    input.pages.as_deref(),
                )
                .await
                .map_err(ErrorData::from)?;

            Ok(output.into_call_tool_result())
        })
    })
}

impl ReadFileOutput {
    fn into_call_tool_result(self) -> CallToolResult {
        match self {
            ReadFileOutput::Text(text) => CallToolResult::success(vec![Content::text(text)]),
            ReadFileOutput::Image { data, mime_type } => {
                CallToolResult::success(vec![Content::image(STANDARD.encode(data), mime_type)])
            }
            ReadFileOutput::Contents(contents) => CallToolResult::success(
                contents
                    .into_iter()
                    .map(|content| match content {
                        ReadContent::Text(text) => Content::text(text),
                        ReadContent::Image { data, mime_type } => {
                            Content::image(STANDARD.encode(data), mime_type)
                        }
                    })
                    .collect(),
            ),
        }
    }
}
