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
//!     +, -, *, /, =, !=, <, <=, >, >=, is_null, numberp, listp, assoc,
//!     append, string=, string-contains, concat, abs, sqrt, eq, member
//!
//! `assoc` is defensive: a non-list alist argument returns nil instead of
//! erroring (see `assoc_fn` — LLM step outputs reach forms as JSON strings,
//! and ~50 registry call sites rely on graceful degradation).
//!
//! # Infix Operator Notation
//!
//! Binary operators can be written infix: `a + b` is equivalent to `(+ a b)`.
//! Chained same-operator expressions are supported: `a + b + c` → `(+ a b c)`.
//! Operator mixing requires explicit parentheses: `(* (+ 1 2) 3)` — `1 + 2 * 3`
//! is NOT supported (no operator precedence). This makes simple scoring and
//! threshold expressions more readable in YAML manifests without adding a
//! parser dependency or sacrificing the sandboxed security model.
//!
//! # Security
//!
//! No `eval` builtin (Lisp code cannot evaluate arbitrary strings). No
//! `load` or `require`. The environment is immutable from Lisp's perspective
//! (define mutates a local scope discarded after evaluation). Bounded by
//! `max_steps` and `max_depth`. Safe for infrastructure manifests provided
//! the caller respects the `category: skill` gate.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;
use thiserror::Error;

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

    pub fn to_vec(&self) -> Vec<LispValue> {
        let mut out = Vec::new();
        let mut cursor: Option<&List> = Some(self);
        while let Some(node) = cursor {
            if node.is_nil() {
                break;
            }
            out.push(node.head.clone());
            cursor = node.tail.as_deref();
        }
        out
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
    budget.charge(source.len())?;
    let tokens = tokenize(source);
    let tokens = expand_infix(&tokens);
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
fn expand_infix(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
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
                operands.push(tokens[j + 1].clone());
                j += 2;
            }
            // Emit: ( op operand1 operand2 ... )
            out.push("(".to_string());
            out.push(op);
            for operand in &operands {
                out.push(operand.clone());
            }
            out.push(")".to_string());
            i = j;
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

/// Check if a token is a valid context for infix transformation — it's an
/// atom (symbol, number, string, true, false, nil) but not a paren or quote.
fn is_infix_context(tok: &str) -> bool {
    tok != "(" && tok != ")" && tok != "'"
}

fn tokenize(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == ';' {
            for c in chars.by_ref() {
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
            while let Some(&c) = chars.peek() {
                chars.next();
                s.push(c);
                if c == '\\' {
                    if let Some(&next) = chars.peek() {
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
        while let Some(&c) = chars.peek() {
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
    tokens
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
                return Ok((LispValue::List(List::from_vec(items)), &remaining[1..]));
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
        let quoted = LispValue::List(List::from_vec(vec![
            LispValue::Symbol("quote".into()),
            form,
        ]));
        return Ok((quoted, next));
    }
    Ok((parse_atom(tok), rest))
}

fn parse_atom(tok: &str) -> LispValue {
    if tok.starts_with('"') && tok.ends_with('"') && tok.len() >= 2 {
        return LispValue::String(unescape_string(&tok[1..tok.len() - 1]));
    }
    if let Ok(i) = tok.parse::<i64>() {
        return LispValue::Int(i);
    }
    if let Ok(f) = tok.parse::<f64>() {
        return LispValue::Float(f);
    }
    match tok {
        "true" => LispValue::Bool(true),
        "false" => LispValue::Bool(false),
        "nil" => LispValue::Nil,
        _ => LispValue::Symbol(tok.to_string()),
    }
}

fn unescape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
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
    out
}

// ── Evaluator ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EvalBudget {
    pub max_steps: u64,
    pub max_depth: u64,
    steps_used: u64,
    depth_current: u64,
}

impl EvalBudget {
    pub fn new(max_steps: u64, max_depth: u64) -> Self {
        EvalBudget {
            max_steps,
            max_depth,
            steps_used: 0,
            depth_current: 0,
        }
    }

    fn tick(&mut self) -> Result<(), LispError> {
        self.charge(1)
    }

    fn charge(&mut self, work: usize) -> Result<(), LispError> {
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
    result
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

        LispValue::Symbol(s) => env
            .borrow()
            .lookup(s)
            .ok_or_else(|| LispError::UnboundSymbol(s.clone())),

        LispValue::List(list) => {
            let items = list.to_vec();
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
                LispValue::List(b) => b.to_vec(),
                _ => return Err(LispError::Runtime("let bindings must be a list".into())),
            };
            let child_env = Rc::new(RefCell::new(Env::child(env)));
            for binding in bindings {
                let pair = match &binding {
                    LispValue::List(p) => p.to_vec(),
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
                LispValue::List(p) => p
                    .to_vec()
                    .iter()
                    .map(|v| match v {
                        LispValue::Symbol(s) => Ok(s.clone()),
                        _ => Err(LispError::Runtime("lambda param must be a symbol".into())),
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
        // far more readable for multi-branch verdict dispatch (e.g.
        // company-research convergence checks).
        "cond" => {
            for clause in args {
                let clause_items = match clause {
                    LispValue::List(cl) => cl.to_vec(),
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
            let func = env
                .borrow()
                .lookup(name)
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
            f(&env, args, budget)
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
            let call_env = Rc::new(RefCell::new(Env::child(closure_env)));
            for (param, arg) in params.iter().zip(args.iter()) {
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
        ("assoc", assoc_fn),
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
        // Absolute value. (abs x) returns the magnitude of a numeric arg.
        // Used by convergence-gap forms that need a symmetric delta.
        ("abs", abs_fn),
        // Square root. (sqrt x) returns the principal root of a numeric arg.
        // Used by marker-space hypotenuse computations (eqm-improvement step 7).
        // Returns a Float.
        ("sqrt", sqrt_fn),
        // Generic equality. (eq a b) returns true iff a and b are structurally
        // equal (delegates to LispValue::PartialEq). This is the value-equality
        // counterpart to `=` (numeric) and `string=` (string-only). Distinct
        // from `=` because `=` errors on non-numbers; `eq` accepts any value
        // type and returns false for mismatched types. Used by `cond` clauses
        // comparing string verdicts.
        ("eq", eq_fn),
        // List membership. (member x list) returns true iff x is structurally
        // equal to an element of `list`. Nil list returns false. Used by the
        // GORILLA maturity-blocks check (company-research-deep step 10).
        ("member", member_fn),
    ]
}

fn as_f64(v: &LispValue) -> Result<f64, LispError> {
    match v {
        LispValue::Int(i) => Ok(*i as f64),
        LispValue::Float(f) => Ok(*f),
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

fn add(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut acc_int: Option<i64> = Some(0);
    let mut acc_float: f64 = 0.0;
    for a in args {
        match a {
            LispValue::Int(i) => {
                acc_int = acc_int.map(|v| v.wrapping_add(*i));
                acc_float += *i as f64;
            }
            LispValue::Float(f) => {
                acc_int = None;
                acc_float += *f;
            }
            _ => {
                return Err(LispError::TypeError {
                    expected: "number".into(),
                    actual: type_of(a),
                });
            }
        }
    }
    Ok(acc_int
        .map(LispValue::Int)
        .unwrap_or(LispValue::Float(acc_float)))
}

fn sub(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.is_empty() {
        return Err(LispError::Arity("- expects at least 1 arg".into()));
    }
    if args.len() == 1 {
        let f = as_f64(&args[0])?;
        return Ok(if matches!(&args[0], LispValue::Int(_)) {
            LispValue::Int(-f as i64)
        } else {
            LispValue::Float(-f)
        });
    }
    let mut acc_int: Option<i64> = match &args[0] {
        LispValue::Int(i) => Some(*i),
        _ => None,
    };
    let mut acc_float = as_f64(&args[0])?;
    for a in &args[1..] {
        let f = as_f64(a)?;
        acc_float -= f;
        if let (Some(i), LispValue::Int(ai)) = (acc_int, a) {
            acc_int = Some(i.wrapping_sub(*ai));
        } else {
            acc_int = None;
        }
    }
    Ok(acc_int
        .map(LispValue::Int)
        .unwrap_or(LispValue::Float(acc_float)))
}

fn mul(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut acc_int: Option<i64> = Some(1);
    let mut acc_float: f64 = 1.0;
    for a in args {
        match a {
            LispValue::Int(i) => {
                acc_int = acc_int.map(|v| v.wrapping_mul(*i));
                acc_float *= *i as f64;
            }
            LispValue::Float(f) => {
                acc_int = None;
                acc_float *= *f;
            }
            _ => {
                return Err(LispError::TypeError {
                    expected: "number".into(),
                    actual: type_of(a),
                });
            }
        }
    }
    Ok(acc_int
        .map(LispValue::Int)
        .unwrap_or(LispValue::Float(acc_float)))
}

fn div(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.is_empty() {
        return Err(LispError::Arity("/ expects at least 1 arg".into()));
    }
    let mut acc = as_f64(&args[0])?;
    for a in &args[1..] {
        let f = as_f64(a)?;
        if f == 0.0 {
            return Err(LispError::Runtime("division by zero".into()));
        }
        acc /= f;
    }
    Ok(LispValue::Float(acc))
}

fn num_eq(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("= expects at least 2 args".into()));
    }
    let first = as_f64(&args[0])?;
    Ok(LispValue::Bool(
        args[1..].iter().all(|a| as_f64(a).ok() == Some(first)),
    ))
}

fn num_ne(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("!= expects at least 2 args".into()));
    }
    let first = as_f64(&args[0])?;
    Ok(LispValue::Bool(
        args[1..].iter().all(|a| as_f64(a).ok() != Some(first)),
    ))
}

fn lt(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("< expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] < w[1])))
}

fn le(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("<= expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] <= w[1])))
}

fn gt(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("> expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] > w[1])))
}

fn ge(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity(">= expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] >= w[1])))
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
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    Ok(LispValue::List(List::from_vec(args.to_vec())))
}

fn length(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("length expects 1 arg".into()));
    }
    match &args[0] {
        LispValue::List(l) => Ok(LispValue::Int(l.len() as i64)),
        LispValue::Nil => Ok(LispValue::Int(0)),
        LispValue::String(s) => Ok(LispValue::Int(s.chars().count() as i64)),
        _ => Err(LispError::TypeError {
            expected: "list/string".into(),
            actual: type_of(&args[0]),
        }),
    }
}

fn nth(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("nth expects 2 args".into()));
    }
    let idx = match &args[0] {
        LispValue::Int(i) => *i as usize,
        _ => {
            return Err(LispError::TypeError {
                expected: "int".into(),
                actual: type_of(&args[0]),
            });
        }
    };
    let items = as_list(&args[1])?.to_vec();
    Ok(if idx >= items.len() {
        LispValue::Nil
    } else {
        items[idx].clone()
    })
}

fn reverse(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("reverse expects 1 arg".into()));
    }
    let mut items = as_list(&args[0])?.to_vec();
    items.reverse();
    Ok(LispValue::List(List::from_vec(items)))
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
/// has ~50 lisp.eval call sites that pass `step_N_result` (an LLM step's
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
    let alist = match as_list(&args[1]) {
        Ok(alist) => alist,
        Err(_) => return Ok(LispValue::Nil),
    };
    let mut cursor = Some(alist.as_ref());
    while let Some(node) = cursor {
        if node.is_nil() {
            break;
        }
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
            if pair.head == *key {
                if let LispValue::String(text) | LispValue::Symbol(text) = &value.head {
                    budget.charge(text.len())?;
                }
                return Ok(value.head.clone());
            }
        }
    }
    Ok(LispValue::Nil)
}

/// List concatenation: `(append l1 l2 ...)` joins multiple lists into one.
/// Nil arguments are treated as empty lists. Non-list, non-nil args error.
/// Returns nil if all args are nil/empty.
fn append_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut combined: Vec<LispValue> = Vec::new();
    for arg in args {
        match arg {
            LispValue::List(l) => combined.extend(l.to_vec()),
            LispValue::Nil => {}
            _ => {
                return Err(LispError::TypeError {
                    expected: "list".into(),
                    actual: type_of(arg),
                });
            }
        }
    }
    Ok(LispValue::List(List::from_vec(combined)))
}

/// String equality: `(string= a b)` returns true iff both args are strings
/// with equal content. Distinct from `=` which is numeric-only.
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
        _ => Ok(LispValue::Bool(false)),
    }
}

/// String concatenation: `(concat s1 s2 ...)` joins multiple strings.
/// Non-string args error. Returns empty string if no args.
fn concat_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    let mut combined = String::new();
    for arg in args {
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

/// Substring containment: `(string-contains needle haystack)` returns true
/// iff `needle` is a non-empty substring of `haystack`. Arg order follows
/// `assoc`/`member` (searched-for first, searched-in second). An empty
/// needle errors rather than returning true — in citation verification an
/// empty needle would verify anything, and a check that fires on correct
/// output is worse than no check.
fn string_contains_fn(
    _env: &Rc<RefCell<Env>>,
    args: &[LispValue],
    _budget: &mut EvalBudget,
) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("string-contains expects 2 args".into()));
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
    let haystack = match &args[1] {
        LispValue::String(s) => s,
        other => {
            return Err(LispError::TypeError {
                expected: "string".into(),
                actual: type_of(other),
            });
        }
    };
    if needle.is_empty() {
        return Err(LispError::Runtime(
            "string-contains: needle must be a non-empty string".into(),
        ));
    }
    Ok(LispValue::Bool(haystack.contains(needle.as_str())))
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
        LispValue::Int(i) => Ok(LispValue::Int(i.wrapping_abs())),
        LispValue::Float(f) => Ok(LispValue::Float(f.abs())),
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
    let f = as_f64(&args[0])?;
    if f < 0.0 {
        return Err(LispError::Runtime(format!("sqrt of negative number {f}")));
    }
    Ok(LispValue::Float(f.sqrt()))
}

/// Generic equality: `(eq a b)` returns true iff a and b are structurally
/// equal (delegates to `LispValue::PartialEq`). Accepts any value type and
/// returns false for mismatched types — distinct from `=` (numeric, errors on
/// non-numbers) and `string=` (string-only). Used by `cond` clauses comparing
/// string verdicts where the author wants a single equality operator.
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
    Ok(LispValue::Bool(args[0] == args[1]))
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
    for item in list.to_vec() {
        charge_value(&item, budget, 0)?;
        charge_value(&args[0], budget, 0)?;
        if item == args[0] {
            return Ok(LispValue::Bool(true));
        }
    }
    Ok(LispValue::Bool(false))
}

// Charge expansion, not just shared Rc nodes: a small DAG can serialize to an
// exponentially large JSON tree. Validation happens before recursive copying.
#[stacksafe::stacksafe]
fn charge_value(value: &LispValue, budget: &mut EvalBudget, depth: u64) -> Result<(), LispError> {
    if depth > budget.max_depth {
        return Err(LispError::DepthLimitExceeded(budget.max_depth));
    }
    budget.tick()?;
    match value {
        LispValue::String(text) | LispValue::Symbol(text) => budget.charge(text.len())?,
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
    budget.tick()?;
    match value {
        Value::String(text) => budget.charge(text.len())?,
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

fn charge_native_arguments(args: &[LispValue], budget: &mut EvalBudget) -> Result<(), LispError> {
    for value in args {
        budget.tick()?;
        match value {
            LispValue::String(text) | LispValue::Symbol(text) => budget.charge(text.len())?,
            LispValue::List(list) => {
                let mut cursor = Some(list.as_ref());
                while let Some(node) = cursor {
                    if node.is_nil() {
                        break;
                    }
                    budget.tick()?;
                    // Nested Rc values clone cheaply; strings and symbols own bytes.
                    if let LispValue::String(text) | LispValue::Symbol(text) = &node.head {
                        budget.charge(text.len())?;
                    }
                    cursor = node.tail.as_deref();
                }
            }
            _ => {}
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

// ── Public API ──────────────────────────────────────────────────────────────

/// Evaluate a Lisp form against a JSON environment, returning a JSON result.
///
/// This is the entry point called by `dispatch_compute` for
/// `compute_ref: "lisp.eval"`. The `form` is the Lisp source; `env_json`
/// is a JSON object whose keys become top-level bindings (values converted
/// to Lisp values via `from_json` — objects become association lists).
pub fn eval_sandboxed(form: &str, env_json: &Value) -> Result<Value, LispError> {
    eval_sandboxed_with_budget(form, env_json, 100000, 1024)
}

pub fn eval_sandboxed_with_budget(
    form: &str,
    env_json: &Value,
    max_steps: u64,
    max_depth: u64,
) -> Result<Value, LispError> {
    let mut budget = EvalBudget::new(max_steps, max_depth);
    let parsed = parse_with_budget(form, &mut budget)?;
    charge_json(env_json, &mut budget, 0)?;
    if parsed.is_empty() {
        return Ok(Value::Null);
    }
    let env = Rc::new(RefCell::new(Env::new_root()));
    if let Value::Object(obj) = env_json {
        for (k, v) in obj {
            env.borrow_mut().define(k.clone(), from_json(v));
        }
    }
    let mut result = LispValue::Nil;
    for form in &parsed {
        result = eval_with_budget(env.clone(), form, &mut budget)?;
    }
    charge_value(&result, &mut budget, 0)?;
    charge_output_layout(&result, &mut budget, 0)?;
    Ok(to_json(&result))
}

// ── Tests ───────────────────────────────────────────────────────────────────
// The tests module header above was a placeholder — the sandbox-budget
// contract is pinned below. These tests guard the `lisp_eval` tool's safety
// envelope: an LLM-supplied form cannot loop forever (step budget), blow the
// stack (depth budget), or escape the environment (no ambient access).

#[cfg(test)]
mod tests {
    use super::*;

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
}
