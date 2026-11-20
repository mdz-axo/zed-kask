//! Deterministic logical decomposition of an `unfolded_plan` into a
//! fan-out / reconsolidate execution plan.
//!
//! The unfolded plan is the artifact `prompt-enhance`'s rewrite stage emits:
//! a dependency-ordered task list (`id`, `title`, `depends_on`, `d_p_mode`,
//! `reference_model_id`, `verification_oracle`). The model is the executor, so
//! nothing here runs the tasks — this module computes the *shape* of the
//! execution the model should follow:
//!
//! - **Fan-out (map):** a wave decomposition. Wave 0 holds the tasks with no
//!   dependencies; wave N holds the tasks whose dependencies are all in
//!   earlier waves. Tasks inside a wave are independent and may be rendered
//!   in parallel; a task may not start before its dependencies' wave.
//! - **Reconsolidate (reduce):** an ordered batch plan. Task outputs are
//!   merged in dependency order, bounded by a byte budget so no single merge
//!   call over-windows. This reuses the MapReduce reduce-round batching from
//!   the compaction pipeline (`kask_compaction::merge_batches`, Dean &
//!   Ghemawat 2004) — the same primitive that bounds the compaction merge
//!   round, applied here to prompt-stage outputs.
//!
//! Both halves are pure functions over the plan, so the result is
//! deterministic and testable without a model call.

use std::collections::{HashMap, HashSet};

use crate::kask_compaction::merge_batches;
use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::v2::schema as acp;
use gpui::{App, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

/// The essentialist cap on unfolded plan items (prompt-enhance rewrite stage).
const MAX_PLAN_ITEMS: usize = 7;

/// One task of an unfolded plan, as emitted by the prompt-enhance rewrite
/// stage. The planner parses only the fields it needs (`id`, `depends_on`);
/// the remaining plan fields (`title`, `d_p_mode`, `reference_model_id`,
/// `verification_oracle`) are not required by the planner and are ignored.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnfoldedTask {
    /// Stable task identifier; unique within the plan.
    pub id: String,
    /// Upstream task IDs this task depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Task title (informational; not required by the planner).
    #[serde(default)]
    pub title: String,
}

/// The planner's validated output: fan-out waves, a budgeted reduce plan and
/// the critical path.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnfoldPlanResult {
    /// Whether the plan passed validation. When false, `errors` is non-empty
    /// and the remaining fields are empty.
    pub valid: bool,
    /// Validation errors (empty when valid).
    pub errors: Vec<String>,
    /// Fan-out waves: wave 0 first. Tasks within a wave are independent and
    /// may execute in parallel; every dependency of a task in wave N lies in
    /// a wave < N.
    pub waves: Vec<Vec<String>>,
    /// Reconsolidate batches: task IDs in dependency order, grouped so each
    /// batch's combined estimated output fits `budget_bytes`. A zero budget
    /// yields one flat batch (legacy behaviour).
    pub reduce_batches: Vec<Vec<String>>,
    /// The longest dependency chain through the plan (scheduling critical
    /// path); empty when the plan is invalid.
    pub critical_path: Vec<String>,
}

/// Input for the `unfold_plan` tool.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct UnfoldPlanToolInput {
    /// JSON text of the unfolded plan: an array of task objects, each with
    /// `id` and `depends_on`.
    pub plan_json: String,
    /// Optional byte budget for one reconsolidate batch. Omit (or pass 0) for
    /// a single flat batch.
    #[serde(default)]
    pub budget_bytes: usize,
    /// Optional estimated output size in bytes per task ID. When omitted, each
    /// task's serialized metadata size is used as the estimate.
    #[serde(default)]
    pub output_sizes: HashMap<String, usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnfoldPlanToolOutput {
    Planned(UnfoldPlanResult),
    Error { error: String },
}

impl From<UnfoldPlanToolOutput> for language_model::LanguageModelToolResultContent {
    fn from(value: UnfoldPlanToolOutput) -> Self {
        match serde_json::to_string_pretty(&value) {
            Ok(json) => json.into(),
            Err(error) => format!("unfold plan serialization failed: {error}").into(),
        }
    }
}

pub struct UnfoldPlanTool;

impl AgentTool for UnfoldPlanTool {
    type Input = UnfoldPlanToolInput;
    type Output = UnfoldPlanToolOutput;

    const NAME: &'static str = "unfold_plan";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        _input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        SharedString::from("Planning unfolded tasks")
    }

    fn run(
        self: std::sync::Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_cx| {
            let input = input
                .recv()
                .await
                .map_err(|e| UnfoldPlanToolOutput::Error {
                    error: format!("failed to receive input: {e}"),
                })?;
            let result = plan_unfolded(&input).map_err(|e| UnfoldPlanToolOutput::Error {
                error: format!("invalid plan: {e}"),
            })?;
            Ok(UnfoldPlanToolOutput::Planned(result))
        })
    }
}

/// Parse and validate an unfolded plan, then compute its execution shape.
///
/// Validation (all failures are collected, never short-circuited):
/// - the document is a JSON array;
/// - every item is an object with a non-empty string `id`;
/// - IDs are unique;
/// - `depends_on` is an array of strings, each naming an existing task;
/// - the dependency graph is acyclic;
/// - the plan holds at most `MAX_PLAN_ITEMS` tasks.
///
/// On success the result carries the fan-out waves, the budgeted reduce
/// batches and the critical path. On failure `valid` is false, `errors` names
/// every problem found, and the shape fields are empty.
pub fn plan_unfolded(input: &UnfoldPlanToolInput) -> Result<UnfoldPlanResult, String> {
    let value: serde_json::Value = serde_json::from_str(&input.plan_json)
        .map_err(|e| format!("plan_json is not valid JSON: {e}"))?;
    let Some(items) = value.as_array() else {
        return Ok(UnfoldPlanResult {
            valid: false,
            errors: vec!["plan_json must be a JSON array of task objects".to_string()],
            waves: Vec::new(),
            reduce_batches: Vec::new(),
            critical_path: Vec::new(),
        });
    };

    let mut errors: Vec<String> = Vec::new();
    let mut tasks: Vec<UnfoldedTask> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (index, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| format!("plan item {index} is not an object"))?;
        let id = object
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("plan item {index} has no string `id`"))?;
        if id.is_empty() {
            errors.push(format!("plan item {index} has an empty `id`"));
            continue;
        }
        if !seen.insert(id.to_string()) {
            errors.push(format!("duplicate task id `{id}`"));
            continue;
        }
        let depends_on = match object.get("depends_on") {
            None => Vec::new(),
            Some(value) => match value {
                serde_json::Value::Null => Vec::new(),
                serde_json::Value::Array(list) => list
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .ok_or_else(|| format!("task `{id}` has a non-string dependency"))
                            .map(|s| s.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                other => {
                    return Err(format!(
                        "task `{id}` has a non-array `depends_on` ({other})"
                    ));
                }
            },
        };
        let title = object
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        tasks.push(UnfoldedTask {
            id: id.to_string(),
            depends_on,
            title,
        });
    }

    if tasks.len() > MAX_PLAN_ITEMS {
        errors.push(format!(
            "plan has {n} tasks, over the {MAX_PLAN_ITEMS}-item essentialist cap",
            n = tasks.len()
        ));
    }

    let ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    for task in &tasks {
        for dependency in &task.depends_on {
            if !ids.contains(dependency.as_str()) {
                errors.push(format!(
                    "task `{}` depends on unknown task `{dependency}`",
                    task.id
                ));
            }
        }
    }

    if !errors.is_empty() {
        return Ok(UnfoldPlanResult {
            valid: false,
            errors,
            waves: Vec::new(),
            reduce_batches: Vec::new(),
            critical_path: Vec::new(),
        });
    }

    // Kahn's algorithm: peel waves of tasks whose dependencies are all
    // satisfied. A non-empty remainder means a cycle.
    let mut remaining: HashMap<&str, usize> = tasks
        .iter()
        .map(|t| (t.id.as_str(), t.depends_on.len()))
        .collect();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for task in &tasks {
        for dependency in &task.depends_on {
            dependents
                .entry(dependency.as_str())
                .or_default()
                .push(task.id.as_str());
        }
    }

    let mut waves: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<&str> = remaining
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    // Deterministic output: order each wave by the task's position in the
    // input plan, never by HashMap iteration order.
    let position = |id: &str| tasks.iter().position(|t| &t.id == id).unwrap_or(0);
    current.sort_by_key(|id| position(id));
    let mut placed = 0usize;
    while !current.is_empty() {
        // One wave: every task whose dependencies were satisfied by earlier
        // waves. Tasks inside a wave are independent and may run in parallel.
        waves.push(current.iter().map(|id| id.to_string()).collect());
        placed += current.len();
        let mut next: Vec<&str> = Vec::new();
        for id in &current {
            if let Some(children) = dependents.get(id) {
                for child in children {
                    let count = remaining.get_mut(child).unwrap();
                    *count -= 1;
                    if *count == 0 {
                        next.push(child);
                    }
                }
            }
        }
        next.sort_by_key(|id| position(id));
        current = next;
    }
    if placed != tasks.len() {
        let cyclic: Vec<String> = remaining
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(id, _)| id.to_string())
            .collect();
        return Ok(UnfoldPlanResult {
            valid: false,
            errors: vec![format!(
                "dependency cycle detected among: {}",
                cyclic.join(", ")
            )],
            waves: Vec::new(),
            reduce_batches: Vec::new(),
            critical_path: Vec::new(),
        });
    }

    // Reconsolidate: merge outputs in dependency order (the wave order is a
    // valid topological order), batching against the byte budget. Task output
    // sizes are the model's estimates when supplied, else the serialized task
    // metadata size.
    let mut order: Vec<&UnfoldedTask> = tasks.iter().collect();
    order.sort_by(|a, b| {
        let wave_of = |id: &str| {
            waves
                .iter()
                .position(|w| w.contains(&id.to_string()))
                .unwrap_or(0)
        };
        wave_of(&a.id).cmp(&wave_of(&b.id))
    });
    let sizes: HashMap<&str, usize> = order
        .iter()
        .map(|task| {
            let estimated = input
                .output_sizes
                .get(&task.id)
                .copied()
                .unwrap_or_else(|| serde_json::to_string(task).map(|s| s.len()).unwrap_or(0));
            (task.id.as_str(), estimated)
        })
        .collect();
    let summaries: Vec<String> = order
        .iter()
        .map(|task| {
            let size = sizes.get(task.id.as_str()).copied().unwrap_or(0);
            "x".repeat(size)
        })
        .collect();
    let batches = merge_batches(&summaries, input.budget_bytes);
    let reduce_batches: Vec<Vec<String>> = batches
        .iter()
        .map(|range| {
            order
                .iter()
                .skip(range.start)
                .take(range.len())
                .map(|task| task.id.clone())
                .collect()
        })
        .collect();

    // Critical path: the longest dependency chain, by task count.
    let mut depth: HashMap<&str, usize> = HashMap::new();
    let mut predecessor: HashMap<&str, Option<&str>> = HashMap::new();
    for task in &order {
        let mut best = 0usize;
        let mut best_parent: Option<&str> = None;
        for dependency in &task.depends_on {
            let candidate = depth.get(dependency.as_str()).copied().unwrap_or(0) + 1;
            if candidate > best {
                best = candidate;
                best_parent = Some(dependency.as_str());
            }
        }
        depth.insert(task.id.as_str(), best);
        predecessor.insert(task.id.as_str(), best_parent);
    }
    let mut end: Option<&str> = order
        .iter()
        .max_by_key(|task| depth.get(task.id.as_str()).copied().unwrap_or(0))
        .map(|task| task.id.as_str());
    let mut critical_path: Vec<String> = Vec::new();
    while let Some(id) = end {
        critical_path.push(id.to_string());
        end = match predecessor.get(id) {
            Some(Some(parent)) => Some(*parent),
            _ => None,
        };
    }
    critical_path.reverse();

    Ok(UnfoldPlanResult {
        valid: true,
        errors: Vec::new(),
        waves,
        reduce_batches,
        critical_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(plan: serde_json::Value) -> UnfoldPlanToolInput {
        UnfoldPlanToolInput {
            plan_json: serde_json::to_string(&plan).expect("plan serializes"),
            budget_bytes: 0,
            output_sizes: HashMap::new(),
        }
    }

    fn task(id: &str, depends_on: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "title": id,
            "depends_on": depends_on,
        })
    }

    #[test]
    fn fan_out_waves_follow_dependency_order() {
        let plan = serde_json::json!([
            task("a", &[]),
            task("b", &["a"]),
            task("c", &["a"]),
            task("d", &["b", "c"]),
        ]);
        let result = plan_unfolded(&input(plan)).expect("valid plan");
        assert!(result.valid, "{:?}", result.errors);
        assert_eq!(
            result.waves,
            vec![
                vec!["a".to_string()],
                vec!["b".to_string(), "c".to_string()],
                vec!["d".into()]
            ]
        );
        assert_eq!(
            result.critical_path,
            vec!["a".to_string(), "b".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn independent_tasks_share_wave_zero() {
        let plan = serde_json::json!([task("a", &[]), task("b", &[]), task("c", &["a", "b"]),]);
        let result = plan_unfolded(&input(plan)).expect("valid plan");
        assert!(result.valid, "{:?}", result.errors);
        assert_eq!(
            result.waves,
            vec![vec!["a".into(), "b".into()], vec!["c".to_string()]]
        );
    }

    #[test]
    fn cycle_is_rejected() {
        let plan = serde_json::json!([task("a", &["b"]), task("b", &["a"]),]);
        let result = plan_unfolded(&input(plan)).expect("cycle parses");
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| e.contains("cycle")),
            "{:?}",
            result.errors
        );
        assert!(result.waves.is_empty());
    }

    #[test]
    fn unknown_dependency_and_duplicate_ids_are_rejected() {
        let plan = serde_json::json!([task("a", &["missing"]), task("a", &[]),]);
        let result = plan_unfolded(&input(plan)).expect("parses");
        assert!(!result.valid);
        let joined = result.errors.join("; ");
        assert!(joined.contains("unknown task"), "{joined}");
        assert!(joined.contains("duplicate"), "{joined}");
    }

    #[test]
    fn over_cap_plan_is_rejected() {
        let plan = serde_json::json!([
            task("a", &[]),
            task("b", &[]),
            task("c", &[]),
            task("d", &[]),
            task("e", &[]),
            task("f", &[]),
            task("g", &[]),
            task("h", &[]),
        ]);
        let result = plan_unfolded(&input(plan)).expect("parses");
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| e.contains("cap")),
            "{:?}",
            result.errors
        );
    }

    #[test]
    fn reduce_batches_fit_the_budget_in_dependency_order() {
        let mut plan = Vec::new();
        for id in ['a', 'b', 'c', 'd', 'e'] {
            plan.push(task(&id.to_string(), &[]));
        }
        let result = plan_unfolded(&UnfoldPlanToolInput {
            plan_json: serde_json::to_string(&serde_json::Value::Array(plan)).expect("serialize"),
            budget_bytes: 250,
            output_sizes: serde_json::from_value(serde_json::json!({
                "a": 100, "b": 100, "c": 100, "d": 100, "e": 100
            }))
            .expect("sizes"),
        })
        .expect("valid plan");
        assert!(result.valid, "{:?}", result.errors);
        // Each batch carries the 128-byte fixed overhead plus 16 bytes of
        // label overhead per task: 128 + 2*116 = 360 > 250, so one task per
        // batch.
        assert_eq!(
            result.reduce_batches.len(),
            5,
            "{:?}",
            result.reduce_batches
        );
        assert_eq!(
            result
                .reduce_batches
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string()
            ]
        );
    }

    #[test]
    fn zero_budget_yields_one_flat_batch() {
        let plan = serde_json::json!([task("a", &[]), task("b", &["a"]), task("c", &["b"]),]);
        let result = plan_unfolded(&input(plan)).expect("valid plan");
        assert!(result.valid, "{:?}", result.errors);
        assert_eq!(
            result.reduce_batches,
            vec![vec!["a".to_string(), "b".to_string(), "c".to_string()]]
        );
    }

    #[test]
    fn non_array_plan_is_rejected() {
        let result = plan_unfolded(&input(serde_json::json!({"not": "an array"}))).expect("parses");
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| e.contains("array")),
            "{:?}",
            result.errors
        );
    }

    #[gpui::test]
    async fn run_dispatches_the_plan_shape(cx: &mut gpui::TestAppContext) {
        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let tool = std::sync::Arc::new(UnfoldPlanTool);
        let input = UnfoldPlanToolInput {
            plan_json: serde_json::to_string(&serde_json::json!([
                task("a", &[]),
                task("b", &["a"]),
            ]))
            .expect("serialize"),
            budget_bytes: 0,
            output_sizes: HashMap::new(),
        };
        let task = cx.update(|cx| tool.run(ToolInput::resolved(input), event_stream, cx));
        let output = task.await.expect("task completes");
        let UnfoldPlanToolOutput::Planned(result) = output else {
            panic!("expected a planned result");
        };
        assert!(result.valid, "{:?}", result.errors);
        assert_eq!(
            result.waves,
            vec![vec!["a".to_string()], vec!["b".to_string()]]
        );
    }
}
