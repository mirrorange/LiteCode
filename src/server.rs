use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo, TasksCapability},
    tool_handler,
};

use crate::services::{
    file_service::FileService, process::ProcessService, task_manager::TaskManager,
};

#[derive(Clone)]
pub struct LiteCodeServer {
    tool_router: ToolRouter<Self>,
    file_service: FileService,
    process_service: ProcessService,
    task_manager: TaskManager,
    working_dir: Arc<Mutex<PathBuf>>,
}

impl LiteCodeServer {
    pub fn new(working_dir: PathBuf) -> Self {
        let working_dir = Arc::new(Mutex::new(working_dir));
        let task_manager = TaskManager::default();

        Self {
            tool_router: crate::tools::build_router(),
            file_service: FileService::new(working_dir.clone()),
            process_service: ProcessService::new(working_dir.clone()),
            task_manager,
            working_dir,
        }
    }

    pub fn working_dir(&self) -> PathBuf {
        self.working_dir
            .lock()
            .expect("working directory lock poisoned")
            .clone()
    }

    pub fn set_working_dir(&self, path: impl AsRef<Path>) {
        *self
            .working_dir
            .lock()
            .expect("working directory lock poisoned") = path.as_ref().to_path_buf();
    }

    pub fn file_service(&self) -> &FileService {
        &self.file_service
    }

    pub fn process_service(&self) -> &ProcessService {
        &self.process_service
    }

    pub fn task_manager(&self) -> &TaskManager {
        &self.task_manager
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LiteCodeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks_with(TasksCapability::server_default())
                .build(),
        )
        .with_server_info(
            Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                .with_title("LiteCode")
                .with_description("Ultra-lightweight coding MCP server built with Rust."),
        )
        .with_instructions(
            "LiteCode exposes a focused set of coding tools over STDIO or Streamable HTTP.",
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rmcp::model::ToolsCapability;

    use super::LiteCodeServer;
    use rmcp::ServerHandler;

    #[test]
    fn advertises_tool_and_task_capabilities() {
        let server = LiteCodeServer::new(PathBuf::from("."));
        let info = server.get_info();

        assert_eq!(info.server_info.name, "litecode");
        assert_eq!(info.capabilities.tools, Some(ToolsCapability::default()));

        let tasks = info.capabilities.tasks.expect("tasks capability");
        assert!(tasks.supports_list());
        assert!(tasks.supports_cancel());
        assert!(tasks.supports_tools_call());
    }

    #[test]
    fn registers_all_required_tools() {
        let router = crate::tools::build_router();
        let tool_names = router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names,
            vec![
                "Bash",
                "Edit",
                "Glob",
                "Grep",
                "NotebookEdit",
                "Read",
                "TaskOutput",
                "TaskStop",
                "Write",
            ]
        );
    }
}
