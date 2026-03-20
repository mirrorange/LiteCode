use std::borrow::Cow;

use rmcp::{
    ErrorData,
    handler::server::router::tool::{AsyncTool, ToolBase},
};

use crate::{
    schema::{EditInput, EditOutput},
    server::LiteCodeServer,
};

pub struct EditTool;

impl ToolBase for EditTool {
    type Parameter = EditInput;
    type Output = EditOutput;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        "Edit".into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some("Performs exact string replacements in files.".into())
    }
}

impl AsyncTool<LiteCodeServer> for EditTool {
    async fn invoke(
        service: &LiteCodeServer,
        input: Self::Parameter,
    ) -> Result<Self::Output, Self::Error> {
        service
            .file_service()
            .edit_file(input)
            .await
            .map_err(Into::into)
    }
}
