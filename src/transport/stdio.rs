use rmcp::{ServiceExt, transport::io::stdio};

use crate::{error::Result, server::LiteCodeServer};

pub async fn serve(server: LiteCodeServer) -> Result<()> {
    server.serve(stdio()).await?.waiting().await?;
    Ok(())
}
