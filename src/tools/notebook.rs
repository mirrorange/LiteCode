use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{NotebookEditInput, NotebookEditOutput},
    server::LiteCodeServer,
};

pub struct NotebookEditTool;

impl ToolBase for NotebookEditTool {
    type Parameter = NotebookEditInput;
    type Output = NotebookEditOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "NotebookEdit".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some("Edits a Jupyter notebook cell by id.".into())
    }
}

impl AsyncTool<LiteCodeServer> for NotebookEditTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .file_service()
            .edit_notebook(input)
            .await
            .map_err(Into::into)
    }
}
