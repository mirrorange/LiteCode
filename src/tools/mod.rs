use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::LiteCodeServer;

pub mod edit;
pub mod glob;
pub mod grep;
pub mod read;
pub mod write;

pub fn build_router() -> ToolRouter<LiteCodeServer> {
    ToolRouter::new()
        .with_async_tool::<read::ReadTool>()
        .with_async_tool::<write::WriteTool>()
        .with_async_tool::<edit::EditTool>()
        .with_async_tool::<glob::GlobTool>()
        .with_async_tool::<grep::GrepTool>()
}
