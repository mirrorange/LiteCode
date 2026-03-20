use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
    model::JsonObject,
};

use crate::{schema::ReadInput, server::LiteCodeServer};

pub struct ReadTool;

impl ToolBase for ReadTool {
    type Parameter = ReadInput;
    type Output = String;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Read".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some("Reads a file from the local filesystem.".into())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        None
    }
}

impl AsyncTool<LiteCodeServer> for ReadTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .file_service()
            .read_file(
                input.file_path,
                input.offset,
                input.limit,
                input.pages.as_deref(),
            )
            .await
            .map_err(Into::into)
    }
}
