#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Sandboxed Lisp interpreter for deterministic manifest compute steps.
//!
//! Design goals (following the `rust_lisp` reference by brundonsmith):
//! - Small footprint, no runtime dependencies beyond serde_json
//! - No I/O, no filesystem, no network, no environment variable access
//! - Bounded recursion depth (1024) and bounded evaluation steps (100000)
//! - JSON-native: input env is `serde_json::Value`, output is `serde_json::Value`
//! - JSON objects become association lists — the classic Lisp data structure
//!
//! The interpreter supports a minimal but practical Lisp subset:
//!   Special forms: quote, if, let, lambda, define, begin, and, or, not, cond
//!   Built-in functions: car, cdr, cons, list, length, nth, reverse,
//!     +, -, *, /, =, !=, <, <=, >, >=, is_null, numberp, listp, stringp,
//!     assoc, assoc!, append, string=, string-contains, starts-with,
//!     ends-with, concat, boundary-ok, abs, sqrt, max, min, eq, member
//!
//! Integer arithmetic is checked (overflow is a typed `Runtime` error, never
//! a silent wrap), and all-Int comparisons/extrema are exact i64. Float
//! operations require finite operands/results and exact integer-to-f64
//! promotion; division still returns a Float.
//!
//! `assoc` is defensive on `Nil` and `String` (LLM step outputs that reached
//! forms as JSON strings return nil so `is_null` guards work), while
//! rejecting non-string primitive scalars (`Int`, `Float`, `Bool`) as the
//! alist argument.
//!
//! # Absent-key convention (`assoc` vs `assoc!`, operator ruling 2026-10-14)
//!
//! - `(assoc key alist)` returns `nil` when `key` is absent; `(string= nil "x")`
//!   returns `false` (null-safe for optional fields, while non-string scalars
//!   like `(string= 1 "1")` error with `TypeError`).
//! - `(assoc! key alist)` is the strict required-key lookup: when `key` is
//!   absent, it errors with `Runtime` naming the missing key and the sorted
//!   list of available keys on `alist` — closing the absent-key vacuity trap
//!   (where a misspelled key like `"tier"` instead of `"verdict"` silently
//!   returned `nil` on every element).
//!
//! # Infix Operator Notation
//!
//! Infix is a token-level rewrite BEFORE parsing (all five behaviors
//! verified live and pinned by `infix_expansion_behavior_is_pinned`):
//! - Bare `a + b` expands to `(+ a b)`; chained same-operator `a + b + c`
//!   folds to `(+ a b c)`.
//! - MIXED operators do NOT associate: `1 + 2 * 3` parses as three
//!   top-level forms `(+ 1 2)`, `*`, `3` and the last form's value (3)
//!   wins.
//! - A PARENTHESIZED `(a + b)` double-wraps to `((+ a b))` and errors
//!   (expected callable) — NOT equivalent to `(+ a b)`.
//! - The rewriter also fires inside prefix forms: `(- 5 - 3)` becomes
//!   `(- (- 5 3))` → -2.
//! Prefer prefix forms around `let`, `if`, recursion, and nested logic.
//!
//! # Security
//!
//! No `eval` builtin (Lisp code cannot evaluate arbitrary strings). No
//! `load` or `require`. The environment is immutable from Lisp's perspective
//! (define mutates a local scope discarded after evaluation). Bounded by
//! `max_steps` and `max_depth` at the sandbox entry points; lower-level raw
//! APIs remain caller-managed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use thiserror::Error;

/// Provisional H4A source admission ceiling in UTF-8 bytes, not an H4C safety measurement.
pub const MAX_SOURCE_BYTES: usize = 65_536;
/// Provisional H4A raw JSON env ceiling; enforced by the tool before decoding.
pub const MAX_ENV_JSON_BYTES: usize = 1_048_576;
/// Provisional H4A requested-work ceiling; the default remains 100,000.
pub const MAX_STEPS: u64 = 2_000_000;
/// Provisional H4A requested-depth ceiling; the default remains 1,024.
pub const MAX_DEPTH: u64 = 8192;
/// H4B ceiling for the pretty JSON result document, excluding tool/MCP envelopes.
pub const MAX_OUTPUT_BYTES: usize = 1_048_576;

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum LispError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("Lisp work exceeded max_steps ({0}) — input, computation, or output is too large")]
    StepLimitExceeded(u64),
    #[error("Lisp exceeded max_depth ({0}) — input nesting or evaluation recursion is too deep")]
    DepthLimitExceeded(u64),
    #[error("Lisp output exceeds the JSON nesting limit ({0})")]
    OutputDepthLimitExceeded(u64),
    #[error("type error: expected {expected}, got {actual}")]
    TypeError { expected: String, actual: String },
    #[error("unbound symbol: {0}")]
    UnboundSymbol(String),
    #[error("arity error: {0}")]
    Arity(String),
    #[error("Lisp evaluation cancelled")]
    Cancelled,
}

// ── Lisp value ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum LispValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Symbol(String),
    /// Linked list (cons cells). `Rc` makes cloning cheap.
    List(Rc<List>),
    /// First-class function (closure capturing its definition environment).
    Lambda {
        params: Vec<String>,
        body: Rc<LispValue>,
        env: Rc<RefCell<Env>>,
    },
    /// Native Rust function.
    NativeFunc(NativeFn),
}

/// Linked list — mirrors `rust_lisp::List`.
#[derive(Debug, Clone)]
pub struct List {
    pub head: LispValue,
    pub tail: Option<Rc<List>>,
}

impl Drop for List {
    #[stacksafe::stacksafe]
    fn drop(&mut self) {
        // Drop nested heads on a growable stack and flat tails iteratively.
        drop(std::mem::replace(&mut self.head, LispValue::Nil));
        let mut tail = self.tail.take();
        while let Some(node) = tail {
            match Rc::try_unwrap(node) {
                Ok(mut node) => tail = node.tail.take(),
                Err(_) => break,
            }
        }
    }
}

impl List {
    pub fn nil() -> Rc<List> {
        Rc::new(List {
            head: LispValue::Nil,
            tail: None,
        })
    }

    pub fn cons(head: LispValue, tail: Rc<List>) -> Rc<List> {
        Rc::new(List {
            head,
            tail: Some(tail),
        })
    }

    pub fn is_nil(&self) -> bool {
        matches!(self.head, LispValue::Nil) && self.tail.is_none()
    }

    pub fn len(&self) -> usize {
        let mut count = 0;
        let mut cursor: Option<&List> = Some(self);
        while let Some(node) = cursor {
            if node.is_nil() {
                break;
            }
            count += 1;
            cursor = node.tail.as_deref();
        }
        count
    }

    pub fn is_empty(&self) -> bool {
        self.is_nil()
    }

    fn iter(&self) -> impl Iterator<Item = &LispValue> {
        std::iter::successors(Some(self), |node| node.tail.as_deref())
            .take_while(|node| !node.is_nil())
            .map(|node| &node.head)
    }

    pub fn to_vec(&self) -> Vec<LispValue> {
        self.iter().cloned().collect()
    }

    pub fn from_vec(items: Vec<LispValue>) -> Rc<List> {
        let mut list = List::nil();
        for item in items.into_iter().rev() {
            list = List::cons(item, list);
        }
        list
    }
}

pub type NativeFn =
    fn(&Rc<RefCell<Env>>, &[LispValue], &mut EvalBudget) -> Result<LispValue, LispError>;

impl PartialEq for LispValue {
    #[stacksafe::stacksafe]
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (LispValue::Nil, LispValue::Nil) => true,
            (LispValue::Bool(a), LispValue::Bool(b)) => a == b,
            (LispValue::Int(a), LispValue::Int(b)) => a == b,
            (LispValue::Float(a), LispValue::Float(b)) => a == b,
            (LispValue::String(a), LispValue::String(b)) => a == b,
            (LispValue::Symbol(a), LispValue::Symbol(b)) => a == b,
            (LispValue::List(a), LispValue::List(b)) => a.to_vec() == b.to_vec(),
            // Lambdas and native functions are never structurally equal —
            // comparing closures by value is unsound. Two distinct lambda
            // values are always unequal, even if they have the same source.
            // This is correct for `assoc` (which compares keys, not functions)
            // and for `=` (which should only be called on numbers).
            (LispValue::Lambda { .. }, LispValue::Lambda { .. }) => false,
            (LispValue::NativeFunc(_), LispValue::NativeFunc(_)) => false,
            _ => false,
        }
    }
}

// ── Environment ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Env {
    pub vars: HashMap<String, LispValue>,
    pub parent: Option<Rc<RefCell<Env>>>,
}

impl Env {
    pub fn new_root() -> Self {
        let mut vars = HashMap::new();
        for (name, f) in default_builtins() {
            vars.insert(name.to_string(), LispValue::NativeFunc(f));
        }
        // `t` is the canonical truth constant — bound in the root env so it
        // resolves to Bool(true) when referenced as a bare symbol (e.g. the
        // default clause of `cond`: `(t default)`). A `let` binding named `t`
        // shadows it, which is correct Lisp behavior.
        vars.insert("t".to_string(), LispValue::Bool(true));
        Env { vars, parent: None }
    }

    pub fn child(parent: Rc<RefCell<Env>>) -> Self {
        Env {
            vars: HashMap::new(),
            parent: Some(parent),
        }
    }

    pub fn lookup(&self, sym: &str) -> Option<LispValue> {
        self.vars
            .get(sym)
            .cloned()
            .or_else(|| self.parent.as_ref().and_then(|p| p.borrow().lookup(sym)))
    }

    pub fn define(&mut self, name: String, value: LispValue) {
        self.vars.insert(name, value);
    }
}

// ── Parser ─────────────────────────────────────────────────────────────────

pub fn parse(source: &str) -> Result<Vec<LispValue>, LispError> {
    parse_with_budget(source, &mut EvalBudget::new(100_000, 1024))
}

fn parse_with_budget(source: &str, budget: &mut EvalBudget) -> Result<Vec<LispValue>, LispError> {
    // Refuse oversized input before tokenization allocates a copy of every token.
    if source.len() > MAX_SOURCE_BYTES {
        return Err(LispError::Parse(format!(
            "source requested {} UTF-8 bytes; allowed {MAX_SOURCE_BYTES}",
            source.len()
        )));
    }
    budget.charge(source.len())?;
    let tokens = tokenize(source, budget)?;
    let tokens = expand_infix(&tokens, budget)?;
    let mut forms = Vec::new();
    let mut rest: &[String] = &tokens;
    while !rest.is_empty() {
        let (form, next) = parse_form(rest, budget)?;
        forms.push(form);
        rest = next;
    }
    Ok(forms)
}

/// Operators that support infix notation: `a + b` → `(+ a b)`.
const INFIX_OPERATORS: &[&str] = &["+", "-", "*", "/", "=", "!=", "<", "<=", ">", ">="];

/// Transform infix operator triplets to prefix form at the token level.
///
/// Scans the token stream for patterns like `a + b` (where the middle token
/// is an operator and the surrounding tokens are not parens) and rewrites them
/// to `(+ a b)`. Handles chained same-operator expressions: `a + b + c` →
/// `(+ a b c)`. Does NOT support operator precedence — mixing operators
/// requires explicit parentheses: `(a + b) * c` works, `a + b * c` does not.
///
/// This is a pre-processing step before `parse_form` — the evaluator sees
/// only standard prefix S-expressions.
fn expand_infix(tokens: &[String], budget: &EvalBudget) -> Result<Vec<String>, LispError> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        check_cancellation(budget.cancellation.as_deref(), "infix", 1)?;
        // Check if tokens[i] is an atom (not a paren or quote) followed by
        // an operator followed by another atom.
        if i + 2 < tokens.len()
            && is_infix_context(&tokens[i])
            && INFIX_OPERATORS.contains(&tokens[i + 1].as_str())
            && is_infix_context(&tokens[i + 2])
        {
            // Collect the operator and all chained operands: a + b + c → (+ a b c)
            let op = tokens[i + 1].clone();
            let mut operands = vec![tokens[i].clone()];
            let mut j = i + 1;
            while j + 1 < tokens.len() && tokens[j] == op && is_infix_context(&tokens[j + 1]) {
                check_cancellation(budget.cancellation.as_deref(), "infix", 2)?;
                operands.push(tokens[j + 1].clone());
                j += 2;
            }
            // Emit: ( op operand1 operand2 ... )
            out.push("(".to_string());
            out.push(op);
            for operand in &operands {
                check_cancellation(budget.cancellation.as_deref(), "infix", 1)?;
                out.push(operand.clone());
            }
            out.push(")".to_string());
            i = j;
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    Ok(out)
}

/// Check if a token is a valid context for infix transformation — it's an
/// atom (symbol, number, string, true, false, nil) but not a paren or quote.
fn is_infix_context(tok: &str) -> bool {
    tok != "(" && tok != ")" && tok != "'"
}

fn tokenize(source: &str, budget: &EvalBudget) -> Result<Vec<String>, LispError> {
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some(&(offset, c)) = chars.peek() {
        check_cancellation(budget.cancellation.as_deref(), "source", offset)?;
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == ';' {
            for (offset, c) in chars.by_ref() {
                check_cancellation(budget.cancellation.as_deref(), "source", offset)?;
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '(' || c == ')' || c == '\'' {
            tokens.push(c.to_string());
            chars.next();
            continue;
        }
        if c == '"' {
            chars.next();
            let mut s = String::from("\"");
            while let Some(&(offset, c)) = chars.peek() {
                check_cancellation(budget.cancellation.as_deref(), "source", offset)?;
                chars.next();
                s.push(c);
                if c == '\\' {
                    if let Some(&(offset, next)) = chars.peek() {
                        check_cancellation(budget.cancellation.as_deref(), "source", offset)?;
                        chars.next();
                        s.push(next);
                    }
                    continue;
                }
                if c == '"' {
                    break;
                }
            }
            tokens.push(s);
            continue;
        }
        let mut atom = String::new();
        while let Some(&(offset, c)) = chars.peek() {
            check_cancellation(budget.cancellation.as_deref(), "source", offset)?;
            if c.is_whitespace() || c == '(' || c == ')' || c == ';' || c == '\'' {
                break;
            }
            atom.push(c);
            chars.next();
        }
        if !atom.is_empty() {
            tokens.push(atom);
        }
    }
    check_cancellation(budget.cancellation.as_deref(), "source", source.len())?;
    Ok(tokens)
}

#[stacksafe::stacksafe]
fn parse_form<'a>(
    tokens: &'a [String],
    budget: &mut EvalBudget,
) -> Result<(LispValue, &'a [String]), LispError> {
    budget.tick()?;
    if tokens.is_empty() {
        return Err(LispError::Parse("unexpected end of input".into()));
    }
    let tok = &tokens[0];
    let rest = &tokens[1..];

    if tok == "(" {
        budget.enter()?;
        let mut items = Vec::new();
        let mut remaining = rest;
        loop {
            if remaining.is_empty() {
                return Err(LispError::Parse("unbalanced parenthesis".into()));
            }
            if remaining[0] == ")" {
                budget.exit();
                return Ok((LispValue::List(budget.make_list(items)?), &remaining[1..]));
            }
            let (form, next) = parse_form(remaining, budget)?;
            items.push(form);
            remaining = next;
        }
    }
    if tok == ")" {
        return Err(LispError::Parse("unexpected ')'".into()));
    }
    if tok == "'" {
        budget.enter()?;
        let (form, next) = parse_form(rest, budget)?;
        budget.exit();
        let quoted =
            LispValue::List(budget.make_list(vec![LispValue::Symbol("quote".into()), form])?);
        return Ok((quoted, next));
    }
    Ok((parse_atom(tok, budget)?, rest))
}

fn parse_atom(tok: &str, budget: &EvalBudget) -> Result<LispValue, LispError> {
    budget.check_cancelled()?;
    if tok.starts_with('"') && tok.ends_with('"') && tok.len() >= 2 {
        return Ok(LispValue::String(unescape_string(
            &tok[1..tok.len() - 1],
            budget,
        )?));
    }
    if let Ok(i) = tok.parse::<i64>() {
        return Ok(LispValue::Int(i));
    }
    let digits = tok.strip_prefix(['+', '-']).unwrap_or(tok);
    let mut integer_spelling = !digits.is_empty();
    for byte in digits.bytes() {
        check_cancellation(budget.cancellation.as_deref(), "numeric-source", 1)?;
        if !byte.is_ascii_digit() {
            integer_spelling = false;
            break;
        }
    }
    if integer_spelling {
        return Err(LispError::Parse(format!(
            "integer literal {tok} is outside signed i64 range ({}..={})",
            i64::MIN,
            i64::MAX
        )));
    }
    if let Ok(f) = tok.parse::<f64>() {
        if !f.is_finite() {
            return Err(LispError::Parse(format!(
                "non-finite numeric literal {tok}; use a finite f64 value"
            )));
        }
        return Ok(LispValue::Float(f));
    }
    Ok(match tok {
        "true" => LispValue::Bool(true),
        "false" => LispValue::Bool(false),
        "nil" => LispValue::Nil,
        _ => LispValue::Symbol(tok.to_string()),
    })
}

fn unescape_string(s: &str, budget: &EvalBudget) -> Result<String, LispError> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        check_cancellation(
            budget.cancellation.as_deref(),
            "unescape",
            s.len() - chars.as_str().len(),
        )?;
        if c == '\\' {
            let next = chars.next();
            check_cancellation(
                budget.cancellation.as_deref(),
                "unescape",
                s.len() - chars.as_str().len(),
            )?;
            match next {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

// The caller stores true with Release; no invocation clears or resets it.
// Test observers run before the load, so a parked real worker resumes into the
// same checkpoint that must observe the caller's cancellation request.
fn check_cancellation(
    cancellation: Option<&AtomicBool>,
    _phase: &'static str,
    _units: usize,
) -> Result<(), LispError> {
    #[cfg(test)]
    tests::observe_checkpoint(_phase, _units);
    if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(LispError::Cancelled);
    }
    Ok(())
}

// ── Evaluator ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct InvocationScopes {
    scopes: RefCell<Vec<Weak<RefCell<Env>>>>,
}

impl Drop for InvocationScopes {
    fn drop(&mut self) {
        // Hold every survivor before breaking any links, including cycles in
        // child scopes no longer reachable from the root. Weak tracking lets
        // acyclic call scopes release their payloads during evaluation.
        let survivors: Vec<_> = self
            .scopes
            .get_mut()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for scope in &survivors {
            let mut scope = scope.borrow_mut();
            scope.vars.clear();
            scope.parent = None;
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvalBudget {
    pub max_steps: u64,
    pub max_depth: u64,
    steps_used: u64,
    depth_current: u64,
    invocation_scopes: Option<Rc<InvocationScopes>>,
    cancellation: Option<Arc<AtomicBool>>,
}

impl EvalBudget {
    pub fn new(max_steps: u64, max_depth: u64) -> Self {
        EvalBudget {
            max_steps,
            max_depth,
            steps_used: 0,
            depth_current: 0,
            invocation_scopes: None,
            cancellation: None,
        }
    }

    fn check_cancelled(&self) -> Result<(), LispError> {
        check_cancellation(self.cancellation.as_deref(), "work", 0)
    }

    fn collect_list(&self, list: &List) -> Result<Vec<LispValue>, LispError> {
        // Keep the checked walk explicit rather than collecting through Result's shunt.
        let mut values = Vec::new();
        for (index, value) in list.iter().enumerate() {
            check_cancellation(self.cancellation.as_deref(), "list", index)?;
            values.push(value.clone());
        }
        Ok(values)
    }

    fn make_list(&self, items: Vec<LispValue>) -> Result<Rc<List>, LispError> {
        let mut list = List::nil();
        for (index, item) in items.into_iter().rev().enumerate() {
            check_cancellation(self.cancellation.as_deref(), "list-build", index)?;
            list = List::cons(item, list);
        }
        Ok(list)
    }

    fn lookup(&self, env: &Env, symbol: &str) -> Result<Option<LispValue>, LispError> {
        check_cancellation(self.cancellation.as_deref(), "lookup", 1)?;
        if let Some(value) = env.vars.get(symbol) {
            return Ok(Some(value.clone()));
        }
        let mut parent = env.parent.clone();
        while let Some(scope) = parent {
            check_cancellation(self.cancellation.as_deref(), "lookup", 1)?;
            let scope = scope.borrow();
            if let Some(value) = scope.vars.get(symbol) {
                return Ok(Some(value.clone()));
            }
            parent = scope.parent.clone();
        }
        Ok(None)
    }

    fn new_scope(&mut self, env: Env) -> Result<Rc<RefCell<Env>>, LispError> {
        if self.invocation_scopes.is_some() {
            // Prepay registration and cleanup traversal before allocating or
            // tracking the scope. Drop stays infallible; this is not a heap quota.
            self.charge(2)?;
        }
        let scope = Rc::new(RefCell::new(env));
        if let Some(registry) = &self.invocation_scopes {
            registry.scopes.borrow_mut().push(Rc::downgrade(&scope));
        }
        #[cfg(test)]
        tests::observe_scope(&scope);
        Ok(scope)
    }

    fn tick(&mut self) -> Result<(), LispError> {
        self.charge(1)
    }

    fn charge(&mut self, work: usize) -> Result<(), LispError> {
        self.check_cancelled()?;
        let work = u64::try_from(work).map_err(|_| LispError::StepLimitExceeded(self.max_steps))?;
        if work > self.max_steps.saturating_sub(self.steps_used) {
            return Err(LispError::StepLimitExceeded(self.max_steps));
        }
        self.steps_used += work;
        Ok(())
    }

    fn enter(&mut self) -> Result<(), LispError> {
        if self.depth_current >= self.max_depth {
            return Err(LispError::DepthLimitExceeded(self.max_depth));
        }
        self.depth_current += 1;
        Ok(())
    }

    fn exit(&mut self) {
        self.depth_current = self.depth_current.saturating_sub(1);
    }
}

pub fn eval(env: Rc<RefCell<Env>>, form: &LispValue) -> Result<LispValue, LispError> {
    // Depth 1024: recursive helpers consume 2–4 frames per list element, so
    // the former 64 overflowed at ~16 elements — real-scale validation lists
    // failed their first attempt. Infinite recursion still trips immediately.
    let mut budget = EvalBudget::new(100000, 1024);
    eval_with_budget(env, form, &mut budget)
}

/// Depth is checked only for compound forms (lists) — atoms don't recurse
/// and don't consume stack frames. This prevents the depth budget from being
/// exhausted by argument evaluation while still bounding actual recursion.
#[stacksafe::stacksafe]
pub fn eval_with_budget(
    env: Rc<RefCell<Env>>,
    form: &LispValue,
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    budget.tick()?;
    let track_depth = matches!(form, LispValue::List(_));
    if track_depth {
        budget.enter()?;
    }
    let result = eval_inner(env, form, budget);
    if track_depth {
        budget.exit();
    }
    let value = result?;
    if let LispValue::Float(number) = &value {
        finite_number(*number, "evaluation")?;
    }
    Ok(value)
}

fn eval_inner(
    env: Rc<RefCell<Env>>,
    form: &LispValue,
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    match form {
        LispValue::Nil
        | LispValue::Bool(_)
        | LispValue::Int(_)
        | LispValue::Float(_)
        | LispValue::String(_) => Ok(form.clone()),

        LispValue::Symbol(s) => budget
            .lookup(&env.borrow(), s)?
            .ok_or_else(|| LispError::UnboundSymbol(s.clone())),

        LispValue::List(list) => {
            let items = budget.collect_list(list)?;
            if items.is_empty() || list.is_nil() {
                return Ok(LispValue::Nil);
            }
            let head = &items[0];
            if let LispValue::Symbol(name) = head {
                return eval_special_form(name, &items[1..], env, budget);
            }
            let func = eval_with_budget(env.clone(), head, budget)?;
            let args: Result<Vec<LispValue>, LispError> = items[1..]
                .iter()
                .map(|a| eval_with_budget(env.clone(), a, budget))
                .collect();
            apply(func, &args?, env, budget)
        }

        LispValue::Lambda { .. } | LispValue::NativeFunc(_) => Ok(form.clone()),
    }
}

fn eval_special_form(
    name: &str,
    args: &[LispValue],
    env: Rc<RefCell<Env>>,
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    match name {
        "quote" => {
            if args.len() != 1 {
                return Err(LispError::Arity("quote expects 1 arg".into()));
            }
            Ok(args[0].clone())
        }
        "if" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(LispError::Arity("if expects 2-3 args".into()));
            }
            let cond = eval_with_budget(env.clone(), &args[0], budget)?;
            if is_truthy(&cond) {
                eval_with_budget(env, &args[1], budget)
            } else if args.len() == 3 {
                eval_with_budget(env, &args[2], budget)
            } else {
                Ok(LispValue::Nil)
            }
        }
        "let" => {
            if args.len() != 2 {
                return Err(LispError::Arity("let expects (bindings body)".into()));
            }
            let bindings = match &args[0] {
                LispValue::List(b) => budget.collect_list(b)?,
                _ => return Err(LispError::Runtime("let bindings must be a list".into())),
            };
            let child_env = budget.new_scope(Env::child(env))?;
            for binding in bindings {
                let pair = match &binding {
                    LispValue::List(p) => budget.collect_list(p)?,
                    _ => return Err(LispError::Runtime("let binding must be a list".into())),
                };
                if pair.len() != 2 {
                    return Err(LispError::Arity("let binding must be (name value)".into()));
                }
                let name = match &pair[0] {
                    LispValue::Symbol(s) => s.clone(),
                    _ => {
                        return Err(LispError::Runtime(
                            "let binding name must be a symbol".into(),
                        ));
                    }
                };
                let value = eval_with_budget(child_env.clone(), &pair[1], budget)?;
                child_env.borrow_mut().define(name, value);
            }
            eval_with_budget(child_env, &args[1], budget)
        }
        "lambda" => {
            if args.len() != 2 {
                return Err(LispError::Arity("lambda expects (params body)".into()));
            }
            let params: Vec<String> = match &args[0] {
                LispValue::List(p) => budget
                    .collect_list(p)?
                    .iter()
                    .map(|v| {
                        budget.check_cancelled()?;
                        match v {
                            LispValue::Symbol(s) => Ok(s.clone()),
                            _ => Err(LispError::Runtime("lambda param must be a symbol".into())),
                        }
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err(LispError::Runtime("lambda params must be a list".into())),
            };
            Ok(LispValue::Lambda {
                params,
                body: Rc::new(args[1].clone()),
                env,
            })
        }
        "define" => {
            if args.len() != 2 {
                return Err(LispError::Arity("define expects (name value)".into()));
            }
            let name = match &args[0] {
                LispValue::Symbol(s) => s.clone(),
                _ => return Err(LispError::Runtime("define name must be a symbol".into())),
            };
            let value = eval_with_budget(env.clone(), &args[1], budget)?;
            env.borrow_mut().define(name, value);
            Ok(LispValue::Nil)
        }
        "begin" => {
            let mut result = LispValue::Nil;
            for form in args {
                result = eval_with_budget(env.clone(), form, budget)?;
            }
            Ok(result)
        }
        "and" => {
            let mut result = LispValue::Bool(true);
            for form in args {
                result = eval_with_budget(env.clone(), form, budget)?;
                if !is_truthy(&result) {
                    return Ok(LispValue::Bool(false));
                }
            }
            Ok(result)
        }
        "or" => {
            for form in args {
                let result = eval_with_budget(env.clone(), form, budget)?;
                if is_truthy(&result) {
                    return Ok(result);
                }
            }
            Ok(LispValue::Bool(false))
        }
        "not" => {
            if args.len() != 1 {
                return Err(LispError::Arity("not expects 1 arg".into()));
            }
            let v = eval_with_budget(env, &args[0], budget)?;
            Ok(LispValue::Bool(!is_truthy(&v)))
        }
        // `cond` — multi-clause conditional. Each clause is (test body...);
        // the first clause whose test is truthy has its body evaluated and
        // the result returned. A clause whose test is the symbol `t` (or the
        // literal `true`) is always taken — the standard default-clause idiom.
        // If no clause matches, returns Nil.
        //
        // Form: (cond (test1 body1...) (test2 body2...) ... (t default...))
        // This is the standard Lisp `cond`; it desugars to nested `if` but is
        // far more readable for multi-branch verdict dispatch (e.g. skill
        // convergence checks).
        "cond" => {
            for (index, clause) in args.iter().enumerate() {
                // Empty clauses skip collection/evaluation work checkpoints.
                check_cancellation(budget.cancellation.as_deref(), "cond", index)?;
                let clause_items = match clause {
                    LispValue::List(cl) => budget.collect_list(cl)?,
                    _ => {
                        return Err(LispError::Runtime("cond clause must be a list".into()));
                    }
                };
                if clause_items.is_empty() {
                    continue;
                }
                let test = eval_with_budget(env.clone(), &clause_items[0], budget)?;
                if is_truthy(&test) {
                    let mut result = LispValue::Nil;
                    for body_form in &clause_items[1..] {
                        result = eval_with_budget(env.clone(), body_form, budget)?;
                    }
                    return Ok(result);
                }
            }
            Ok(LispValue::Nil)
        }
        _ => {
            let func = budget
                .lookup(&env.borrow(), name)?
                .ok_or_else(|| LispError::UnboundSymbol(name.to_string()))?;
            let args_eval: Result<Vec<LispValue>, LispError> = args
                .iter()
                .map(|a| eval_with_budget(env.clone(), a, budget))
                .collect();
            apply(func, &args_eval?, env, budget)
        }
    }
}

fn apply(
    func: LispValue,
    args: &[LispValue],
    env: Rc<RefCell<Env>>,
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    match func {
        LispValue::NativeFunc(f) => {
            charge_native_arguments(args, budget)?;
            check_cancellation(budget.cancellation.as_deref(), "native-before", 0)?;
            let result = f(&env, args, budget);
            check_cancellation(budget.cancellation.as_deref(), "native-after", 0)?;
            result
        }
        LispValue::Lambda {
            params,
            body,
            env: closure_env,
        } => {
            if args.len() != params.len() {
                return Err(LispError::Arity(format!(
                    "lambda expected {} args, got {}",
                    params.len(),
                    args.len()
                )));
            }
            let call_env = budget.new_scope(Env::child(closure_env))?;
            for (param, arg) in params.iter().zip(args.iter()) {
                budget.check_cancelled()?;
                call_env.borrow_mut().define(param.clone(), arg.clone());
            }
            eval_with_budget(call_env, &body, budget)
        }
        _ => Err(LispError::TypeError {
            expected: "callable".into(),
            actual: type_of(&func),
        }),
    }
}

fn is_truthy(v: &LispValue) -> bool {
    match v {
        LispValue::Nil => false,
        LispValue::Bool(b) => *b,
        _ => true,
    }
}

fn type_of(v: &LispValue) -> String {
    match v {
        LispValue::Nil => "nil".into(),
        LispValue::Bool(_) => "boolean".into(),
        LispValue::Int(_) => "int".into(),
        LispValue::Float(_) => "float".into(),
        LispValue::String(_) => "string".into(),
        LispValue::Symbol(_) => "symbol".into(),
        LispValue::List(_) => "list".into(),
        LispValue::Lambda { .. } => "lambda".into(),
        LispValue::NativeFunc(_) => "native-function".into(),
    }
}

// ── Built-in functions ──────────────────────────────────────────────────────

fn default_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("+", add),
        ("-", sub),
        ("*", mul),
        ("/", div),
        ("=", num_eq),
        ("!=", num_ne),
        ("<", lt),
        ("<=", le),
        (">", gt),
        (">=", ge),
        ("car", car),
        ("cdr", cdr),
        ("cons", cons),
        ("list", list_fn),
        ("length", length),
        ("nth", nth),
        ("reverse", reverse),
        ("is_null", is_null),
        ("numberp", numberp),
        ("listp", listp),
        // String predicate. (stringp x) returns true iff x is a String —
        // the string counterpart of numberp/listp (added lisp-repair L2;
        // forms previously guarded string fields with (not (listp x)),
        // which also passes for numbers and booleans).
        ("stringp", stringp),
        ("assoc", assoc_fn),
        ("assoc!", assoc_strict_fn),
        // List concatenation. (append l1 l2 ...) joins multiple lists.
        // Nil arguments are treated as empty lists. Non-list, non-nil args
        // error. This is the standard Lisp `append` — it does NOT cons the
        // last arg as a tail (that's `append!` in some dialects); all args
        // must be lists.
        ("append", append_fn),
        // String equality. (string= a b) returns true iff both args are
        // strings with equal content. Distinct from `=` which is numeric-only.
        // This is the primary way to compare string values from JSON fields.
        ("string=", string_eq_fn),
        // String concatenation. (concat s1 s2 ...) joins multiple strings.
        // Non-string args error. Use this to build defect labels from field
        // names (e.g. (concat "missing_" key)).
        ("concat", concat_fn),
        // Substring containment. (string-contains needle haystack) returns
        // true iff needle is a non-empty substring of haystack. Arg order
        // follows assoc/member: searched-for first, searched-in second.
        // An empty needle errors — in citation verification an empty needle
        // would verify anything, and a check that fires on correct output is
        // worse than no check.
        ("string-contains", string_contains_fn),
        // Prefix/suffix checks. (starts-with prefix subject) and
        // (ends-with suffix subject) — needle-first like string-contains,
        // but no length guard: a prefix/suffix longer than the subject is a
        // legitimate false (the check is directional). Empty prefix/suffix
        // errors (the string-contains precedent).
        ("starts-with", starts_with_fn),
        ("ends-with", ends_with_fn),
        // Absolute value. (abs x) returns the magnitude of a numeric arg.
        // Used by convergence-gap forms that need a symmetric delta.
        ("abs", abs_fn),
        // Square root. (sqrt x) returns the principal root of a numeric arg.
        // Used by marker-space hypotenuse computations (eqm Improve step 6).
        // Returns a Float.
        ("sqrt", sqrt_fn),
        // Numeric extrema. (max x ...) / (min x ...) return the largest /
        // smallest argument unchanged (Int stays Int). Replaces hand-written
        // `(if (< a b) a b)` clamps in skill forms. Zero args is an arity
        // error; a non-number is a type error.
        ("max", max_fn),
        ("min", min_fn),
        // Generic equality. (eq a b) returns true iff a and b are structurally
        // equal (with controlled list traversal). This is the value-equality
        // counterpart to `=` (numeric) and `string=` (string-only). Distinct
        // from `=` because `=` errors on non-numbers; `eq` accepts any value
        // type and returns false for mismatched types. Used by `cond` clauses
        // comparing string verdicts.
        ("eq", eq_fn),
        // List membership. (member x list) returns true iff x is structurally
        // equal to an element of `list`. Nil list returns false.
        ("member", member_fn),
        // Boundary admissibility. (boundary-ok prefix value) returns true iff
        // value begins with prefix and the first character after the prefix is
        // printable non-whitespace — the closed-allowlist character-class
        // check the dialect lacked (nth 0 is a type error on a string).
        // Replaces the per-form blocklist of space/tab/newline, which could
        // not enumerate Unicode whitespace and passed CR, VT, FF, NBSP and
        // ZWSP (lisp-repair L4, live 2026-10-18). An empty prefix checks the
        // value's first character; a value equal to the prefix is false; a
        // non-string is a type error.
        ("boundary-ok", boundary_ok_fn),
    ]
}

fn finite_number(value: f64, operation: &str) -> Result<f64, LispError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(LispError::Runtime(format!(
            "non-finite number {value} in {operation}; use finite operands and a result within f64 range"
        )))
    }
}

fn as_f64(v: &LispValue, operation: &str) -> Result<f64, LispError> {
    match v {
        LispValue::Int(i) => {
            // Binary64 has 53 significant bits. Bits discarded above that
            // precision must all be zero; unsigned_abs also handles i64::MIN.
            let magnitude = i.unsigned_abs();
            let discarded =
                (u64::BITS - magnitude.leading_zeros()).saturating_sub(f64::MANTISSA_DIGITS);
            if magnitude.trailing_zeros() < discarded {
                return Err(LispError::Runtime(format!(
                    "integer {i} in {operation} cannot be represented exactly as f64; keep integer-only operations or use an exactly representable operand"
                )));
            }
            Ok(*i as f64)
        }
        LispValue::Float(f) => finite_number(*f, operation),
        _ => Err(LispError::TypeError {
            expected: "number".into(),
            actual: type_of(v),
        }),
    }
}

fn as_list(v: &LispValue) -> Result<Rc<List>, LispError> {
    match v {
        LispValue::List(l) => Ok(l.clone()),
        LispValue::Nil => Ok(List::nil()),
        _ => Err(LispError::TypeError {
            expected: "list".into(),
            actual: type_of(v),
        }),
    }
}

fn all_integers(args: &[LispValue], budget: &EvalBudget) -> Result<bool, LispError> {
    for arg in args {
        budget.check_cancelled()?;
        if !matches!(arg, LispValue::Int(_)) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Choose the numeric path before accumulating: a later Float must not make
/// a mixed operation overflow an otherwise irrelevant integer prefix.
fn arithmetic_fold(
    name: &str,
    initial: &LispValue,
    args: &[LispValue],
    integer_op: fn(i64, i64) -> Option<i64>,
    float_op: fn(f64, f64) -> f64,
    budget: &EvalBudget,
) -> Result<LispValue, LispError> {
    if let LispValue::Int(initial) = initial
        && all_integers(args, budget)?
    {
        let mut acc = *initial;
        for arg in args {
            budget.check_cancelled()?;
            if let LispValue::Int(value) = arg {
                acc = integer_op(acc, *value).ok_or_else(|| {
                    LispError::Runtime(format!(
                        "integer overflow in {name}: {acc} {name} {value} exceeds i64"
                    ))
                })?;
            }
        }
        return Ok(LispValue::Int(acc));
    }
    let mut acc = as_f64(initial, name)?;
    // Validate every operand before folding, preserving error precedence when
    // an early operation would overflow but a later operand is invalid.
    for arg in args {
        budget.check_cancelled()?;
        as_f64(arg, name)?;
    }
    for arg in args {
        budget.check_cancelled()?;
        acc = finite_number(float_op(acc, as_f64(arg, name)?), name)?;
    }
    Ok(LispValue::Float(acc))
}

fn add(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    arithmetic_fold(
        "+",
        &LispValue::Int(0),
        args,
        i64::checked_add,
        |a, b| a + b,
        budget,
    )
}

fn sub(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let Some((first, rest)) = args.split_first() else {
        return Err(LispError::Arity("- expects at least 1 arg".into()));
    };
    if rest.is_empty() {
        return match first {
            LispValue::Int(i) => Ok(LispValue::Int(i.checked_neg().ok_or_else(|| {
                LispError::Runtime(format!("integer overflow in -: -{i} exceeds i64"))
            })?)),
            LispValue::Float(f) => Ok(LispValue::Float(-finite_number(*f, "-")?)),
            other => Err(LispError::TypeError {
                expected: "number".into(),
                actual: type_of(other),
            }),
        };
    }
    arithmetic_fold("-", first, rest, i64::checked_sub, |a, b| a - b, budget)
}

fn mul(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    arithmetic_fold(
        "*",
        &LispValue::Int(1),
        args,
        i64::checked_mul,
        |a, b| a * b,
        budget,
    )
}

fn div(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.is_empty() {
        return Err(LispError::Arity("/ expects at least 1 arg".into()));
    }
    let mut acc = as_f64(&args[0], "/")?;
    for a in &args[1..] {
        budget.check_cancelled()?;
        let f = as_f64(a, "/")?;
        if f == 0.0 {
            return Err(LispError::Runtime("division by zero".into()));
        }
        acc = finite_number(acc / f, "/")?;
    }
    Ok(LispValue::Float(acc))
}

fn num_eq(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    compare_chain("=", args, |a, b| a == b, |a, b| a == b, budget)
}

fn num_ne(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    compare_chain("!=", args, |a, b| a != b, |a, b| a != b, budget)
}

/// Comparison helper for every numeric comparator (=, !=, <, <=, >, >=):
/// when every arg is an Int, compare as exact i64 — the f64 coercion lost
/// every integer above 2^53 ((= 2^53+1 2^53) silently returned true;
/// lisp-repair L2, pinned in the agent emission matrix). Otherwise fall
/// back to finite f64 with exact integer promotion and type errors
/// PROPAGATED — a non-number anywhere is a type error, never a silent false
/// (the former `.ok()` swallow made
/// (= 1 "x") silently false). Comparison runs over adjacent pairs, the
/// documented 3+ arg contract for every numeric comparator.
fn compare_chain(
    name: &str,
    args: &[LispValue],
    ord: fn(i64, i64) -> bool,
    ford: fn(f64, f64) -> bool,
    budget: &EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity(format!("{name} expects at least 2 args")));
    }
    if all_integers(args, budget)? {
        for pair in args.windows(2) {
            budget.check_cancelled()?;
            if !matches!(pair, [LispValue::Int(a), LispValue::Int(b)] if ord(*a, *b)) {
                return Ok(LispValue::Bool(false));
            }
        }
        return Ok(LispValue::Bool(true));
    }
    let mut previous = as_f64(&args[0], name)?;
    let mut result = true;
    // A false comparison must not hide an invalid later operand.
    for arg in &args[1..] {
        budget.check_cancelled()?;
        let current = as_f64(arg, name)?;
        result &= ford(previous, current);
        previous = current;
    }
    Ok(LispValue::Bool(result))
}

fn lt(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    compare_chain("<", args, |a, b| a < b, |a, b| a < b, budget)
}

fn le(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    compare_chain("<=", args, |a, b| a <= b, |a, b| a <= b, budget)
}

fn gt(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    compare_chain(">", args, |a, b| a > b, |a, b| a > b, budget)
}

fn ge(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    compare_chain(">=", args, |a, b| a >= b, |a, b| a >= b, budget)
}

fn car(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("car expects 1 arg".into()));
    }
    let list = as_list(&args[0])?;
    if list.is_nil() {
        return Ok(LispValue::Nil);
    }
    Ok(list.head.clone())
}

fn cdr(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("cdr expects 1 arg".into()));
    }
    let list = as_list(&args[0])?;
    Ok(match &list.tail {
        Some(tail) => LispValue::List(tail.clone()),
        None => LispValue::List(List::nil()),
    })
}

fn cons(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("cons expects 2 args".into()));
    }
    let tail = as_list(&args[1])?;
    Ok(LispValue::List(List::cons(args[0].clone(), tail)))
}

fn list_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut items = Vec::with_capacity(args.len());
    for arg in args {
        budget.check_cancelled()?;
        items.push(arg.clone());
    }
    Ok(LispValue::List(budget.make_list(items)?))
}

fn length(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("length expects 1 arg".into()));
    }
    match &args[0] {
        // Traversal charges (lisp-repair L3): one step per spine node or
        // UTF-8 byte, charged here rather than at argument pass. Cancellation
        // also checks each visited node/scalar without consuming extra steps.
        LispValue::List(l) => {
            let mut count = 0;
            for _ in l.iter() {
                budget.check_cancelled()?;
                count += 1;
            }
            budget.charge(count)?;
            Ok(LispValue::Int(count as i64))
        }
        LispValue::Nil => Ok(LispValue::Int(0)),
        LispValue::String(s) => {
            budget.charge(s.len())?;
            let mut count = 0;
            for character in s.chars() {
                check_cancellation(
                    budget.cancellation.as_deref(),
                    "string-length",
                    character.len_utf8(),
                )?;
                count += 1;
            }
            Ok(LispValue::Int(count))
        }
        _ => Err(LispError::TypeError {
            expected: "list/string".into(),
            actual: type_of(&args[0]),
        }),
    }
}

fn nth(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("nth expects 2 args".into()));
    }
    let idx = match &args[0] {
        LispValue::Int(i) if *i < 0 => {
            return Err(LispError::Runtime(format!(
                "nth: negative index {i} — indices are 0-based non-negative integers; use (- (length list) 1) for the last element"
            )));
        }
        LispValue::Int(i) => *i as usize,
        _ => {
            return Err(LispError::TypeError {
                expected: "int".into(),
                actual: type_of(&args[0]),
            });
        }
    };
    let list = as_list(&args[1])?;
    // to_vec clones the spine and every string head — charge that walk
    // before cloning (lisp-repair L3: traversal pays, not argument pass).
    charge_list_spine(&list, budget)?;
    let items = budget.collect_list(&list)?;
    Ok(if idx >= items.len() {
        LispValue::Nil
    } else {
        items[idx].clone()
    })
}

fn reverse(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("reverse expects 1 arg".into()));
    }
    // to_vec clones the spine and every string head; from_vec allocates a
    // new cons per element — charge the traversal before doing it
    // (lisp-repair L3).
    let list = as_list(&args[0])?;
    charge_list_spine(&list, budget)?;
    let mut items = budget.collect_list(&list)?;
    items.reverse();
    Ok(LispValue::List(budget.make_list(items)?))
}

fn is_null(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("is_null expects 1 arg".into()));
    }
    Ok(LispValue::Bool(match &args[0] {
        LispValue::Nil => true,
        LispValue::List(l) => l.is_nil(),
        _ => false,
    }))
}

/// Number predicate: `(numberp x)` returns true if x is an Int or Float.
fn numberp(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("numberp expects 1 arg".into()));
    }
    Ok(LispValue::Bool(matches!(
        args[0],
        LispValue::Int(_) | LispValue::Float(_)
    )))
}

/// String predicate: `(stringp x)` returns true if x is a String. The string
/// counterpart of `numberp`/`listp` — before it existed, forms guarded string
/// fields with `(not (listp x))`, which also passes for numbers and booleans
/// (the skill audit hit this; added lisp-repair L2).
fn stringp(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("stringp expects 1 arg".into()));
    }
    Ok(LispValue::Bool(matches!(args[0], LispValue::String(_))))
}
/// List predicate: `(listp x)` returns true if x is a List or Nil.
/// Used to guard `assoc` against non-list inputs (e.g., when a prior step
/// returns a boolean instead of a JSON object).
fn listp(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("listp expects 1 arg".into()));
    }
    Ok(LispValue::Bool(matches!(
        args[0],
        LispValue::List(_) | LispValue::Nil
    )))
}

/// Association list lookup: `(assoc key alist)` returns the value associated
/// with `key` in the association list, or nil if not found. JSON objects
/// are converted to association lists at the `from_json` boundary, so this
/// is the primary way to access JSON object fields from Lisp.
///
/// Defensive-access contract: a NON-LIST `alist` (string, number, boolean)
/// returns nil rather than erroring. This is deliberate. The manifest fleet
/// has 36 lisp.eval call sites (lisp-repair L0 count: 21 `.agents/skills/`,
/// 15 `kask/`) that pass `step_N_result` (an LLM step's
/// output) directly to `assoc`, and LLM steps emit prose / markdown-wrapped
/// JSON that parses as a JSON string at the env boundary. Erroring with
/// "type error: expected list, got string" crashed whole cascades (the
/// essentialist and idiomatic-lisp outages); nil flows into the forms'
/// existing `is_null` guards, which read it as the documented stable-0
/// default. Explicit `listp` guards in manifests remain valid — they document
/// intent — but are no longer load-bearing.
fn assoc_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("assoc expects 2 args".into()));
    }
    let key = &args[0];
    let alist = match &args[1] {
        // String values from LLM prose outputs degrade to Nil so existing
        // `is_null` guards work; primitive non-string scalars (Int/Float/Bool)
        // are rejected as type errors (operator ruling 2026-10-14, R-3).
        LispValue::String(_) => return Ok(LispValue::Nil),
        other => as_list(other)?,
    };
    lookup_alist(key, &alist, budget).map(|found| found.unwrap_or(LispValue::Nil))
}

/// Strict association-list lookup: `(assoc! key alist)` returns the value
/// associated with `key`, or errors naming `key` and the sorted list of
/// available keys in `alist` when absent (operator ruling 2026-10-14, R-3).
fn assoc_strict_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("assoc! expects 2 args".into()));
    }
    let key = &args[0];
    let alist = as_list(&args[1])?;
    if let Some(value) = lookup_alist(key, &alist, budget)? {
        return Ok(value);
    }
    let mut available_keys = Vec::new();
    let mut cursor = Some(alist.as_ref());
    while let Some(node) = cursor {
        if node.is_nil() {
            break;
        }
        budget.check_cancelled()?;
        cursor = node.tail.as_deref();
        if let LispValue::List(pair) = &node.head {
            if let Some(value) = pair.tail.as_deref() {
                if !value.is_nil() && value.tail.as_deref().is_none_or(List::is_nil) {
                    if let LispValue::String(k) | LispValue::Symbol(k) = &pair.head {
                        available_keys.push(k.clone());
                    }
                }
            }
        }
    }
    budget.check_cancelled()?;
    available_keys.sort();
    available_keys.dedup();
    budget.check_cancelled()?;
    let key_display = match key {
        LispValue::String(s) => format!("{s:?}"),
        LispValue::Symbol(s) => s.clone(),
        LispValue::Lambda { .. } => "<lambda>".into(),
        LispValue::NativeFunc(_) => "<native-function>".into(),
        LispValue::List(_) => {
            // Raw Debug follows captured environments and can cycle. Project
            // only charged data, never a callable's environment, for this error.
            charge_value(key, budget, 0)?;
            charge_output_layout(key, budget, 0)?;
            let projected = to_json(key);
            budget.check_cancelled()?;
            let display = format!("{projected:?}");
            budget.check_cancelled()?;
            display
        }
        other => format!("{other:?}"),
    };
    let message =
        format!("assoc!: key {key_display} not found — available keys: {available_keys:?}");
    budget.check_cancelled()?;
    Err(LispError::Runtime(message))
}

fn lookup_alist(
    key: &LispValue,
    alist: &Rc<List>,
    budget: &mut EvalBudget,
) -> Result<Option<LispValue>, LispError> {
    let mut cursor = Some(alist.as_ref());
    while let Some(node) = cursor {
        if node.is_nil() {
            break;
        }
        budget.check_cancelled()?;
        cursor = node.tail.as_deref();
        if let LispValue::List(pair) = &node.head {
            let Some(value) = pair.tail.as_deref() else {
                continue;
            };
            // Reject malformed candidates without cloning or walking their tail.
            if value.is_nil() || !value.tail.as_deref().is_none_or(List::is_nil) {
                continue;
            }
            charge_value(&pair.head, budget, 0)?;
            charge_value(key, budget, 0)?;
            if equal_with_budget(&pair.head, key, budget)? {
                if let LispValue::String(text) | LispValue::Symbol(text) = &value.head {
                    budget.charge(text.len())?;
                }
                return Ok(Some(value.head.clone()));
            }
        }
    }
    Ok(None)
}

/// List concatenation: `(append l1 l2 ...)` joins multiple lists into one.
/// Nil arguments are treated as empty lists. Non-list, non-nil args error.
/// Returns nil if all args are nil/empty.
fn append_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut combined: Vec<LispValue> = Vec::new();
    for arg in args {
        budget.check_cancelled()?;
        match arg {
            // to_vec clones the spine and every string head; from_vec
            // allocates a new cons per output element — charge per input
            // before extending (lisp-repair L3: traversal pays, not
            // argument pass).
            LispValue::List(l) => {
                charge_list_spine(l, budget)?;
                for value in l.iter() {
                    budget.check_cancelled()?;
                    combined.push(value.clone());
                }
            }
            LispValue::Nil => {}
            _ => {
                return Err(LispError::TypeError {
                    expected: "list".into(),
                    actual: type_of(arg),
                });
            }
        }
    }
    Ok(LispValue::List(budget.make_list(combined)?))
}

/// String equality: `(string= a b)` returns true iff both args are strings
/// with equal content. `Nil` (from nullable JSON fields or absent `assoc`
/// keys) evaluates to false; non-string scalars/lists (`Int`, `Float`,
/// `Bool`, `List`) error with `TypeError` (operator ruling 2026-10-14, R-2).
fn string_eq_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("string= expects 2 args".into()));
    }
    match (&args[0], &args[1]) {
        (LispValue::String(a), LispValue::String(b)) => Ok(LispValue::Bool(a == b)),
        (LispValue::Nil, LispValue::String(_))
        | (LispValue::String(_), LispValue::Nil)
        | (LispValue::Nil, LispValue::Nil) => Ok(LispValue::Bool(false)),
        (LispValue::String(_) | LispValue::Nil, other) | (other, _) => Err(LispError::TypeError {
            expected: "string".into(),
            actual: type_of(other),
        }),
    }
}

/// String concatenation: `(concat s1 s2 ...)` joins multiple strings.
/// Non-string args error. Returns empty string if no args.
fn concat_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut combined = String::new();
    for arg in args {
        budget.check_cancelled()?;
        match arg {
            LispValue::String(s) => combined.push_str(s),
            _ => {
                return Err(LispError::TypeError {
                    expected: "string".into(),
                    actual: type_of(arg),
                });
            }
        }
    }
    Ok(LispValue::String(combined))
}

/// Needle-first string-pair extraction for the verbatim substring checks:
/// arity 2 with both args strings, type errors naming the expected type.
/// Needle-first follows `assoc`/`member` (searched-for first, searched-in
/// second); per-check guards (empty needle, length) stay at the call site.
fn string_pair<'a>(name: &str, args: &'a [LispValue]) -> Result<(&'a str, &'a str), LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity(format!("{name} expects 2 args")));
    }
    let needle = match &args[0] {
        LispValue::String(s) => s,
        other => {
            return Err(LispError::TypeError {
                expected: "string".into(),
                actual: type_of(other),
            });
        }
    };
    let subject = match &args[1] {
        LispValue::String(s) => s,
        other => {
            return Err(LispError::TypeError {
                expected: "string".into(),
                actual: type_of(other),
            });
        }
    };
    Ok((needle, subject))
}

/// Substring containment: `(string-contains needle haystack)` returns true
/// iff `needle` is a non-empty substring of `haystack`. Arg order follows
/// `assoc`/`member` (searched-for first, searched-in second). An empty
/// needle errors rather than returning true — in citation verification an
/// empty needle would verify anything, and a check that fires on correct
/// output is worse than no check. A needle longer than the haystack also
/// errors naming the probable reversal — a longer needle can never be a
/// substring, and the silent false of a reversed call hid a 20-quote
/// verification failure (2026-09-28).
fn string_contains_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let (needle, haystack) = string_pair("string-contains", args)?;
    if needle.is_empty() {
        return Err(LispError::Runtime(
            "string-contains: needle must be a non-empty string".into(),
        ));
    }
    if needle.len() > haystack.len() {
        return Err(LispError::Runtime(format!(
            "string-contains: needle ({} bytes) is longer than haystack ({} bytes) — arguments probably reversed; signature is (string-contains needle haystack), needle first",
            needle.len(),
            haystack.len()
        )));
    }
    Ok(LispValue::Bool(haystack.contains(needle)))
}

/// Prefix check: `(starts-with prefix subject)` returns true iff `subject`
/// begins with `prefix`. Arg order follows `string-contains` (searched-for
/// first, searched-in second). A prefix longer than the subject is a
/// legitimate false, not an error — unlike `string-contains` there is no
/// reversal ambiguity worth a length guard: the check is directional, and
/// the lexical-gate use case checks short prefixes against short verbatim
/// extracts where a guard would break the check. An empty prefix errors
/// rather than returning true — every string starts with the empty prefix,
/// and a check that fires on correct output is worse than no check (the
/// `string-contains` precedent).
fn starts_with_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let (prefix, subject) = string_pair("starts-with", args)?;
    if prefix.is_empty() {
        return Err(LispError::Runtime(
            "starts-with: prefix must be a non-empty string".into(),
        ));
    }
    Ok(LispValue::Bool(subject.starts_with(prefix)))
}

/// Suffix check: `(ends-with suffix subject)` returns true iff `subject`
/// ends with `suffix`. Same conventions as `starts-with`: needle-first,
/// no length guard (a suffix longer than the subject is a legitimate
/// false), empty suffix errors.
fn ends_with_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let (suffix, subject) = string_pair("ends-with", args)?;
    if suffix.is_empty() {
        return Err(LispError::Runtime(
            "ends-with: suffix must be a non-empty string".into(),
        ));
    }
    Ok(LispValue::Bool(subject.ends_with(suffix)))
}

/// Boundary check: `(boundary-ok prefix value)` returns true iff `value` is a
/// string beginning with `prefix` whose FIRST character AFTER the prefix is
/// admissible. It is the character-access primitive the dialect previously
/// lacked — `(nth 0 s)` is a type error on a string, so a form could not
/// inspect a single character, and without one there is no way to say "the
/// character that follows this label is real content".
///
/// The predicate is BOUNDARY-scoped on purpose. A value-wide "contains no
/// whitespace" test would reject legitimate multi-word values (`boundary:a
/// b`), and the invariant callers state is about what follows the label
/// prefix, not about the whole value.
///
/// The admissible class is a CLOSED ALLOWLIST, not a blocklist. A blocklist
/// of space/tab/newline is unfalsifiable by construction: it enumerates the
/// characters an author thought of, so any character it omits — a carriage
/// return, a vertical tab, a form feed, a no-break space, a zero-width
/// space — passes and masquerades as content (live 2026-10-18: all five of
/// those passed the VC-0 typed-reference predicate while the stated
/// invariant was "no leading whitespace"). Unicode's own whitespace
/// predicate is not a sufficient instrument either: `char::is_whitespace`
/// is true for NBSP and the ideographic space but FALSE for ZWSP, which is
/// invisible and therefore equally able to masquerade as content. The
/// allowlist closes the class: a character is admissible iff it is printable
/// non-whitespace (`!c.is_whitespace() && !c.is_control()`). `is_control()`
/// covers the whole Cc class (CR, VT, FF, NUL, DEL …); `is_whitespace()`
/// covers the Unicode White_Space set (LF, NBSP, ideographic space …).
/// Format characters (Cf: ZWSP, BOM, ZWJ, soft hyphen) are excluded
/// explicitly because a zero-width character is invisible — a value whose
/// first character renders as nothing cannot be shown to a human reviewer,
/// and evidence must be human-checkable.
///
/// An empty `prefix` is legitimate and checks the value's first character.
/// A `value` that is exactly `prefix` (nothing after it) is FALSE, not an
/// error — there is no character to inspect, and "no" is the real answer to
/// "does usable content follow the label". A non-string argument is a
/// `TypeError`. Unlike `string-contains` there is no reversal ambiguity
/// worth a guard: `prefix` is a short constant label and `value` is the
/// candidate, and a mismatched pair is a plain false.
fn boundary_ok_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let (prefix, value) = string_pair("boundary-ok", args)?;
    // Charging the prefix scan follows the L3 convention that traversal
    // pays where it happens (cf. `length`): strip_prefix walks prefix
    // bytes before the single-character inspection.
    budget.charge(prefix.len())?;
    Ok(LispValue::Bool(
        value
            .strip_prefix(prefix)
            .and_then(|rest| rest.chars().next())
            .is_some_and(admissible_leading_char),
    ))
}

/// The admissible-character predicate shared by `boundary-ok` and any future
/// character-class builtin. See `boundary_ok_fn` for why this is an
/// allowlist and why `is_control`/`is_whitespace` alone are insufficient.
fn admissible_leading_char(c: char) -> bool {
    !c.is_whitespace() && !c.is_control() && !is_invisible_format(c)
}

/// Format characters (Unicode general category Cf) that occupy no visible
/// width: zero-width space/joiner/non-joiner, BOM, soft hyphen, word
/// joiner, and the bidi embedding controls. Named explicitly because the
/// Rust standard library exposes no `is_format` predicate; the set is the
/// Cf characters a leading-position check can plausibly meet.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'                  // soft hyphen
        | '\u{061c}'                // Arabic letter mark
        | '\u{180e}'                // Mongolian vowel separator
        | '\u{200b}'..='\u{200f}'   // ZWSP, ZWNJ, ZWJ, LRM, RLM
        | '\u{202a}'..='\u{202e}'   // bidi embedding/override
        | '\u{2060}'..='\u{2064}'   // word joiner, invisible operators
        | '\u{2066}'..='\u{2069}'   // bidi isolates
        | '\u{feff}'                // BOM / zero-width no-break space
    )
}

/// Absolute value: `(abs x)` returns the magnitude of a numeric arg.
/// Preserves Int vs Float: `(abs -3)` → `3` (Int), `(abs -3.5)` → `3.5` (Float).
fn abs_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("abs expects 1 arg".into()));
    }
    match &args[0] {
        LispValue::Int(i) => Ok(LispValue::Int(
            // Checked, not wrapping: abs(i64::MIN) wrapped to a negative
            // value silently (lisp-repair L2).
            i.checked_abs().ok_or_else(|| {
                LispError::Runtime(format!("integer overflow in abs: |{i}| exceeds i64"))
            })?,
        )),
        LispValue::Float(f) => Ok(LispValue::Float(finite_number(*f, "abs")?.abs())),
        _ => Err(LispError::TypeError {
            expected: "number".into(),
            actual: type_of(&args[0]),
        }),
    }
}

/// Square root: `(sqrt x)` returns the principal root as a Float.
/// Negative input errors (no complex numbers). Used by marker-space
/// hypotenuse computations.
fn sqrt_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("sqrt expects 1 arg".into()));
    }
    let f = as_f64(&args[0], "sqrt")?;
    if f < 0.0 {
        return Err(LispError::Runtime(format!("sqrt of negative number {f}")));
    }
    Ok(LispValue::Float(f.sqrt()))
}

/// Numeric extremum over one or more args: keeps the winning argument's own
/// representation (Int or Float). `prefer` decides whether a candidate
/// replaces the current best. Int-vs-Int comparisons are exact i64 (the
/// f64 coercion misordered integers above 2^53 — lisp-repair L2); mixed
/// numeric pairs compare as finite f64 only when both conversions are exact.
fn extremum(
    name: &str,
    args: &[LispValue],
    prefer: fn(f64, f64) -> bool,
    iprefer: fn(i64, i64) -> bool,
    budget: &EvalBudget,
) -> Result<LispValue, LispError> {
    let Some((first, rest)) = args.split_first() else {
        return Err(LispError::Arity(format!("{name} expects at least 1 arg")));
    };
    // Validate even a sole argument, without promoting an integer that never
    // participates in a mixed comparison.
    for arg in args {
        budget.check_cancelled()?;
        match arg {
            LispValue::Int(_) => {}
            LispValue::Float(value) => {
                finite_number(*value, name)?;
            }
            _ => {
                return Err(LispError::TypeError {
                    expected: "number".into(),
                    actual: type_of(arg),
                });
            }
        }
    }
    let mut best = first;
    for arg in rest {
        budget.check_cancelled()?;
        let wins = match (best, arg) {
            (LispValue::Int(b), LispValue::Int(a)) => iprefer(*a, *b),
            _ => prefer(as_f64(arg, name)?, as_f64(best, name)?),
        };
        if wins {
            best = arg;
        }
    }
    Ok(best.clone())
}

/// `(max x ...)` returns the largest numeric argument.
fn max_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    extremum(
        "max",
        args,
        |candidate, best| candidate > best,
        |a, b| a > b,
        budget,
    )
}

/// `(min x ...)` returns the smallest numeric argument.
fn min_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    extremum(
        "min",
        args,
        |candidate, best| candidate < best,
        |a, b| a < b,
        budget,
    )
}

// Stream list equality so controlled calls never enter the raw PartialEq
// traversal, which has no invocation budget. Scalar semantics stay identical.
#[stacksafe::stacksafe]
fn equal_with_budget(
    left: &LispValue,
    right: &LispValue,
    budget: &EvalBudget,
) -> Result<bool, LispError> {
    check_cancellation(budget.cancellation.as_deref(), "equality", 1)?;
    if let (LispValue::List(left), LispValue::List(right)) = (left, right) {
        let mut left = left.iter();
        let mut right = right.iter();
        loop {
            match (left.next(), right.next()) {
                (Some(left), Some(right)) => {
                    if !equal_with_budget(left, right, budget)? {
                        return Ok(false);
                    }
                }
                (None, None) => return Ok(true),
                _ => return Ok(false),
            }
        }
    }
    // Scalar equality uses std string comparison; the caller has already
    // charged both values. Lists never enter the raw recursive PartialEq path.
    let equal = left == right;
    budget.check_cancelled()?;
    Ok(equal)
}

/// Generic structural equality, type-sensitive and false for callables.
fn eq_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("eq expects 2 args".into()));
    }
    charge_value(&args[0], budget, 0)?;
    charge_value(&args[1], budget, 0)?;
    Ok(LispValue::Bool(equal_with_budget(
        &args[0], &args[1], budget,
    )?))
}

/// List membership: `(member x list)` returns true iff `x` is structurally
/// equal to an element of `list`. Nil list returns false. Non-list second
/// arg errors. Used by the GORILLA maturity-blocks check.
fn member_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("member expects 2 args".into()));
    }
    let list = as_list(&args[1])?;
    if list.is_nil() {
        return Ok(LispValue::Bool(false));
    }
    for item in list.iter() {
        budget.check_cancelled()?;
        charge_value(&item, budget, 0)?;
        charge_value(&args[0], budget, 0)?;
        if equal_with_budget(item, &args[0], budget)? {
            return Ok(LispValue::Bool(true));
        }
    }
    Ok(LispValue::Bool(false))
}

/// Charge the cost of a `to_vec`-style traversal: one tick per spine node
/// plus the byte cost of string/symbol heads (which own their bytes; nested
/// lists are Rc-cloned cheaply and are charged by their own traversals).
/// This is the per-node walk the argument-passing charge used to perform on
/// every call — lisp-repair L3 moved it from every argument pass to the
/// builtins that actually traverse (`append`/`nth`/`reverse`). Deep
/// structural work (`eq`/`assoc`/`member`) keeps the recursive
/// `charge_value`, which is what those builtins actually do.
fn charge_list_spine(list: &Rc<List>, budget: &mut EvalBudget) -> Result<(), LispError> {
    let mut cursor = Some(list.as_ref());
    while let Some(node) = cursor {
        if node.is_nil() {
            break;
        }
        budget.tick()?;
        if let LispValue::String(text) | LispValue::Symbol(text) = &node.head {
            budget.charge(text.len())?;
        }
        cursor = node.tail.as_deref();
    }
    Ok(())
}

// Charge expansion, not just shared Rc nodes: a small DAG can serialize to an
// exponentially large JSON tree. Validation happens before recursive copying.
#[stacksafe::stacksafe]
fn charge_value(value: &LispValue, budget: &mut EvalBudget, depth: u64) -> Result<(), LispError> {
    if depth > budget.max_depth {
        return Err(LispError::DepthLimitExceeded(budget.max_depth));
    }
    check_cancellation(budget.cancellation.as_deref(), "value-traversal", 1)?;
    budget.tick()?;
    match value {
        LispValue::String(text) | LispValue::Symbol(text) => budget.charge(text.len())?,
        LispValue::Float(number) => {
            finite_number(*number, "sandbox result")?;
        }
        LispValue::List(list) => {
            let mut cursor = Some(list.as_ref());
            while let Some(node) = cursor {
                if node.is_nil() {
                    break;
                }
                charge_value(&node.head, budget, depth.saturating_add(1))?;
                cursor = node.tail.as_deref();
            }
        }
        _ => {}
    }
    Ok(())
}

#[stacksafe::stacksafe]
fn charge_json(value: &Value, budget: &mut EvalBudget, depth: u64) -> Result<(), LispError> {
    if depth > budget.max_depth {
        return Err(LispError::DepthLimitExceeded(budget.max_depth));
    }
    check_cancellation(budget.cancellation.as_deref(), "json-traversal", 1)?;
    budget.tick()?;
    match value {
        Value::String(text) => budget.charge(text.len())?,
        Value::Number(number) if number.as_i64().is_none() && number.as_u64().is_some() => {
            return Err(LispError::Runtime(format!(
                "JSON integer {number} is outside signed i64 range ({}..={})",
                i64::MIN,
                i64::MAX
            )));
        }
        Value::Array(values) => {
            for value in values {
                charge_json(value, budget, depth.saturating_add(1))?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                budget.charge(key.len().saturating_add(2))?;
                charge_json(value, budget, depth.saturating_add(2))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Charge one tick per argument plus the byte cost of top-level string and
/// symbol arguments. List nodes are NOT re-charged here: every node is
/// charged once at creation (parse source bytes, `list`'s per-arg ticks, or
/// `charge_json` for env values), and the builtins that actually traverse
/// or clone a list charge it themselves (`assoc`/`member`/`eq` via
/// `charge_value`; `length`/`nth`/`reverse`/`append` per node). The former
/// per-node re-charge on EVERY argument pass made O(1) list ops (`is_null`,
/// `car`, `cdr`) cost O(len) per call — a 600-element walker paid 368,531
/// steps and exceeded the 100,000 default (lisp-repair L3; pinned by
/// `walker_step_cost_is_linear_and_fits_default_budget` and
/// `constant_time_list_ops_do_not_charge_per_node` in the test module).
fn charge_native_arguments(args: &[LispValue], budget: &mut EvalBudget) -> Result<(), LispError> {
    for value in args {
        budget.tick()?;
        if let LispValue::String(text) | LispValue::Symbol(text) = value {
            budget.charge(text.len())?;
        }
    }
    Ok(())
}

// serde_json's default reader allows 128 nested containers. Stay below that
// boundary so the ordinary Value returned to callers is safe to serialize/drop;
// raising evaluation depth must not raise this wire-format boundary.
const MAX_JSON_DEPTH: u64 = 128;

#[stacksafe::stacksafe]
fn charge_output_layout(
    value: &LispValue,
    budget: &mut EvalBudget,
    depth: u64,
) -> Result<(), LispError> {
    if depth >= MAX_JSON_DEPTH {
        return Err(LispError::OutputDepthLimitExceeded(MAX_JSON_DEPTH));
    }
    // Pretty-print indentation is work too, even when the value is a small DAG.
    budget.charge((depth as usize) * 2)?;
    if let LispValue::List(list) = value {
        let mut cursor = Some(list.as_ref());
        while let Some(node) = cursor {
            if node.is_nil() {
                break;
            }
            charge_output_layout(&node.head, budget, depth + 1)?;
            cursor = node.tail.as_deref();
        }
    }
    Ok(())
}

// ── JSON interop ────────────────────────────────────────────────────────────

/// Convert a `serde_json::Value` into a `LispValue`.
/// JSON objects become association lists: `{"a": 1, "b": 2}` → `(("a" . 1) ("b" . 2))`
#[stacksafe::stacksafe]
pub fn from_json(value: &Value) -> LispValue {
    match value {
        Value::Null => LispValue::Nil,
        Value::Bool(b) => LispValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                LispValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                LispValue::Float(f)
            } else {
                LispValue::Nil
            }
        }
        Value::String(s) => LispValue::String(s.clone()),
        Value::Array(arr) => LispValue::List(List::from_vec(arr.iter().map(from_json).collect())),
        Value::Object(obj) => {
            // JSON objects become association lists — the classic Lisp structure.
            let pairs: Vec<LispValue> = obj
                .iter()
                .map(|(k, v)| {
                    LispValue::List(List::from_vec(vec![
                        LispValue::String(k.clone()),
                        from_json(v),
                    ]))
                })
                .collect();
            LispValue::List(List::from_vec(pairs))
        }
    }
}

/// Convert a `LispValue` into a `serde_json::Value`.
///
/// Association lists (lists of 2-element lists with string keys) are converted
/// to JSON objects, so `(list (list "key" val) (list "key2" val2))` becomes
/// `{"key": val, "key2": val2}`. This is the natural round-trip for structured
/// data produced by lisp.eval compute steps — the alist is Lisp's native
/// key-value representation, and downstream Jinja2 templates expect JSON
/// objects for dot-path access.
#[stacksafe::stacksafe]
pub fn to_json(value: &LispValue) -> Value {
    match value {
        LispValue::Nil => Value::Null,
        LispValue::Bool(b) => Value::Bool(*b),
        LispValue::Int(i) => Value::Number((*i).into()),
        LispValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        LispValue::String(s) => Value::String(s.clone()),
        LispValue::Symbol(s) => Value::String(s.clone()),
        LispValue::List(list) => {
            if list.is_nil() {
                return Value::Array(Vec::new());
            }
            let items = list.to_vec();
            if is_alist(&items) {
                let mut map = serde_json::Map::new();
                for pair in &items {
                    if let LispValue::List(pair_list) = pair {
                        let pair_items = pair_list.to_vec();
                        if pair_items.len() == 2 {
                            if let LispValue::String(key) = &pair_items[0] {
                                map.insert(key.clone(), to_json(&pair_items[1]));
                            }
                        }
                    }
                }
                Value::Object(map)
            } else {
                Value::Array(items.iter().map(to_json).collect())
            }
        }
        LispValue::Lambda { .. } => Value::String("<lambda>".into()),
        LispValue::NativeFunc(_) => Value::String("<native-function>".into()),
    }
}

/// Check if a list of LispValues is an association list — every element is a
/// 2-element list whose first element is a string. An empty list is NOT an
/// alist (it stays as an empty JSON array to avoid ambiguity with an empty
/// object, which could mask missing data).
fn is_alist(items: &[LispValue]) -> bool {
    if items.is_empty() {
        return false;
    }
    items.iter().all(|item| {
        if let LispValue::List(pair) = item {
            let pair_items = pair.to_vec();
            pair_items.len() == 2 && matches!(pair_items[0], LispValue::String(_))
        } else {
            false
        }
    })
}

// The counting pass retains no document bytes. Serde remains authoritative for
// UTF-8, escaping, indentation and the already-projected object's keys.
#[derive(Default)]
struct OutputByteCounter<'a> {
    bytes: usize,
    exceeded: bool,
    cancellation: Option<&'a AtomicBool>,
    cancelled: bool,
}

impl std::io::Write for OutputByteCounter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if check_cancellation(self.cancellation, "output-write", 0).is_err() {
            self.cancelled = true;
            // Interrupted is retried by write_all, so it cannot mean cancelled.
            return Err(std::io::Error::other("Lisp evaluation cancelled"));
        }
        if bytes.len() > MAX_OUTPUT_BYTES - self.bytes {
            self.exceeded = true;
            return Err(std::io::Error::other("max_output_bytes exceeded"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn check_output_json_depth(
    value: &Value,
    depth: u64,
    cancellation: Option<&AtomicBool>,
) -> Result<(), LispError> {
    check_cancellation(cancellation, "output-depth", 1)?;
    if depth >= MAX_JSON_DEPTH {
        return Err(LispError::OutputDepthLimitExceeded(MAX_JSON_DEPTH));
    }
    match value {
        Value::String(text) if text.len() > MAX_OUTPUT_BYTES => {
            return Err(LispError::Runtime(format!(
                "max_output_bytes exceeded; allowed {MAX_OUTPUT_BYTES} pretty JSON UTF-8 bytes"
            )));
        }
        Value::Array(values) => {
            for value in values {
                check_output_json_depth(value, depth + 1, cancellation)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                check_cancellation(cancellation, "output-key", 1)?;
                if key.len() > MAX_OUTPUT_BYTES {
                    return Err(LispError::Runtime(format!(
                        "max_output_bytes exceeded; allowed {MAX_OUTPUT_BYTES} pretty JSON UTF-8 bytes"
                    )));
                }
                check_output_json_depth(value, depth + 1, cancellation)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn pretty_json_output_size(value: &Value) -> Result<usize, LispError> {
    pretty_json_output_size_with_cancellation(value, None)
}

fn pretty_json_output_size_with_cancellation(
    value: &Value,
    cancellation: Option<&AtomicBool>,
) -> Result<usize, LispError> {
    // Public formatting also accepts Rust-created Values, not just sandbox
    // results. Bound recursion before entering serde's recursive serializer.
    check_output_json_depth(value, 0, cancellation)?;
    check_cancellation(cancellation, "output-count-before", 0)?;
    let mut counter = OutputByteCounter {
        cancellation,
        ..OutputByteCounter::default()
    };
    serde_json::to_writer_pretty(&mut counter, value).map_err(|error| {
        if counter.cancelled {
            LispError::Cancelled
        } else if counter.exceeded {
            LispError::Runtime(format!(
                "max_output_bytes exceeded; allowed {MAX_OUTPUT_BYTES} pretty JSON UTF-8 bytes"
            ))
        } else {
            LispError::Runtime(format!("failed to count pretty JSON output: {error}"))
        }
    })?;
    check_cancellation(cancellation, "output-count-after", 0)?;
    Ok(counter.bytes)
}

/// Format a result as pretty JSON, admitting at most [`MAX_OUTPUT_BYTES`] UTF-8
/// bytes before requesting the final document buffer. Also enforces the JSON
/// nesting boundary for raw Rust-created Values; no trusted-Value exemption.
/// This is a document bound, not a tool-envelope, total-heap or time quota.
pub fn format_json_output(value: &Value) -> Result<String, LispError> {
    let size = pretty_json_output_size(value)?;
    #[cfg(test)]
    tests::observe_output_allocation(size);
    let mut bytes = Vec::with_capacity(size);
    serde_json::to_writer_pretty(&mut bytes, value).map_err(|error| {
        LispError::Runtime(format!("failed to format pretty JSON output: {error}"))
    })?;
    String::from_utf8(bytes)
        .map_err(|error| LispError::Runtime(format!("pretty JSON output is not UTF-8: {error}")))
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Evaluate a Lisp form against a JSON environment, returning a JSON result.
///
/// Shared by the agent tool and corpus Lisp query parser. `form` is Lisp
/// source; `env_json` keys become top-level bindings, with values converted
/// via `from_json` (objects become association lists).
pub fn eval_sandboxed(form: &str, env_json: &Value) -> Result<Value, LispError> {
    eval_sandboxed_with_budget(form, env_json, 100000, 1024)
}

/// Evaluate with a per-invocation, monotonic cancellation flag.
///
/// The caller requests cancellation by storing `true` with Release ordering;
/// checkpoints load with Acquire and return [`LispError::Cancelled`]. The flag
/// is never reset. Existing work/depth/output limits remain independent.
/// Custom scans and traversals check each iteration. Bounded std operations
/// (including scalar string operations), raw JSON conversion and destruction
/// are nonpreemptible; scope teardown always completes before this returns.
/// This is cooperative cancellation, not a time or heap quota.
pub fn eval_sandboxed_with_cancellation(
    form: &str,
    env_json: &Value,
    max_steps: u64,
    max_depth: u64,
    cancellation: Arc<AtomicBool>,
) -> Result<Value, LispError> {
    eval_sandboxed_inner(form, env_json, max_steps, max_depth, Some(cancellation))
}

pub fn eval_sandboxed_with_budget(
    form: &str,
    env_json: &Value,
    max_steps: u64,
    max_depth: u64,
) -> Result<Value, LispError> {
    eval_sandboxed_inner(form, env_json, max_steps, max_depth, None)
}

fn eval_sandboxed_inner(
    form: &str,
    env_json: &Value,
    max_steps: u64,
    max_depth: u64,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<Value, LispError> {
    check_cancellation(cancellation.as_deref(), "entry", 0)?;
    for (field, requested, allowed) in [
        ("max_steps", max_steps, MAX_STEPS),
        ("max_depth", max_depth, MAX_DEPTH),
    ] {
        if requested > allowed {
            return Err(LispError::Runtime(format!(
                "{field} requested {requested}; allowed {allowed}"
            )));
        }
    }
    let mut budget = EvalBudget::new(max_steps, max_depth);
    budget.cancellation = cancellation;
    // This guard belongs only to the stateless JSON boundary. Raw eval callers
    // retain ownership of their environments and may reuse escaping closures.
    budget.invocation_scopes = Some(Rc::new(InvocationScopes::default()));
    let parsed = parse_with_budget(form, &mut budget)?;
    charge_json(env_json, &mut budget, 0)?;
    if parsed.is_empty() {
        pretty_json_output_size_with_cancellation(&Value::Null, budget.cancellation.as_deref())?;
        budget.check_cancelled()?;
        return Ok(Value::Null);
    }
    let env = budget.new_scope(Env::new_root())?;
    if let Value::Object(obj) = env_json {
        for (k, v) in obj {
            // charge_json bounds this raw conversion by max_steps/max_depth.
            // Copying/JSON conversion is nonpreemptible, bracketed per binding.
            check_cancellation(budget.cancellation.as_deref(), "json-input-before", 0)?;
            env.borrow_mut().define(k.clone(), from_json(v));
            check_cancellation(budget.cancellation.as_deref(), "json-input-after", 0)?;
        }
    }
    let mut result = LispValue::Nil;
    for form in &parsed {
        result = eval_with_budget(env.clone(), form, &mut budget)?;
    }
    charge_value(&result, &mut budget, 0)?;
    charge_output_layout(&result, &mut budget, 0)?;
    // charge_value/layout bound expanded copying by max_steps and JSON depth.
    check_cancellation(budget.cancellation.as_deref(), "json-output-before", 0)?;
    let result = to_json(&result);
    check_cancellation(budget.cancellation.as_deref(), "json-output-after", 0)?;
    pretty_json_output_size_with_cancellation(&result, budget.cancellation.as_deref())?;
    // Teardown is infallible and deliberately uninterruptible. Recheck after
    // it as well, before returning the Value to the caller's publication fence.
    drop(budget.invocation_scopes.take());
    check_cancellation(budget.cancellation.as_deref(), "publish", 0)?;
    Ok(result)
}

// ── Tests ───────────────────────────────────────────────────────────────────
// These tests guard the `lisp_eval` tool's sandbox boundaries:
// an LLM-supplied form cannot loop forever (step budget), blow the
// stack (depth budget), or escape the environment (no ambient access).

#[cfg(test)]
mod property_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Weak;

    type CheckpointObserver = Box<dyn FnMut(&'static str, usize)>;
    thread_local! {
        static CHECKPOINT_OBSERVER: RefCell<Option<CheckpointObserver>> = RefCell::new(None);
    }

    pub(super) fn observe_checkpoint(phase: &'static str, units: usize) {
        CHECKPOINT_OBSERVER.with_borrow_mut(|observer| {
            if let Some(observer) = observer {
                observer(phase, units);
            }
        });
    }

    struct CheckpointProbe;

    impl CheckpointProbe {
        fn start(observer: impl FnMut(&'static str, usize) + 'static) -> Self {
            CHECKPOINT_OBSERVER.with_borrow_mut(|slot| {
                assert!(slot.is_none(), "observer must not nest");
                *slot = Some(Box::new(observer));
            });
            Self
        }
    }

    impl Drop for CheckpointProbe {
        fn drop(&mut self) {
            CHECKPOINT_OBSERVER.with_borrow_mut(|slot| *slot = None);
        }
    }

    /// expect: "A pre-cancelled public invocation allocates no scopes" [H6A]
    #[test]
    fn h6a_pre_cancelled_public_entry() {
        let flag = Arc::new(AtomicBool::new(false));
        flag.store(true, Ordering::Release);
        let _probe = ScopeProbe::start(None);
        let result =
            eval_sandboxed_with_cancellation("42", &Value::Null, 100_000, 1024, flag.clone());
        assert!(matches!(result, Err(LispError::Cancelled)));
        assert_eq!(
            result.expect_err("cancelled").to_string(),
            "Lisp evaluation cancelled"
        );
        assert!(
            flag.load(Ordering::Acquire),
            "the evaluator must never reset the flag"
        );
        OBSERVED_SCOPES.with_borrow(|observation| {
            assert!(observation.as_ref().expect("scope probe").scopes.is_empty());
        });
    }

    /// expect: "External cancellation stops real workers at bounded checkpoints, after complete teardown" [H6A]
    #[test]
    fn h6a_worker_cancellation_checkpoints_and_teardown() {
        use std::sync::mpsc::sync_channel;
        use std::time::Duration;

        let cycle =
            "(define root (lambda () root)) (let () (begin (define orphan (lambda () orphan)) 0))";
        let cases = [
            (
                "source",
                format!(";{}\n42", "é".repeat(2048)),
                Value::Null,
                1200,
                false,
            ),
            (
                "source",
                format!("\"{}\"", "🦎".repeat(2048)),
                Value::Null,
                1200,
                false,
            ),
            (
                "unescape",
                format!("\"{}\"", "\\né".repeat(2048)),
                Value::Null,
                1200,
                false,
            ),
            ("infix", "1 + ".repeat(512) + "1", Value::Null, 100, false),
            (
                "json-traversal",
                "xs".into(),
                serde_json::json!({"xs": vec![1; 512]}),
                100,
                false,
            ),
            (
                "list",
                format!("{cycle} (reverse xs)"),
                serde_json::json!({"xs": vec![1; 512]}),
                100,
                true,
            ),
            (
                "list-build",
                format!("{cycle} (reverse xs)"),
                serde_json::json!({"xs": vec![1; 512]}),
                100,
                true,
            ),
            (
                "equality",
                format!("{cycle} (eq xs xs)"),
                serde_json::json!({"xs": vec![1; 512]}),
                100,
                true,
            ),
            (
                "value-traversal",
                format!("{cycle} xs"),
                serde_json::json!({"xs": vec![1; 512]}),
                100,
                true,
            ),
            (
                "output-depth",
                format!("{cycle} xs"),
                serde_json::json!({"xs": vec![1; 512]}),
                100,
                true,
            ),
            (
                "output-write",
                format!("{cycle} seed"),
                serde_json::json!({"seed": "\0".repeat(4096)}),
                100,
                true,
            ),
            (
                "cond",
                format!("{cycle} (cond {})", "() ".repeat(1000)),
                Value::Null,
                100,
                true,
            ),
            (
                "work",
                format!("{cycle} (define f (lambda () (f))) (f)"),
                Value::Null,
                0,
                true,
            ),
        ];
        for (phase, form, env, threshold, expect_scopes) in cases {
            if phase == "cond" {
                assert_eq!(
                    eval_sandboxed_with_budget(&form, &env, MAX_STEPS, 1024)
                        .expect("empty cond clauses preserve nil"),
                    Value::Null,
                );
                let steps = minimal_steps(&form, &env, 1024);
                assert_eq!(steps, 4128, "clause cancellation must not add work charges");
                eprintln!("H6A cond default: minimal_steps={steps}");
            }
            let flag = Arc::new(AtomicBool::new(false));
            let worker_flag = flag.clone();
            let (reached_tx, reached_rx) = sync_channel(1);
            let (resume_tx, resume_rx) = sync_channel(1);
            let (done_tx, done_rx) = sync_channel(1);
            let worker = std::thread::spawn(move || {
                let scope_probe = ScopeProbe::start(None);
                let mut visits = 0;
                let mut units_seen = 0;
                let mut max_units = 0;
                let mut previous_offset = 0;
                let mut parked = false;
                let _checkpoint_probe = CheckpointProbe::start(move |observed, units| {
                    if observed != phase || parked {
                        return;
                    }
                    visits += 1;
                    if phase == "cond" {
                        // Only the actual cond loop emits this label/index;
                        // empty collect_list calls emit no list checkpoints.
                        assert_eq!(units, visits - 1, "every skipped clause must check");
                        max_units = max_units.max(units - previous_offset);
                        previous_offset = units;
                        units_seen = units;
                    } else if matches!(phase, "source" | "unescape" | "list" | "list-build") {
                        // Actual iterator byte/node positions, not claimed
                        // chunk sizes. Each new list starts at index zero.
                        if matches!(phase, "list" | "list-build") && units == 0 {
                            previous_offset = 0;
                        }
                        assert!(units >= previous_offset);
                        max_units = max_units.max(units - previous_offset);
                        previous_offset = units;
                        units_seen = units;
                    } else {
                        units_seen += units;
                        max_units = max_units.max(units);
                    }
                    let scope_count = OBSERVED_SCOPES.with_borrow(|observation| {
                        observation.as_ref().expect("scope probe").scopes.len()
                    });
                    let reached = if phase == "work" {
                        scope_count >= 12
                    } else {
                        visits >= threshold
                    };
                    if reached {
                        parked = true;
                        reached_tx
                            .send((visits, units_seen, max_units, scope_count))
                            .expect("checkpoint receipt");
                        resume_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("caller resumes worker");
                    }
                });
                let result = eval_sandboxed_with_cancellation(
                    &form,
                    &env,
                    MAX_STEPS,
                    1024,
                    worker_flag.clone(),
                );
                assert!(
                    matches!(result, Err(LispError::Cancelled)),
                    "{phase}: {result:?}"
                );
                assert!(worker_flag.load(Ordering::Acquire));
                if expect_scopes {
                    scope_probe.assert_reclaimed();
                } else {
                    OBSERVED_SCOPES.with_borrow(|observation| {
                        assert!(observation.as_ref().expect("scope probe").scopes.is_empty());
                    });
                }
                // Acknowledgment follows weak-probe proof, never just the flag store.
                done_tx.send(()).expect("teardown receipt");
            });
            let (visits, units, max_units, scope_count) = reached_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("real evaluator reaches checkpoint");
            eprintln!(
                "H6A {phase}: visits={visits} position_or_units={units} max_gap={max_units} scopes={scope_count}"
            );
            assert!(visits > 0);
            if phase == "cond" {
                assert_eq!((visits, units, max_units), (100, 99, 1));
                assert!(max_units <= 64, "empty-clause checkpoint spacing");
            } else if matches!(phase, "source" | "unescape") {
                assert!(units > 1024, "the actual scan must cross a chunk boundary");
                assert!(max_units <= 4, "at most one UTF-8 scalar per check");
            } else if phase != "work" {
                assert!(max_units <= 2, "at most two visited nodes per check");
            }
            if expect_scopes {
                assert!(
                    scope_count >= 2,
                    "root and orphan scopes must already exist"
                );
            }
            // The cancelled worker is still parked: no global/runtime control leaks.
            assert_eq!(
                eval_sandboxed("(+ 20 22)", &Value::Null).expect("independent default invocation"),
                Value::from(42)
            );
            flag.store(true, Ordering::Release);
            resume_tx.send(()).expect("resume cancelled worker");
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker teardown acknowledged");
            worker.join().expect("worker exited without panic");
        }
    }

    /// expect: "Cancellation brackets native phases and final publication without skipping cleanup" [H6A]
    #[test]
    fn h6a_phase_fences_and_raw_environment_ownership() {
        let source =
            "(define f (lambda () f)) (let () (begin (define g (lambda () g)) 0)) (reverse xs)";
        for phase in [
            "json-input-before",
            "json-input-after",
            "json-output-before",
            "json-output-after",
            "native-before",
            "native-after",
            "output-count-before",
            "output-count-after",
            "publish",
        ] {
            let flag = Arc::new(AtomicBool::new(false));
            let signal = flag.clone();
            let _observer = CheckpointProbe::start(move |observed, _| {
                if observed == phase {
                    signal.store(true, Ordering::Release);
                }
            });
            let probe = ScopeProbe::start(None);
            let result = eval_sandboxed_with_cancellation(
                source,
                &serde_json::json!({"xs": [1, 2, 3]}),
                MAX_STEPS,
                1024,
                flag,
            );
            assert!(
                matches!(result, Err(LispError::Cancelled)),
                "{phase}: {result:?}"
            );
            probe.assert_reclaimed();
        }
        // A separate JSON invocation's cancellation cannot clear a caller's raw root.
        let env = Rc::new(RefCell::new(Env::new_root()));
        for form in parse("(define f (lambda (x) (+ x 1)))").expect("raw definition") {
            eval(env.clone(), &form).expect("caller-owned definition");
        }
        assert!(matches!(
            eval_sandboxed_with_cancellation(
                "42",
                &Value::Null,
                MAX_STEPS,
                1024,
                Arc::new(AtomicBool::new(true))
            ),
            Err(LispError::Cancelled)
        ));
        let call = parse("(f 41)").expect("raw call");
        assert_eq!(
            eval(env.clone(), &call[0]).expect("raw root survives"),
            LispValue::Int(42)
        );
        env.borrow_mut().vars.clear();
    }

    /// expect: "Missing-key diagnostics never follow cyclic closure environments" [H6A]
    #[test]
    fn h6a_callable_keys_have_bounded_diagnostics() {
        for key in ["f", "(list f)", "car", "(list car)"] {
            let source = format!("(define f (lambda () f)) (assoc! {key} nil)");
            let probe = ScopeProbe::start(None);
            let error = eval_sandboxed_with_cancellation(
                &source,
                &Value::Null,
                MAX_STEPS,
                1024,
                Arc::new(AtomicBool::new(false)),
            )
            .expect_err("missing key");
            assert!(matches!(error, LispError::Runtime(_)), "{key}: {error}");
            let message = error.to_string();
            assert!(message.contains("not found") && message.contains("available keys: []"));
            assert!(message.contains(if key.contains('f') {
                "<lambda>"
            } else {
                "<native-function>"
            }));
            probe.assert_reclaimed();
        }
    }

    /// expect: "Output cancellation is distinct from quota and write_all cannot retry it" [H6A]
    #[test]
    fn h6a_output_cancellation_and_observer_unwind() {
        use std::io::Write;
        let flag = AtomicBool::new(true);
        let mut counter = OutputByteCounter {
            cancellation: Some(&flag),
            ..OutputByteCounter::default()
        };
        let error = counter.write_all(b"null").expect_err("cancelled writer");
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert!(counter.cancelled);
        assert!(!counter.exceeded);
        assert_eq!(counter.bytes, 0);
        let unwind = std::panic::catch_unwind(|| {
            let _observer = CheckpointProbe::start(|_, _| panic!("observer unwind"));
            eval_sandboxed("42", &Value::Null)
        });
        assert!(unwind.is_err());
        assert_eq!(
            eval_sandboxed("42", &Value::Null).expect("observer reset after unwind"),
            Value::from(42)
        );
    }

    /// expect: "Escaping amplification hits the pretty-document cap, not input/work admission" [H4B]
    #[test]
    fn h4b_sandbox_pretty_document_limit() {
        let env = serde_json::json!({"seed": "\0".repeat(174_759)});
        assert!(serde_json::to_vec(&env).expect("env JSON").len() < MAX_ENV_JSON_BYTES);
        for padding in [20, 21] {
            let form = format!(
                "(define f (lambda () f)) (concat seed \"{}\")",
                "x".repeat(padding)
            );
            let expected =
                Value::String(format!("{}{}", "\0".repeat(174_759), "x".repeat(padding)));
            assert_eq!(
                serde_json::to_vec_pretty(&expected).expect("oracle").len(),
                1_048_556 + padding
            );
            let probe = ScopeProbe::start(None);
            let result = eval_sandboxed_with_budget(&form, &env, 2_000_000, 1024);
            if padding == 20 {
                assert_eq!(result.expect("exact boundary must fit"), expected);
            } else {
                assert!(
                    matches!(result, Err(LispError::Runtime(ref error))
                    if error.contains("max_output_bytes") && error.contains("1048576")),
                    "cap+1 must fail at the output-byte limit"
                );
            }
            probe.assert_reclaimed();
        }
    }

    /// expect: "Pretty-byte admission includes UTF-8, escapes, keys and layout exactly as serde emits" [H4B]
    #[test]
    fn h4b_formatter_boundaries_and_compatibility() {
        for value in [
            Value::Null,
            serde_json::json!([true, false, -7, 2.5, [], {}]),
            serde_json::json!({"é\n\"": ["\0\u{001f}\n\r\t\u{0008}\u{000c}\\\"", "🦎"]}),
        ] {
            let oracle = serde_json::to_string_pretty(&value).expect("serde oracle");
            assert_eq!(
                pretty_json_output_size(&value).expect("count"),
                oracle.len()
            );
            assert_eq!(format_json_output(&value).expect("format"), oracle);
        }
        for (unit, repeats, padding) in [
            ("x", MAX_OUTPUT_BYTES - 2, 0),
            ("é", (MAX_OUTPUT_BYTES - 2) / 2, 0),
            ("\0", 174_759, 20),
        ] {
            for extra in [0, 1] {
                let value = Value::String(format!(
                    "{}{}",
                    unit.repeat(repeats),
                    "x".repeat(padding + extra)
                ));
                let oracle = serde_json::to_string_pretty(&value).expect("serde oracle");
                assert_eq!(oracle.len(), MAX_OUTPUT_BYTES + extra);
                OUTPUT_BUFFER_REQUESTS.with_borrow_mut(|requests| *requests = Some(Vec::new()));
                let result = format_json_output(&value);
                let requests = OUTPUT_BUFFER_REQUESTS
                    .with_borrow_mut(|requests| requests.take().expect("allocation observation"));
                if extra == 0 {
                    assert_eq!(requests, [MAX_OUTPUT_BYTES]);
                    assert_eq!(result.expect("exact boundary"), oracle);
                } else {
                    assert!(requests.is_empty(), "refusal must precede the final buffer");
                    assert!(matches!(result, Err(LispError::Runtime(ref error))
                        if error.contains("max_output_bytes") && error.contains("1048576")));
                }
            }
        }
        for mut value in [serde_json::json!([[""]]), serde_json::json!({"é\n": ""})] {
            let overhead = serde_json::to_vec_pretty(&value)
                .expect("empty layout")
                .len();
            for extra in [0, 1] {
                let text = "x".repeat(MAX_OUTPUT_BYTES - overhead + extra);
                match &mut value {
                    Value::Array(values) => values[0][0] = Value::String(text),
                    Value::Object(values) => {
                        *values.values_mut().next().expect("one key") = Value::String(text)
                    }
                    _ => unreachable!("fixture container"),
                }
                assert_eq!(
                    serde_json::to_vec_pretty(&value)
                        .expect("layout oracle")
                        .len(),
                    MAX_OUTPUT_BYTES + extra
                );
                assert_eq!(format_json_output(&value).is_ok(), extra == 0);
            }
        }
        for form in ["", "; no program", "nil"] {
            let result = eval_sandboxed(form, &serde_json::json!({})).expect("null result");
            assert_eq!(format_json_output(&result).expect("null text"), "null");
        }
    }

    /// expect: "Only the final alist key projection counts, while existing work guards still apply" [H4B]
    #[test]
    fn h4b_output_duplicate_key_projection() {
        let seed = "\0".repeat(90_000);
        let env = serde_json::json!({"seed": seed});
        assert!(serde_json::to_vec(&env).expect("raw env").len() < MAX_ENV_JSON_BYTES);
        let form = "(list (list \"same\" seed) (list \"same\" seed) (list \"same\" 1))";
        let raw = serde_json::json!([["same", seed], ["same", seed], ["same", 1]]);
        assert!(
            serde_json::to_vec_pretty(&raw)
                .expect("preprojection oracle")
                .len()
                > MAX_OUTPUT_BYTES
        );
        let result = eval_sandboxed_with_budget(form, &env, MAX_STEPS, 1024)
            .expect("small projected result");
        assert_eq!(result, serde_json::json!({"same": 1}));
        assert_eq!(
            format_json_output(&result).expect("object text"),
            "{\n  \"same\": 1\n}"
        );
        assert!(matches!(
            eval_sandboxed_with_budget(form, &env, 100, 1024),
            Err(LispError::StepLimitExceeded(100))
        ));
    }

    /// expect: "Counting is per-call, stops before accepting cap+1, and formatting guards raw Value depth" [H4B]
    #[test]
    fn h4b_counter_isolation_and_depth() {
        use std::io::Write;
        let mut counter = OutputByteCounter {
            bytes: MAX_OUTPUT_BYTES - 1,
            ..OutputByteCounter::default()
        };
        counter.write_all(b"x").expect("last byte fits");
        assert!(counter.write_all(b"x").is_err());
        assert_eq!(counter.bytes, MAX_OUTPUT_BYTES);
        assert!(counter.exceeded);
        let mut fresh = OutputByteCounter::default();
        fresh.write_all(b"null").expect("new counter");
        assert_eq!(fresh.bytes, 4);
        assert!(!fresh.exceeded);
        std::thread::scope(|scope| {
            for extra in [0, 1, 0, 1] {
                scope.spawn(move || {
                    let value = Value::String("x".repeat(MAX_OUTPUT_BYTES - 2 + extra));
                    assert_eq!(format_json_output(&value).is_ok(), extra == 0);
                    assert_eq!(
                        format_json_output(&Value::Null).expect("fresh call"),
                        "null"
                    );
                });
            }
        });
        for containers in [127, 128, 1024] {
            let mut value = Value::Null;
            for _ in 0..containers {
                value = Value::Array(vec![value]);
            }
            OUTPUT_BUFFER_REQUESTS.with_borrow_mut(|requests| *requests = Some(Vec::new()));
            let result = format_json_output(&value);
            let requests = OUTPUT_BUFFER_REQUESTS
                .with_borrow_mut(|requests| requests.take().expect("allocation observation"));
            if containers == 127 {
                let text = result.expect("legal depth");
                assert_eq!(requests, [text.len()]);
                assert_eq!(
                    text,
                    serde_json::to_string_pretty(&value).expect("serde oracle")
                );
            } else {
                assert!(
                    requests.is_empty(),
                    "depth refusal must precede the final buffer"
                );
                assert!(matches!(
                    result,
                    Err(LispError::OutputDepthLimitExceeded(128))
                ));
            }
            // The helper borrows raw Values; their owner still controls dropping
            // arbitrarily deep Rust-created data. Peel this fixture iteratively.
            while let Value::Array(mut values) = value {
                value = values.pop().expect("one child");
            }
        }
    }

    /// expect: "Source admission counts UTF-8 bytes and precedes tokenization" [H4A]
    #[test]
    fn h4a_source_admission_precedes_parsing() {
        for padding in ["x", "é"] {
            let source = format!("1;{}", padding.repeat((65_536 - 2) / padding.len()));
            assert_eq!(source.len(), 65_536);
            assert_eq!(
                eval_sandboxed_with_budget(&source, &serde_json::json!({}), 2_000_000, 8192)
                    .expect("exact source ceiling is admitted"),
                serde_json::json!(1)
            );
            assert!(
                parse(&source).is_ok(),
                "standalone parser admits the ceiling"
            );
            let malformed = format!("({source}");
            for result in [
                parse(&malformed).map(|_| ()),
                eval_sandboxed_with_budget(&malformed, &Value::Null, 2_000_000, 8192).map(|_| ()),
            ] {
                assert!(
                    matches!(result, Err(LispError::Parse(ref error))
                    if error.contains("source") && error.contains("65537") && error.contains("65536")),
                    "source cap must precede malformed syntax: {result:?}"
                );
            }
        }
    }

    /// expect: "Requested budgets are rejected, not clamped, before source or env work" [H4A]
    #[test]
    fn h4a_requested_budget_admission() {
        let empty = serde_json::json!({});
        assert_eq!(
            eval_sandboxed_with_budget("1", &empty, 2_000_000, 8192)
                .expect("explicit ceilings are admitted"),
            serde_json::json!(1)
        );
        for (steps, depth, field, requested, allowed) in [
            (2_000_001, 1024, "max_steps", 2_000_001, 2_000_000),
            (u64::MAX, 1024, "max_steps", u64::MAX, 2_000_000),
            (100_000, 8193, "max_depth", 8193, 8192),
            (100_000, u64::MAX, "max_depth", u64::MAX, 8192),
        ] {
            let result = eval_sandboxed_with_budget("(", &Value::from(u64::MAX), steps, depth);
            assert!(
                matches!(result, Err(LispError::Runtime(ref error))
                if error.contains(field) && error.contains(&requested.to_string())
                    && error.contains(&allowed.to_string())),
                "{result:?}"
            );
        }
        assert!(matches!(
            eval_sandboxed_with_budget("1", &empty, 0, 1024),
            Err(LispError::StepLimitExceeded(0))
        ));
        assert_eq!(
            eval_sandboxed_with_budget("1", &empty, 100_000, 0)
                .expect("atoms do not consume recursion depth"),
            serde_json::json!(1)
        );
        assert!(matches!(
            eval_sandboxed_with_budget("(+ 1 2)", &empty, 100_000, 0),
            Err(LispError::DepthLimitExceeded(0))
        ));
    }

    /// expect: "Tracked registrations prepay cleanup work even when the result is tiny" [H4A]
    #[test]
    fn h4a_registry_work_is_budgeted() {
        let mut tracked = EvalBudget::new(2, 1024);
        tracked.invocation_scopes = Some(Rc::new(InvocationScopes::default()));
        let scope = tracked
            .new_scope(Env::new_root())
            .expect("two ticks admit one entry");
        assert_eq!(tracked.steps_used, 2);
        assert!(matches!(
            tracked.new_scope(Env::new_root()),
            Err(LispError::StepLimitExceeded(2))
        ));
        assert_eq!(
            tracked
                .invocation_scopes
                .as_ref()
                .expect("registry")
                .scopes
                .borrow()
                .len(),
            1
        );
        drop(scope);
        drop(tracked);

        let source = "(define f (lambda (n) (if (= n 0) 1 (let () (f (- n 1)))))) (f 100)";
        let empty = serde_json::json!({});
        let mut untracked = EvalBudget::new(0, 1024);
        untracked
            .new_scope(Env::new_root())
            .expect("raw scopes have no registry charge");
        assert_eq!(untracked.steps_used, 0);
        // Measured through the public entry: 1,406 ticks without registry
        // charging, 1,810 with it. This budget separates those behaviors.
        let probe = ScopeProbe::start(None);
        assert!(matches!(
            eval_sandboxed_with_budget(source, &empty, 1600, 1024),
            Err(LispError::StepLimitExceeded(_))
        ));
        probe.assert_reclaimed();
        assert_eq!(
            eval_sandboxed(source, &empty).expect("default budget still fits"),
            serde_json::json!(1)
        );
    }

    #[derive(Default)]
    struct ScopeObservation {
        scopes: Vec<Weak<RefCell<Env>>>,
        panic_at: Option<usize>,
    }

    thread_local! {
        static OBSERVED_SCOPES: RefCell<Option<ScopeObservation>> = const { RefCell::new(None) };
        static OUTPUT_BUFFER_REQUESTS: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
    }

    pub(super) fn observe_output_allocation(capacity: usize) {
        OUTPUT_BUFFER_REQUESTS.with_borrow_mut(|requests| {
            if let Some(requests) = requests {
                requests.push(capacity);
            }
        });
    }

    pub(super) fn observe_scope(scope: &Rc<RefCell<Env>>) {
        let inject_panic = OBSERVED_SCOPES.with_borrow_mut(|observation| {
            let Some(observation) = observation else {
                return false;
            };
            observation.scopes.push(Rc::downgrade(scope));
            observation.panic_at == Some(observation.scopes.len())
        });
        assert!(!inject_panic, "injected scope-allocation unwind");
    }

    struct ScopeProbe;

    impl ScopeProbe {
        fn start(panic_at: Option<usize>) -> Self {
            OBSERVED_SCOPES.with_borrow_mut(|observation| {
                assert!(observation.is_none(), "scope probes cannot overlap");
                *observation = Some(ScopeObservation {
                    scopes: Vec::new(),
                    panic_at,
                });
            });
            Self
        }

        fn assert_reclaimed(&self) {
            OBSERVED_SCOPES.with_borrow(|observation| {
                let scopes = &observation.as_ref().expect("active scope probe").scopes;
                assert!(!scopes.is_empty(), "probe must observe real evaluation");
                assert!(
                    scopes.iter().all(|scope| scope.upgrade().is_none()),
                    "sandbox invocation retained an environment after completion"
                );
            });
        }
    }

    impl Drop for ScopeProbe {
        fn drop(&mut self) {
            OBSERVED_SCOPES.with_borrow_mut(|observation| drop(observation.take()));
        }
    }

    /// expect: "Completed sandbox calls release all invocation environments, including cycles" [P4]
    #[test]
    fn sandbox_reclaims_scopes_on_every_exit() {
        let output_failure = format!(
            "(define f (lambda (x) x)) '{}1{}",
            "(".repeat(128),
            ")".repeat(128)
        );
        let cases = [
            (
                "(define f (lambda (x) x)) (let () (begin (define g (lambda () 1)) 0))",
                100_000,
                1024,
                "success",
            ),
            (
                "(define f (lambda (x) x)) (let () (begin (define g (lambda () 1)) missing))",
                100_000,
                1024,
                "runtime",
            ),
            ("(define f (lambda (x) (f x))) (f 1)", 200, 1024, "steps"),
            ("(define f (lambda (x) (f x))) (f 1)", 100_000, 8, "depth"),
            (output_failure.as_str(), 100_000, 256, "output"),
        ];
        for (source, steps, depth, exit) in cases {
            let probe = ScopeProbe::start(None);
            let result = eval_sandboxed_with_budget(source, &serde_json::json!({}), steps, depth);
            match exit {
                "success" => {
                    assert_eq!(result.expect("legitimate invocation"), serde_json::json!(0))
                }
                "runtime" => assert!(matches!(result, Err(LispError::UnboundSymbol(_)))),
                "steps" => assert!(matches!(result, Err(LispError::StepLimitExceeded(_)))),
                "depth" => assert!(matches!(result, Err(LispError::DepthLimitExceeded(_)))),
                "output" => assert!(matches!(
                    result,
                    Err(LispError::OutputDepthLimitExceeded(_))
                )),
                _ => panic!("unknown exit fixture"),
            }
            probe.assert_reclaimed();
        }
    }

    /// expect: "Numeric errors reclaim invocation scopes even after creating closure cycles" [P4]
    #[test]
    fn h3_numeric_errors_reclaim_scopes() {
        for expression in ["(* 1e308 2.0)", "(/ 9223372036854775807 1)", "(sqrt -1)"] {
            let probe = ScopeProbe::start(None);
            let source = format!(
                "(define f (lambda (x) (f x))) (let () (begin (define g (lambda () 1)) {expression}))"
            );
            assert!(
                matches!(
                    eval_sandboxed(&source, &serde_json::json!({})),
                    Err(LispError::Runtime(_))
                ),
                "{expression}"
            );
            probe.assert_reclaimed();
        }
    }

    /// expect: "Repeated stateless invocations leave no cumulative scope retention" [P4]
    #[test]
    fn repeated_invocations_release_all_scopes() {
        for _ in 0..100 {
            let probe = ScopeProbe::start(None);
            let output = eval_sandboxed(
                "(define make (lambda (x) (lambda (y) (+ x y)))) (define add (make 40)) (add 2)",
                &serde_json::json!({}),
            )
            .expect("repeated escaping-closure invocation");
            assert_eq!(output, serde_json::json!(42));
            probe.assert_reclaimed();
        }
    }

    /// expect: "Unwinding a sandbox invocation releases scopes after temporary borrows end" [P4]
    #[test]
    fn sandbox_reclaims_scopes_during_unwind() {
        let probe = ScopeProbe::start(Some(2));
        let result = std::panic::catch_unwind(|| {
            eval_sandboxed(
                "(define f (lambda (x) x)) (let () 0)",
                &serde_json::json!({}),
            )
        });
        assert!(
            result.is_err(),
            "the fixture must unwind after forming a cycle"
        );
        probe.assert_reclaimed();
    }

    /// expect: "Invocation cleanup preserves recursive and escaping lexical closures" [P3]
    #[test]
    fn sandbox_preserves_lexical_closure_behavior() {
        for (source, expected) in [
            (
                "(define f (lambda (n) (if (= n 0) 0 (+ 1 (f (- n 1)))))) (f 5)",
                serde_json::json!(5),
            ),
            (
                "(define even (lambda (n) (if (= n 0) true (odd (- n 1))))) (define odd (lambda (n) (if (= n 0) false (even (- n 1))))) (even 6)",
                serde_json::json!(true),
            ),
            (
                "(define make (lambda (x) (lambda (y) (+ x y)))) (define add (make 40)) (add 2)",
                serde_json::json!(42),
            ),
            (
                "(let ((x 1) (f (lambda () x))) (begin (define x 2) (f)))",
                serde_json::json!(2),
            ),
            ("(list (lambda (x) x))", serde_json::json!(["<lambda>"])),
        ] {
            let probe = ScopeProbe::start(None);
            assert_eq!(
                eval_sandboxed(source, &serde_json::json!({})).expect("lexical closure fixture"),
                expected
            );
            probe.assert_reclaimed();
        }
    }

    /// expect: "Integer source tokens outside i64 fail instead of silently rounding to floats" [P8]
    #[test]
    fn integer_source_tokens_do_not_fall_back_to_float() {
        for token in [
            "9223372036854775808",
            "-9223372036854775809",
            "+9223372036854775808",
            "184467440737095516160",
        ] {
            let error = eval_sandboxed(token, &serde_json::json!({}))
                .expect_err("out-of-range integer spelling must be rejected");
            assert!(matches!(error, LispError::Parse(_)), "{token}: {error}");
            assert!(error.to_string().contains(token), "error must name {token}");
        }
    }

    /// expect: "Numeric admission preserves exact i64 endpoints and explicit float or text values" [P3]
    #[test]
    fn numeric_admission_preserves_legitimate_spellings() {
        for (source, expected) in [
            ("-9223372036854775808", serde_json::json!(i64::MIN)),
            ("9223372036854775807", serde_json::json!(i64::MAX)),
            (
                "9007199254740993",
                serde_json::json!(9_007_199_254_740_993_i64),
            ),
            ("(eq 1.0 1)", serde_json::json!(false)),
            ("(eq 1e0 1.0)", serde_json::json!(true)),
            ("1e100", serde_json::json!(1e100)),
            (
                "\"184467440737095516160\"",
                serde_json::json!("184467440737095516160"),
            ),
        ] {
            assert_eq!(
                eval_sandboxed(source, &serde_json::json!({}))
                    .expect("legitimate numeric spelling"),
                expected
            );
        }
    }

    /// expect: "Parsed JSON integers outside i64 are refused at the sandbox boundary" [P8]
    #[test]
    fn parsed_json_integers_require_i64_at_sandbox_boundary() {
        for input in [
            serde_json::json!({"value": u64::MAX}),
            serde_json::json!({"nested": [{"value": u64::MAX}]}),
        ] {
            let error = eval_sandboxed("1", &input).expect_err("unrepresentable JSON integer");
            assert!(error.to_string().contains("integer"));
            assert!(error.to_string().contains("i64"));
        }
        for value in [
            serde_json::json!(i64::MIN),
            serde_json::json!(i64::MAX),
            serde_json::json!(1.0),
            serde_json::json!(1e100),
        ] {
            assert_eq!(
                eval_sandboxed("value", &serde_json::json!({"value": value}))
                    .expect("representable parsed number"),
                value
            );
        }
    }

    /// expect: "Raw eval callers keep their own environments usable across calls" [P3]
    #[test]
    fn raw_evaluation_keeps_caller_owned_environments() {
        let env = Rc::new(RefCell::new(Env::new_root()));
        for form in parse("(define f (lambda (x) (+ x 1)))").expect("definition parses") {
            eval(env.clone(), &form).expect("definition evaluates");
        }
        let form = parse("(f 2)").expect("call parses").remove(0);
        assert_eq!(
            to_json(&eval(env.clone(), &form).expect("retained raw closure works")),
            serde_json::json!(3)
        );
        env.borrow_mut().vars.clear();
    }

    /// expect: "Malformed or oversized Lisp work returns an error instead of killing the host" [P4]
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "hostile input runs in a bounded subprocess on the test thread, never GPUI"
    )]
    fn hostile_inputs_are_process_safe() -> Result<(), Box<dyn std::error::Error>> {
        const CHILD_CASE: &str = "HKASK_LISP_SAFETY_TEST_CASE";
        if let Ok(case) = std::env::var(CHILD_CASE) {
            let empty = serde_json::json!({});
            let result = match case.as_str() {
                "nested" => eval_sandboxed_with_budget(
                    &format!("'{}1{}", "(".repeat(20_000), ")".repeat(20_000)),
                    &empty,
                    100_000,
                    32,
                ),
                "quotes" => eval_sandboxed_with_budget(
                    &format!("{}1", "'".repeat(20_000)),
                    &empty,
                    100_000,
                    32,
                ),
                "unclosed" => eval_sandboxed_with_budget(&"(".repeat(20_000), &empty, 100_000, 32),
                "source" => eval_sandboxed_with_budget(
                    &format!("\"{}\"", "x".repeat(10_000)),
                    &empty,
                    1_000,
                    32,
                ),
                "environment" => {
                    let nested = (0..200).fold(Value::Null, |value, _| Value::Array(vec![value]));
                    eval_sandboxed_with_budget(
                        "input",
                        &serde_json::json!({"input": nested}),
                        100_000,
                        32,
                    )
                }
                "strings" => eval_sandboxed_with_budget(
                    "(define grow (lambda (n x) (if (= n 0) x (grow (- n 1) (concat x x))))) (grow 16 \"a\")",
                    &empty,
                    5_000,
                    1024,
                ),
                "shared_lists" => eval_sandboxed_with_budget(
                    "(define grow (lambda (n x) (if (= n 0) x (grow (- n 1) (list x x))))) (grow 16 '(1))",
                    &empty,
                    5_000,
                    1024,
                ),
                "output_depth" => eval_sandboxed_with_budget(
                    &format!("'{}1{}", "(".repeat(2_000), ")".repeat(2_000)),
                    &empty,
                    100_000,
                    2_001,
                ),
                "malformed_pairs" => {
                    let program = "(define double (lambda (n xs) (if (= n 0) xs (double (- n 1) (append xs xs))))) (define p (double 13 '(1))) (assoc \"absent\" (double 13 (list p)))";
                    assert!(eval_sandboxed(program, &empty)?.is_null());
                    return Ok(());
                }
                "json_lifecycle" => {
                    let form = format!("'{}1{}", "(".repeat(100), ")".repeat(100));
                    let output = eval_sandboxed(&form, &empty)?;
                    let serialized = serde_json::to_string_pretty(&output)?;
                    let reparsed: Value = serde_json::from_str(&serialized)?;
                    drop(reparsed);
                    drop(output);
                    return Ok(());
                }
                "flat_list" => {
                    let form = format!("'({})", "1 ".repeat(10_000));
                    let output = eval_sandboxed(&form, &empty)?;
                    assert_eq!(output.as_array().map(Vec::len), Some(10_000));
                    let serialized = serde_json::to_string_pretty(&output)?;
                    drop(output);
                    assert_eq!(
                        serde_json::from_str::<Value>(&serialized)?
                            .as_array()
                            .map(Vec::len),
                        Some(10_000)
                    );
                    return Ok(());
                }
                _ => return Err("unknown subprocess case".into()),
            };
            assert!(result.is_err(), "{case} escaped the resource budget");
            return Ok(());
        }
        for case in [
            "nested",
            "quotes",
            "unclosed",
            "source",
            "environment",
            "strings",
            "shared_lists",
            "output_depth",
            "malformed_pairs",
            "json_lifecycle",
            "flat_list",
        ] {
            let mut child = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "tests::hostile_inputs_are_process_safe",
                    "--nocapture",
                ])
                .env(CHILD_CASE, case)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while child.try_wait()?.is_none() {
                if std::time::Instant::now() >= deadline {
                    child.kill()?;
                    child.wait()?;
                    return Err(format!("{case} exceeded subprocess deadline").into());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let output = child.wait_with_output()?;
            assert!(
                output.status.success(),
                "{case} failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    #[test]
    fn discarded_symbol_expansion_still_consumes_work_budget() {
        let program = format!(
            "(define double (lambda (n xs) (if (= n 0) xs (double (- n 1) (append xs xs))))) (begin (double 8 (list '{})) 0)",
            "s".repeat(1_000)
        );
        assert!(matches!(
            eval_sandboxed_with_budget(&program, &serde_json::json!({}), 5_000, 1024),
            Err(LispError::StepLimitExceeded(_))
        ));
    }

    #[test]
    fn output_nesting_is_bounded_independently_of_evaluation_depth() {
        let program = format!("'{}1{}", "(".repeat(256), ")".repeat(256));
        assert!(
            eval_sandboxed_with_budget(&program, &serde_json::json!({}), 100_000, 1024).is_err()
        );
    }

    #[test]
    fn arithmetic_evaluates() {
        let result = eval_sandboxed("(+ 1 2)", &serde_json::json!({})).unwrap();
        assert_eq!(result, serde_json::json!(3));
    }

    #[test]
    fn environment_bindings_resolve() {
        let env = serde_json::json!({"x": 10});
        let result = eval_sandboxed("(* x x)", &env).unwrap();
        assert_eq!(result, serde_json::json!(100));
    }

    #[test]
    fn unbound_symbol_is_an_error_not_a_crash() {
        let err = eval_sandboxed("nosuchsymbol", &serde_json::json!({})).unwrap_err();
        assert!(matches!(err, LispError::UnboundSymbol(_)));
    }

    #[test]
    fn parse_error_surfaces_as_parse_variant() {
        let err = eval_sandboxed("(+ 1", &serde_json::json!({})).unwrap_err();
        assert!(matches!(err, LispError::Parse(_)));
    }

    #[test]
    fn empty_program_yields_null() {
        let result = eval_sandboxed("", &serde_json::json!({})).unwrap();
        assert_eq!(result, serde_json::json!(null));
    }

    #[test]
    fn infinite_loop_hits_step_budget() {
        // (define (loop) (loop)) then call it — unbounded recursion consumes
        // steps and must be rejected by the step/depth limit, never hang.
        // `define` has no function sugar — bind a lambda instead.
        let program = "(define loop (lambda (n) (loop (+ n 1)))) (loop 0)";
        let err =
            eval_sandboxed_with_budget(program, &serde_json::json!({}), 1000, 1024).unwrap_err();
        assert!(
            matches!(
                err,
                LispError::StepLimitExceeded(_) | LispError::DepthLimitExceeded(_)
            ),
            "unbounded recursion must hit a budget, got: {err}"
        );
    }

    /// The observed live failure behind the 64→1024 default raise: a
    /// recursive helper over a real-scale validation list (134 claim
    /// assignments) needed depth ~300 — the former default of 64 overflowed
    /// at ~16 elements and the first attempt failed, wasting turns on
    /// retries. The DEFAULT budget (no explicit max_depth) must cover it.
    #[test]
    fn recursive_helper_over_real_scale_list_succeeds_at_default_depth() {
        let assignments: Vec<Value> = (0..134)
            .map(|index| {
                serde_json::json!({"claim_id": format!("c{index}"), "provenance": "tool_verified"})
            })
            .collect();
        let program = r#"
            (define count-verified
              (lambda (lst)
                (if (< (length lst) 1)
                    0
                    (+ (if (string-contains "tool_verified" (assoc "provenance" (car lst))) 1 0)
                       (count-verified (cdr lst))))))
            (count-verified assignments)
        "#;
        let result = eval_sandboxed(program, &serde_json::json!({ "assignments": assignments }))
            .expect("a 134-element recursive helper must succeed at the default depth");
        assert_eq!(result, serde_json::json!(134));
    }

    #[test]
    fn deep_recursion_hits_depth_budget_before_stack_overflow() {
        // Depth budget far below the real stack limit: the interpreter must
        // reject, not segfault.
        let program = "(define f (lambda (n) (+ 1 (f (+ n 1))))) (f 0)";
        let err =
            eval_sandboxed_with_budget(program, &serde_json::json!({}), 1_000_000, 32).unwrap_err();
        assert!(
            matches!(err, LispError::DepthLimitExceeded(32)),
            "expected depth limit, got: {err}"
        );
    }

    #[test]
    fn generous_budget_completes_legitimate_recursion() {
        // A bounded recursive sum must complete within the default budget —
        // the limits stop runaway programs, not legitimate compute.
        // Default depth budget is 64 — recursion deeper than that needs an
        // explicit larger budget (the lisp_eval tool's documented knobs).
        let program = "(define sum (lambda (n) (if (= n 0) 0 (+ n (sum (- n 1)))))) (sum 40)";
        let result =
            eval_sandboxed_with_budget(program, &serde_json::json!({}), 100_000, 128).unwrap();
        assert_eq!(result, serde_json::json!(820));
    }

    /// The minimal `max_steps` at which `form` completes against `env` —
    /// the exact step cost, measured deterministically (doubling then binary
    /// search; every probe re-runs the same form, so the answer is stable).
    /// L3's quantification instrument: no clocks, only the budget boundary.
    fn minimal_steps(form: &str, env: &Value, max_depth: u64) -> u64 {
        let mut hi = 1u64;
        while eval_sandboxed_with_budget(form, env, hi, max_depth).is_err() {
            hi = hi.saturating_mul(2);
            assert!(hi <= 1 << 24, "{form} did not complete even at 16M steps");
        }
        let mut lo = 1u64;
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            if eval_sandboxed_with_budget(form, env, mid, max_depth).is_err() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hi
    }

    /// lisp-repair L3: the walker's step cost is linear in element count and
    /// real-scale walks fit the default budget. BEFORE the fix this was
    /// quadratic — `charge_native_arguments` re-charged every list node on
    /// each argument pass, so the O(1) `is_null`/`cdr` pair cost O(len) per
    /// level: 600 elements needed ~365,000 steps against the 100,000
    /// default (the matrix specimen). The eprintln line is the before/after
    /// receipt; run with `--nocapture` when re-quantifying.
    #[test]
    fn walker_step_cost_is_linear_and_fits_default_budget() {
        let form = "(define count (lambda (lst) (if (is_null lst) 0 (+ 1 (count (cdr lst)))))) (count items)";
        let steps_100 = minimal_steps(
            form,
            &serde_json::json!({"items": (0..100).collect::<Vec<i64>>()}),
            8192,
        );
        let steps_600 = minimal_steps(
            form,
            &serde_json::json!({"items": (0..600).collect::<Vec<i64>>()}),
            8192,
        );
        eprintln!("walker step cost: 100 elements = {steps_100}, 600 elements = {steps_600}");
        // Linear bound: 6x elements must not cost more than 6x + constant
        // headroom (the quadratic model costs 36x).
        assert!(
            steps_600 <= steps_100 * 6 + 2_000,
            "walker cost is not linear: 100 -> {steps_100}, 600 -> {steps_600}"
        );
        // Real scale fits the default budget with an order of magnitude of
        // headroom.
        assert!(
            steps_600 < 10_000,
            "600-element walker should fit the 100,000 default easily, costs {steps_600}"
        );
    }

    /// Traversal builtins pay one step per node (the L3 cost model): a
    /// 10,000-element env list (~10,020 steps to load) plus one traversal
    /// (~10,000) exceeds a 15,000 budget, while the O(1) controls below
    /// still pass — nodes are charged at creation and at real traversal,
    /// never for merely passing a list as an argument.
    #[test]
    fn traversal_builtins_charge_per_node() {
        let env = serde_json::json!({"items": (0..10_000i64).collect::<Vec<i64>>()});
        for form in [
            "(length items)",
            "(nth 0 items)",
            "(reverse items)",
            "(append items items)",
        ] {
            assert!(
                matches!(
                    eval_sandboxed_with_budget(form, &env, 15_000, 1024),
                    Err(LispError::StepLimitExceeded(_))
                ),
                "{form} should charge one step per traversed node"
            );
        }
        // Success control at the same scale: 1,000 elements, traversal
        // included, fits a 25,000 budget.
        let small = serde_json::json!({"items": (0..1_000i64).collect::<Vec<i64>>()});
        assert_eq!(
            eval_sandboxed_with_budget("(length items)", &small, 25_000, 1024)
                .expect("1,000-element length fits"),
            serde_json::json!(1_000)
        );
    }

    /// The fix receipt (lisp-repair L3): O(1) list operations no longer
    /// charge per node. Before the fix, `(is_null (cdr items))` on a
    /// 10,000-element list paid ~10,000 steps for the `cdr` argument pass
    /// and exceeded a 15,000 budget; now the whole form costs a handful of
    /// steps after the ~10,020 env load.
    #[test]
    fn constant_time_list_ops_do_not_charge_per_node() {
        let env = serde_json::json!({"items": (0..10_000i64).collect::<Vec<i64>>()});
        assert_eq!(
            eval_sandboxed_with_budget("(is_null (cdr items))", &env, 15_000, 1024)
                .expect("O(1) list ops must not charge per node"),
            serde_json::json!(false)
        );
    }

    #[test]
    fn step_budget_counts_across_multiple_top_level_forms() {
        // Two forms share one budget: a cheap first form + runaway second
        // must still trip the limit.
        let err = eval_sandboxed_with_budget(
            "(+ 1 1) (define loop (lambda (n) (loop (+ n 1)))) (loop 0)",
            &serde_json::json!({}),
            500,
            64,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            LispError::StepLimitExceeded(_) | LispError::DepthLimitExceeded(_)
        ));
    }

    #[test]
    fn string_contains_finds_substring() {
        let result = eval_sandboxed(
            r#"(string-contains "growth" "revenue growth of 12%")"#,
            &serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(result, serde_json::json!(true));
    }

    #[test]
    fn string_contains_rejects_absent_substring() {
        let result = eval_sandboxed(
            r#"(string-contains "decline" "revenue growth of 12%")"#,
            &serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(result, serde_json::json!(false));
    }

    #[test]
    fn string_contains_empty_needle_errors_not_verifies() {
        // An empty needle would "verify" any source text — in citation
        // verification that is a check that fires on correct output. It
        // must error, not return true.
        let err = eval_sandboxed(
            r#"(string-contains "" "revenue growth of 12%")"#,
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert!(matches!(err, LispError::Runtime(_)), "got: {err}");
    }

    #[test]
    fn string_contains_non_string_args_error() {
        let err = eval_sandboxed(
            r#"(string-contains 5 "revenue growth of 12%")"#,
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert!(matches!(err, LispError::TypeError { .. }), "got: {err}");
    }

    #[test]
    fn string_contains_longer_needle_errors_naming_reversal() {
        // The 2026-09-28 incident shape: (string-contains haystack needle)
        // reversed. A needle longer than the haystack can never be a
        // substring, so the builtin errors loudly instead of silently
        // returning false — the silent false hid a 20-quote verification
        // failure and was first mis-filed as a tool bug.
        let err = eval_sandboxed(
            r#"(string-contains "hello world" "world")"#,
            &serde_json::json!({}),
        )
        .unwrap_err();
        let LispError::Runtime(message) = &err else {
            panic!("longer needle must be a runtime error, got: {err}");
        };
        assert!(
            message.contains("probably reversed"),
            "error must name the probable reversal: {message}"
        );
        assert!(
            message.contains("needle first"),
            "error must state the signature order: {message}"
        );
    }

    #[test]
    fn string_contains_equal_length_different_content_stays_false() {
        // Equal length is not a reversal signature: different content is a
        // legitimate false and identical content a legitimate true — the
        // guard fires only on a strictly longer needle.
        let result =
            eval_sandboxed(r#"(string-contains "abc" "xyz")"#, &serde_json::json!({})).unwrap();
        assert_eq!(result, serde_json::json!(false));
    }

    #[test]
    fn boundary_ok_accepts_admissible_content() {
        for form in [
            r#"(boundary-ok "record:" "record:x")"#,
            r#"(boundary-ok "record:" "record:hidden-not-leading")"#,
            r#"(boundary-ok "artifact:" "artifact:skill/foo")"#,
            // Legitimate multi-word values must pass: the check is
            // boundary-scoped, not value-wide.
            r#"(boundary-ok "boundary:" "boundary:a b c")"#,
            r#"(boundary-ok "expectation:" "expectation:Two Words")"#,
            // Empty prefix checks the value's own first character.
            r#"(boundary-ok "" "ok")"#,
        ] {
            assert_eq!(
                eval_sandboxed(form, &serde_json::json!({})).unwrap(),
                serde_json::json!(true),
                "form: {form}"
            );
        }
    }

    #[test]
    fn boundary_ok_rejects_every_blocklist_bypass() {
        // The forms that defeated the space/tab/newline blocklist in the VC-0
        // typed predicate (live 2026-10-18). The Lisp reader unescapes only
        // \n, \t, \r, \" and \\, so those are written literally; characters
        // it does not unescape (VT, FF, NBSP, ZWSP, ideographic space, soft
        // hyphen, BOM) enter as DATA via env — which is also how they reach a
        // real form, carried in from a JSON step output.
        for form in [
            r#"(boundary-ok "record:" "record: hidden")"#,
            r#"(boundary-ok "record:" "record:\thidden")"#,
            r#"(boundary-ok "record:" "record:\nhidden")"#,
            r#"(boundary-ok "record:" "record:\rhidden")"#,
        ] {
            assert_eq!(
                eval_sandboxed(form, &serde_json::json!({})).unwrap(),
                serde_json::json!(false),
                "form: {form}"
            );
        }
        // Known-positive control first, so a broken env binding cannot make
        // the loop below pass vacuously.
        assert_eq!(
            eval_sandboxed(
                r#"(boundary-ok "record:" v)"#,
                &serde_json::json!({ "v": "record:x" })
            )
            .unwrap(),
            serde_json::json!(true),
            "control: env-delivered admissible value must pass"
        );
        for ch in [
            '\u{000b}', // VT
            '\u{000c}', // FF
            '\u{00a0}', // NBSP
            '\u{200b}', // ZWSP
            '\u{3000}', // ideographic space
            '\u{00ad}', // soft hyphen (Cf)
            '\u{feff}', // BOM (Cf)
        ] {
            let value = format!("record:{ch}hidden");
            assert_eq!(
                eval_sandboxed(
                    r#"(boundary-ok "record:" v)"#,
                    &serde_json::json!({ "v": value })
                )
                .unwrap(),
                serde_json::json!(false),
                "value: {value:?}"
            );
        }
    }

    #[test]
    fn boundary_ok_value_equal_to_prefix_is_false_not_error() {
        // Nothing follows the label: there is no character to inspect, and
        // "no usable content" is the real answer — not an error.
        assert_eq!(
            eval_sandboxed(
                r#"(boundary-ok "record:" "record:")"#,
                &serde_json::json!({})
            )
            .unwrap(),
            serde_json::json!(false)
        );
    }

    #[test]
    fn boundary_ok_prefix_mismatch_is_false() {
        assert_eq!(
            eval_sandboxed(
                r#"(boundary-ok "record:" "artifact:x")"#,
                &serde_json::json!({})
            )
            .unwrap(),
            serde_json::json!(false)
        );
    }

    #[test]
    fn boundary_ok_non_string_and_arity_error() {
        assert!(matches!(
            eval_sandboxed(r#"(boundary-ok "record:" 5)"#, &serde_json::json!({})).unwrap_err(),
            LispError::TypeError { .. }
        ));
        assert!(matches!(
            eval_sandboxed(r#"(boundary-ok "record:" nil)"#, &serde_json::json!({})).unwrap_err(),
            LispError::TypeError { .. }
        ));
        assert!(matches!(
            eval_sandboxed(r#"(boundary-ok "record:")"#, &serde_json::json!({})).unwrap_err(),
            LispError::Arity(_)
        ));
    }

    #[test]
    fn boundary_ok_reads_the_character_after_the_prefix_not_the_value_start() {
        // The reason the primitive takes the prefix: a form must ask about
        // the character FOLLOWING the label. A leading space on the whole
        // value is the caller's own concatenation and must not decide the
        // boundary verdict.
        assert_eq!(
            eval_sandboxed(
                r#"(boundary-ok "record:" "  record:x")"#,
                &serde_json::json!({})
            )
            .unwrap(),
            serde_json::json!(false),
            "the prefix is not at position 0, so there is no post-prefix char"
        );
    }

    #[test]
    fn starts_with_and_ends_with_match_directionally() {
        // The adhd-mode lexical-gate shapes: prefix openers, suffix closers.
        let env = &serde_json::json!({});
        assert_eq!(
            eval_sandboxed(r#"(starts-with "Let me" "Let me think")"#, env).unwrap(),
            serde_json::json!(true),
            "a prefix-shaped opener extract matches its forbidden prefix"
        );
        assert_eq!(
            eval_sandboxed(r#"(starts-with "Run" "Run the tests")"#, env).unwrap(),
            serde_json::json!(true)
        );
        assert_eq!(
            eval_sandboxed(r#"(ends-with "that helps" "Hope that helps")"#, env).unwrap(),
            serde_json::json!(true),
            "a suffix-shaped closer extract matches its forbidden suffix"
        );
        assert_eq!(
            eval_sandboxed(r#"(ends-with "tests" "Run the tests")"#, env).unwrap(),
            serde_json::json!(true)
        );
    }

    #[test]
    fn starts_with_ends_with_longer_needle_is_legitimate_false() {
        // The contrast with string-contains's reversal guard: these checks are
        // directional, so a prefix/suffix longer than the subject is a normal
        // false, never an error. This is exactly the case where a
        // string-contains rewrite of the adhd-mode lexical gate broke (the
        // guard errored the whole gate on short extracts — verified live
        // 2026-09-30, which motivated these builtins).
        let env = &serde_json::json!({});
        assert_eq!(
            eval_sandboxed(r#"(starts-with "Great question" "Run the tests")"#, env).unwrap(),
            serde_json::json!(false),
            "a longer prefix is a legitimate false, not a reversal error"
        );
        assert_eq!(
            eval_sandboxed(r#"(ends-with "anything else" "end of draft")"#, env).unwrap(),
            serde_json::json!(false)
        );
        assert_eq!(
            eval_sandboxed(r#"(starts-with "Let me" "Sure!")"#, env).unwrap(),
            serde_json::json!(false)
        );
    }

    #[test]
    fn starts_with_ends_with_empty_needle_errors() {
        // The string-contains precedent: every string starts/ends with the
        // empty needle, and a check that fires on correct output is worse
        // than no check.
        let err = eval_sandboxed(
            r#"(starts-with "" "Run the tests")"#,
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert!(matches!(err, LispError::Runtime(_)), "got: {err}");
        let err = eval_sandboxed(r#"(ends-with "" "Run the tests")"#, &serde_json::json!({}))
            .unwrap_err();
        assert!(matches!(err, LispError::Runtime(_)), "got: {err}");
    }

    #[test]
    fn starts_with_ends_with_non_string_args_error() {
        let err = eval_sandboxed(
            r#"(starts-with 5 "revenue growth of 12%")"#,
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert!(matches!(err, LispError::TypeError { .. }), "got: {err}");
        let err = eval_sandboxed(r#"(ends-with "growth" 42)"#, &serde_json::json!({})).unwrap_err();
        assert!(matches!(err, LispError::TypeError { .. }), "got: {err}");
    }

    /// expect: "Skill forms clamp and take extrema with `max`/`min` instead of
    /// hand-written `(if (< a b) a b)` lambdas; an empty call is an arity
    /// error, never a silent nil" [F9]
    #[test]
    fn max_and_min_take_numeric_extrema() {
        let env = serde_json::json!({"score": -0.25});
        assert_eq!(
            eval_sandboxed("(max 1 2.5 -3)", &env).unwrap(),
            serde_json::json!(2.5)
        );
        assert_eq!(
            eval_sandboxed("(min 1 2.5 -3)", &env).unwrap(),
            serde_json::json!(-3)
        );
        assert_eq!(
            eval_sandboxed("(max 0 score)", &env).unwrap(),
            serde_json::json!(0)
        );
        assert!(matches!(
            eval_sandboxed("(max)", &env).unwrap_err(),
            LispError::Arity(_)
        ));
        assert!(matches!(
            eval_sandboxed("(min 1 \"a\")", &env).unwrap_err(),
            LispError::TypeError { .. }
        ));
    }
}
