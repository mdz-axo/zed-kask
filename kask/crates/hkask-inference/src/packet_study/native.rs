//! Native Responses request/response codecs shared by HTTP and offline replay.
//! No transport, credentials, retries, or provider fallback belong here.

use serde_json::{Value, json};

use super::budget::Usage;

const COUNT_FIELDS: &[&str] = &[
    "model",
    "input",
    "instructions",
    "reasoning",
    "text",
    "tools",
    "tool_choice",
];
const FIXED_FIELDS: &[&str] = &[
    "store",
    "background",
    "stream",
    "service_tier",
    "truncation",
];

pub(super) fn body(
    model: &str,
    messages: Vec<Value>,
    max_output_tokens: u64,
) -> Result<Value, String> {
    validate_model(model)?;
    validate_input(&Value::Array(messages.clone()))?;
    if max_output_tokens == 0 {
        return Err("max_output_tokens must be a positive integer".into());
    }
    Ok(json!({
        "model": model,
        "input": messages,
        "max_output_tokens": max_output_tokens,
        "store": false,
        "background": false,
        "stream": false,
        "service_tier": "default",
        "truncation": "disabled",
        "reasoning": {"effort": "none"},
        "tools": [],
        "tool_choice": "none",
        "text": {"format": {"type": "text"}}
    }))
}

fn validate_model(model: &str) -> Result<(), String> {
    if model.is_empty() || model.chars().any(char::is_whitespace) || model.contains('/') {
        return Err("native model ID required, without whitespace or provider prefix".into());
    }
    Ok(())
}

fn validate_input(input: &Value) -> Result<(), String> {
    let messages = input.as_array().ok_or("input must be a message array")?;
    if messages.is_empty() {
        return Err("input must contain retained public messages".into());
    }
    for message in messages {
        let object = message
            .as_object()
            .ok_or("input message must be an object")?;
        if object.len() != 2 || !object.contains_key("role") || !object.contains_key("content") {
            return Err("input messages support only role and string content".into());
        }
        if !matches!(
            message["role"].as_str(),
            Some("system" | "developer" | "user" | "assistant")
        ) || !message["content"].is_string()
        {
            return Err("input requires a public text-message role and string content".into());
        }
    }
    Ok(())
}

/// Project only documented input-shaping fields from a validated generation body.
/// Reference: https://developers.openai.com/api/reference/python/resources/responses/subresources/input_tokens/methods/count
/// `instructions` is supported and copied verbatim if present. Generation-only
/// settings are checked before omission; unknown fields (including server-side
/// state and sampling knobs) cannot silently escape the count/body binding.
pub(super) fn count_body(generation: &Value) -> Result<Value, String> {
    let object = generation
        .as_object()
        .ok_or("generation body must be an object")?;
    for key in object.keys() {
        if !COUNT_FIELDS.contains(&key.as_str())
            && !FIXED_FIELDS.contains(&key.as_str())
            && key != "max_output_tokens"
        {
            return Err(format!("unsupported generation field: {key}"));
        }
    }
    let model = generation["model"]
        .as_str()
        .ok_or("model must be a string")?;
    let input = generation["input"]
        .as_array()
        .ok_or("input must be a message array")?;
    let max_output_tokens = generation["max_output_tokens"]
        .as_u64()
        .ok_or("max_output_tokens must be a positive integer")?;
    let expected = body(model, input.clone(), max_output_tokens)?;
    for key in COUNT_FIELDS.iter().chain(FIXED_FIELDS).copied() {
        if key != "instructions" && generation.get(key) != expected.get(key) {
            return Err(format!("unsupported or missing generation setting: {key}"));
        }
    }
    if let Some(instructions) = generation.get("instructions")
        && !instructions.is_string()
    {
        return Err("instructions must be a string when present".into());
    }
    let mut count = serde_json::Map::new();
    for key in COUNT_FIELDS {
        if let Some(value) = generation.get(*key) {
            count.insert((*key).into(), value.clone());
        }
    }
    Ok(Value::Object(count))
}

pub(super) fn parse_count(value: &Value) -> Result<u64, String> {
    if !value.is_object() || value["object"] != "response.input_tokens" {
        return Err("native response.input_tokens object required".into());
    }
    reject_error(value)?;
    value["input_tokens"]
        .as_u64()
        .ok_or_else(|| "response.input_tokens requires a nonnegative integer input_tokens".into())
}

fn reject_error(value: &Value) -> Result<(), String> {
    if value.get("error").is_some_and(|error| !error.is_null()) {
        return Err("native response contains a non-null error".into());
    }
    Ok(())
}

fn usage(value: &Value) -> Result<Option<Usage>, String> {
    let Some(raw) = value.get("usage").filter(|raw| !raw.is_null()) else {
        return Ok(None);
    };
    if !raw.is_object() {
        return Err("native usage must be an object or null".into());
    }
    fn count(raw: &Value, key: &str) -> Result<Option<u64>, String> {
        match raw.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("native usage.{key} must be a nonnegative integer")),
        }
    }
    let prompt_tokens = count(raw, "input_tokens")?;
    let completion_tokens = count(raw, "output_tokens")?;
    let reasoning_tokens = match raw.get("output_tokens_details") {
        None | Some(Value::Null) => None,
        Some(details) if details.is_object() => count(details, "reasoning_tokens")?,
        Some(_) => {
            return Err("native usage.output_tokens_details must be an object or null".into());
        }
    };
    if let (Some(reasoning), Some(output)) = (reasoning_tokens, completion_tokens)
        && reasoning > output
    {
        return Err("reasoning_tokens exceeds output_tokens".into());
    }
    Ok(Some(Usage {
        prompt_tokens,
        completion_tokens,
        reasoning_tokens,
        // Native Responses does not report a monetary cost in this contract.
        reported_cost_microusd: None,
    }))
}

pub(super) fn parse_response(
    value: &Value,
    model: &str,
    cap_check: bool,
) -> Result<(String, Option<Usage>), String> {
    if !value.is_object() || value["object"] != "response" {
        return Err("native response object required".into());
    }
    validate_model(model)?;
    if value["model"].as_str() != Some(model) {
        return Err("native response model does not match the requested model".into());
    }
    reject_error(value)?;
    let truncated = match value["status"].as_str() {
        Some("completed") => {
            if value.get("incomplete_details").is_some_and(|details| !details.is_null()) {
                return Err("completed response has non-null incomplete_details".into());
            }
            false
        }
        Some("incomplete") if cap_check => {
            let details = value["incomplete_details"]
                .as_object()
                .ok_or("cap-check incomplete_details must be an object")?;
            if details.len() != 1 || details.get("reason").and_then(Value::as_str) != Some("max_output_tokens") {
                return Err("cap-check incomplete reason must be max_output_tokens".into());
            }
            true
        }
        _ => return Err("native response must be completed (cap-check alone permits max_output_tokens incomplete)".into()),
    };
    let output = value["output"]
        .as_array()
        .ok_or("native output must be an array")?;
    let mut text = String::new();
    for item in output {
        if !item.is_object() || item["type"] != "message" || item["role"] != "assistant" {
            return Err(
                "native output must contain only assistant messages, not tools or reasoning items"
                    .into(),
            );
        }
        match item["status"].as_str() {
            Some("completed") => {}
            Some("incomplete") if truncated => {}
            _ => return Err("unexpected native output message status".into()),
        }
        let content = item["content"]
            .as_array()
            .ok_or("message content must be an array")?;
        for part in content {
            if !part.is_object() || part["type"] != "output_text" {
                return Err(
                    "native message content must be output_text, not refusal or non-text content"
                        .into(),
                );
            }
            text.push_str(
                part["text"]
                    .as_str()
                    .ok_or("output_text.text must be a string")?,
            );
        }
    }
    let usage = usage(value)?;
    if truncated {
        // This sentinel is cap-validation evidence, never a scientific judgment.
        return Ok(("[cap-check incomplete: max_output_tokens]".into(), usage));
    }
    if text.trim().is_empty() {
        return Err("completed native response has no nonempty output text".into());
    }
    Ok((text, usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = "gpt-5.4-mini-2026-03-17";

    fn request() -> Value {
        body(
            MODEL,
            vec![json!({"role":"user", "content":"Judge the receipt."})],
            1024,
        )
        .expect("valid native request")
    }

    fn response() -> Value {
        json!({
            "object":"response", "model":MODEL, "status":"completed", "error":null,
            "incomplete_details":null,
            "output":[{"type":"message", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":"judgment"}]}],
            "usage":{"input_tokens":19, "output_tokens":7,
                "output_tokens_details":{"reasoning_tokens":2}}
        })
    }

    #[test]
    fn native_request_and_documented_count_projection() {
        let mut generation = request();
        assert!(generation.get("messages").is_none());
        assert_eq!(generation["input"][0]["content"], "Judge the receipt.");
        assert_eq!(generation["reasoning"], json!({"effort":"none"}));
        for key in ["store", "background", "stream"] {
            assert_eq!(generation[key], false);
        }
        assert_eq!(generation["service_tier"], "default");
        assert_eq!(generation["truncation"], "disabled");
        generation["instructions"] = json!("Retain the public history.");
        let count = count_body(&generation).expect("supported count projection");
        assert_eq!(count.as_object().expect("object").len(), COUNT_FIELDS.len());
        for key in COUNT_FIELDS {
            assert_eq!(count[*key], generation[*key]);
        }
        assert!(count.get("max_output_tokens").is_none());
        assert!(count.get("store").is_none());
        assert!(
            count_body(&request())
                .expect("no instructions required")
                .get("instructions")
                .is_none()
        );
    }

    #[test]
    fn generation_rejects_unknown_state_sampling_and_fixed_setting_drift() {
        for key in [
            "previous_response_id",
            "conversation",
            "seed",
            "temperature",
            "messages",
            "metadata",
            "parallel_tool_calls",
        ] {
            let mut value = request();
            value[key] = Value::Null;
            assert!(count_body(&value).is_err(), "unknown field {key}");
        }
        for (key, bad) in [
            ("store", json!(true)),
            ("background", json!(true)),
            ("stream", json!(true)),
            ("service_tier", json!("flex")),
            ("truncation", json!("auto")),
            ("reasoning", json!({"effort":"low"})),
            ("tools", json!([{"type":"web_search"}])),
            ("tool_choice", json!("auto")),
            ("text", json!({"format":{"type":"json_object"}})),
            ("max_output_tokens", json!(0)),
            ("instructions", Value::Null),
        ] {
            let mut value = request();
            value[key] = bad;
            assert!(count_body(&value).is_err(), "setting {key}");
        }
        for key in COUNT_FIELDS
            .iter()
            .chain(FIXED_FIELDS)
            .copied()
            .filter(|key| *key != "instructions")
        {
            let mut value = request();
            value.as_object_mut().expect("object").remove(key);
            assert!(count_body(&value).is_err(), "missing {key}");
        }
        assert!(count_body(&Value::Null).is_err());
    }

    #[test]
    fn input_accepts_retained_text_history_only() {
        let history = ["system", "developer", "user", "assistant"]
            .map(|role| json!({"role":role,"content":"text"}));
        assert!(body(MODEL, history.to_vec(), 1).is_ok());
        for message in [
            json!({"role":"tool","content":"result"}),
            json!({"role":"user","content":[]}),
            json!({"role":"user","content":"text","extra":true}),
            Value::Null,
        ] {
            assert!(body(MODEL, vec![message], 1).is_err());
        }
        assert!(body(MODEL, vec![], 1).is_err());
        for model in ["", " model", "openrouter/model"] {
            assert!(body(model, vec![json!({"role":"user","content":"x"})], 1).is_err());
        }
    }

    #[test]
    fn count_response_requires_native_discriminator_and_unsigned_integer() {
        for count in [json!(0), json!(8192), json!(u64::MAX)] {
            assert_eq!(
                parse_count(&json!({"object":"response.input_tokens","input_tokens":count}))
                    .expect("integer count"),
                count.as_u64().expect("u64")
            );
        }
        for count in [Value::Null, json!(-1), json!(1.0), json!("10"), json!(true)] {
            assert!(
                parse_count(&json!({"object":"response.input_tokens","input_tokens":count}))
                    .is_err()
            );
        }
        for value in [
            json!({"input_tokens":1}),
            json!({"object":"response.input_tokens"}),
            json!({"object":"response.input_tokens","input_tokens":1,"error":false}),
            Value::Null,
        ] {
            assert!(parse_count(&value).is_err());
        }
    }

    #[test]
    fn completed_text_and_native_usage_are_preserved_without_reported_cost() {
        let mut value = response();
        value["output"][0]["content"]
            .as_array_mut()
            .expect("content")
            .push(json!({"type":"output_text","text":" continued"}));
        value["usage"]["cost"] = json!(100);
        let (text, usage) = parse_response(&value, MODEL, false).expect("native response");
        assert_eq!(text, "judgment continued");
        let usage = usage.expect("reported usage");
        assert_eq!(usage.prompt_tokens, Some(19));
        assert_eq!(usage.completion_tokens, Some(7));
        assert_eq!(usage.reasoning_tokens, Some(2));
        assert_eq!(usage.reported_cost_microusd, None);
    }

    #[test]
    fn missing_usage_is_unknown_and_invalid_usage_is_not_zero() {
        for raw in [
            Value::Null,
            json!({}),
            json!({"input_tokens":null,"output_tokens":null}),
        ] {
            let mut value = response();
            value["usage"] = raw;
            let (_, usage) = parse_response(&value, MODEL, false).expect("unknown usage retained");
            if let Some(usage) = usage {
                assert_eq!(usage.prompt_tokens, None);
                assert_eq!(usage.completion_tokens, None);
                assert_eq!(usage.reasoning_tokens, None);
            }
        }
        let mut value = response();
        value.as_object_mut().expect("object").remove("usage");
        assert!(
            parse_response(&value, MODEL, false)
                .expect("missing usage")
                .1
                .is_none()
        );
        for raw in [
            json!(false),
            json!({"input_tokens":-1}),
            json!({"output_tokens":1.5}),
            json!({"output_tokens_details":[]}),
            json!({"output_tokens_details":{"reasoning_tokens":"2"}}),
            json!({"output_tokens":1,"output_tokens_details":{"reasoning_tokens":2}}),
        ] {
            value["usage"] = raw;
            assert!(parse_response(&value, MODEL, false).is_err());
        }
    }

    #[test]
    fn statuses_model_errors_and_empty_output_fail_closed() {
        for status in [
            Value::Null,
            json!("queued"),
            json!("in_progress"),
            json!("failed"),
            json!("cancelled"),
            json!("incomplete"),
            json!({}),
        ] {
            let mut value = response();
            value["status"] = status;
            assert!(parse_response(&value, MODEL, false).is_err());
            assert!(parse_response(&value, MODEL, true).is_err());
        }
        for (key, bad) in [
            ("model", json!("different")),
            ("object", json!("chat.completion")),
            ("error", json!({"message":"failed"})),
            ("incomplete_details", json!({})),
            ("output", json!([])),
            ("output", Value::Null),
        ] {
            let mut value = response();
            value[key] = bad;
            assert!(parse_response(&value, MODEL, false).is_err(), "field {key}");
        }
        let mut value = response();
        value["output"][0]["content"][0]["text"] = json!(" \n");
        assert!(parse_response(&value, MODEL, true).is_err());
    }

    #[test]
    fn only_cap_check_accepts_cap_induced_incomplete_even_without_text() {
        let mut value = response();
        value["status"] = json!("incomplete");
        value["incomplete_details"] = json!({"reason":"max_output_tokens"});
        for output in [
            json!([]),
            json!([{"type":"message","role":"assistant","status":"incomplete","content":[]}]),
            json!([{"type":"message","role":"assistant","status":"incomplete","content":[{"type":"output_text","text":""}]}]),
        ] {
            value["output"] = output;
            assert!(parse_response(&value, MODEL, false).is_err());
            let (text, usage) =
                parse_response(&value, MODEL, true).expect("cap truncation evidence");
            assert_eq!(text, "[cap-check incomplete: max_output_tokens]");
            assert_eq!(
                usage
                    .expect("usage survives empty output")
                    .completion_tokens,
                Some(7)
            );
        }
        for details in [
            Value::Null,
            json!({}),
            json!({"reason":"content_filter"}),
            json!({"reason":"max_output_tokens","extra":true}),
        ] {
            value["incomplete_details"] = details;
            assert!(parse_response(&value, MODEL, true).is_err());
        }
    }

    #[test]
    fn tools_reasoning_refusal_nontext_and_bad_message_shapes_are_rejected() {
        for item in [
            json!({"type":"function_call"}),
            json!({"type":"reasoning","summary":[]}),
            json!({"type":"web_search_call"}),
            Value::Null,
        ] {
            let mut value = response();
            value["output"].as_array_mut().expect("output").push(item);
            assert!(parse_response(&value, MODEL, false).is_err());
        }
        for part in [
            json!({"type":"refusal","refusal":"No"}),
            json!({"type":"output_image"}),
            json!({"type":"output_text","text":null}),
            Value::Null,
        ] {
            let mut value = response();
            value["output"][0]["content"]
                .as_array_mut()
                .expect("content")
                .push(part);
            assert!(parse_response(&value, MODEL, false).is_err());
        }
        for (key, bad) in [
            ("status", json!("incomplete")),
            ("status", Value::Null),
            ("role", json!("tool")),
            ("content", Value::Null),
        ] {
            let mut value = response();
            value["output"][0][key] = bad;
            assert!(parse_response(&value, MODEL, false).is_err());
        }
    }
}
