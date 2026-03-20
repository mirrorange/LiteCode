use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{BashInput, BashOutput},
    server::LiteCodeServer,
};

pub struct BashTool;

impl ToolBase for BashTool {
    type Parameter = BashInput;
    type Output = BashOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Bash".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some("Executes a given bash command and returns its output.".into())
    }
}

impl AsyncTool<LiteCodeServer> for BashTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .process_service()
            .bash(input, service.task_manager())
            .await
            .map_err(Into::into)
    }
}
