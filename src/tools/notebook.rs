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
        Some(
            r#"Completely replaces the contents of a specific cell in a Jupyter notebook (.ipynb file) with new source. Jupyter notebooks are interactive documents that combine code, text, and visualizations, commonly used for data analysis and scientific computing. The notebook_path parameter must be an absolute path, not a relative path. The cell_number is 0-indexed. Use edit_mode=insert to add a new cell at the index specified by cell_number. Use edit_mode=delete to delete the cell at the index specified by cell_number."#
                .into(),
        )
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
