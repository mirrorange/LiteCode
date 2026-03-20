use rmcp::{
    ErrorData,
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

const NOTEBOOK_EDIT_DESCRIPTION: &str = r#"Completely replaces the contents of a specific cell in a Jupyter notebook (.ipynb file) with new source. Jupyter notebooks are interactive documents that combine code, text, and visualizations, commonly used for data analysis and scientific computing. The notebook_path parameter must be an absolute path, not a relative path. The cell_number is 0-indexed. Use edit_mode=insert to add a new cell at the index specified by cell_number. Use edit_mode=delete to delete the cell at the index specified by cell_number."#;

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
            let request = parse_notebook_edit_request(context.arguments.unwrap_or_default())?;
            let output = context
                .service
                .file_service()
                .edit_notebook_with_cell_number(request.input, request.cell_number)
                .await
                .map_err(ErrorData::from)?;

            let value = serde_json::to_value(output)
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
            Ok(CallToolResult::structured(value))
        })
    })
}

#[derive(Debug, Clone)]
struct NotebookEditRequest {
    input: NotebookEditInput,
    cell_number: Option<usize>,
}

fn parse_notebook_edit_request(
    mut arguments: rmcp::model::JsonObject,
) -> Result<NotebookEditRequest, ErrorData> {
    let cell_number = match arguments.remove("cell_number") {
        Some(value) => Some(parse_cell_number(value)?),
        None => None,
    };
    let input = parse_json_object(arguments)?;

    Ok(NotebookEditRequest { input, cell_number })
}

fn parse_cell_number(value: serde_json::Value) -> Result<usize, ErrorData> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            ErrorData::invalid_params("cell_number must be a non-negative integer.", None)
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_notebook_edit_request;

    #[test]
    fn notebook_edit_request_accepts_cell_number() {
        let request = parse_notebook_edit_request(
            json!({
                "notebook_path": "/tmp/test.ipynb",
                "new_source": "print('hi')\n",
                "edit_mode": "replace",
                "cell_number": 2
            })
            .as_object()
            .unwrap()
            .clone(),
        )
        .unwrap();

        assert_eq!(request.cell_number, Some(2));
        assert_eq!(request.input.cell_id, None);
    }
}
