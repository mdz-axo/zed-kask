//! Settings API routes — read/write REPL inference settings via REST.
//!
//! Same settings as the `/repl` slash command and `kask settings` CLI.
//! Persisted to ~/.config/hkask/settings.json. Magna Carta P3 (Generative
//! Space): all settings exposed equally across every surface.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::ApiState;

/// JSON shape for the settings response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SettingsResponse {
    pub tool_loop_limit: usize,
    pub context_turns: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub min_p: f32,
    pub typical_p: f32,
    pub max_tokens: u32,
    pub seed: Option<u32>,
    pub gas_heuristic: u64,
    pub gas_cap: u64,
    #[serde(alias = "auto_compact")]
    pub auto_condense: bool,
    /// Disable thinking/reasoning tokens for inference calls.
    pub disable_thinking: bool,
    pub context_length: Option<u32>,
    pub supports_thinking: Option<bool>,
}

impl Default for SettingsResponse {
    fn default() -> Self {
        Self {
            tool_loop_limit: 21,
            context_turns: 3,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            max_tokens: 512,
            seed: None,
            gas_heuristic: 500,
            gas_cap: 10_000,
            auto_condense: true,
            disable_thinking: false,
            context_length: None,
            supports_thinking: None,
        }
    }
}

/// Load settings from disk via the shared service.
fn load_settings() -> SettingsResponse {
    hkask_services_core::load_settings()
}

/// Save settings to disk via the shared service.
fn save_settings(settings: &SettingsResponse) -> Result<(), ApiError> {
    hkask_services_core::save_settings(settings).map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })
}

/// JSON shape for updating settings. All fields optional — only present
/// fields are applied; omitted fields keep their current value.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    pub tool_loop_limit: Option<usize>,
    pub context_turns: Option<usize>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub typical_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<Option<u32>>,
    pub gas_heuristic: Option<u64>,
    pub gas_cap: Option<u64>,
    #[serde(alias = "auto_compact")]
    pub auto_condense: Option<bool>,
}

/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with settings route registered
pub fn settings_router() -> OpenApiRouter<ApiState> {
    use utoipa_axum::routes;
    OpenApiRouter::new().routes(routes!(get_settings, update_settings))
}

/// GET /api/settings — return current settings.
#[utoipa::path(
    get,
    path = "/api/settings",
    tag = "settings",
    responses(
        (status = 200, description = "Current settings", body = SettingsResponse),
    ),
)]
async fn get_settings(
    State(_state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Json<SettingsResponse> {
    Json(load_settings())
}

/// PUT /api/settings — update settings, merge with current values.
#[utoipa::path(
    put,
    path = "/api/settings",
    tag = "settings",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "Settings updated", body = SettingsResponse),
        (status = 500, description = "Failed to persist settings"),
    ),
)]
async fn update_settings(
    State(_state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<UpdateSettingsRequest>,
) -> impl IntoResponse {
    let _ = auth;
    let mut settings = load_settings();

    if let Some(v) = req.tool_loop_limit
        && v > 0
    {
        settings.tool_loop_limit = v;
    }
    if let Some(v) = req.context_turns {
        settings.context_turns = v;
    }
    if let Some(v) = req.temperature
        && (0.0..=2.0).contains(&v)
    {
        settings.temperature = v;
    }
    if let Some(v) = req.top_p
        && (0.0..=1.0).contains(&v)
    {
        settings.top_p = v;
    }
    if let Some(v) = req.top_k
        && v >= 1
    {
        settings.top_k = v;
    }
    if let Some(v) = req.min_p
        && (0.0..=1.0).contains(&v)
    {
        settings.min_p = v;
    }
    if let Some(v) = req.typical_p
        && (0.0..=1.0).contains(&v)
    {
        settings.typical_p = v;
    }
    if let Some(v) = req.max_tokens
        && v > 0
    {
        settings.max_tokens = v;
    }
    if let Some(v) = req.seed {
        settings.seed = v;
    }
    if let Some(v) = req.gas_heuristic
        && v > 0
    {
        settings.gas_heuristic = v;
    }
    if let Some(v) = req.gas_cap
        && v > 0
    {
        settings.gas_cap = v;
    }
    if let Some(v) = req.auto_condense {
        settings.auto_condense = v;
    }

    if let Err(e) = save_settings(&settings) {
        tracing::error!(
            target: "hkask.settings",
            error = %e,
            "Failed to persist settings — changes will be lost on restart"
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Settings saved to memory but could not be persisted: {}", e),
                "settings": settings
            })),
        )
            .into_response();
    }
    Json(settings).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // in the request are changed; all others keep their current values.

    #[test]
    fn update_settings_merge_preserves_unspecified_fields() {
        // Start with defaults
        let mut settings = SettingsResponse::default();

        // Only update temperature
        let req = UpdateSettingsRequest {
            temperature: Some(0.3),
            tool_loop_limit: None,
            context_turns: None,
            top_p: None,
            top_k: None,
            min_p: None,
            typical_p: None,
            max_tokens: None,
            seed: None,
            gas_heuristic: None,
            gas_cap: None,
            auto_condense: None,
        };

        // Apply the merge (same logic as the PUT handler)
        if let Some(v) = req.temperature
            && (0.0..=2.0).contains(&v)
        {
            settings.temperature = v;
        }

        // Temperature should be updated
        assert!((settings.temperature - 0.3).abs() < f32::EPSILON);
        // Unspecified fields should retain defaults
        assert_eq!(settings.tool_loop_limit, 21);
        assert_eq!(settings.context_turns, 3);
        assert!((settings.top_p - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn update_settings_out_of_range_is_ignored() {
        let mut settings = SettingsResponse::default();
        let req = UpdateSettingsRequest {
            temperature: Some(3.0), // out of range
            tool_loop_limit: None,
            context_turns: None,
            top_p: None,
            top_k: None,
            min_p: None,
            typical_p: None,
            max_tokens: None,
            seed: None,
            gas_heuristic: None,
            gas_cap: None,
            auto_condense: None,
        };
        if let Some(v) = req.temperature
            && (0.0..=2.0).contains(&v)
        {
            settings.temperature = v;
        }
        // Out-of-range value should be silently ignored (no change)
        assert!((settings.temperature - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn update_settings_seed_merge() {
        let mut settings = SettingsResponse::default();
        // Default seed is None (random)
        assert!(settings.seed.is_none());

        // Set a specific seed
        let req = UpdateSettingsRequest {
            seed: Some(Some(42)),
            temperature: None,
            tool_loop_limit: None,
            context_turns: None,
            top_p: None,
            top_k: None,
            min_p: None,
            typical_p: None,
            max_tokens: None,
            gas_heuristic: None,
            gas_cap: None,
            auto_condense: None,
        };
        if let Some(v) = req.seed {
            settings.seed = v;
        }
        assert_eq!(settings.seed, Some(42));

        // Reset to random via Some(None)
        let req = UpdateSettingsRequest {
            seed: Some(None),
            temperature: None,
            tool_loop_limit: None,
            context_turns: None,
            top_p: None,
            top_k: None,
            min_p: None,
            typical_p: None,
            max_tokens: None,
            gas_heuristic: None,
            gas_cap: None,
            auto_condense: None,
        };
        if let Some(v) = req.seed {
            settings.seed = v;
        }
        assert_eq!(settings.seed, None);

        // None (outer) means "don't change" — seed stays None
        let req = UpdateSettingsRequest {
            seed: None,
            temperature: None,
            tool_loop_limit: None,
            context_turns: None,
            top_p: None,
            top_k: None,
            min_p: None,
            typical_p: None,
            max_tokens: None,
            gas_heuristic: None,
            gas_cap: None,
            auto_condense: None,
        };
        if let Some(v) = req.seed {
            settings.seed = v;
        }
        assert_eq!(settings.seed, None);
    }

    // ── Pure merge helper (no I/O) for property tests ─────────────────────

    /// Apply an `UpdateSettingsRequest` to a `SettingsResponse` in-place.
    /// Replicates the exact merge logic from `update_settings()` handler.
    fn apply_merge(settings: &mut SettingsResponse, req: &UpdateSettingsRequest) {
        if let Some(v) = req.tool_loop_limit
            && v > 0
        {
            settings.tool_loop_limit = v;
        }
        if let Some(v) = req.context_turns {
            settings.context_turns = v;
        }
        if let Some(v) = req.temperature
            && (0.0..=2.0).contains(&v)
        {
            settings.temperature = v;
        }
        if let Some(v) = req.top_p
            && (0.0..=1.0).contains(&v)
        {
            settings.top_p = v;
        }
        if let Some(v) = req.top_k
            && v >= 1
        {
            settings.top_k = v;
        }
        if let Some(v) = req.min_p
            && (0.0..=1.0).contains(&v)
        {
            settings.min_p = v;
        }
        if let Some(v) = req.typical_p
            && (0.0..=1.0).contains(&v)
        {
            settings.typical_p = v;
        }
        if let Some(v) = req.max_tokens
            && v > 0
        {
            settings.max_tokens = v;
        }
        if let Some(v) = req.seed {
            settings.seed = v;
        }
        if let Some(v) = req.gas_heuristic
            && v > 0
        {
            settings.gas_heuristic = v;
        }
        if let Some(v) = req.gas_cap
            && v > 0
        {
            settings.gas_cap = v;
        }
        if let Some(v) = req.auto_condense {
            settings.auto_condense = v;
        }
    }

    // ── Property tests (proptest) ─────────────────────────────────────────

    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        /// Strategy for an arbitrary `UpdateSettingsRequest`.
        /// Each field is independently Some(valid) or None.
        fn arb_update_request() -> impl Strategy<Value = UpdateSettingsRequest> {
            let opt_usize = prop_oneof![
                1 => Just(None),
                3 => (1usize..1000).prop_map(Some),
            ];
            let opt_f32_range = |lo: f32, hi: f32| {
                prop_oneof![
                    1 => Just(None),
                    3 => (lo..hi).prop_map(Some),
                ]
            };
            let opt_u32_pos = prop_oneof![
                1 => Just(None),
                3 => (1u32..1000).prop_map(Some),
            ];
            let opt_bool = prop_oneof![
                1 => Just(None),
                1 => Just(Some(true)),
                1 => Just(Some(false)),
            ];
            let opt_seed = prop_oneof![
                1 => Just(None),
                1 => Just(Some(None)),
                2 => (0u32..).prop_map(|v| Some(Some(v))),
            ];

            (
                opt_usize.clone(),
                opt_usize.clone(),
                opt_f32_range(0.0, 2.0),
                opt_f32_range(0.0, 1.0),
                opt_u32_pos.clone(),
                opt_f32_range(0.0, 1.0),
                opt_f32_range(0.0, 1.0),
                opt_u32_pos.clone(),
                opt_seed,
                opt_u32_pos.clone().prop_map(|v| v.map(|x| x as u64)),
                opt_u32_pos.prop_map(|v| v.map(|x| x as u64)),
                opt_bool,
            )
                .prop_map(
                    |(
                        tool_loop_limit,
                        context_turns,
                        temperature,
                        top_p,
                        top_k,
                        min_p,
                        typical_p,
                        max_tokens,
                        seed,
                        gas_heuristic,
                        gas_cap,
                        auto_condense,
                    )| UpdateSettingsRequest {
                        tool_loop_limit,
                        context_turns,
                        temperature,
                        top_p,
                        top_k,
                        min_p,
                        typical_p,
                        max_tokens,
                        seed,
                        gas_heuristic,
                        gas_cap,
                        auto_condense,
                    },
                )
        }

        proptest! {
            #[test]
            fn merge_idempotent(
                req in arb_update_request()
            ) {
                let mut s1 = SettingsResponse::default();
                let mut s2 = SettingsResponse::default();

                // Apply once
                apply_merge(&mut s1, &req);
                // Apply twice
                apply_merge(&mut s2, &req);
                apply_merge(&mut s2, &req);

                // Both should be identical
                prop_assert!(settings_eq(&s1, &s2),
                    "merge not idempotent: once={s1:?}, twice={s2:?}");
            }
        }

        proptest! {
            #[test]
            fn unspecified_fields_preserved(
                req in arb_update_request()
            ) {
                let mut settings = SettingsResponse::default();
                let original = SettingsResponse::default();
                apply_merge(&mut settings, &req);

                // Every field that was None in the request should retain default
                if req.tool_loop_limit.is_none() {
                    prop_assert_eq!(settings.tool_loop_limit, original.tool_loop_limit);
                }
                if req.context_turns.is_none() {
                    prop_assert_eq!(settings.context_turns, original.context_turns);
                }
                if req.temperature.is_none() {
                    prop_assert!((settings.temperature - original.temperature).abs() < f32::EPSILON);
                }
                if req.top_p.is_none() {
                    prop_assert!((settings.top_p - original.top_p).abs() < f32::EPSILON);
                }
                if req.top_k.is_none() {
                    prop_assert_eq!(settings.top_k, original.top_k);
                }
                if req.min_p.is_none() {
                    prop_assert!((settings.min_p - original.min_p).abs() < f32::EPSILON);
                }
                if req.typical_p.is_none() {
                    prop_assert!((settings.typical_p - original.typical_p).abs() < f32::EPSILON);
                }
                if req.max_tokens.is_none() {
                    prop_assert_eq!(settings.max_tokens, original.max_tokens);
                }
                if req.seed.is_none() {
                    prop_assert_eq!(settings.seed, original.seed);
                }
                if req.gas_heuristic.is_none() {
                    prop_assert_eq!(settings.gas_heuristic, original.gas_heuristic);
                }
                if req.gas_cap.is_none() {
                    prop_assert_eq!(settings.gas_cap, original.gas_cap);
                }
                if req.auto_condense.is_none() {
                    prop_assert_eq!(settings.auto_condense, original.auto_condense);
                }
            }
        }

        proptest! {
            #[test]
            fn out_of_range_values_ignored(
                temp in (2.0f32..10.0),
                top_p in (1.0f32..10.0),
                min_p in (1.0f32..10.0),
                typical_p in (1.0f32..10.0),
            ) {
                let mut settings = SettingsResponse::default();
                let original = SettingsResponse::default();

                let req = UpdateSettingsRequest {
                    temperature: Some(temp),
                    top_p: Some(top_p),
                    min_p: Some(min_p),
                    typical_p: Some(typical_p),
                    tool_loop_limit: None,
                    context_turns: None,
                    top_k: None,
                    max_tokens: None,
                    seed: None,
                    gas_heuristic: None,
                    gas_cap: None,
                    auto_condense: None,
                };
                apply_merge(&mut settings, &req);

                // All out-of-range values should be ignored — settings unchanged
                prop_assert!(settings_eq(&settings, &original),
                    "out-of-range values should be ignored");
            }
        }

        /// Field-by-field equality for SettingsResponse (f32 uses epsilon).
        fn settings_eq(a: &SettingsResponse, b: &SettingsResponse) -> bool {
            a.tool_loop_limit == b.tool_loop_limit
                && a.context_turns == b.context_turns
                && (a.temperature - b.temperature).abs() < f32::EPSILON
                && (a.top_p - b.top_p).abs() < f32::EPSILON
                && a.top_k == b.top_k
                && (a.min_p - b.min_p).abs() < f32::EPSILON
                && (a.typical_p - b.typical_p).abs() < f32::EPSILON
                && a.max_tokens == b.max_tokens
                && a.seed == b.seed
                && a.gas_heuristic == b.gas_heuristic
                && a.gas_cap == b.gas_cap
                && a.auto_condense == b.auto_condense
        }
    }
}
