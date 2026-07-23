use hkask_capability::DelegationToken;
use hkask_capability::{ToolFuture, ToolInfo, ToolPort, ToolPortError};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopToolPort;

impl ToolPort for NoopToolPort {
    fn invoke<'a>(
        &'a self,
        _server: &'a str,
        tool: &'a str,
        _args: Value,
        _token: &'a DelegationToken,
    ) -> ToolFuture<'a, Result<Value, ToolPortError>> {
        Box::pin(async move {
            Err(ToolPortError::NotFound(hkask_types::NotFound {
                entity_type: "tool".to_string(),
                id: tool.to_string(),
            }))
        })
    }

    fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
        Box::pin(async { Vec::new() })
    }

    fn get_tool_info<'a>(&'a self, _tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
        Box::pin(async { None })
    }
}
