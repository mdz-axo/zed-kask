//! Regression tests for mutex poison recovery patterns.
//!
//! Verifies that `lock().unwrap_or_else(|e| e.into_inner())` correctly
//! recovers from a poisoned mutex, preventing permanent lock poisoning
//! from crashing the MCP server after a single panicking request.
