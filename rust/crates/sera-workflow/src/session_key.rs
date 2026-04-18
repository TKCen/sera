/// Build the canonical session key for a workflow execution.
///
/// Format: `"workflow:{agent_id}:{workflow_name}"`
pub fn workflow_session_key(agent_id: &str, workflow_name: &str) -> String {
    format!("workflow:{agent_id}:{workflow_name}")
}
