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
        Some("Regex-based content search.".into())
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
