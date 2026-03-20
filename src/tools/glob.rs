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
        Some("Fast file pattern matching using glob syntax.".into())
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
