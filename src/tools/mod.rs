use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::LiteCodeServer;

pub mod bash;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod notebook;
pub mod read;
pub mod task_output;
pub mod task_stop;
pub mod write;

pub fn build_router() -> ToolRouter<LiteCodeServer> {
    ToolRouter::new()
        .with_async_tool::<bash::BashTool>()
        .with_async_tool::<read::ReadTool>()
        .with_async_tool::<write::WriteTool>()
        .with_async_tool::<edit::EditTool>()
        .with_async_tool::<glob::GlobTool>()
        .with_async_tool::<grep::GrepTool>()
        .with_async_tool::<notebook::NotebookEditTool>()
        .with_async_tool::<task_output::TaskOutputTool>()
        .with_async_tool::<task_stop::TaskStopTool>()
}
