//! Structured POSIX `shlex` command prefix policy rules (`dpv:Policy` /
//! `cso:authorization`, Study 4 / C3).
//!
//! Reference model: `openinterpreter/openinterpreter` `codex-rs/execpolicy`
//! (`PrefixPattern` / `PrefixRule`), implemented in pure Rust over
//! `shell_command_parser::extract_commands` so compound shell commands
//! (`&&`, `;`, `|`) and quoted arguments cannot bypass prefix or destructive-`rm`
//! policies.

use crate::tool_permissions::normalize_path;
use shell_command_parser::extract_commands;
use std::path::Path;
use util::shell::ShellKind;

/// Policy decision returned by a matching [`PrefixRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Confirm,
    Deny,
}

/// One token slot in a [`PrefixPattern`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternToken {
    /// Matches one exact token (case-sensitive).
    Single(String),
    /// Matches any token in the closed alternative set (case-sensitive).
    Alts(Vec<String>),
}

impl PatternToken {
    pub fn single(token: impl Into<String>) -> Self {
        Self::Single(token.into())
    }

    pub fn alts(tokens: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Alts(tokens.into_iter().map(Into::into).collect())
    }

    pub fn matches(&self, token: &str) -> bool {
        match self {
            Self::Single(expected) => token == expected,
            Self::Alts(options) => options.iter().any(|opt| opt == token),
        }
    }
}

/// A tokenized command prefix pattern (e.g. `["git", Alts(["status", "diff", "log"])]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixPattern {
    pub tokens: Vec<PatternToken>,
}

impl PrefixPattern {
    pub fn new(tokens: Vec<PatternToken>) -> Self {
        Self { tokens }
    }

    /// Returns `true` when `argv` starts with every token in this prefix pattern.
    /// An empty pattern never matches (fail-closed).
    pub fn matches_tokens(&self, argv: &[String]) -> bool {
        if self.tokens.is_empty() || argv.len() < self.tokens.len() {
            return false;
        }
        self.tokens
            .iter()
            .zip(argv.iter())
            .all(|(pat, actual)| pat.matches(actual))
    }
}

/// A single structured command-prefix policy rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixRule {
    pub pattern: PrefixPattern,
    pub decision: PolicyDecision,
    pub justification: Option<String>,
}

impl PrefixRule {
    pub fn new(
        pattern: PrefixPattern,
        decision: PolicyDecision,
        justification: Option<String>,
    ) -> Self {
        Self {
            pattern,
            decision,
            justification,
        }
    }
}

/// Ordered collection of [`PrefixRule`] entries evaluated over parsed sub-commands.
#[derive(Debug, Default, Clone)]
pub struct CommandPolicy {
    rules: Vec<PrefixRule>,
}

impl CommandPolicy {
    pub fn new(rules: Vec<PrefixRule>) -> Self {
        Self { rules }
    }

    /// Evaluates a raw terminal command against all prefix rules.
    ///
    /// Precedence across every extracted sub-command:
    /// 1. `Deny` if ANY sub-command matches a `Deny` rule.
    /// 2. `Confirm` if ANY sub-command matches a `Confirm` rule, or if POSIX
    ///    sub-command extraction / word tokenization fails closed.
    /// 3. `Allow` ONLY if ALL sub-commands match at least one `Allow` rule and
    ///    zero parse/tokenization failures occurred.
    /// 4. `None` when no rule decided the command (caller falls back to default).
    pub fn evaluate(&self, raw_command: &str, shell_kind: ShellKind) -> Option<PolicyDecision> {
        let (commands, allow_enabled) = if shell_kind.supports_posix_chaining() {
            match extract_commands(raw_command) {
                Some(cmds) if !cmds.is_empty() => (cmds, true),
                _ => (vec![raw_command.to_string()], false),
            }
        } else {
            (vec![raw_command.to_string()], false)
        };

        let mut any_confirm = false;
        let mut all_allow = allow_enabled;

        for cmd in &commands {
            let Some(argv) = tokenize_posix_command(cmd) else {
                // Unclosed quote or malformed escape: fail closed (never auto-allow).
                all_allow = false;
                any_confirm = true;
                continue;
            };
            if argv.is_empty() {
                all_allow = false;
                continue;
            }

            let mut matched_allow = false;
            for rule in &self.rules {
                if rule.pattern.matches_tokens(&argv) {
                    match rule.decision {
                        PolicyDecision::Deny => return Some(PolicyDecision::Deny),
                        PolicyDecision::Confirm => any_confirm = true,
                        PolicyDecision::Allow => matched_allow = true,
                    }
                }
            }
            if !matched_allow {
                all_allow = false;
            }
        }

        if any_confirm {
            return Some(PolicyDecision::Confirm);
        }
        if all_allow {
            return Some(PolicyDecision::Allow);
        }
        None
    }
}

/// Pure-Rust POSIX `shlex` word tokenizer.
///
/// Handles single quotes, double quotes (with `\"`, `\\`, `\$`, `` \` ``),
/// unquoted backslash escapes, and whitespace boundaries. Returns `None`
/// (fail-closed) on unclosed quotes or dangling escapes.
pub fn tokenize_posix_command(input: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            c if c.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            '\'' => {
                in_token = true;
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '\'' {
                        closed = true;
                        break;
                    }
                    current.push(c);
                }
                if !closed {
                    return None;
                }
            }
            '"' => {
                in_token = true;
                let mut closed = false;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            let next = chars.next()?;
                            if matches!(next, '"' | '\\' | '$' | '`' | '\n') {
                                current.push(next);
                            } else {
                                current.push('\\');
                                current.push(next);
                            }
                        }
                        other => current.push(other),
                    }
                }
                if !closed {
                    return None;
                }
            }
            '\\' => {
                in_token = true;
                let next = chars.next()?;
                current.push(next);
            }
            other => {
                in_token = true;
                current.push(other);
            }
        }
    }

    if in_token {
        tokens.push(current);
    }
    Some(tokens)
}

/// Returns `true` when a single shell command invokes `rm` against a
/// catastrophic filesystem target (`/`, `/*`, `~`, `~/`, `~/*`, `$HOME`,
/// `${HOME}`, `.`, `./`, `./*`, `..`, `../`, `../*`), including quoted paths,
/// traversal paths (`/tmp/../../`), full executable paths (`/bin/rm`), and
/// common wrappers (`sudo`, `env`, `command`).
pub fn is_hardcoded_destructive_rm(command: &str) -> bool {
    let tokens = match tokenize_posix_command(command) {
        Some(tokens) => tokens,
        // Fall back to whitespace splitting if quoting is malformed so an
        // unclosed quote cannot mask an obvious `rm -rf /`.
        None => command
            .split_whitespace()
            .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
            .collect(),
    };

    let Some(rm_ix) = find_rm_command_index(&tokens) else {
        return false;
    };

    let mut past_double_dash = false;
    for arg in &tokens[rm_ix + 1..] {
        if !past_double_dash && arg == "--" {
            past_double_dash = true;
            continue;
        }
        if !past_double_dash && arg.starts_with('-') && arg != "-" {
            continue;
        }
        if is_catastrophic_rm_target(arg) {
            return true;
        }
    }
    false
}

fn find_rm_command_index(tokens: &[String]) -> Option<usize> {
    let mut ix = 0;
    while ix < tokens.len() {
        let tok = &tokens[ix];
        // Skip leading shell env assignments (`FOO=bar rm -rf /`).
        if tok.contains('=') && !tok.starts_with('-') && !tok.starts_with('/') {
            ix += 1;
            continue;
        }
        let base = Path::new(tok)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(tok.as_str());
        if base.eq_ignore_ascii_case("rm") {
            return Some(ix);
        }
        // Step past transparent command wrappers (`sudo`, `env`, `command`) and their flags.
        if matches!(
            base.to_ascii_lowercase().as_str(),
            "sudo" | "env" | "command"
        ) {
            ix += 1;
            while ix < tokens.len() && tokens[ix].starts_with('-') {
                ix += 1;
            }
            continue;
        }
        return None;
    }
    None
}

fn is_catastrophic_rm_target(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Strip a single trailing wildcard (`/*`, `~/*`, `./*`, `../*`, `$HOME/*`).
    let without_glob = trimmed.strip_suffix('*').unwrap_or(trimmed);

    // Home-directory forms: `~`, `~/`, `~/*`, `$HOME`, `$HOME/`, `${HOME}`, `${HOME}/`.
    if matches!(
        without_glob,
        "~" | "~/" | "$HOME" | "$HOME/" | "${HOME}" | "${HOME}/"
    ) {
        return true;
    }
    for prefix in ["~", "$HOME", "${HOME}"] {
        if let Some(rest) = without_glob.strip_prefix(prefix) {
            if rest.starts_with('/') && normalize_path(rest) == "/" {
                return true;
            }
        }
    }

    // Root, current dir, and parent dir (after collapsing `.` / `..` traversals).
    let mut normalized = normalize_path(without_glob);
    if normalized.is_empty() && !Path::new(without_glob).has_root() {
        normalized = ".".to_string();
    }
    matches!(normalized.as_str(), "/" | "." | "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_posix_command_handles_quotes_escapes_and_fails_closed() {
        assert_eq!(
            tokenize_posix_command(r#"git commit -m "hello world" 'a b' foo\ bar"#),
            Some(vec![
                "git".into(),
                "commit".into(),
                "-m".into(),
                "hello world".into(),
                "a b".into(),
                "foo bar".into(),
            ])
        );
        assert_eq!(
            tokenize_posix_command("rm -rf \"/\""),
            Some(vec!["rm".into(), "-rf".into(), "/".into()])
        );
        assert_eq!(tokenize_posix_command("echo 'unclosed"), None);
        assert_eq!(tokenize_posix_command("echo \"unclosed"), None);
        assert_eq!(tokenize_posix_command("echo trailing\\"), None);
    }

    #[test]
    fn command_policy_rejects_shell_chaining_bypass_and_enforces_precedence() {
        let policy = CommandPolicy::new(vec![
            PrefixRule::new(
                PrefixPattern::new(vec![
                    PatternToken::single("cargo"),
                    PatternToken::alts(["test", "check"]),
                ]),
                PolicyDecision::Allow,
                None,
            ),
            PrefixRule::new(
                PrefixPattern::new(vec![
                    PatternToken::single("git"),
                    PatternToken::alts(["status", "diff", "log"]),
                ]),
                PolicyDecision::Allow,
                None,
            ),
            PrefixRule::new(
                PrefixPattern::new(vec![
                    PatternToken::single("git"),
                    PatternToken::single("push"),
                ]),
                PolicyDecision::Confirm,
                Some("remote mutation".into()),
            ),
            PrefixRule::new(
                PrefixPattern::new(vec![PatternToken::single("rm")]),
                PolicyDecision::Deny,
                Some("destructive".into()),
            ),
        ]);

        // Valid single and chained allowed commands.
        assert_eq!(
            policy.evaluate("cargo test -p agent", ShellKind::Posix),
            Some(PolicyDecision::Allow)
        );
        assert_eq!(
            policy.evaluate("cargo check && git status", ShellKind::Posix),
            Some(PolicyDecision::Allow)
        );

        // Falsifier: shell-chaining bypass (`cargo test && rm -rf /tmp/foo`) must be Denied.
        assert_eq!(
            policy.evaluate("cargo test && rm -rf /tmp/foo", ShellKind::Posix),
            Some(PolicyDecision::Deny)
        );
        // Mixed Allow + Confirm resolves to Confirm.
        assert_eq!(
            policy.evaluate("cargo test && git push origin main", ShellKind::Posix),
            Some(PolicyDecision::Confirm)
        );
        // Mixed Allow + unmatched command must NOT auto-allow.
        assert_eq!(
            policy.evaluate("cargo test && curl https://example.com", ShellKind::Posix),
            None
        );
        // Quoted command token still matches cleanly.
        assert_eq!(
            policy.evaluate("'cargo' \"test\" --lib", ShellKind::Posix),
            Some(PolicyDecision::Allow)
        );
    }

    #[test]
    fn hardcoded_destructive_rm_blocks_quoted_path_and_wrapper_bypasses() {
        for dangerous in [
            "rm -rf /",
            "rm -rf \"/\"",
            "rm -rf '/'",
            "rm -rf /*",
            "rm / -rf",
            "rm -rf /tmp/../../",
            "rm -rf \"/tmp/../../\"",
            "'/bin/rm' -rf /",
            "sudo rm -rf ~/*",
            "env FOO=1 /usr/bin/rm -rf \"$HOME\"",
            "rm -rf ${HOME}/foo/..",
            "rm -rf .",
            "rm -rf \"./*\"",
            "rm -rf ..",
            "rm -rf /tmp /",
        ] {
            assert!(
                is_hardcoded_destructive_rm(dangerous),
                "expected catastrophic rm to be blocked: {dangerous}"
            );
        }

        for safe in [
            "rm -rf /tmp/foo",
            "rm -rf ~/subdir",
            "rm -rf ./target",
            "rm -rf $HOME/cache",
            "echo rm -rf /",
            "git rm foo.txt",
        ] {
            assert!(
                !is_hardcoded_destructive_rm(safe),
                "expected non-catastrophic command not to be blocked: {safe}"
            );
        }
    }
}
