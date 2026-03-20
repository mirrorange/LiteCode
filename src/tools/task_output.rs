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
        Some(
            r#"- Retrieves output from a running or completed background shell task
- Takes a task_id parameter identifying the task
- Returns the task output along with status information
- Use block=true (default) to wait for task completion
- Use block=false for non-blocking check of current status
- Task IDs can be found using the /tasks command
- Works with background shell tasks started by the Bash tool"#
                .into(),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(Arc::new(
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "description": "The task ID to get output from",
                        "type": "string"
                    },
                    "block": {
                        "description": "Whether to wait for completion",
                        "default": true,
                        "type": "boolean"
                    },
                    "timeout": {
                        "description": "Max wait time in ms",
                        "default": 30000,
                        "type": "number",
                        "minimum": 0,
                        "maximum": 600000
                    }
                },
                "required": ["task_id", "block", "timeout"],
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "additionalProperties": false
            })
            .as_object()
            .expect("task output schema object")
            .clone(),
        ))
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
