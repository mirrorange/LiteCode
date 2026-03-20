use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
    model::JsonObject,
};

use crate::{
    schema::{TaskOutputInput, TaskOutputResponse},
    server::LiteCodeServer,
};

pub struct TaskOutputTool;

impl ToolBase for TaskOutputTool {
    type Parameter = TaskOutputInput;
    type Output = TaskOutputResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "TaskOutput".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some("Retrieves output from a running or completed background task.".into())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        None
    }
}

impl AsyncTool<LiteCodeServer> for TaskOutputTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .task_manager()
            .task_output(input)
            .await
            .map_err(Into::into)
    }
}
