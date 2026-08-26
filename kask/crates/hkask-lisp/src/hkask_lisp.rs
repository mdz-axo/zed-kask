#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Sandboxed Lisp interpreter for deterministic manifest compute steps.
//!
//! Design goals (following the `rust_lisp` reference by brundonsmith):
//! - Small footprint, no runtime dependencies beyond serde_json
//! - No I/O, no filesystem, no network, no environment variable access
//! - Bounded recursion depth (64) and bounded evaluation steps (100000)
//! - JSON-native: input env is `serde_json::Value`, output is `serde_json::Value`
//! - JSON objects become association lists — the classic Lisp data structure
//!
//! The interpreter supports a minimal but practical Lisp subset:
//!   Special forms: quote, if, let, lambda, define, begin, and, or, not, cond
//!   Built-in functions: car, cdr, cons, list, length, nth, reverse,
//!     +, -, *, /, =, !=, <, <=, >, >=, is_null, numberp, assoc, append,
//!     string=, concat, abs, sqrt, eq, member
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
    #[error("evaluation exceeded max_steps ({0}) — possible infinite loop")]
    StepLimitExceeded(u64),
    #[error("evaluation exceeded max_depth ({0}) — possible infinite recursion")]
    DepthLimitExceeded(u64),
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

pub type NativeFn = fn(&Rc<RefCell<Env>>, &[LispValue]) -> Result<LispValue, LispError>;

impl PartialEq for LispValue {
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
    let tokens = tokenize(source);
    let tokens = expand_infix(&tokens);
    let mut forms = Vec::new();
    let mut rest: &[String] = &tokens;
    while !rest.is_empty() {
        let (form, next) = parse_form(rest)?;
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

fn parse_form(tokens: &[String]) -> Result<(LispValue, &[String]), LispError> {
    if tokens.is_empty() {
        return Err(LispError::Parse("unexpected end of input".into()));
    }
    let tok = &tokens[0];
    let rest = &tokens[1..];

    if tok == "(" {
        let mut items = Vec::new();
        let mut remaining = rest;
        loop {
            if remaining.is_empty() {
                return Err(LispError::Parse("unbalanced parenthesis".into()));
            }
            if remaining[0] == ")" {
                return Ok((LispValue::List(List::from_vec(items)), &remaining[1..]));
            }
            let (form, next) = parse_form(remaining)?;
            items.push(form);
            remaining = next;
        }
    }
    if tok == ")" {
        return Err(LispError::Parse("unexpected ')'".into()));
    }
    if tok == "'" {
        let (form, next) = parse_form(rest)?;
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
        self.steps_used += 1;
        if self.steps_used > self.max_steps {
            return Err(LispError::StepLimitExceeded(self.max_steps));
        }
        Ok(())
    }

    fn enter(&mut self) -> Result<(), LispError> {
        self.depth_current += 1;
        if self.depth_current > self.max_depth {
            return Err(LispError::DepthLimitExceeded(self.max_depth));
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.depth_current = self.depth_current.saturating_sub(1);
    }
}

pub fn eval(env: Rc<RefCell<Env>>, form: &LispValue) -> Result<LispValue, LispError> {
    let mut budget = EvalBudget::new(100000, 64);
    eval_with_budget(env, form, &mut budget)
}

/// Depth is checked only for compound forms (lists) — atoms don't recurse
/// and don't consume stack frames. This prevents the depth budget from being
/// exhausted by argument evaluation while still bounding actual recursion.
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
        LispValue::NativeFunc(f) => f(&env, args),
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

fn add(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

fn sub(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

fn mul(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

fn div(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

fn num_eq(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("= expects at least 2 args".into()));
    }
    let first = as_f64(&args[0])?;
    Ok(LispValue::Bool(
        args[1..].iter().all(|a| as_f64(a).ok() == Some(first)),
    ))
}

fn num_ne(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("!= expects at least 2 args".into()));
    }
    let first = as_f64(&args[0])?;
    Ok(LispValue::Bool(
        args[1..].iter().all(|a| as_f64(a).ok() != Some(first)),
    ))
}

fn lt(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("< expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] < w[1])))
}

fn le(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("<= expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] <= w[1])))
}

fn gt(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity("> expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] > w[1])))
}

fn ge(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() < 2 {
        return Err(LispError::Arity(">= expects at least 2 args".into()));
    }
    let nums: Vec<f64> = args.iter().map(as_f64).collect::<Result<_, _>>()?;
    Ok(LispValue::Bool(nums.windows(2).all(|w| w[0] >= w[1])))
}

fn car(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("car expects 1 arg".into()));
    }
    let list = as_list(&args[0])?;
    if list.is_nil() {
        return Ok(LispValue::Nil);
    }
    Ok(list.head.clone())
}

fn cdr(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("cdr expects 1 arg".into()));
    }
    let list = as_list(&args[0])?;
    Ok(match &list.tail {
        Some(tail) => LispValue::List(tail.clone()),
        None => LispValue::List(List::nil()),
    })
}

fn cons(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("cons expects 2 args".into()));
    }
    let tail = as_list(&args[1])?;
    Ok(LispValue::List(List::cons(args[0].clone(), tail)))
}

fn list_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    Ok(LispValue::List(List::from_vec(args.to_vec())))
}

fn length(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

fn nth(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

fn reverse(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 1 {
        return Err(LispError::Arity("reverse expects 1 arg".into()));
    }
    let mut items = as_list(&args[0])?.to_vec();
    items.reverse();
    Ok(LispValue::List(List::from_vec(items)))
}

fn is_null(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn numberp(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn listp(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn assoc_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("assoc expects 2 args".into()));
    }
    let key = &args[0];
    let alist = match as_list(&args[1]) {
        Ok(alist) => alist,
        Err(_) => return Ok(LispValue::Nil),
    };
    for pair in alist.to_vec() {
        if let LispValue::List(pair_list) = &pair {
            let pair_items = pair_list.to_vec();
            if pair_items.len() == 2 && pair_items[0] == *key {
                return Ok(pair_items[1].clone());
            }
        }
    }
    Ok(LispValue::Nil)
}

/// List concatenation: `(append l1 l2 ...)` joins multiple lists into one.
/// Nil arguments are treated as empty lists. Non-list, non-nil args error.
/// Returns nil if all args are nil/empty.
fn append_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn string_eq_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn concat_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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

/// Absolute value: `(abs x)` returns the magnitude of a numeric arg.
/// Preserves Int vs Float: `(abs -3)` → `3` (Int), `(abs -3.5)` → `3.5` (Float).
fn abs_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn sqrt_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
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
fn eq_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("eq expects 2 args".into()));
    }
    Ok(LispValue::Bool(args[0] == args[1]))
}

/// List membership: `(member x list)` returns true iff `x` is structurally
/// equal to an element of `list`. Nil list returns false. Non-list second
/// arg errors. Used by the GORILLA maturity-blocks check.
fn member_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("member expects 2 args".into()));
    }
    let list = as_list(&args[1])?;
    if list.is_nil() {
        return Ok(LispValue::Bool(false));
    }
    for item in list.to_vec() {
        if item == args[0] {
            return Ok(LispValue::Bool(true));
        }
    }
    Ok(LispValue::Bool(false))
}

// ── JSON interop ────────────────────────────────────────────────────────────

/// Convert a `serde_json::Value` into a `LispValue`.
/// JSON objects become association lists: `{"a": 1, "b": 2}` → `(("a" . 1) ("b" . 2))`
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
    eval_sandboxed_with_budget(form, env_json, 100000, 64)
}

pub fn eval_sandboxed_with_budget(
    form: &str,
    env_json: &Value,
    max_steps: u64,
    max_depth: u64,
) -> Result<Value, LispError> {
    let parsed = parse(form)?;
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
    let mut budget = EvalBudget::new(max_steps, max_depth);
    for form in &parsed {
        result = eval_with_budget(env.clone(), form, &mut budget)?;
    }
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
        let err = eval_sandboxed_with_budget(program, &serde_json::json!({}), 1000, 64)
            .unwrap_err();
        assert!(
            matches!(
                err,
                LispError::StepLimitExceeded(_) | LispError::DepthLimitExceeded(_)
            ),
            "unbounded recursion must hit a budget, got: {err}"
        );
    }

    #[test]
    fn deep_recursion_hits_depth_budget_before_stack_overflow() {
        // Depth budget far below the real stack limit: the interpreter must
        // reject, not segfault.
        let program = "(define f (lambda (n) (+ 1 (f (+ n 1))))) (f 0)";
        let err = eval_sandboxed_with_budget(program, &serde_json::json!({}), 1_000_000, 32)
            .unwrap_err();
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
}
