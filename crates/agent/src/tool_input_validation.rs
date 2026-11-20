//! Pre-execution tool-argument validation, bounded syntactic repair,
//! conversational tool gating, and explicit private-memory directive parsing.

use crate::{UserMessage, UserMessageContent};
use serde_json::{Map, Value};
use std::path::Path;

/// Binary/media/archive extensions that `read_file` (a text/image source-code
/// tool) must never open as text buffers.
pub(crate) const UNSUPPORTED_READ_FILE_BINARY_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "wma", "mp4", "mkv", "mov", "avi", "webm",
    "flv", "wmv", "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "pdf", "wasm", "so",
    "dylib", "dll", "exe", "bin", "o", "a", "pyc", "class", "sqlite", "sqlite3", "db",
];

/// Required string parameter names that identify a concrete locator, pattern,
/// command, or query and therefore must not be blank or whitespace-only.
const NON_BLANK_REQUIRED_STRING_FIELDS: &[&str] = &[
    "glob",
    "path",
    "source_path",
    "destination_path",
    "regex",
    "command",
    "url",
    "query",
    "name",
    "skill_name",
    "board_id",
    "task_id",
    "goal_id",
    "transcript_id",
    "media_path",
];

/// True when `path` has a binary audio/video/archive/database extension that
/// `read_file` must reject before opening as a text buffer.
pub(crate) fn is_unsupported_binary_read_path(path: &str) -> bool {
    Path::new(path.trim())
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            UNSUPPORTED_READ_FILE_BINARY_EXTENSIONS
                .iter()
                .any(|blocked| ext.eq_ignore_ascii_case(blocked))
        })
}

/// Extract an explicit user instruction to remember/save a note or preference
/// (English or Spanish), returning the normalized preference text when present.
/// Interrogatives ("do you remember…?", "¿recuerdas…?") return `None`.
pub(crate) fn extract_explicit_memory_request(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() || trimmed.ends_with('?') || trimmed.starts_with('¿') {
        return None;
    }

    let lower_full = trimmed.to_lowercase();
    for interrogative in [
        "do you remember",
        "did you remember",
        "can you remember",
        "could you remember",
        "what do you remember",
        "recuerdas ",
        "te acuerdas",
    ] {
        if lower_full.starts_with(interrogative)
            || lower_full.contains(&format!(" {interrogative}"))
        {
            return None;
        }
    }

    const PREFIXES: &[&str] = &[
        "please remember that ",
        "please remember: ",
        "please remember ",
        "remember that ",
        "remember: ",
        "remember ",
        "save to memory: ",
        "save to memory ",
        "store in memory: ",
        "store in memory ",
        "por favor recuerda que ",
        "por favor recuerda: ",
        "por favor recuerda ",
        "recuerda que ",
        "recuerda: ",
        "recuerda ",
        "guarda en memoria: ",
        "guarda en memoria ",
    ];

    // Check from the start of the prompt first, then after clause boundaries
    // (e.g. "In this thread, please remember “I prefer brief, direct answers in Spanish”").
    let candidate = match_memory_prefix(trimmed, PREFIXES).or_else(|| {
        for sep in [". ", ", ", "; ", "— ", " - ", ": ", "\n"] {
            let mut search_from = 0;
            while let Some(rel_idx) = trimmed[search_from..].find(sep) {
                let after = &trimmed[search_from + rel_idx + sep.len()..];
                if let Some(extracted) = match_memory_prefix(after.trim_start(), PREFIXES) {
                    return Some(extracted);
                }
                search_from += rel_idx + sep.len();
            }
        }
        None
    })?;

    let cleaned = strip_surrounding_quotes_and_punctuation(candidate);
    (cleaned.chars().count() >= 3).then(|| cleaned.to_string())
}

fn match_memory_prefix<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    // Match ASCII/UTF-8 prefix case-insensitively while slicing on valid char
    // boundaries of `text`.
    let lower = text.to_lowercase();
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            let prefix_chars = prefix.chars().count();
            let byte_offset = text
                .char_indices()
                .nth(prefix_chars)
                .map_or(text.len(), |(idx, _)| idx);
            return Some(&text[byte_offset..]);
        }
    }
    None
}

fn strip_surrounding_quotes_and_punctuation(raw: &str) -> &str {
    let mut s = raw.trim();
    // Strip trailing period/semicolon outside quotes first (e.g. `remember "foo".`).
    s = s.trim_end_matches(['.', ';']).trim();
    for (open, close) in [
        ('"', '"'),
        ('\'', '\''),
        ('“', '”'),
        ('‘', '’'),
        ('«', '»'),
        ('`', '`'),
    ] {
        if s.starts_with(open)
            && s.ends_with(close)
            && s.len() >= open.len_utf8() + close.len_utf8()
        {
            s = &s[open.len_utf8()..s.len() - close.len_utf8()];
            break;
        }
    }
    s.trim().trim_end_matches('.').trim()
}

/// True when the user's message asks for file, code, search, workspace, command,
/// or external-tool work. Plain conversational messages ("ok", "dime ok",
/// "hola", "remember I prefer…", "what model are you?") return `false` so local
/// models do not receive or execute unsolicited tools.
pub(crate) fn prompt_requires_tools(message: &UserMessage) -> bool {
    let mut combined_text = String::new();
    for item in message.content.iter() {
        match item {
            UserMessageContent::Mention { .. } | UserMessageContent::Image(_) => return true,
            UserMessageContent::Text(text) => {
                if !combined_text.is_empty() {
                    combined_text.push('\n');
                }
                combined_text.push_str(text);
            }
        }
    }
    prompt_text_requires_tools(&combined_text)
}

pub(crate) fn prompt_text_requires_tools(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Slash commands / skill invocations require tools.
    if trimmed.starts_with('/') {
        return true;
    }
    // An explicit "remember …" preference with no file/path/code markers is
    // handled by private-memory persistence, not file/search tools.
    let is_memory_directive = extract_explicit_memory_request(trimmed).is_some();

    // Structural code, URL, glob, or path indicators.
    if trimmed.contains("```")
        || trimmed.contains('`')
        || trimmed.contains("http://")
        || trimmed.contains("https://")
        || trimmed.contains("file://")
        || trimmed.contains('*')
        || trimmed.contains("./")
        || trimmed.contains("../")
        || trimmed.contains("~/")
    {
        return true;
    }

    let lower = trimmed.to_lowercase();

    // Check tokens for file paths (`dir/file`) or known project file extensions.
    for raw_tok in lower.split_whitespace() {
        let tok = raw_tok.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\''
                    | '“'
                    | '”'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | ','
                    | ';'
                    | ':'
                    | '?'
                    | '!'
            )
        });
        if tok.contains('/')
            && !matches!(tok, "and/or" | "yes/no" | "true/false" | "sí/no" | "si/no")
            && !tok.starts_with("ollama/")
            && !tok.starts_with("llama.cpp/")
        {
            return true;
        }
        if let Some((_, ext)) = tok.rsplit_once('.')
            && matches!(
                ext,
                "rs" | "toml"
                    | "json"
                    | "yaml"
                    | "yml"
                    | "md"
                    | "hbs"
                    | "j2"
                    | "ts"
                    | "tsx"
                    | "js"
                    | "jsx"
                    | "css"
                    | "html"
                    | "sh"
                    | "sql"
                    | "lock"
                    | "lean"
                    | "c"
                    | "h"
                    | "cpp"
                    | "go"
                    | "java"
                    | "py"
                    | "txt"
                    | "csv"
                    | "pdf"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "mp3"
                    | "mp4"
                    | "wav"
            )
        {
            return true;
        }
    }

    if is_memory_directive {
        return false;
    }

    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .collect();

    const TOOL_INTENT_WORDS: &[&str] = &[
        // English workspace / file / search / code / shell / MCP terms
        "read",
        "open",
        "inspect",
        "cat",
        "list",
        "ls",
        "dir",
        "find",
        "locate",
        "search",
        "grep",
        "rg",
        "glob",
        "edit",
        "modify",
        "update",
        "change",
        "patch",
        "write",
        "create",
        "delete",
        "remove",
        "rename",
        "move",
        "copy",
        "run",
        "execute",
        "build",
        "compile",
        "test",
        "tests",
        "check",
        "lint",
        "clippy",
        "cargo",
        "git",
        "commit",
        "diff",
        "status",
        "branch",
        "debug",
        "diagnose",
        "fix",
        "resolve",
        "refactor",
        "implement",
        "review",
        "audit",
        "benchmark",
        "fetch",
        "browse",
        "search_web",
        "kanban",
        "board",
        "task",
        "tasks",
        "goal",
        "goals",
        "curator",
        "corpus",
        "gallery",
        "transcribe",
        "nostr",
        "forecast",
        "scenario",
        "skill",
        "skills",
        "tool",
        "tools",
        "file",
        "files",
        "folder",
        "folders",
        "directory",
        "directories",
        "path",
        "paths",
        "repo",
        "repository",
        "workspace",
        "project",
        "code",
        "codebase",
        "crate",
        "crates",
        "module",
        "modules",
        "function",
        "struct",
        "enum",
        "trait",
        "diagnostics",
        "terminal",
        // Spanish workspace / file / search / code / shell terms
        "lee",
        "leer",
        "abre",
        "abrir",
        "inspecciona",
        "inspeccionar",
        "lista",
        "listar",
        "busca",
        "buscar",
        "encuentra",
        "encontrar",
        "edita",
        "editar",
        "modifica",
        "modificar",
        "escribe",
        "escribir",
        "crea",
        "crear",
        "borra",
        "borrar",
        "elimina",
        "eliminar",
        "ejecuta",
        "ejecutar",
        "compila",
        "compilar",
        "prueba",
        "probar",
        "revisa",
        "revisar",
        "arregla",
        "arreglar",
        "corrige",
        "corregir",
        "archivo",
        "archivos",
        "carpeta",
        "carpetas",
        "directorio",
        "directorios",
        "ruta",
        "rutas",
        "repositorio",
        "proyecto",
        "código",
        "codigo",
        "herramienta",
        "herramientas",
    ];

    words.iter().any(|w| TOOL_INTENT_WORDS.contains(w))
}

/// Repair safe syntactic quirks (e.g. stringified JSON object, stringified
/// scalar numbers/booleans, `null` on optional fields) and strictly validate
/// `input` against `schema` BEFORE executing `tool_name`.
///
/// Never invents missing required values (e.g. a missing `glob` on `find_path`
/// is always rejected).
pub(crate) fn validate_and_repair_tool_input(
    tool_name: &str,
    schema: &Value,
    input: Value,
) -> Result<Value, String> {
    let repaired = repair_value_for_schema(input, schema);
    validate_value_against_schema(tool_name, "$", &repaired, schema)?;

    if tool_name == "read_file"
        && let Some(path) = repaired.get("path").and_then(Value::as_str)
        && is_unsupported_binary_read_path(path)
    {
        return Err(format!(
            "Invalid arguments for tool '{tool_name}': `path` ({path:?}) is a binary/media file and cannot be inspected as text by `read_file`"
        ));
    }

    Ok(repaired)
}

fn repair_value_for_schema(value: Value, schema: &Value) -> Value {
    let schema_type = schema.get("type").and_then(Value::as_str);

    match value {
        Value::Null => {
            // A tool with an object schema and zero required fields tolerates
            // `null` by normalizing to `{}`.
            let has_required = schema
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|req| !req.is_empty());
            if schema_type == Some("object") && !has_required {
                Value::Object(Map::new())
            } else {
                Value::Null
            }
        }
        Value::String(ref raw) => {
            let trimmed = raw.trim();
            if schema_type == Some("object")
                && trimmed.starts_with('{')
                && let Ok(parsed @ Value::Object(_)) = serde_json::from_str::<Value>(trimmed)
            {
                return repair_value_for_schema(parsed, schema);
            }
            if schema_type == Some("array")
                && trimmed.starts_with('[')
                && let Ok(parsed @ Value::Array(_)) = serde_json::from_str::<Value>(trimmed)
            {
                return repair_value_for_schema(parsed, schema);
            }
            if schema_type_allows(schema, "integer") && !schema_type_allows(schema, "string") {
                if let Ok(int_val) = trimmed.parse::<i64>() {
                    return Value::Number(int_val.into());
                }
            }
            if schema_type_allows(schema, "number") && !schema_type_allows(schema, "string") {
                if let Ok(float_val) = trimmed.parse::<f64>()
                    && let Some(num) = serde_json::Number::from_f64(float_val)
                {
                    return Value::Number(num);
                }
            }
            if schema_type_allows(schema, "boolean") && !schema_type_allows(schema, "string") {
                if trimmed.eq_ignore_ascii_case("true") {
                    return Value::Bool(true);
                }
                if trimmed.eq_ignore_ascii_case("false") {
                    return Value::Bool(false);
                }
            }
            value
        }
        Value::Object(map) => {
            let required: Vec<&str> = schema
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let props = schema.get("properties").and_then(Value::as_object);
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                let is_req = required.iter().any(|r| *r == k);
                // Strip `null` on optional fields so non-Option `#[serde(default)]`
                // fields (such as `offset: usize`) deserialize cleanly instead of failing.
                if v.is_null() && !is_req {
                    continue;
                }
                let repaired_child = match props.and_then(|p| p.get(&k)) {
                    Some(prop_schema) => repair_value_for_schema(v, prop_schema),
                    None => v,
                };
                out.insert(k, repaired_child);
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                Value::Array(
                    items
                        .into_iter()
                        .map(|item| repair_value_for_schema(item, item_schema))
                        .collect(),
                )
            } else {
                Value::Array(items)
            }
        }
        other => other,
    }
}

fn schema_type_allows(schema: &Value, expected: &str) -> bool {
    if let Some(t) = schema.get("type") {
        match t {
            Value::String(s) if s == expected => return true,
            Value::Array(arr) if arr.iter().any(|v| v.as_str() == Some(expected)) => return true,
            _ => {}
        }
    }
    for combinator in ["anyOf", "oneOf"] {
        if let Some(variants) = schema.get(combinator).and_then(Value::as_array)
            && variants.iter().any(|sub| schema_type_allows(sub, expected))
        {
            return true;
        }
    }
    false
}

fn validate_value_against_schema(
    tool_name: &str,
    path: &str,
    value: &Value,
    schema: &Value,
) -> Result<(), String> {
    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        if variants
            .iter()
            .any(|sub| validate_value_against_schema(tool_name, path, value, sub).is_ok())
        {
            return Ok(());
        }
        return Err(format!(
            "Invalid arguments for tool '{tool_name}': `{path}` did not match any allowed schema shape (received {})",
            describe_value_kind(value)
        ));
    }
    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        if variants
            .iter()
            .any(|sub| validate_value_against_schema(tool_name, path, value, sub).is_ok())
        {
            return Ok(());
        }
        return Err(format!(
            "Invalid arguments for tool '{tool_name}': `{path}` did not match any allowed schema variant (received {})",
            describe_value_kind(value)
        ));
    }

    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array)
        && !enum_values.contains(value)
    {
        return Err(format!(
            "Invalid arguments for tool '{tool_name}': `{path}` must be one of {enum_values:?}, received {value}"
        ));
    }

    let nullable = schema
        .get("nullable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if value.is_null() && nullable {
        return Ok(());
    }

    if let Some(type_val) = schema.get("type") {
        let matches_type = match type_val {
            Value::String(expected) => value_matches_json_type(value, expected),
            Value::Array(types) => types
                .iter()
                .filter_map(Value::as_str)
                .any(|expected| value_matches_json_type(value, expected)),
            _ => true,
        };
        if !matches_type {
            return Err(format!(
                "Invalid arguments for tool '{tool_name}': `{path}` expected type {type_val}, received {}",
                describe_value_kind(value)
            ));
        }
    }

    if let Value::Object(map) = value {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            let props = schema.get("properties").and_then(Value::as_object);
            for req_key in required.iter().filter_map(Value::as_str) {
                let Some(field_val) = map.get(req_key) else {
                    let expected_hint = props
                        .and_then(|p| p.get(req_key))
                        .and_then(|s| s.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or("value");
                    return Err(format!(
                        "Invalid arguments for tool '{tool_name}': missing required field `{req_key}` (expected {expected_hint})"
                    ));
                };
                if field_val.is_null() {
                    let field_nullable = props.and_then(|p| p.get(req_key)).is_some_and(|s| {
                        s.get("nullable").and_then(Value::as_bool) == Some(true)
                            || schema_type_allows(s, "null")
                    });
                    if !field_nullable {
                        return Err(format!(
                            "Invalid arguments for tool '{tool_name}': required field `{req_key}` must not be null"
                        ));
                    }
                }
                if path == "$"
                    && NON_BLANK_REQUIRED_STRING_FIELDS.contains(&req_key)
                    && let Some(s) = field_val.as_str()
                    && s.trim().is_empty()
                {
                    return Err(format!(
                        "Invalid arguments for tool '{tool_name}': required field `{req_key}` must not be empty"
                    ));
                }
            }
        }

        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (k, v) in map {
                if let Some(prop_schema) = props.get(k) {
                    let child_path = if path == "$" {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    validate_value_against_schema(tool_name, &child_path, v, prop_schema)?;
                }
            }
        }
    }

    if let Value::Array(items) = value
        && let Some(item_schema) = schema.get("items")
    {
        for (idx, item) in items.iter().enumerate() {
            let child_path = format!("{path}[{idx}]");
            validate_value_against_schema(tool_name, &child_path, item, item_schema)?;
        }
    }

    Ok(())
}

fn value_matches_json_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn describe_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_explicit_memory_requests_and_ignores_questions() {
        assert_eq!(
            extract_explicit_memory_request(
                "Please remember “I prefer brief, direct answers in Spanish”"
            ),
            Some("I prefer brief, direct answers in Spanish".to_string())
        );
        assert_eq!(
            extract_explicit_memory_request(
                "In this locked thread, remember: \"I prefer brief, direct answers in Spanish\"."
            ),
            Some("I prefer brief, direct answers in Spanish".to_string())
        );
        assert_eq!(
            extract_explicit_memory_request(
                "Por favor recuerda que prefiero respuestas breves y directas en español."
            ),
            Some("prefiero respuestas breves y directas en español".to_string())
        );
        assert_eq!(
            extract_explicit_memory_request("Do you remember my language preference?"),
            None
        );
        assert_eq!(
            extract_explicit_memory_request("¿Recuerdas mi preferencia de idioma?"),
            None
        );
        assert_eq!(extract_explicit_memory_request("ok"), None);
    }

    #[test]
    fn conversational_prompts_do_not_require_tools() {
        for plain in [
            "ok",
            "dime ok",
            "hola",
            "hello",
            "thanks!",
            "gracias",
            "Please remember “I prefer brief, direct answers in Spanish”",
            "What model and provider are you?",
            "¿Qué modelo eres?",
            "Explain ownership in two sentences",
        ] {
            assert!(
                !prompt_text_requires_tools(plain),
                "plain conversational prompt should not require tools: {plain:?}"
            );
        }

        for tool_needing in [
            "list tools",
            "read Cargo.toml",
            "find *.rs files",
            "list directory crates/agent",
            "search for uses_compact_profile in thread.rs",
            "run cargo test",
            "lee el archivo README.md",
        ] {
            assert!(
                prompt_text_requires_tools(tool_needing),
                "workspace/tool prompt must enable tools: {tool_needing:?}"
            );
        }
    }

    #[test]
    fn validates_and_repairs_tool_arguments_before_execution() {
        let find_path_schema = json!({
            "type": "object",
            "properties": {
                "glob": { "type": "string" },
                "offset": { "type": "integer" }
            },
            "required": ["glob"]
        });

        // Reported bug: `find_path` called with missing `glob`.
        let missing = validate_and_repair_tool_input("find_path", &find_path_schema, json!({}));
        assert!(
            missing
                .as_ref()
                .is_err_and(|e| e.contains("missing required field `glob`")),
            "got: {missing:?}"
        );

        // Blank required `glob` is also rejected.
        let blank =
            validate_and_repair_tool_input("find_path", &find_path_schema, json!({ "glob": "  " }));
        assert!(
            blank
                .as_ref()
                .is_err_and(|e| e.contains("required field `glob` must not be empty")),
            "got: {blank:?}"
        );

        // Safe repairs: stringified object + stringified integer + null optional field.
        let repaired = validate_and_repair_tool_input(
            "find_path",
            &find_path_schema,
            json!("{\"glob\":\"**/*.rs\",\"offset\":\"20\"}"),
        )
        .expect("stringified object + integer repairs cleanly");
        assert_eq!(repaired, json!({ "glob": "**/*.rs", "offset": 20 }));

        let null_optional = validate_and_repair_tool_input(
            "find_path",
            &find_path_schema,
            json!({ "glob": "*.toml", "offset": null }),
        )
        .expect("null optional field stripped");
        assert_eq!(null_optional, json!({ "glob": "*.toml" }));

        // Reported bug: `read_file` attempting to inspect an MP3 file.
        let read_file_schema = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        });
        let mp3_err = validate_and_repair_tool_input(
            "read_file",
            &read_file_schema,
            json!({ "path": "project/audio/track.mp3" }),
        );
        assert!(
            mp3_err
                .as_ref()
                .is_err_and(|e| e.contains("binary/media file")),
            "got: {mp3_err:?}"
        );
    }
}
