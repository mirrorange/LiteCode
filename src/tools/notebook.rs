use rmcp::{
    handler::server::{
        tool::{ToolCallContext, ToolRoute, parse_json_object, schema_for_output, schema_for_type},
        wrapper::Parameters,
    },
    model::{CallToolResult, Tool},
};

use crate::{
    schema::{NotebookEditInput, NotebookEditOutput},
    server::LiteCodeServer,
};

const NOTEBOOK_EDIT_DESCRIPTION: &str = r#"Edits a specific cell in a Jupyter notebook (.ipynb file). Jupyter notebooks are interactive documents that combine code, text, and visualizations, commonly used for data analysis and scientific computing. The notebook_path parameter must be an absolute path, not a relative path. Target a cell with either cell_id or the 0-indexed cell_number. If both are provided, they must refer to the same cell, or the same insertion point when edit_mode=insert. Use edit_mode=insert to add a new cell at the specified index or after the specified cell_id. Use edit_mode=delete to delete the specified cell."#;

pub fn route() -> ToolRoute<LiteCodeServer> {
    let tool = Tool::new(
        "NotebookEdit",
        NOTEBOOK_EDIT_DESCRIPTION,
        schema_for_type::<Parameters<NotebookEditInput>>(),
    )
    .with_raw_output_schema(
        schema_for_output::<NotebookEditOutput>()
            .expect("NotebookEdit output schema should be valid"),
    );

    ToolRoute::new_dyn(tool, |context: ToolCallContext<'_, LiteCodeServer>| {
        Box::pin(async move {
            let input =
                parse_json_object::<NotebookEditInput>(context.arguments.unwrap_or_default())?;
            let output = context
                .service
                .file_service()
                .edit_notebook(input)
                .await
                .map_err(rmcp::ErrorData::from)?;

            let value = serde_json::to_value(output)
                .map_err(|error| rmcp::ErrorData::internal_error(error.to_string(), None))?;
            Ok(CallToolResult::structured(value))
        })
    })
}
