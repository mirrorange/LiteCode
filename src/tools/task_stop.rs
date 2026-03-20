use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    error::LiteCodeError,
    schema::{TaskStopInput, TaskStopOutput},
    server::LiteCodeServer,
};

pub struct TaskStopTool;

impl ToolBase for TaskStopTool {
    type Parameter = TaskStopInput;
    type Output = TaskStopOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "TaskStop".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            r#"
- Stops a running background shell task by its ID
- Takes a task_id parameter identifying the task to stop
- Returns a success or failure status
- Use this tool when you need to terminate a long-running shell task
"#
            .into(),
        )
    }
}

impl AsyncTool<LiteCodeServer> for TaskStopTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        let task_id = input.task_id.or(input.shell_id).ok_or_else(|| {
            LiteCodeError::invalid_input("TaskStop requires task_id or shell_id.")
        })?;

        service
            .task_manager()
            .stop_task(&task_id)
            .await
            .map_err(Into::into)
    }
}
