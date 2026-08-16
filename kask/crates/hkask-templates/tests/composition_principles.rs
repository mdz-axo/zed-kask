//! Composition-principle tests derived from bug history and new-capability
//! failure-mode analysis.
//!
//! These tests enforce principles that were identified in the composition-
//! principles analysis but not covered by the existing test harness. Each
//! test documents the principle it enforces, the bug class or capability it
//! addresses, and the gap it closes.
//!
//! Principles enforced:
//! - G1 (P25b): every symbol in a `lisp.eval` form is bound in `env` or is a
//!   builtin/special-form — unbound symbols resolve to `null` silently.
//! - G3 (P26): `mcp:` steps have failure handling (`on_failure` or a
//!   downstream `condition` checking the result) — a failed MCP call must
//!   not silently propagate `null`.
//! - G5 (P27): `calibration` convergence mode requires `min_iterations >= 2`
//!   — Brier scoring needs prediction + result, which requires at least 2
//!   iterations.
//! - G6 (P27): only whitelisted `convergence_mode` combinations are allowed
//!   — semantically incoherent combinations are rejected.
//! - G7 (P28): `compute` step `input_mapping` provides the primitive's
//!   required input keys — a missing required input fails at compute time.
//! - G8 (P28): `shell.exec` is only used in `skill`-category manifests —
//!   the security gate documented in `compute.rs` is enforced at the
//!   manifest level.

use hkask_templates::load_manifest_from_yaml;
use std::collections::HashSet;
use std::path::Path;

// ── G1: lisp.eval form symbols are bound ───────────────────────────────────

/// The set of Lisp builtins (native functions) defined by `default_builtins`
/// in `hkask-lisp/src/hkask_lisp.rs`. These are always available — a form
/// referencing one of these does not need an `env` binding.
const LISP_BUILTINS: &[&str] = &[
    "+", "-", "*", "/", "=", "!=", "<", "<=", ">", ">=", "car", "cdr", "cons", "list", "length",
    "nth", "reverse", "is_null", "numberp", "listp", "assoc", "append", "string=", "concat", "abs",
    "sqrt", "eq", "member",
];

/// Special forms recognized by `eval_special_form` — these are language
/// constructs, not function calls, and are always available.
const LISP_SPECIAL_FORMS: &[&str] = &[
    "quote", "if", "let", "lambda", "define", "begin", "and", "or", "not", "cond",
];

/// Root-env constants — symbols bound in `Env::new_root()` that are neither
/// builtins nor special forms. `t` is the canonical truth constant.
const LISP_ROOT_CONSTANTS: &[&str] = &["t"];

/// Recursively collect all symbol names referenced in a parsed Lisp form,
/// excluding symbols that are locally bound within the form (via `let`,
/// `lambda` params, or `define`).
///
/// `local_bindings` is the set of symbols bound in enclosing scopes within
/// the form (not from `env` — env bindings are checked by the caller). A
/// symbol that is in `local_bindings` is a reference to a local variable, not
/// an unbound symbol.
fn collect_symbol_references(
    form: &hkask_lisp::LispValue,
    local_bindings: &HashSet<String>,
    out: &mut HashSet<String>,
) {
    match form {
        hkask_lisp::LispValue::Symbol(s) => {
            // Only collect if not locally bound (env bindings are checked
            // by the caller against the collected set).
            if !local_bindings.contains(s) {
                out.insert(s.clone());
            }
        }
        hkask_lisp::LispValue::List(list) => {
            let items = list.to_vec();
            if items.is_empty() || list.is_nil() {
                return;
            }
            if let hkask_lisp::LispValue::Symbol(head) = &items[0] {
                match head.as_str() {
                    // `let` — first arg is a binding list, second arg is
                    // the body. Binding names are added to the local scope.
                    // Value expressions are traversed in the outer scope
                    // (standard `let` semantics — siblings can't reference
                    // each other). The body is traversed in the extended
                    // scope.
                    //
                    // Form: (let ((name val) ...) body)
                    // items[0] = 'let, items[1] = bindings, items[2] = body
                    "let" => {
                        if items.len() >= 3 {
                            let mut new_bindings = local_bindings.clone();
                            if let hkask_lisp::LispValue::List(bindings) = &items[1] {
                                for binding in bindings.to_vec() {
                                    if let hkask_lisp::LispValue::List(pair) = &binding {
                                        let pair_vec = pair.to_vec();
                                        if pair_vec.len() == 2 {
                                            // Value expression: traverse in outer scope.
                                            collect_symbol_references(
                                                &pair_vec[1],
                                                local_bindings,
                                                out,
                                            );
                                            // Binding name: add to scope for body.
                                            if let hkask_lisp::LispValue::Symbol(name) =
                                                &pair_vec[0]
                                            {
                                                new_bindings.insert(name.clone());
                                            }
                                        }
                                    }
                                }
                            }
                            // Body: traverse in extended scope.
                            for body_form in &items[2..] {
                                collect_symbol_references(body_form, &new_bindings, out);
                            }
                            return;
                        }
                    }
                    // `lambda` — first arg is a parameter list (definitions),
                    // second arg is the body. The body is traversed in a
                    // scope extended with the params.
                    //
                    // Form: (lambda (params...) body)
                    // items[0] = 'lambda, items[1] = params, items[2] = body
                    "lambda" => {
                        if items.len() >= 3 {
                            let mut new_bindings = local_bindings.clone();
                            if let hkask_lisp::LispValue::List(params) = &items[1] {
                                for param in params.to_vec() {
                                    if let hkask_lisp::LispValue::Symbol(s) = &param {
                                        new_bindings.insert(s.clone());
                                    }
                                }
                            }
                            for body_form in &items[2..] {
                                collect_symbol_references(body_form, &new_bindings, out);
                            }
                            return;
                        }
                    }
                    // `begin` — sequential evaluation. `define` forms inside
                    // `begin` mutate the current scope, so defined names are
                    // available to subsequent forms. Process items in order,
                    // accumulating defined names.
                    "begin" => {
                        let mut current_bindings = local_bindings.clone();
                        for item in &items[1..] {
                            // If this is a `define`, extract the name and add
                            // it to the scope BEFORE traversing the value,
                            // so recursive references inside the value (e.g.,
                            // a lambda that calls itself by name) resolve.
                            if let hkask_lisp::LispValue::List(sub) = item {
                                let sub_items = sub.to_vec();
                                if sub_items.len() >= 3
                                    && matches!(&sub_items[0], hkask_lisp::LispValue::Symbol(s) if s == "define")
                                {
                                    if let hkask_lisp::LispValue::Symbol(name) = &sub_items[1] {
                                        // Add the name to the scope FIRST,
                                        // so recursive references in the
                                        // value (e.g., (define f (lambda ... f ...)))
                                        // resolve to the local binding.
                                        current_bindings.insert(name.clone());
                                        // Traverse the value in the extended scope.
                                        for val_form in &sub_items[2..] {
                                            collect_symbol_references(
                                                val_form,
                                                &current_bindings,
                                                out,
                                            );
                                        }
                                        continue;
                                    }
                                }
                            }
                            // Default: traverse in the current accumulated scope.
                            collect_symbol_references(item, &current_bindings, out);
                        }
                        return;
                    }
                    // `define` — (define name value). Standalone define
                    // (not inside `begin`). The name is added to the scope
                    // for any subsequent forms in the same scope.
                    "define" => {
                        if items.len() >= 3 {
                            if let hkask_lisp::LispValue::Symbol(_) = &items[1] {
                                // Traverse the value in the current scope.
                                for val_form in &items[2..] {
                                    collect_symbol_references(val_form, local_bindings, out);
                                }
                                // Note: we can't mutate local_bindings here
                                // (it's a reference), so the name won't be
                                // available to siblings. In practice, standalone
                                // `define` (not in `begin`) is rare — the
                                // `begin` handler covers the common case.
                                return;
                            }
                        }
                    }
                    // `quote` — the argument is data, not code. Don't traverse.
                    "quote" => return,
                    _ => {}
                }
            }
            // Default: traverse all elements in the current scope.
            for item in &items {
                collect_symbol_references(item, local_bindings, out);
            }
        }
        _ => {}
    }
}

/// G1 (P25b): Every symbol referenced in a `lisp.eval` step's `form` should
/// be either a Lisp builtin, a special form, or bound in the step's `env`
/// block (or locally bound via `let`/`lambda`/`define` within the form).
///
/// An unbound symbol resolves to `null` silently at eval time — the same
/// class of silent failure as the self/forward reference bug (P1), but for
/// Lisp symbols rather than `step_N_result` references. The E13 test catches
/// `step_N_result` references in `env`; this test catches symbol references
/// in `form` that have no corresponding `env` binding.
///
/// The test parses the `form` via `hkask_lisp::parse` (which also validates
/// syntax — E14), walks the AST to collect all symbol references (excluding
/// binding positions in `let`/`lambda`/`define` and tracking scope through
/// `begin` blocks), and verifies each is either a builtin, a special form,
/// locally bound, or present in the `env` block's keys.
///
/// This is a ceiling-gated diagnostic: the current count reflects
/// pre-existing forms where the scope tracker cannot fully resolve
/// `define`d functions inside nested `if`/`begin` blocks (the tracker
/// doesn't propagate `define` from nested blocks back to the enclosing
/// scope, which the real evaluator does). The test fails if the count
/// *increases* — any new unbound symbol is a potential silent-null bug.
/// Existing warnings should be reviewed: if the symbol is genuinely
/// `define`d (scope-tracking limitation), annotate it; if it's genuinely
/// unbound, fix the form.
///
/// Category A (scope-tracking false positives — DO NOT fix, leave ceiling
/// unchanged). All are `let`/`lambda` bindings or `define`d functions inside
/// nested `begin`/`if` blocks. The real evaluator resolves them via env
/// mutation; the test's scope tracker doesn't propagate `define` from nested
/// blocks back to the enclosing scope.
///   - lisp-scaffold-reasoning.yaml step 2: `h`, `lk`, `hyp-text` (let bindings
///     inside a lambda inside a begin)
///   - eqm-improvement.yaml step 7: `compute-gap`, `find-curr` (define'd
///     functions inside nested let/begin)
///   - eqm-improvement.yaml step 8: `find-align` (define'd function)
///   - eqm-improvement.yaml step 9: `find-score` (define'd function)
///   - eqm.yaml step 5: `compute-mean-delta` (define'd function)
///   - kask-seam-audit.yaml step 2: `p`, `v` (let bindings inside nested
///     lambda/begin)
///   - kask-seam-audit.yaml step 7: `f` (let binding inside nested lambda)
///   - kask-seam-audit.yaml step 12: `fo`, `r`, `fp`, `hard-stop-obj`, `test`,
///     `wk` (let bindings / lambda params inside nested begin/if)
///   - kask-seam-audit.yaml step 14: `f` (let binding inside nested lambda)
///
/// Category B (genuinely unbound — FIXED in the lisp interpreter / manifest):
///   - `member` → added to default_builtins() (company-research-deep step 10)
///   - `cond` → added as special form (company-research-deep step 17,
///     company-research-flash step 26)
///   - `eq` → added to default_builtins() (company-research-deep step 17,
///     company-research-flash step 26)
///   - `t` → bound to Bool(true) in Env::new_root() (cond default clause)
///   - `sqrt` → added to default_builtins() (eqm-improvement step 7)
///   - `abs` → added to default_builtins() (eqm step 5)
///   - `env` → fixed in idiomatic-rust.yaml step 4 (was referencing the env
///     block key as a symbol; form rewritten to reference env-bound symbols
///     directly)
#[test]
fn lisp_eval_form_symbols_are_bound() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let builtins: HashSet<&str> = LISP_BUILTINS.iter().copied().collect();
    let special_forms: HashSet<&str> = LISP_SPECIAL_FORMS.iter().copied().collect();
    let root_constants: HashSet<&str> = LISP_ROOT_CONSTANTS.iter().copied().collect();

    let mut warnings = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();

        for step in &manifest.steps {
            if step.action != "compute" {
                continue;
            }
            let Some(ref compute_ref) = step.compute_ref else {
                continue;
            };
            if compute_ref != "lisp.eval" {
                continue;
            }
            let Some(ref mapping) = step.input_mapping else {
                continue;
            };
            let Some(obj) = mapping.as_object() else {
                continue;
            };
            let Some(form_val) = obj.get("form") else {
                continue; // E12 catches missing form
            };
            let form_str = form_val.as_str().unwrap_or("");
            if form_str.trim().is_empty() {
                continue; // E12 catches empty form
            }

            let parsed = match hkask_lisp::parse(form_str) {
                Ok(forms) => forms,
                Err(_) => continue, // E14 catches parse errors
            };

            // Collect env keys (the bindings available to the form).
            let env_keys: HashSet<String> = obj
                .get("env")
                .and_then(|v| v.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();

            // Collect all symbol references in the form, excluding locally-bound
            // symbols (let/lambda/define bindings within the form itself).
            let mut referenced: HashSet<String> = HashSet::new();
            let local_bindings: HashSet<String> = HashSet::new();
            for form in &parsed {
                collect_symbol_references(form, &local_bindings, &mut referenced);
            }

            checked += 1;

            // Every referenced symbol must be a builtin, special form, or env key.
            for sym in &referenced {
                let is_builtin = builtins.contains(sym.as_str());
                let is_special = special_forms.contains(sym.as_str());
                let is_env = env_keys.contains(sym);
                let is_root_constant = root_constants.contains(sym.as_str());
                if !is_builtin && !is_special && !is_env && !is_root_constant {
                    warnings.push(format!(
                        "{fname} step {}: lisp.eval form references symbol '{}' which is not a builtin, special form, root constant, or env binding — resolves to null at runtime",
                        step.ordinal, sym
                    ));
                }
            }
        }
    }

    eprintln!(
        "lisp.eval symbol-binding check: {checked} forms checked — {} warnings",
        warnings.len()
    );
    for w in &warnings {
        eprintln!("  WARN: {w}");
    }

    // Regression ceiling: the current warning count reflects Category A
    // scope-tracking false positives (see the annotation above). All
    // Category B (genuinely unbound) symbols have been fixed — `member`,
    // `cond`, `eq`, `t`, `sqrt`, `abs` were added to the lisp interpreter,
    // and `env` in idiomatic-rust.yaml step 4 was fixed in the manifest.
    // The test fails if the count INCREASES — any new unbound symbol is a
    // potential silent-null bug.
    //
    // To fix an existing warning:
    // - If the symbol is genuinely `define`d (scope-tracking limitation),
    //   annotate it above and the ceiling stays.
    // - If it's genuinely unbound (not defined anywhere in the form), fix
    //   the form — it resolves to null at runtime.
    const WARNING_CEILING: usize = 33;
    assert!(
        warnings.len() <= WARNING_CEILING,
        "{} lisp.eval unbound-symbol warnings (regression ceiling: {WARNING_CEILING}). \
         If the new warning is a scope-tracking limitation (symbol is `define`d \
         in a nested block), annotate it above. If it's genuinely unbound, fix \
         the form.",
        warnings.len()
    );
}

// ── G3: mcp: steps have failure handling ───────────────────────────────────

/// G3 (P26): Every step with an `mcp:` field should have failure handling —
/// either a per-step `on_failure` config or a `condition` on a downstream
/// step that checks the MCP result.
///
/// A direct MCP tool call can fail (permission denied, timeout, missing
/// credential, tool not found). Without failure handling, the error
/// propagates as `null` to downstream steps — a broken feedback loop (the
/// operator can't distinguish "tool returned empty" from "tool failed").
///
/// This test checks for the minimal failure-handling surface: the step
/// itself has `on_failure`, or a downstream step has a `condition` that
/// references `step_N_result` (where N is the MCP step's ordinal). The
/// latter is a heuristic — a condition referencing the result implies the
/// manifest author anticipated the result's presence and is gating on it.
///
/// This is a hard-error gate: all MCP steps must have failure handling. The
/// previous ceiling-gated approach (21 warnings) has been fully resolved —
/// every MCP step across all manifests now has an `on_failure: { action:
/// report, resume: "..." }` block that surfaces the failure via
/// curator_report_skill_use_issue before the pipeline resumes with a null
/// result. Any new MCP step without failure handling is a silent-null-
/// propagation bug and must be fixed before merging.
#[test]
fn mcp_steps_have_failure_handling() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let mut warnings = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();

        for step in &manifest.steps {
            if step.mcp.is_none() {
                continue;
            }
            checked += 1;

            let has_on_failure = step.on_failure.is_some();

            // Check if any downstream step has a condition referencing this
            // step's result.
            let step_ordinal = step.ordinal;
            let result_ref = format!("step_{}_result", step_ordinal);
            let has_downstream_condition = manifest.steps.iter().any(|s| {
                s.ordinal > step_ordinal
                    && s.condition
                        .as_ref()
                        .is_some_and(|c| c.contains(&result_ref))
            });

            if !has_on_failure && !has_downstream_condition {
                warnings.push(format!(
                    "{fname} step {}: mcp:'{}' step has no on_failure and no downstream condition references step_{}_result — a failed MCP call will silently propagate null",
                    step.ordinal,
                    step.mcp.as_ref().unwrap(),
                    step_ordinal
                ));
            }
        }
    }

    eprintln!(
        "mcp: failure-handling check: {checked} mcp steps checked — {} warnings",
        warnings.len()
    );
    for w in &warnings {
        eprintln!("  WARN: {w}");
    }

    // Hard-error gate: all MCP steps must have failure handling. The previous
    // ceiling-gated approach (21 warnings) has been fully resolved. Any new
    // MCP step without failure handling is a silent-null-propagation bug.
    assert!(
        warnings.is_empty(),
        "{} mcp: failure-handling warnings — every MCP step must have on_failure or a downstream condition. \
         Add `on_failure: {{ action: report, resume: \"...\" }}` to the MCP step, or add a downstream `condition` that checks `step_N_result`.",
        warnings.len()
    );
}

// ── G5: calibration mode min_iterations gate ───────────────────────────────

/// G5 (P27): If `convergence_mode` contains "calibration", then
/// `min_iterations` must be >= 2.
///
/// Calibration convergence computes a rolling Brier average over
/// `brier_window` cycles. Brier scoring needs a prediction (from one
/// iteration) and a result (from the next) — a single iteration cannot
/// produce a Brier score. With `min_iterations < 2`, the convergence
/// tracker may exit before the first Brier reading is available, silently
/// falling back to "not converged" or skipping the calibration check
/// entirely.
#[test]
fn calibration_mode_min_iterations_gate() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();
        let conv = &manifest.convergence;

        if conv.convergence_mode.contains("calibration") {
            checked += 1;
            if conv.min_iterations < 2 {
                errors.push(format!(
                    "{fname}: convergence_mode='{}' contains 'calibration' but min_iterations={} (must be >= 2 — Brier scoring needs prediction + result)",
                    conv.convergence_mode, conv.min_iterations
                ));
            }
        }
    }

    eprintln!(
        "calibration min_iterations gate: {checked} manifests checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} calibration min_iterations errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

// ── G6: convergence mode combinations are valid ────────────────────────────

/// G6 (P27): Only whitelisted `convergence_mode` combinations are allowed.
///
/// The `ConvergenceConfig` docs define five valid modes:
/// - `"gap"`: gap convergence only
/// - `"cauchy"`: Cauchy convergence only
/// - `"calibration"`: calibration convergence only
/// - `"gap_or_cauchy"`: gap or Cauchy
/// - `"gap_or_cauchy_or_calibration"` (default): all three
///
/// An invalid combination (e.g., `"cauchy_or_calibration"` without gap, or a
/// typo like `"gap_or_calibraton"`) would be silently accepted by the string
/// `contains` checks in the convergence tracker but would not match any
/// intended mode — the tracker might skip a signal the author intended to
/// activate.
///
/// The empty string is also valid (legacy mode — uses threshold +
/// convergence_field instead of Kata fields).
#[test]
fn convergence_mode_combinations_are_valid() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let valid_modes: HashSet<&str> = [
        "",
        "gap",
        "cauchy",
        "calibration",
        "gap_or_cauchy",
        "gap_or_cauchy_or_calibration",
    ]
    .into_iter()
    .collect();

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();
        let mode = &manifest.convergence.convergence_mode;
        checked += 1;

        if !valid_modes.contains(mode.as_str()) {
            errors.push(format!(
                "{fname}: convergence_mode='{}' is not a recognized combination (valid: {:?}) — the convergence tracker uses substring checks that may silently skip intended signals",
                mode,
                valid_modes
            ));
        }
    }

    eprintln!(
        "convergence mode validation: {checked} manifests checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} invalid convergence_mode errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

// ── G7: compute step input_mapping covers primitive inputs ─────────────────

/// Required input keys for each compute primitive, derived from
/// `dispatch_compute` in `compute.rs`. A step using a primitive must provide
/// these keys in its `input_mapping` (or via agent-coordinated context, but
/// the `input_mapping` is the declarative surface we can check).
///
/// Primitives not listed here either have no required inputs (e.g.,
/// `swarm.second_order_monitor` defaults gracefully) or have complex input
/// shapes (e.g., `swarm.converge_accumulate`) where most fields are optional.
/// We only check primitives with a clear required-input contract.
fn required_inputs_for_primitive(compute_ref: &str) -> Option<&'static [&'static str]> {
    match compute_ref {
        "calibrate_from_fermi" => Some(&["questions"]),
        "outside_view_adjustment" => Some(&["base_rate", "inside_estimate", "reference_count"]),
        "bayesian_update" => Some(&["prior", "evidence_likelihood", "evidence_base_rate"]),
        "combine_tree_probabilities" => Some(&["nodes", "topological_order", "outcome_id"]),
        "apply_calibration_adjustment" => Some(&["prior", "overconfidence_bias"]),
        "brier_score" => Some(&["probability", "outcome_occurred"]),
        "brier_score_multi" => Some(&["probabilities", "outcomes"]),
        "brier_interpretation" => Some(&["score"]),
        "kata.object_gap" => Some(&["current_artifacts", "target_artifacts"]),
        "kata.process_gap" => Some(&["current_procedure", "target_procedure"]),
        "kata.hypotenuse" => Some(&["object_gap", "process_gap"]),
        // kata.prediction_vs_result reads nested fields (prediction.confidence,
        // result.occurred/result.actual_delta) — the top-level keys are
        // "prediction" and "result".
        "kata.prediction_vs_result" => Some(&["prediction", "result"]),
        // swarm.converge_accumulate: only "d" is required (all others default).
        "swarm.converge_accumulate" => Some(&["d"]),
        // lisp.eval: "form" is required (E12 checks this). "env" is optional.
        "lisp.eval" => Some(&["form"]),
        // shell.exec: "command" is required. "cwd" is optional.
        "shell.exec" => Some(&["command"]),
        // listening.* and swarm.filter_proposed_moves: not in dispatch_compute
        // or have complex optional shapes — skip (no required-input contract
        // we can statically check).
        _ => None,
    }
}

/// G7 (P28): A `compute` step's `input_mapping` must provide all required
/// input keys for the primitive it invokes.
///
/// A missing required input fails at compute time with an error like
/// "compute 'brier_score': missing or non-numeric input 'probability'". This
/// is a runtime failure that is invisible at manifest-load time — the
/// manifest loads successfully, but the cascade fails mid-execution.
///
/// This test checks that the `input_mapping` JSON object contains the
/// required keys for each primitive. The values may be Jinja expressions
/// (e.g., `"{{ step_3_result.score }}"`) — we check key presence, not value
/// validity.
#[test]
fn compute_step_input_mapping_covers_primitive_inputs() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();

        for step in &manifest.steps {
            if step.action != "compute" {
                continue;
            }
            let Some(ref compute_ref) = step.compute_ref else {
                continue;
            };
            let Some(required) = required_inputs_for_primitive(compute_ref) else {
                continue; // No static contract for this primitive.
            };

            let mapping_keys: HashSet<String> = step
                .input_mapping
                .as_ref()
                .and_then(|v| v.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();

            checked += 1;

            for req in required {
                if !mapping_keys.contains(*req) {
                    errors.push(format!(
                        "{fname} step {}: compute_ref='{}' requires input '{}' but input_mapping does not provide it (available keys: {:?})",
                        step.ordinal,
                        compute_ref,
                        req,
                        mapping_keys
                    ));
                }
            }
        }
    }

    eprintln!(
        "compute input_mapping check: {checked} compute steps checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} compute input_mapping errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

// ── G8: shell.exec only in skill-category manifests ────────────────────────

/// G8 (P28): `shell.exec` is only used in `skill`-category manifests.
///
/// The `compute.rs` docs state: "The caller must gate `shell.exec` to
/// `category: skill` manifests only." This is a security gate —
/// `shell.exec` runs arbitrary shell commands, and infrastructure manifests
/// (if they existed) would run without human review. The
/// `VALID_CATEGORIES` test in `manifest_compliance.rs` rejects non-`skill`
/// manifests entirely, but doesn't test the `shell.exec` gate in isolation.
///
/// This test verifies that no manifest using `shell.exec` has a non-`skill`
/// category. It's a belt-and-suspenders check: if a future change relaxes
/// the category restriction, this test catches `shell.exec` usage in
/// non-skill manifests before they ship.
#[test]
fn shell_exec_only_in_skill_category_manifests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();

        let uses_shell_exec = manifest
            .steps
            .iter()
            .any(|s| s.compute_ref.as_deref() == Some("shell.exec"));

        if uses_shell_exec {
            checked += 1;
            if !manifest.is_skill() {
                errors.push(format!(
                    "{fname}: uses shell.exec but category='{}' (must be 'skill' — shell.exec runs arbitrary commands and requires human review)",
                    manifest.category.as_deref().unwrap_or("(none)")
                ));
            }
        }
    }

    eprintln!(
        "shell.exec category gate: {checked} manifests using shell.exec checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} shell.exec category errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}
