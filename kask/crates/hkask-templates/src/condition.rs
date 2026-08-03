//! Step-condition and choice-branch evaluation.
//!
//! Extracted from the executor (continues the budget.rs / convergence.rs /
//! compute.rs / input_mapping.rs extraction pattern). `evaluate_step_condition`
//! gates a step on AND/OR/NOT/comparison expressions over the context;
//! `parse_choice_condition` backs the `choice` action's branch conditions.
//! Both resolve context references via `input_mapping::resolve_dot_path`.

use crate::input_mapping::resolve_dot_path;
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

/// Evaluate a step condition expression against the context.
/// Supported: "var_name" (truthy), "NOT var_name" (falsy),
/// "a AND b" (both truthy), "a OR b" (either truthy).
pub(crate) fn evaluate_step_condition(condition: &str, context: &HashMap<String, Value>) -> bool {
    let condition = condition.trim();

    // Check for boolean operators
    if let Some(pos) = condition.find(" AND ") {
        let left = &condition[..pos].trim();
        let right = &condition[pos + 5..].trim();
        return evaluate_step_condition(left, context) && evaluate_step_condition(right, context);
    }
    if let Some(pos) = condition.find(" OR ") {
        let left = &condition[..pos].trim();
        let right = &condition[pos + 4..].trim();
        return evaluate_step_condition(left, context) || evaluate_step_condition(right, context);
    }

    // Check for negation
    if let Some(inner) = condition.strip_prefix("NOT ") {
        return !evaluate_step_condition(inner.trim(), context);
    }

    // Comparison: <lhs> <op> <rhs>  (e.g. step_1_result.mode == 'plussing', count > 0)
    if let Some((lhs, op, rhs)) = parse_step_comparison(condition) {
        return eval_step_comparison(lhs, op, rhs, context);
    }

    // Simple variable check: is it truthy in context?
    // Also resolve dot-paths like "step_1_result.intervention_needed"
    let key = condition;
    let resolved = resolve_dot_path(key, context);
    let val: Option<&Value> = context.get(key).or(resolved.as_ref());
    match val {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty() && s != "false" && s != "0",
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Null) => false,
        None => false,
    }
}

/// Parse a leaf comparison expression into (lhs, operator, rhs).
/// Operators: <=, >=, ==, !=, <, > (two-char checked before one-char to avoid
/// prefix collisions). Returns None if no operator is present.
fn parse_step_comparison(condition: &str) -> Option<(&str, &str, &str)> {
    let c = condition.trim();
    for op in &["<=", ">=", "==", "!=", "<", ">"] {
        if let Some(pos) = c.find(op) {
            let lhs = c[..pos].trim();
            let rhs = c[pos + op.len()..].trim();
            if lhs.is_empty() || rhs.is_empty() {
                continue;
            }
            return Some((lhs, op, rhs));
        }
    }
    None
}

/// Resolve an operand to a JSON value: a quoted literal, a context dot-path/key,
/// a number literal, or a bare-word string literal.
fn resolve_operand(s: &str, context: &HashMap<String, Value>) -> Option<Value> {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        return Some(Value::String(s[1..s.len() - 1].to_string()));
    }
    if let Some(v) = context.get(s) {
        return Some(v.clone());
    }
    if let Some(v) = resolve_dot_path(s, context) {
        return Some(v);
    }
    if let Ok(n) = s.parse::<f64>() {
        return Some(serde_json::json!(n));
    }
    // JSON literals: unquoted `true`/`false`/`null` are JSON booleans/null, NOT
    // bare-word strings. Without this, a condition like `flag == true` resolves
    // the rhs to `String("true")`, so `Bool(true) == String` is always false —
    // boolean `step.condition` gates silently never fire (an advertised invariant
    // with no enforcement point). Context keys (checked above) still take
    // precedence, so a key literally named "true" is read as that key first.
    match s {
        "true" => return Some(Value::Bool(true)),
        "false" => return Some(Value::Bool(false)),
        "null" => return Some(Value::Null),
        _ => {}
    }
    // SMELL 10 fix: log when an operand is not found in context — this makes a
    // silently-false condition (e.g. step_1_result.mode == 'plussing' where
    // step_1_result.mode is missing) observable for debugging.
    warn!(
        target: "reg.skill.cascade.step_executed",
        operand = s,
        "condition operand not found in context; treating as literal string"
    );
    Some(Value::String(s.to_string()))
}

/// Evaluate a leaf comparison. Numeric for ordering ops; structural (==/!=) for
/// equality. Falls back to string ordering for non-numeric <, <=, >, >=.
fn eval_step_comparison(lhs: &str, op: &str, rhs: &str, context: &HashMap<String, Value>) -> bool {
    let l = match resolve_operand(lhs, context) {
        Some(v) => v,
        None => return false,
    };
    let r = match resolve_operand(rhs, context) {
        Some(v) => v,
        None => return false,
    };
    match op {
        "==" => l == r,
        "!=" => l != r,
        "<" | "<=" | ">" | ">=" => match (l.as_f64(), r.as_f64()) {
            (Some(a), Some(b)) => match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a >= b,
            },
            _ => {
                let ls = l
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| l.to_string());
                let rs = r
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| r.to_string());
                match op {
                    "<" => ls < rs,
                    "<=" => ls <= rs,
                    ">" => ls > rs,
                    _ => ls >= rs,
                }
            }
        },
        _ => false,
    }
}

/// Parse a simple choice condition string like "composite < 0.15" or "findings == 0".
/// Returns `Some((field, operator, value))` or `None` if unparseable.
pub(crate) fn parse_choice_condition(condition: &str) -> Option<(&str, &str, &str)> {
    let condition = condition.trim();
    for op in &["<=", ">=", "==", "<", ">"] {
        if let Some(pos) = condition.find(op) {
            let field = condition[..pos].trim();
            let value = condition[pos + op.len()..].trim();
            if !field.is_empty() && !value.is_empty() {
                return Some((field, *op, value));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Boolean `step.condition` gating — `resolve_operand` now recognizes unquoted
    // `true`/`false`/`null` as JSON literals. Before this fix, a gate like
    // `flag == true` resolved `true` to String("true"), so Bool == String was
    // always false and the gate silently never fired.
    #[test]
    fn resolve_operand_recognizes_json_literals() {
        let ctx = HashMap::new();
        assert_eq!(resolve_operand("true", &ctx), Some(Value::Bool(true)));
        assert_eq!(resolve_operand("false", &ctx), Some(Value::Bool(false)));
        assert_eq!(resolve_operand("null", &ctx), Some(Value::Null));
        // quoted literals are still strings, not booleans
        assert_eq!(
            resolve_operand("'true'", &ctx),
            Some(Value::String("true".into()))
        );
        // numbers still numbers
        assert_eq!(resolve_operand("3.5", &ctx), Some(serde_json::json!(3.5)));
    }

    #[test]
    fn step_condition_boolean_gating_works() {
        let mut ctx = HashMap::new();
        ctx.insert("flag".into(), Value::Bool(true));
        // The gate that previously never fired:
        assert!(evaluate_step_condition("flag == true", &ctx));
        assert!(!evaluate_step_condition("flag == false", &ctx));
        assert!(evaluate_step_condition("flag != false", &ctx));

        // An absent flag stays default-deny — it does NOT match `true`.
        let empty: HashMap<String, Value> = HashMap::new();
        assert!(!evaluate_step_condition("flag == true", &empty));
        assert!(!evaluate_step_condition("flag == false", &empty));

        // String compares are unaffected.
        let mut sctx = HashMap::new();
        sctx.insert("fix_mode".into(), Value::String("blockers".into()));
        assert!(evaluate_step_condition("fix_mode == 'blockers'", &sctx));
        assert!(!evaluate_step_condition("fix_mode == 'none'", &sctx));
        // OR over enabling values (the code-review implement gate).
        assert!(evaluate_step_condition(
            "fix_mode == 'blockers' OR fix_mode == 'should_fix' OR fix_mode == 'all'",
            &sctx
        ));
        sctx.insert("fix_mode".into(), Value::String("none".into()));
        assert!(!evaluate_step_condition(
            "fix_mode == 'blockers' OR fix_mode == 'should_fix' OR fix_mode == 'all'",
            &sctx
        ));
    }

    #[test]
    fn step_condition_null_literal() {
        let mut ctx = HashMap::new();
        ctx.insert("v".into(), Value::Null);
        assert!(evaluate_step_condition("v == null", &ctx));
    }
}
