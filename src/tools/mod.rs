use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::LiteCodeServer;

pub fn build_router() -> ToolRouter<LiteCodeServer> {
    ToolRouter::new()
}
