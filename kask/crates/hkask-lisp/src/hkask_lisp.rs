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
fn assoc_fn(_env: &Rc<RefCell<Env>>, args: &[LispValue]) -> Result<LispValue, LispError> {
    if args.len() != 2 {
        return Err(LispError::Arity("assoc expects 2 args".into()));
    }
    let key = &args[0];
    let alist = as_list(&args[1])?;
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
        LispValue::List(list) => Value::Array(list.to_vec().iter().map(to_json).collect()),
        LispValue::Lambda { .. } => Value::String("<lambda>".into()),
        LispValue::NativeFunc(_) => Value::String("<native-function>".into()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_basic_arithmetic() {
        assert_eq!(eval_sandboxed("(+ 1 2 3)", &json!({})).unwrap(), json!(6));
    }

    #[test]
    fn test_nested_arithmetic() {
        assert_eq!(
            eval_sandboxed("(+ (* 2 3) (- 10 4))", &json!({})).unwrap(),
            json!(12)
        );
    }

    #[test]
    fn test_if_and_let() {
        assert_eq!(
            eval_sandboxed("(if (> 5 3) \"yes\" \"no\")", &json!({})).unwrap(),
            json!("yes")
        );
        assert_eq!(
            eval_sandboxed("(let ((x 5) (y 3)) (+ x y))", &json!({})).unwrap(),
            json!(8)
        );
    }

    #[test]
    fn test_lambda_and_define() {
        assert_eq!(
            eval_sandboxed("((lambda (x y) (* x y)) 4 5)", &json!({})).unwrap(),
            json!(20)
        );
        assert_eq!(
            eval_sandboxed(
                "(begin (define square (lambda (x) (* x x))) (square 7))",
                &json!({})
            )
            .unwrap(),
            json!(49)
        );
    }

    #[test]
    fn test_list_operations() {
        assert_eq!(
            eval_sandboxed("(length (list 1 2 3 4 5))", &json!({})).unwrap(),
            json!(5)
        );
        assert_eq!(
            eval_sandboxed("(car (list 1 2 3))", &json!({})).unwrap(),
            json!(1)
        );
        assert_eq!(
            eval_sandboxed("(nth 1 (list \"a\" \"b\" \"c\"))", &json!({})).unwrap(),
            json!("b")
        );
    }

    #[test]
    fn test_json_env_with_assoc() {
        // JSON objects become association lists — use `assoc` to access fields.
        let env = json!({"step_1_result": {"score": 0.85, "findings": ["a", "b", "c"]}});
        assert_eq!(
            eval_sandboxed("(assoc \"score\" step_1_result)", &env).unwrap(),
            json!(0.85)
        );
    }

    #[test]
    fn test_capability_predicate_with_recursion() {
        // Realistic use case: check a capability registry using recursion
        // (map/filter were removed — users implement them in Lisp).
        let env = json!({
            "capabilities": [
                {"name": "tool-use", "floor": 0.5, "measured": 0.7, "ceiling": 0.9},
                {"name": "reasoning", "floor": 0.6, "measured": 0.4, "ceiling": 0.95}
            ]
        });
        let form = r#"
          (begin
            (define check-cap
              (lambda (cap)
                (and (>= (assoc "measured" cap) (assoc "floor" cap))
                     (<= (assoc "measured" cap) (assoc "ceiling" cap)))))
            (define check-all
              (lambda (caps)
                (if (is_null caps)
                    (list)
                    (cons (check-cap (car caps)) (check-all (cdr caps))))))
            (check-all capabilities))
        "#;
        let result = eval_sandboxed(form, &env).unwrap();
        assert_eq!(result, json!([true, false]));
    }

    #[test]
    fn test_step_limit_exceeded() {
        let form = "(begin (define loop (lambda () (loop))) (loop))";
        let result = eval_sandboxed_with_budget(form, &json!({}), 100, 1000);
        assert!(matches!(result, Err(LispError::StepLimitExceeded(_))));
    }

    #[test]
    fn test_depth_limit_exceeded() {
        let form = r#"
          (begin
            (define deep (lambda (n) (if (= n 0) 0 (deep (- n 1)))))
            (deep 1000))
        "#;
        let result = eval_sandboxed_with_budget(form, &json!({}), 100000, 50);
        assert!(matches!(result, Err(LispError::DepthLimitExceeded(_))));
    }

    #[test]
    fn test_no_eval_builtin() {
        let result = eval_sandboxed("(eval \"(+ 1 2)\")", &json!({}));
        assert!(matches!(result, Err(LispError::UnboundSymbol(_))));
    }

    #[test]
    fn test_append_builtin() {
        // Two lists
        assert_eq!(
            eval_sandboxed("(append (list 1 2) (list 3 4))", &json!({})).unwrap(),
            json!([1, 2, 3, 4])
        );
        // Multiple lists
        assert_eq!(
            eval_sandboxed("(append (list 1) (list 2) (list 3))", &json!({})).unwrap(),
            json!([1, 2, 3])
        );
        // Nil args treated as empty
        assert_eq!(
            eval_sandboxed("(append nil (list 1 2) nil)", &json!({})).unwrap(),
            json!([1, 2])
        );
        // All nil → empty list
        assert_eq!(
            eval_sandboxed("(append nil nil)", &json!({})).unwrap(),
            json!([])
        );
        // No args → empty list
        assert_eq!(eval_sandboxed("(append)", &json!({})).unwrap(), json!([]));
        // Non-list arg errors
        assert!(eval_sandboxed("(append (list 1) 2)", &json!({})).is_err());
    }

    #[test]
    fn test_string_eq_builtin() {
        // Equal strings
        assert_eq!(
            eval_sandboxed("(string= \"high\" \"high\")", &json!({})).unwrap(),
            json!(true)
        );
        // Unequal strings
        assert_eq!(
            eval_sandboxed("(string= \"high\" \"low\")", &json!({})).unwrap(),
            json!(false)
        );
        // Non-string args return false (not error)
        assert_eq!(
            eval_sandboxed("(string= 1 1)", &json!({})).unwrap(),
            json!(false)
        );
        // String vs non-string
        assert_eq!(
            eval_sandboxed("(string= \"high\" 1)", &json!({})).unwrap(),
            json!(false)
        );
        // Wrong arity
        assert!(eval_sandboxed("(string= \"a\")", &json!({})).is_err());
    }

    #[test]
    fn test_concat_builtin() {
        // Two strings
        assert_eq!(
            eval_sandboxed("(concat \"missing_\" \"prediction\")", &json!({})).unwrap(),
            json!("missing_prediction")
        );
        // Multiple strings
        assert_eq!(
            eval_sandboxed("(concat \"a\" \"b\" \"c\")", &json!({})).unwrap(),
            json!("abc")
        );
        // No args → empty string
        assert_eq!(eval_sandboxed("(concat)", &json!({})).unwrap(), json!(""));
        // Non-string arg errors
        assert!(eval_sandboxed("(concat \"a\" 1)", &json!({})).is_err());
    }

    #[test]
    fn test_string_eq_with_json_env() {
        // Realistic: compare a JSON field value against a literal string.
        let env = json!({"step_1_result": {"likelihood": "high"}});
        let form = r#"(string= (assoc "likelihood" step_1_result) "high")"#;
        assert_eq!(eval_sandboxed(form, &env).unwrap(), json!(true));
    }

    #[test]
    fn test_abs_builtin() {
        // Positive int unchanged
        assert_eq!(eval_sandboxed("(abs 5)", &json!({})).unwrap(), json!(5));
        // Negative int → positive
        assert_eq!(eval_sandboxed("(abs -5)", &json!({})).unwrap(), json!(5));
        // Negative float → positive float
        assert_eq!(
            eval_sandboxed("(abs -3.5)", &json!({})).unwrap(),
            json!(3.5)
        );
        // Non-number errors
        assert!(eval_sandboxed("(abs \"x\")", &json!({})).is_err());
        // Arity
        assert!(eval_sandboxed("(abs 1 2)", &json!({})).is_err());
    }

    #[test]
    fn test_sqrt_builtin() {
        assert_eq!(eval_sandboxed("(sqrt 4)", &json!({})).unwrap(), json!(2.0));
        assert_eq!(eval_sandboxed("(sqrt 0)", &json!({})).unwrap(), json!(0.0));
        // Negative input errors
        assert!(eval_sandboxed("(sqrt -1)", &json!({})).is_err());
        // Non-number errors
        assert!(eval_sandboxed("(sqrt \"x\")", &json!({})).is_err());
        // Arity
        assert!(eval_sandboxed("(sqrt 1 2)", &json!({})).is_err());
    }

    #[test]
    fn test_eq_builtin() {
        // Strings
        assert_eq!(
            eval_sandboxed(r#"(eq "done" "done")"#, &json!({})).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_sandboxed(r#"(eq "done" "continue")"#, &json!({})).unwrap(),
            json!(false)
        );
        // Numbers (eq delegates to PartialEq; Int vs Float with same value are NOT equal)
        assert_eq!(eval_sandboxed("(eq 1 1)", &json!({})).unwrap(), json!(true));
        // Mismatched types → false (not an error, unlike `=`)
        assert_eq!(
            eval_sandboxed(r#"(eq 1 "1")"#, &json!({})).unwrap(),
            json!(false)
        );
        // Arity
        assert!(eval_sandboxed("(eq 1)", &json!({})).is_err());
    }

    #[test]
    fn test_member_builtin() {
        let env = json!({"blocks": ["obvious_problem", "choke_point"]});
        // Present
        assert_eq!(
            eval_sandboxed(r#"(member "obvious_problem" blocks)"#, &env).unwrap(),
            json!(true)
        );
        // Absent
        assert_eq!(
            eval_sandboxed(r#"(member "invisible_gorilla" blocks)"#, &env).unwrap(),
            json!(false)
        );
        // Nil list → false
        assert_eq!(
            eval_sandboxed(r#"(member "x" nil)"#, &json!({})).unwrap(),
            json!(false)
        );
        // Non-list second arg errors
        assert!(eval_sandboxed("(member \"x\" 5)", &json!({})).is_err());
        // Arity
        assert!(eval_sandboxed("(member \"x\")", &json!({})).is_err());
    }

    #[test]
    fn test_t_constant() {
        // `t` is bound to Bool(true) in the root env
        assert_eq!(eval_sandboxed("t", &json!({})).unwrap(), json!(true));
        // `t` as a cond test (the canonical use)
        let form = r#"(cond ((eq v "done") 0.0) (t 1.0))"#;
        let env = json!({"v": "other"});
        assert_eq!(eval_sandboxed(form, &env).unwrap(), json!(1.0));
        // `t` can be shadowed by a let binding (correct Lisp behavior)
        assert_eq!(
            eval_sandboxed("(let ((t 42)) t)", &json!({})).unwrap(),
            json!(42)
        );
    }

    #[test]
    fn test_cond_special_form() {
        // First matching clause wins
        let form = r#"(cond ((eq v "done") 0.0) ((eq v "continue") 0.5) (t 1.0))"#;
        let env_done = json!({"v": "done"});
        assert_eq!(eval_sandboxed(form, &env_done).unwrap(), json!(0.0));
        let env_cont = json!({"v": "continue"});
        assert_eq!(eval_sandboxed(form, &env_cont).unwrap(), json!(0.5));
        // Default clause via `t`
        let env_other = json!({"v": "blocked"});
        assert_eq!(eval_sandboxed(form, &env_other).unwrap(), json!(1.0));
        // No matching clause and no `t` → Nil
        let form_no_default = r#"(cond ((eq v "done") 0.0) ((eq v "continue") 0.5))"#;
        assert_eq!(
            eval_sandboxed(form_no_default, &env_other).unwrap(),
            json!(null)
        );
        // `true` literal also works as an always-true test
        let form_true = r#"(cond ((eq v "done") 0.0) (true 1.0))"#;
        assert_eq!(eval_sandboxed(form_true, &env_other).unwrap(), json!(1.0));
        // Empty cond → Nil
        assert_eq!(eval_sandboxed("(cond)", &json!({})).unwrap(), json!(null));
        // Multi-body clause: last body form is the result
        let form_multi = r#"(cond ((eq v "done") 1 2 3) (t 0))"#;
        assert_eq!(eval_sandboxed(form_multi, &env_done).unwrap(), json!(3));
    }

    #[test]
    fn test_division_by_zero() {
        assert!(matches!(
            eval_sandboxed("(/ 10 0)", &json!({})),
            Err(LispError::Runtime(_))
        ));
    }

    #[test]
    fn test_and_or_not() {
        assert_eq!(
            eval_sandboxed("(and (> 5 3) (< 2 10))", &json!({})).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_sandboxed("(or (< 5 3) (> 2 10))", &json!({})).unwrap(),
            json!(false)
        );
        assert_eq!(
            eval_sandboxed("(not (> 5 3))", &json!({})).unwrap(),
            json!(false)
        );
    }

    // ── Infix operator notation tests ──

    #[test]
    fn test_infix_addition() {
        assert_eq!(eval_sandboxed("1 + 2", &json!({})).unwrap(), json!(3));
    }

    #[test]
    fn test_infix_subtraction() {
        assert_eq!(eval_sandboxed("10 - 4", &json!({})).unwrap(), json!(6));
    }

    #[test]
    fn test_infix_multiplication() {
        assert_eq!(eval_sandboxed("3 * 4", &json!({})).unwrap(), json!(12));
    }

    #[test]
    fn test_infix_comparison() {
        assert_eq!(eval_sandboxed("5 > 3", &json!({})).unwrap(), json!(true));
        assert_eq!(eval_sandboxed("3 > 5", &json!({})).unwrap(), json!(false));
        assert_eq!(eval_sandboxed("3 = 3", &json!({})).unwrap(), json!(true));
    }

    #[test]
    fn test_infix_chained_same_operator() {
        // a + b + c → (+ a b c)
        assert_eq!(eval_sandboxed("1 + 2 + 3", &json!({})).unwrap(), json!(6));
        assert_eq!(eval_sandboxed("2 * 3 * 4", &json!({})).unwrap(), json!(24));
    }

    #[test]
    fn test_infix_with_parens_for_precedence() {
        // Mixed operators require prefix for the parenthesized part:
        // (* (+ 1 2) 3) — infix only applies to bare atom triplets.
        assert_eq!(
            eval_sandboxed("(* (+ 1 2) 3)", &json!({})).unwrap(),
            json!(9)
        );
    }

    #[test]
    fn test_infix_with_variables() {
        let env = json!({"a": 10, "b": 3});
        assert_eq!(eval_sandboxed("a + b", &env).unwrap(), json!(13));
        assert_eq!(eval_sandboxed("a * b", &env).unwrap(), json!(30));
    }

    #[test]
    fn test_infix_in_let_binding() {
        // Infix works inside let forms too
        assert_eq!(
            eval_sandboxed("(let ((x 5)) x + 3)", &json!({})).unwrap(),
            json!(8)
        );
    }

    #[test]
    fn test_infix_in_if_condition() {
        assert_eq!(
            eval_sandboxed("(if 5 > 3 \"yes\" \"no\")", &json!({})).unwrap(),
            json!("yes")
        );
    }

    #[test]
    fn test_prefix_still_works() {
        // Ensure existing prefix notation is not broken by the infix transform
        assert_eq!(eval_sandboxed("(+ 1 2 3)", &json!({})).unwrap(), json!(6));
        assert_eq!(eval_sandboxed("(* 2 3)", &json!({})).unwrap(), json!(6));
        assert_eq!(eval_sandboxed("(> 5 3)", &json!({})).unwrap(), json!(true));
    }

    // ── numberp tests ──

    #[test]
    fn test_numberp_int() {
        assert_eq!(
            eval_sandboxed("(numberp 42)", &json!({})).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn test_numberp_float() {
        assert_eq!(
            eval_sandboxed("(numberp 3.14)", &json!({})).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn test_numberp_string() {
        assert_eq!(
            eval_sandboxed("(numberp \"hello\")", &json!({})).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn test_numberp_nil() {
        assert_eq!(
            eval_sandboxed("(numberp nil)", &json!({})).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn test_numberp_in_conditional() {
        // Realistic use case from the idiomatic-rust manifest
        let env = json!({"score": 0.85});
        assert_eq!(
            eval_sandboxed("(if (numberp score) score 1.0)", &env).unwrap(),
            json!(0.85)
        );
    }

    // ── listp tests ──

    #[test]
    fn test_listp_list() {
        assert_eq!(
            eval_sandboxed("(listp (list 1 2 3))", &json!({})).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn test_listp_nil() {
        assert_eq!(
            eval_sandboxed("(listp nil)", &json!({})).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn test_listp_boolean() {
        assert_eq!(
            eval_sandboxed("(listp true)", &json!({})).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn test_listp_string() {
        assert_eq!(
            eval_sandboxed("(listp \"hello\")", &json!({})).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn test_listp_json_object() {
        let env = json!({"step_4_result": {"compiled": true}});
        assert_eq!(
            eval_sandboxed("(listp step_4_result)", &env).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn test_listp_in_conditional_guard() {
        // Guards assoc against non-list step results (upstream-rebase manifest)
        let env = json!({"step_4_result": true});
        assert_eq!(
            eval_sandboxed("(if (not (listp step_4_result)) 0 1)", &env).unwrap(),
            json!(0)
        );
    }

    #[test]
    fn test_principle_constraints_form_with_string() {
        // The principle-constraints manifest's lisp.eval form must guard
        // against step_2_result being a string (when the LLM emits text
        // instead of a JSON object).
        let form = "(if (listp step_2_result) (let ((summary (assoc \"summary\" step_2_result))) (if (is_null summary) 0 (let ((enforced (assoc \"enforced\" summary)) (gaps (assoc \"gaps\" summary))) (+ (if (is_null enforced) 0 enforced) (if (is_null gaps) 0 gaps))))) 0)";
        let env = json!({"step_2_result": "some text"});
        assert_eq!(eval_sandboxed(form, &env).unwrap(), json!(0));
    }

    #[test]
    fn test_principle_constraints_form_with_object() {
        // When the select step produces valid JSON, the form extracts
        // summary.enforced + summary.gaps as the convergence signal.
        let form = "(if (listp step_2_result) (let ((summary (assoc \"summary\" step_2_result))) (if (is_null summary) 0 (let ((enforced (assoc \"enforced\" summary)) (gaps (assoc \"gaps\" summary))) (+ (if (is_null enforced) 0 enforced) (if (is_null gaps) 0 gaps))))) 0)";
        let env = json!({
            "step_2_result": {
                "summary": {
                    "enforced": 5,
                    "gaps": 3
                }
            }
        });
        assert_eq!(eval_sandboxed(form, &env).unwrap(), json!(8));
    }

    // ── lisp-scaffold-reasoning manifest form tests ──
    // Pins the exact form used in kask/registry/manifests/lisp-scaffold-reasoning.yaml
    // step 2. If the interpreter changes in a way that breaks this form, these
    // tests fail before the skill is invoked in production.

    fn scaffold_form() -> &'static str {
        r##"
        (let ((hyps (assoc "hypotheses" step_1_result)))
          (if (is_null hyps)
              (list "no_hypotheses_field")
              (let ((n (length hyps)))
                (begin
                  (define count-defects
                    (if (< n 3)
                        (list "insufficient_count_below_3")
                        (if (> n 7)
                            (list "excessive_count_above_7")
                            (list))))
                  (define check-completeness
                    (lambda (hs acc)
                      (if (is_null hs)
                          acc
                          (let ((h (car hs))
                                (acc2 (if (is_null (assoc "prediction" h))
                                          (cons "missing_prediction" acc)
                                          acc)))
                            (let ((acc3 (if (is_null (assoc "falsifier" h))
                                            (cons "missing_falsifier" acc2)
                                            acc2)))
                              (check-completeness (cdr hs) acc3))))))
                  (define completeness-defects (check-completeness hyps (list)))
                  (define check-diversity
                    (lambda (hs nh nm nl)
                      (if (is_null hs)
                          (let ((distinct (+ (if (> nh 0) 1 0) (if (> nm 0) 1 0) (if (> nl 0) 1 0))))
                            (if (< distinct 2)
                                (list "insufficient_diversity_below_2")
                                (list)))
                          (let ((h (car hs))
                                (lk (assoc "likelihood" h))
                                (is-high (string= lk "high"))
                                (is-med (string= lk "medium"))
                                (is-low (string= lk "low")))
                            (check-diversity
                              (cdr hs)
                              (if is-high (+ nh 1) nh)
                              (if is-med (+ nm 1) nm)
                              (if is-low (+ nl 1) nl))))))
                  (define diversity-defects (check-diversity hyps 0 0 0))
                  (define check-duplicates
                    (lambda (hs seen)
                      (if (is_null hs)
                          (list)
                          (let ((h (car hs))
                                (hyp-text (assoc "hypothesis" h))
                                (hyp-str (if (is_null hyp-text) "" hyp-text)))
                            (if (not (is_null (assoc hyp-str seen)))
                                (cons "duplicate_hypothesis" (check-duplicates (cdr hs) seen))
                                (check-duplicates (cdr hs) (cons (list hyp-str true) seen)))))))
                  (define duplicate-defects (check-duplicates hyps (list)))
                  (append
                    count-defects completeness-defects
                    diversity-defects duplicate-defects)))))
        "##
    }

    #[test]
    fn scaffold_form_valid_count_returns_empty_defects() {
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "prediction": "p", "falsifier": "f", "likelihood": "medium"},
                    {"rank": 3, "hypothesis": "c", "prediction": "p", "falsifier": "f", "likelihood": "low"}
                ],
                "notes": "ok"
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        assert_eq!(result, json!([]));
    }

    #[test]
    fn scaffold_form_too_few_returns_defect() {
        // 1 hypothesis: count defect + diversity defect (only 1 likelihood value).
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"}
                ]
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        let defects = result.as_array().expect("result is a list");
        assert!(defects.contains(&json!("insufficient_count_below_3")));
        assert!(defects.contains(&json!("insufficient_diversity_below_2")));
    }

    #[test]
    fn scaffold_form_too_many_returns_defect() {
        // 8 hypotheses: count defect. Diversity check passes (all 3 likelihoods present).
        // Note: the deeply nested let structure in check-duplicates/diversity can
        // hit the depth limit (64) with 8 hypotheses — if that happens, the form
        // returns a DepthLimitExceeded error, which is a known limitation of the
        // reference form. The test uses 8 hypotheses but only asserts the count
        // defect is present if the form succeeds.
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 3, "hypothesis": "c", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 4, "hypothesis": "d", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 5, "hypothesis": "e", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 6, "hypothesis": "g", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 7, "hypothesis": "h", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 8, "hypothesis": "i", "prediction": "p", "falsifier": "f", "likelihood": "high"}
                ]
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        let defects = result.as_array().expect("result is a list");
        assert!(defects.contains(&json!("excessive_count_above_7")));
        // 8 hypotheses all with likelihood "high" → diversity defect too.
        assert!(defects.contains(&json!("insufficient_diversity_below_2")));
    }

    #[test]
    fn scaffold_form_missing_hypotheses_field_returns_defect() {
        let notes = "model returned none.";
        let env = json!({
            "step_1_result": {
                "notes": notes
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        // Expected: a one-element list containing the defect string.
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let defect = arr[0].as_str().unwrap_or("");
        let expected: &str = "no_hypotheses_field";
        assert_eq!(defect, expected);
    }

    #[test]
    fn scaffold_form_missing_prediction_returns_completeness_defect() {
        // 3 hypotheses, diverse likelihoods, but h2 is missing "prediction".
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "falsifier": "f", "likelihood": "medium"},
                    {"rank": 3, "hypothesis": "c", "prediction": "p", "falsifier": "f", "likelihood": "low"}
                ]
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        let defects = result.as_array().expect("result is a list");
        assert!(defects.contains(&json!("missing_prediction")));
    }

    #[test]
    fn scaffold_form_insufficient_diversity_returns_diversity_defect() {
        // 3 hypotheses, all with the same likelihood — diversity check fires.
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 3, "hypothesis": "c", "prediction": "p", "falsifier": "f", "likelihood": "high"}
                ]
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        let defects = result.as_array().expect("result is a list");
        assert!(defects.contains(&json!("insufficient_diversity_below_2")));
    }

    #[test]
    fn scaffold_form_duplicate_hypothesis_returns_duplicate_defect() {
        // 3 hypotheses, diverse likelihoods, but h1 and h3 have the same text.
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "same", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "prediction": "p", "falsifier": "f", "likelihood": "medium"},
                    {"rank": 3, "hypothesis": "same", "prediction": "p", "falsifier": "f", "likelihood": "low"}
                ]
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        let defects = result.as_array().expect("result is a list");
        assert!(defects.contains(&json!("duplicate_hypothesis")));
    }

    // ── Step 4 form: convergence score ──
    // Pins the exact form used in lisp-scaffold-reasoning.yaml step 4.
    // Score = 1.0 - (defect_count / n). Pure prefix (infix can't handle
    // nested-paren operands — see expand_infix/is_infix_context).

    fn scaffold_score_form() -> &'static str {
        r##"
        (let ((hyps (assoc "hypotheses" current)))
          (if (is_null hyps)
              0.0
              (let ((n (length hyps)))
                (if (= n 0)
                    0.0
                    (- 1.0 (/ defect_count (* n 4)))))))
        "##
    }

    #[test]
    fn scaffold_score_no_defects_returns_one() {
        let env = json!({
            "current": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "prediction": "p", "falsifier": "f", "likelihood": "medium"},
                    {"rank": 3, "hypothesis": "c", "prediction": "p", "falsifier": "f", "likelihood": "low"}
                ]
            },
            "defect_count": 0
        });
        let result = eval_sandboxed(scaffold_score_form(), &env).unwrap();
        assert_eq!(result, json!(1.0));
    }

    #[test]
    fn scaffold_score_one_defect_of_three_returns_eleven_twelfths() {
        // Score = 1.0 - (1 / (3 * 4)) = 1.0 - 1/12 = 11/12
        let env = json!({
            "current": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"},
                    {"rank": 2, "hypothesis": "b", "prediction": "p", "falsifier": "f", "likelihood": "medium"},
                    {"rank": 3, "hypothesis": "c", "prediction": "p", "falsifier": "f", "likelihood": "low"}
                ]
            },
            "defect_count": 1
        });
        let result = eval_sandboxed(scaffold_score_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - (1.0 - 1.0 / 12.0)).abs() < 1e-9);
    }

    #[test]
    fn scaffold_score_missing_hypotheses_returns_zero() {
        let env = json!({
            "current": {},
            "defect_count": 5
        });
        let result = eval_sandboxed(scaffold_score_form(), &env).unwrap();
        assert_eq!(result, json!(0.0));
    }

    // ── Convergence-fix form tests ──
    // Pins the exact forms used in the 7 manifests fixed for the phantom
    // convergence_metric bug. Each form computes a real structural-validity
    // score from the prior step's actual output fields.

    fn mcda_robustness_form() -> &'static str {
        r##"
        (let ((reversals (assoc "rank_reversals" step_4_result))
              (critical (assoc "critical_weights" step_4_result)))
          (let ((r (if (is_null reversals) 0 (length reversals)))
                (c (if (is_null critical) 0 (length critical))))
            (let ((raw (+ r c)))
              (if (> raw 1) 1.0 raw))))
        "##
    }

    #[test]
    fn mcda_robust_decision_scores_zero() {
        let env = json!({
            "step_4_result": {
                "rank_reversals": [],
                "critical_weights": [],
                "decision_robustness": "robust"
            }
        });
        let result = eval_sandboxed(mcda_robustness_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn mcda_fragile_decision_caps_at_one() {
        let env = json!({
            "step_4_result": {
                "rank_reversals": [{"criterion": "a"}, {"criterion": "b"}],
                "critical_weights": [{"criterion": "c"}, {"criterion": "d"}]
            }
        });
        let result = eval_sandboxed(mcda_robustness_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mcda_missing_fields_score_zero() {
        let env = json!({"step_4_result": {}});
        let result = eval_sandboxed(mcda_robustness_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn structured_extraction_gap_form() -> &'static str {
        r##"
        (let ((cov (assoc "field_coverage" step_3_result))
              (unresolved (assoc "unresolved_fields" step_3_result)))
          (let ((ratio (if (is_null cov) 0.0 (let ((r (assoc "coverage_ratio" cov))) (if (numberp r) r 0.0))))
                (total (if (is_null cov) 1 (let ((t (assoc "total_fields" cov))) (if (numberp t) t 1))))
                (uc (if (is_null unresolved) 0 (length unresolved))))
            (let ((penalty (if (> total 0) (/ uc total) 0)))
              (let ((penalty (if (> penalty 0.5) 0.5 penalty)))
                (+ (- 1.0 ratio) penalty)))))
        "##
    }

    #[test]
    fn extraction_full_coverage_no_unresolved_scores_zero() {
        let env = json!({
            "step_3_result": {
                "field_coverage": {"total_fields": 4, "populated_fields": 4, "coverage_ratio": 1.0},
                "unresolved_fields": []
            }
        });
        let result = eval_sandboxed(structured_extraction_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn extraction_half_coverage_scores_above_zero() {
        let env = json!({
            "step_3_result": {
                "field_coverage": {"total_fields": 10, "populated_fields": 5, "coverage_ratio": 0.5},
                "unresolved_fields": ["a", "b"]
            }
        });
        let result = eval_sandboxed(structured_extraction_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 1 - 0.5 + min(2/10, 0.5) = 0.5 + 0.2 = 0.7
        assert!((score - 0.7).abs() < 1e-9);
    }

    #[test]
    fn extraction_missing_coverage_scores_one() {
        let env = json!({"step_3_result": {}});
        let result = eval_sandboxed(structured_extraction_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn scenario_gap_form() -> &'static str {
        r##"
        (let ((div (assoc "divergence_score" step_5_result))
              (con (assoc "consistency_score" step_5_result))
              (cov (assoc "coverage_score" step_5_result))
              (pflag (assoc "parametric_variation_flag" step_5_result)))
          (let ((d (if (numberp div) div 0.0))
                (c (if (numberp con) con 0.0))
                (v (if (numberp cov) cov 0.0))
                (pen (if pflag 0.15 0.0)))
            (let ((mn (if (< d c) (if (< d v) d v) (if (< c v) c v))))
              (let ((gap (- 1.0 mn)))
                (if (> (+ gap pen) 1.0) 1.0 (+ gap pen))))))
        "##
    }

    #[test]
    fn scenario_passing_gate_no_parametric_scores_low() {
        let env = json!({
            "step_5_result": {
                "divergence_score": 0.9,
                "consistency_score": 0.85,
                "coverage_score": 0.8,
                "parametric_variation_flag": false
            }
        });
        let result = eval_sandboxed(scenario_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // min = 0.8; gap = 0.2; pen = 0; total = 0.2
        assert!((score - 0.2).abs() < 1e-9);
    }

    #[test]
    fn scenario_parametric_flag_adds_penalty() {
        let env = json!({
            "step_5_result": {
                "divergence_score": 0.9,
                "consistency_score": 0.85,
                "coverage_score": 0.8,
                "parametric_variation_flag": true
            }
        });
        let result = eval_sandboxed(scenario_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // min = 0.8; gap = 0.2; pen = 0.15; total = 0.35
        assert!((score - 0.35).abs() < 1e-9);
    }

    #[test]
    fn scenario_missing_scores_zero_min_scores_one() {
        let env = json!({"step_5_result": {}});
        let result = eval_sandboxed(scenario_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // all scores default to 0.0; min = 0; gap = 1.0; pen = 0; total = 1.0
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn wardley_confidence_gap_form() -> &'static str {
        r##"
        (let ((recs (assoc "recommendations" step_5_result)))
          (if (is_null recs)
              1.0
              (let ((n (length recs)))
                (if (= n 0)
                    1.0
                    (begin
                      (define count-low
                        (lambda (rs acc)
                          (if (is_null rs)
                              acc
                              (let ((conf (assoc "confidence" (car rs))))
                                (count-low
                                  (cdr rs)
                                  (if (numberp conf)
                                      (if (< conf 0.5) (+ acc 1) acc)
                                      (+ acc 1)))))))
                      (let ((low (count-low recs 0)))
                        (/ low n)))))))
        "##
    }

    #[test]
    fn wardley_all_high_confidence_scores_zero() {
        let env = json!({
            "step_5_result": {
                "recommendations": [
                    {"confidence": 0.9},
                    {"confidence": 0.8},
                    {"confidence": 0.7}
                ]
            }
        });
        let result = eval_sandboxed(wardley_confidence_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn wardley_half_low_confidence_scores_half() {
        let env = json!({
            "step_5_result": {
                "recommendations": [
                    {"confidence": 0.9},
                    {"confidence": 0.3}
                ]
            }
        });
        let result = eval_sandboxed(wardley_confidence_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn wardley_missing_recommendations_scores_one() {
        let env = json!({"step_5_result": {}});
        let result = eval_sandboxed(wardley_confidence_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn diataxis_quality_form() -> &'static str {
        r##"
        (let ((wt (assoc "weighted_total" step_4_result))
              (directives (assoc "refinement_directives" step_4_result)))
          (let ((score (if (numberp wt) wt 1.0))
                (dc (if (is_null directives) 0 (length directives))))
            (let ((raw (+ score (* dc 0.05))))
              (if (> raw 1.0) 1.0 raw))))
        "##
    }

    #[test]
    fn diataxis_perfect_diagram_scores_zero() {
        let env = json!({
            "step_4_result": {
                "weighted_total": 0.0,
                "refinement_directives": []
            }
        });
        let result = eval_sandboxed(diataxis_quality_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn diataxis_two_directives_adds_penalty() {
        let env = json!({
            "step_4_result": {
                "weighted_total": 0.1,
                "refinement_directives": ["fix1", "fix2"]
            }
        });
        let result = eval_sandboxed(diataxis_quality_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 0.1 + 2 * 0.05 = 0.2
        assert!((score - 0.2).abs() < 1e-9);
    }

    #[test]
    fn diataxis_missing_weighted_total_scores_one() {
        let env = json!({"step_4_result": {}});
        let result = eval_sandboxed(diataxis_quality_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn refactor_gap_form() -> &'static str {
        r##"
        (if (is_null step_7_result)
            1.0
            (let ((depth (assoc "depth_test_results" step_7_result))
                  (dep-check (assoc "dependency_check" step_7_result))
                  (p68 (assoc "p6_p7_p8_compliance" step_7_result))
                  (checklist (assoc "checklist" step_7_result)))
              (begin
                (define count-shallow
                  (lambda (ds acc)
                    (if (is_null ds)
                        acc
                        (let ((verdict (assoc "verdict" (car ds))))
                          (count-shallow
                            (cdr ds)
                            (if (string= verdict "shallow") (+ acc 1) acc))))))
                (define count-not-done
                  (lambda (cs acc)
                    (if (is_null cs)
                        acc
                        (let ((st (assoc "status" (car cs))))
                          (count-not-done
                            (cdr cs)
                            (if (string= st "not_done") (+ acc 1) acc))))))
                (let ((shallow (count-shallow (if (is_null depth) (list) depth) 0))
                      (dep-v (let ((v (assoc "violations" dep-check))) (if (is_null v) 0 (length v))))
                      (p68-v (let ((v (assoc "violations" p68))) (if (is_null v) 0 (length v))))
                      (nd (count-not-done (if (is_null checklist) (list) checklist) 0)))
                  (let ((raw (+ shallow dep-v p68-v nd)))
                    (if (> raw 1) 1.0 raw))))))
        "##
    }

    #[test]
    fn refactor_clean_verify_scores_zero() {
        let env = json!({
            "step_7_result": {
                "depth_test_results": [{"verdict": "deep"}],
                "dependency_check": {"violations": []},
                "p6_p7_p8_compliance": {"violations": []},
                "checklist": [{"item": "x", "status": "done"}]
            }
        });
        let result = eval_sandboxed(refactor_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn refactor_shallow_and_violations_score_nonzero() {
        let env = json!({
            "step_7_result": {
                "depth_test_results": [{"verdict": "shallow"}],
                "dependency_check": {"violations": ["v1"]},
                "p6_p7_p8_compliance": {"violations": []},
                "checklist": [{"item": "x", "status": "not_done"}]
            }
        });
        let result = eval_sandboxed(refactor_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 1 shallow + 1 dep violation + 0 p68 + 1 not_done = 3, capped at 1.0
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn refactor_skipped_verify_scores_one() {
        // step 7 was skipped (decision != proceed_to_refactor); the Jinja
        // template renders step_7_result as JSON null, which from_json
        // converts to LispValue::Nil. The form's nil-guard returns 1.0.
        let env = json!({"step_7_result": null});
        let result = eval_sandboxed(refactor_gap_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn graph_audit_findings_form() -> &'static str {
        r##"
        (let ((findings (assoc "quality_findings" step_3_result)))
          (let ((n (if (is_null findings) 0 (length findings))))
            (let ((raw (/ n 10)))
              (if (> raw 1.0) 1.0 raw))))
        "##
    }

    #[test]
    fn graph_audit_no_findings_scores_zero() {
        let env = json!({"step_3_result": {"quality_findings": []}});
        let result = eval_sandboxed(graph_audit_findings_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn graph_audit_five_findings_scores_half() {
        let env = json!({"step_3_result": {"quality_findings": ["a", "b", "c", "d", "e"]}});
        let result = eval_sandboxed(graph_audit_findings_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn graph_audit_missing_findings_scores_zero() {
        let env = json!({"step_3_result": {}});
        let result = eval_sandboxed(graph_audit_findings_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn graph_audit_blockers_form() -> &'static str {
        r##"
        (let ((blockers (assoc "blockers" step_15_result)))
          (let ((n (if (is_null blockers) 0 (length blockers))))
            (let ((raw (/ n 5)))
              (if (> raw 1.0) 1.0 raw))))
        "##
    }

    #[test]
    fn graph_audit_no_blockers_scores_zero() {
        let env = json!({"step_15_result": {"blockers": []}});
        let result = eval_sandboxed(graph_audit_blockers_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn graph_audit_three_blockers_scores_six_tenths() {
        let env = json!({"step_15_result": {"blockers": ["b1", "b2", "b3"]}});
        let result = eval_sandboxed(graph_audit_blockers_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.6).abs() < 1e-9);
    }

    // ── Phantom convergence_metric fix: 8 manifest forms ──
    // Pins the exact forms used in the 8 manifests fixed for the phantom
    // convergence_metric bug (code-review, kali-audit, kata-coaching,
    // kata-improvement, lora-training, proptest, replica-discovery, tdd).
    // Each form computes a real structural-validity score from the prior
    // step's actual output fields, replacing the phantom
    // `convergence_metric` binding that silently defaulted to 1.0.

    fn code_review_open_findings_form() -> &'static str {
        r##"
        (+ (assoc "blockers" (assoc "severity_counts" step_3_result))
           (* 0.5 (assoc "should_fix" (assoc "severity_counts" step_3_result))))
        "##
    }

    #[test]
    fn code_review_two_blockers_four_should_fix_scores_four() {
        let env = json!({"step_3_result": {"severity_counts": {"blockers": 2, "should_fix": 4, "nit": 1, "fyi": 0}}});
        let result = eval_sandboxed(code_review_open_findings_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 2 + 0.5*4 = 4.0
        assert!((score - 4.0).abs() < 1e-9);
    }

    #[test]
    fn code_review_zero_findings_scores_zero() {
        let env = json!({"step_3_result": {"severity_counts": {"blockers": 0, "should_fix": 0, "nit": 0, "fyi": 0}}});
        let result = eval_sandboxed(code_review_open_findings_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn kali_audit_critical_high_form() -> &'static str {
        r##"
        (+ (length (assoc "critical" (assoc "findings_by_severity" (assoc "report" step_3_result))))
           (length (assoc "high" (assoc "findings_by_severity" (assoc "report" step_3_result)))))
        "##
    }

    #[test]
    fn kali_audit_one_critical_two_high_scores_three() {
        let env = json!({"step_3_result": {"report": {"findings_by_severity": {"critical": ["c1"], "high": ["h1", "h2"], "medium": [], "low": []}}}});
        let result = eval_sandboxed(kali_audit_critical_high_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 3.0).abs() < 1e-9);
    }

    #[test]
    fn kali_audit_no_findings_scores_zero() {
        let env = json!({"step_3_result": {"report": {"findings_by_severity": {"critical": [], "high": [], "medium": [], "low": []}}}});
        let result = eval_sandboxed(kali_audit_critical_high_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn kata_coaching_tight_loop_form() -> &'static str {
        r##"
        (if (string= (assoc "coach_assessment" step_5_result) "tight-loop") 0.0 1.0)
        "##
    }

    #[test]
    fn kata_coaching_tight_loop_scores_zero() {
        let env = json!({"step_5_result": {"coach_assessment": "tight-loop"}});
        let result = eval_sandboxed(kata_coaching_tight_loop_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn kata_coaching_open_loop_scores_one() {
        let env = json!({"step_5_result": {"coach_assessment": "open-loop"}});
        let result = eval_sandboxed(kata_coaching_tight_loop_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn kata_improvement_beginner_form() -> &'static str {
        r##"
        (if (is_null step_2_result)
            1.0
            (let ((a (assoc "automaticity_self_assess" step_2_result)))
              (if (is_null a) 1.0 (- 1.0 a))))
        "##
    }

    #[test]
    fn kata_improvement_beginner_high_automaticity_scores_low() {
        let env = json!({"step_2_result": {"automaticity_self_assess": 0.7}});
        let result = eval_sandboxed(kata_improvement_beginner_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 1.0 - 0.7 = 0.3
        assert!((score - 0.3).abs() < 1e-9);
    }

    #[test]
    fn kata_improvement_beginner_null_drill_scores_one() {
        // pdca-cycle or observation-drill ran (step_2_result is null)
        let env = json!({"step_2_result": null});
        let result = eval_sandboxed(kata_improvement_beginner_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    fn kata_improvement_experiment_form() -> &'static str {
        r##"
        (- 7
           (+ (if (is_null (assoc "obstacle" step_10_result)) 0 1)
              (if (is_null (assoc "next_experiment" step_10_result)) 0 1)
              (if (is_null (assoc "prediction" step_10_result)) 0 1)
              (if (is_null (assoc "measurement_method" step_10_result)) 0 1)
              (if (is_null (assoc "success_criterion" step_10_result)) 0 1)
              (if (is_null (assoc "learning_commitment" step_10_result)) 0 1)
              (if (is_null (assoc "when_to_check" step_10_result)) 0 1)))
        "##
    }

    #[test]
    fn kata_improvement_experiment_all_fields_scores_zero() {
        let env = json!({"step_10_result": {"obstacle": "x", "next_experiment": "y", "prediction": "z", "measurement_method": "m", "success_criterion": "s", "learning_commitment": "l", "when_to_check": "w"}});
        let result = eval_sandboxed(kata_improvement_experiment_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn kata_improvement_experiment_three_missing_scores_three() {
        let env = json!({"step_10_result": {"obstacle": "x", "next_experiment": "y", "prediction": "z", "measurement_method": "m"}});
        let result = eval_sandboxed(kata_improvement_experiment_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 3.0).abs() < 1e-9);
    }

    fn lora_training_findings_count_form() -> &'static str {
        r##"
        (let ((f (assoc "findings" step_3_result)))
          (if (is_null f) 0 (length f)))
        "##
    }

    #[test]
    fn lora_training_three_findings_scores_three() {
        let env =
            json!({"step_3_result": {"findings": [{"id": "f1"}, {"id": "f2"}, {"id": "f3"}]}});
        let result = eval_sandboxed(lora_training_findings_count_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 3.0).abs() < 1e-9);
    }

    #[test]
    fn lora_training_no_findings_scores_zero() {
        let env = json!({"step_3_result": {"findings": []}});
        let result = eval_sandboxed(lora_training_findings_count_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn proptest_failures_form() -> &'static str {
        r##"
        (let ((f (assoc "failures_found" step_5_result)))
          (if (is_null f) 0 (length f)))
        "##
    }

    #[test]
    fn proptest_one_failure_scores_one() {
        let env = json!({"step_5_result": {"failures_found": [{"property_name": "p1"}]}});
        let result = eval_sandboxed(proptest_failures_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn proptest_no_failures_scores_zero() {
        let env = json!({"step_5_result": {"failures_found": []}});
        let result = eval_sandboxed(proptest_failures_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn replica_discovery_missing_fields_form() -> &'static str {
        r##"
        (let ((m (assoc "fields_missing" step_12_result)))
          (if (is_null m) 0 (length m)))
        "##
    }

    #[test]
    fn replica_discovery_two_missing_scores_two() {
        let env = json!({"step_12_result": {"fields_missing": ["field_a", "field_b"], "fields_present": ["field_c"]}});
        let result = eval_sandboxed(replica_discovery_missing_fields_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 2.0).abs() < 1e-9);
    }

    #[test]
    fn replica_discovery_no_missing_scores_zero() {
        let env = json!({"step_12_result": {"fields_missing": [], "fields_present": ["a", "b"]}});
        let result = eval_sandboxed(replica_discovery_missing_fields_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    fn tdd_gap_coverage_form() -> &'static str {
        r##"
        (let ((g (assoc "gaps" step_6_result))
              (c (assoc "coverage_percentage" step_6_result)))
          (let ((gc (if (is_null g) 0 (length g)))
                (cc (if (numberp c) c 0.0)))
            (+ gc (/ (- 100 cc) 100))))
        "##
    }

    #[test]
    fn tdd_one_gap_85pct_coverage_scores_one_and_tenth() {
        let env = json!({"step_6_result": {"gaps": [{"id": "g1"}], "coverage_percentage": 85.7}});
        let result = eval_sandboxed(tdd_gap_coverage_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 1 + (100 - 85.7)/100 = 1 + 0.143 = 1.143
        assert!((score - 1.143).abs() < 1e-2);
    }

    #[test]
    fn tdd_no_gaps_full_coverage_scores_zero() {
        let env = json!({"step_6_result": {"gaps": [], "coverage_percentage": 100.0}});
        let result = eval_sandboxed(tdd_gap_coverage_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        assert!((score - 0.0).abs() < 1e-9);
    }

    // ── Skill-bundler composition-score form ──
    // Pins the exact form used in kask/registry/manifests/skill-bundler.yaml
    // step 5. The score combines coverage (from compose), overlap (from
    // compose), and validation violations/warnings/ontology_gaps (from
    // validate) into a single deterministic convergence signal. Lower =
    // better. This is the falsifier anchor: if `lisp.eval` is removed, the
    // score becomes LLM-internal and non-reproducible — this test pins the
    // deterministic computation so the drift is detectable.

    fn skill_bundler_composition_score_form() -> &'static str {
        r##"
        (let ((coverage (assoc "coverage" step_2_result))
              (overlap (assoc "overlap" step_2_result))
              (v (length (assoc "violations" step_4_result)))
              (w (length (assoc "warnings" step_4_result)))
              (o (length (assoc "ontology_gaps" step_4_result))))
          (+ 0.0
             (* 2.0 (- 1.0 coverage))
             (* 0.5 overlap)
             (+ v w) o))
        "##
    }

    #[test]
    fn skill_bundler_full_coverage_no_overlap_no_violations_scores_zero() {
        let env = json!({
            "step_2_result": {"coverage": 1.0, "overlap": 0},
            "step_4_result": {"violations": [], "warnings": [], "ontology_gaps": []}
        });
        let result = eval_sandboxed(skill_bundler_composition_score_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 2.0*(1-1) + 0.5*0 + 0 + 0 + 0 = 0.0
        assert!((score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn skill_bundler_half_coverage_penalizes_double() {
        let env = json!({
            "step_2_result": {"coverage": 0.5, "overlap": 0},
            "step_4_result": {"violations": [], "warnings": [], "ontology_gaps": []}
        });
        let result = eval_sandboxed(skill_bundler_composition_score_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 2.0*(1-0.5) + 0 + 0 = 1.0
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn skill_bundler_overlap_and_violations_add_to_score() {
        let env = json!({
            "step_2_result": {"coverage": 1.0, "overlap": 2},
            "step_4_result": {
                "violations": [{"rule_id": "V1"}],
                "warnings": [{"rule_id": "V7"}],
                "ontology_gaps": [{"axis": "pko"}]
            }
        });
        let result = eval_sandboxed(skill_bundler_composition_score_form(), &env).unwrap();
        let score = result.as_f64().expect("score is a float");
        // 2.0*0 + 0.5*2 + (1+1) + 1 = 0 + 1.0 + 2 + 1 = 4.0
        assert!((score - 4.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod cfr_fix_verification {
    use super::*;
    /// The fixed CFR convergence form must:
    /// 1. Compute correctly when env values are numeric: 1.0 + 0.05*2 = 1.1
    /// 2. Not error when env values are stringified (Jinja stringification):
    ///    numberp guards fall back to defaults (1.0 and 1) → 1.0 + 0.05*1 = 1.05
    /// 3. Not silently return the wrong value (the bare-infix bug).
    #[test]
    fn cfr_fixed_form_numeric_env() {
        let form = "(let ((hd (if (numberp hypervolume_delta) hypervolume_delta 1.0)) (nd (if (numberp new_non_dominated) new_non_dominated 1))) (+ hd (* 0.05 nd)))";
        let env = serde_json::json!({
            "hypervolume_delta": 1.0,
            "new_non_dominated": 2
        });
        let result = eval_sandboxed_with_budget(form, &env, 100000, 64).unwrap();
        let score = result.as_f64().expect("result is a float");
        assert!((score - 1.1).abs() < 1e-9, "expected 1.1, got {score}");
    }

    #[test]
    fn cfr_fixed_form_string_env_no_error() {
        let form = "(let ((hd (if (numberp hypervolume_delta) hypervolume_delta 1.0)) (nd (if (numberp new_non_dominated) new_non_dominated 1))) (+ hd (* 0.05 nd)))";
        // Jinja stringification: values are strings, not numbers.
        // numberp returns false → defaults kick in: hd=1.0, nd=1 → 1.05
        let env = serde_json::json!({
            "hypervolume_delta": "true",
            "new_non_dominated": "1"
        });
        let result = eval_sandboxed_with_budget(form, &env, 100000, 64).unwrap();
        let score = result.as_f64().expect("result is a float");
        assert!((score - 1.05).abs() < 1e-9, "expected 1.05, got {score}");
    }

    /// Pin the old broken behavior to document what the fix prevents.
    #[test]
    fn cfr_old_bare_infix_form_silent_wrong_result() {
        let form = "hypervolume_delta + 0.05 * new_non_dominated";
        let env = serde_json::json!({
            "hypervolume_delta": 1.0,
            "new_non_dominated": 2
        });
        // The old form returns 2 (new_non_dominated's value), not 1.1.
        let result = eval_sandboxed_with_budget(form, &env, 100000, 64).unwrap();
        let score = result.as_f64().expect("result is a float");
        assert!(
            (score - 2.0).abs() < 1e-9,
            "old form returns {score}, not 1.1 — this is the bug the fix addresses"
        );
    }
}

#[cfg(test)]
mod audit_repro {
    use super::*;

    #[test]
    fn lisp_scaffold_step4_stringified_defect_count() {
        let form = "(let ((hyps (assoc \"hypotheses\" current))) (if (is_null hyps) 0.0 (let ((n (length hyps))) (if (= n 0) 0.0 (- 1.0 (/ defect_count (* n 4)))))))";
        // If step_2_result is absent, default(0) might stringify to "0"
        let env = serde_json::json!({
            "current": {"hypotheses": [{"h": "a"}, {"h": "b"}, {"h": "c"}]},
            "defect_count": "0"  // stringified
        });
        match eval_sandboxed_with_budget(form, &env, 100000, 64) {
            Ok(v) => println!("lisp-scaffold OK: {}", v),
            Err(e) => println!("lisp-scaffold ERR: {}", e),
        }
    }

    #[test]
    fn swarm_steering_stringified_credit_ceiling() {
        let form = "(let ((directive step_1_result)) (if (is_null directive) (list \"no_directive\") (let ((seq (assoc \"execution_sequence\" directive))) (if (is_null seq) (list \"missing_seq\") (let ((credits (assoc \"credits_authorized\" (car seq)))) (if (is_null credits) (list \"missing_credits\") (if (> credits credit_ceiling) (list \"exceeds\") (list))))))))";
        let env = serde_json::json!({
            "step_1_result": {"execution_sequence": [{"credits_authorized": 100}]},
            "credit_ceiling": "50"  // stringified
        });
        match eval_sandboxed_with_budget(form, &env, 100000, 64) {
            Ok(v) => println!("swarm-steering OK: {}", v),
            Err(e) => println!("swarm-steering ERR: {}", e),
        }
    }
}

#[cfg(test)]
mod audit_fix_verification {
    use super::*;

    #[test]
    fn lisp_scaffold_step4_fixed_stringified_defect_count() {
        let form = "(let ((hyps (assoc \"hypotheses\" current))) (if (is_null hyps) 0.0 (let ((n (length hyps))) (if (= n 0) 0.0 (let ((dc (if (numberp defect_count) defect_count 0))) (- 1.0 (/ dc (* n 4))))))))";
        let env = serde_json::json!({
            "current": {"hypotheses": [{"h": "a"}, {"h": "b"}, {"h": "c"}]},
            "defect_count": "0"  // stringified — numberp guard falls back to 0
        });
        let result = eval_sandboxed_with_budget(form, &env, 100000, 64).unwrap();
        let score = result.as_f64().expect("result is a float");
        assert!((score - 1.0).abs() < 1e-9, "expected 1.0, got {score}");
    }

    #[test]
    fn swarm_steering_fixed_stringified_credit_ceiling() {
        let form = "(let ((directive step_1_result)) (if (is_null directive) (list \"no_directive\") (let ((seq (assoc \"execution_sequence\" directive))) (if (is_null seq) (list \"missing_seq\") (let ((credits (assoc \"credits_authorized\" (car seq)))) (if (is_null credits) (list \"missing_credits\") (if (and (not (is_null credits)) (let ((cc (if (numberp credit_ceiling) credit_ceiling 50))) (> credits cc))) (list \"exceeds\") (list))))))))";
        let env = serde_json::json!({
            "step_1_result": {"execution_sequence": [{"credits_authorized": 100}]},
            "credit_ceiling": "50"  // stringified — numberp guard falls back to 50
        });
        let result = eval_sandboxed_with_budget(form, &env, 100000, 64).unwrap();
        let arr = result.as_array().expect("result is a list");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], serde_json::json!("exceeds"));
    }
}
