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
//!   Special forms: quote, if, let, lambda, define, begin, and, or, not
//!   Built-in functions: car, cdr, cons, list, length, nth, reverse,
//!     +, -, *, /, =, !=, <, <=, >, >=, is_null, numberp, assoc
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
        ("assoc", assoc_fn),
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
                (if (< n 3)
                    (list "insufficient_count_below_3")
                    (if (> n 7)
                        (list "excessive_count_above_7")
                        (list))))))
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
        let env = json!({
            "step_1_result": {
                "hypotheses": [
                    {"rank": 1, "hypothesis": "a", "prediction": "p", "falsifier": "f", "likelihood": "high"}
                ]
            }
        });
        let result = eval_sandboxed(scaffold_form(), &env).unwrap();
        assert_eq!(result, json!(["insufficient_count_below_3"]));
    }

    #[test]
    fn scaffold_form_too_many_returns_defect() {
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
        assert_eq!(result, json!(["excessive_count_above_7"]));
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
                (if (= n 0) 0.0 (- 1.0 (/ defect_count n))))))
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
    fn scaffold_score_one_defect_of_three_returns_two_thirds() {
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
        assert!((score - (1.0 - 1.0 / 3.0)).abs() < 1e-9);
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
}
