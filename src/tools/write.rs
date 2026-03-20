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
        Some("Writes a file to the local filesystem.".into())
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
