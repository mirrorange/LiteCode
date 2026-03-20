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
    use std::{collections::BTreeMap, path::PathBuf};

    use rmcp::model::ToolsCapability;
    use serde_json::Value;

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

    #[test]
    fn tool_metadata_stays_in_sync_with_docs() {
        let actual_tools = crate::tools::build_router()
            .list_all()
            .into_iter()
            .map(|tool| {
                let value = serde_json::to_value(tool).expect("serialize tool");
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("tool name")
                    .to_string();
                (name, value)
            })
            .collect::<BTreeMap<_, _>>();

        let expected_tools = serde_json::from_str::<Value>(include_str!("../docs/tools.json"))
            .expect("parse docs/tools.json")
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools array")
            .iter()
            .map(|tool| {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("documented tool name")
                    .to_string();
                (name, tool.clone())
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            actual_tools.keys().collect::<Vec<_>>(),
            expected_tools.keys().collect::<Vec<_>>()
        );

        for (name, expected) in expected_tools {
            let actual = actual_tools.get(&name).expect("actual tool entry");
            assert_eq!(actual.get("name"), expected.get("name"), "tool name mismatch");
            assert_eq!(
                normalized_string(actual.get("description")),
                normalized_string(expected.get("description")),
                "tool description mismatch for {name}"
            );

            compare_schema(&name, "inputSchema", actual, &expected);
            compare_schema(&name, "outputSchema", actual, &expected);
        }
    }

    fn compare_schema(tool_name: &str, schema_key: &str, actual: &Value, expected: &Value) {
        match (actual.get(schema_key), expected.get(schema_key)) {
            (None, None) => {}
            (Some(actual_schema), Some(expected_schema)) => {
                assert_eq!(
                    actual_schema.get("additionalProperties"),
                    expected_schema.get("additionalProperties"),
                    "{tool_name} {schema_key} additionalProperties mismatch"
                );
                assert_eq!(
                    actual_schema.get("required"),
                    expected_schema.get("required"),
                    "{tool_name} {schema_key} required mismatch"
                );

                let actual_properties = actual_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .expect("actual properties");
                let expected_properties = expected_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .expect("expected properties");

                assert_eq!(
                    actual_properties.keys().collect::<Vec<_>>(),
                    expected_properties.keys().collect::<Vec<_>>(),
                    "{tool_name} {schema_key} property keys mismatch"
                );

                for (property_name, expected_property) in expected_properties {
                    let actual_property = actual_properties
                        .get(property_name)
                        .unwrap_or_else(|| panic!("missing property {property_name}"));
                    if expected_property.get("description").is_some() {
                        assert_eq!(
                            normalized_string(actual_property.get("description")),
                            normalized_string(expected_property.get("description")),
                            "{tool_name} {schema_key}.{property_name} description mismatch"
                        );
                    }
                    if expected_property.get("default").is_some() {
                        assert_eq!(
                            actual_property.get("default"),
                            expected_property.get("default"),
                            "{tool_name} {schema_key}.{property_name} default mismatch"
                        );
                    }
                    if expected_property.get("minimum").is_some() {
                        assert_eq!(
                            actual_property.get("minimum"),
                            expected_property.get("minimum"),
                            "{tool_name} {schema_key}.{property_name} minimum mismatch"
                        );
                    }
                    if expected_property.get("maximum").is_some() {
                        assert_eq!(
                            actual_property.get("maximum"),
                            expected_property.get("maximum"),
                            "{tool_name} {schema_key}.{property_name} maximum mismatch"
                        );
                    }
                }
            }
            (actual_schema, expected_schema) => panic!(
                "{tool_name} {schema_key} presence mismatch: actual={actual_schema:?} expected={expected_schema:?}"
            ),
        }
    }

    fn normalized_string(value: Option<&Value>) -> Option<String> {
        value
            .and_then(Value::as_str)
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}
