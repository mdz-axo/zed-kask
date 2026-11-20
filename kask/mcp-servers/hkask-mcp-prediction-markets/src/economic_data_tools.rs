//! Economic-data and EQM tool methods for `PredictionMarketsServer`.
//!
//! These are the 14 economic-data tool wrappers (FRED, World Bank, DBnomics),
//! the EQM rationale-scoring tool, and the MCDA weight-and-score tool. Each
//! is a thin forwarder to the `economic_data` provider modules, `eqm`, or
//! `mcda` — the server struct holds the state (HTTP client, FRED key,
//! inference port), the provider modules hold the implementation.
//! Extracted from the lib root to shrink the god-file.

use super::economic_data::dbnomics::{
    DbnomicsGetDatasetRequest, DbnomicsGetSeriesRequest, DbnomicsListProvidersRequest,
    DbnomicsSearchRequest,
};
use super::economic_data::fred::{
    FredGetObservationsRequest, FredGetReleaseRequest, FredGetSeriesInfoRequest,
    FredListCategoriesRequest, FredSearchSeriesRequest,
};
use super::economic_data::worldbank::{
    WbGetIndicatorInfoRequest, WbGetObservationsRequest, WbListCountriesRequest,
    WbListTopicsRequest, WbSearchIndicatorsRequest,
};
use super::economic_data::{EconomicDataClient, dbnomics, fred, worldbank};
use super::eqm;
use super::eqm::ScoreRationaleRequest;
use super::mcda;
use super::mcda::WeightScoreRequest;
use super::{McpToolError, PredictionMarketsServer, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};

// `#[tool_router]` generates `economic_data_tools_router()`, merged into the
// server's `combined_router()` in the lib root. Without this attribute the
// `#[tool]` methods below compile (they emit `*_tool_attr` associated fns) but
// are never added to any router, so they vanish from the MCP tool list silently.
#[tool_router(router = economic_data_tools_router, vis = "pub")]
impl PredictionMarketsServer {
    // ═══════════════════ FRED economic data tools ═══════════════════

    /// Search FRED (Federal Reserve Economic Data) series by text.
    /// Returns series IDs with metadata (title, units, frequency, popularity).
    /// Use to discover economic time series for analysis, forecasting, or
    /// the data radar.
    #[tool(
        description = "Search FRED economic data series by text. Returns series IDs with title, units, frequency, and popularity. Example: search 'nonfarm payrolls' to find PAYEMS."
    )]
    pub async fn fred_search_series(
        &self,
        Parameters(req): Parameters<FredSearchSeriesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fred_search_series", async {
            self.record_call("fred_search_series");
            let result = fred::search_series(
                &EconomicDataClient::new(&self.http),
                self.fred_api_key.as_deref(),
                &req,
            )
            .await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Fetch time series observations from FRED.
    /// Returns date-value pairs, most recent first. Supports date range
    /// filtering, frequency transformation, and units transformation.
    #[tool(
        description = "Fetch FRED time series observations by series ID. Returns date-value pairs (most recent first). Supports date range, frequency aggregation (d, w, bw, m, q, sa, a) and units transformations (lin, chg, ch1, pch, pc1, pca, cch, cca, log). The response's units field is the transformation applied, not the series' unit of measure (see fred_get_series_info). Example: series_id='PAYEMS' for nonfarm payrolls, series_id='FEDFUNDS' for Fed funds rate."
    )]
    pub async fn fred_get_observations(
        &self,
        Parameters(req): Parameters<FredGetObservationsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fred_get_observations", async {
            self.record_call("fred_get_observations");
            let result = fred::get_observations(
                &EconomicDataClient::new(&self.http),
                self.fred_api_key.as_deref(),
                &req,
            )
            .await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Get metadata for a single FRED series.
    #[tool(
        description = "Get FRED series metadata: title, units, frequency, seasonal adjustment, date range, notes. Example: series_id='CPIAUCSL' for CPI All Items."
    )]
    pub async fn fred_get_series_info(
        &self,
        Parameters(req): Parameters<FredGetSeriesInfoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fred_get_series_info", async {
            self.record_call("fred_get_series_info");
            let result = fred::get_series_info(
                &EconomicDataClient::new(&self.http),
                self.fred_api_key.as_deref(),
                &req,
            )
            .await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Browse the FRED category tree.
    #[tool(
        description = "Browse FRED category tree. Returns child categories for a given parent (default: root). Use to discover economic data by domain (e.g., Employment, Prices, Interest Rates)."
    )]
    pub async fn fred_list_categories(
        &self,
        Parameters(req): Parameters<FredListCategoriesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fred_list_categories", async {
            self.record_call("fred_list_categories");
            let result = fred::list_categories(
                &EconomicDataClient::new(&self.http),
                self.fred_api_key.as_deref(),
                &req,
            )
            .await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Get FRED release metadata and its series list.
    #[tool(
        description = "Get FRED release metadata (name, description, last_updated, next_release) and its series list. Use to track data release schedules. Example: release_id=50 for 'Employment Situation Summary' (the monthly jobs report)."
    )]
    pub async fn fred_get_release(
        &self,
        Parameters(req): Parameters<FredGetReleaseRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fred_get_release", async {
            self.record_call("fred_get_release");
            let result = fred::get_release(
                &EconomicDataClient::new(&self.http),
                self.fred_api_key.as_deref(),
                &req,
            )
            .await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    // ═══════════════════ World Bank economic data tools ═══════════════════

    /// Search World Bank indicators by text.
    /// Returns indicator IDs with name, unit, source, and topics.
    /// The World Bank API covers ~29,500 indicators across 45+ databases
    /// for all countries — the global complement to FRED's US-centric data.
    #[tool(
        description = "Search World Bank indicators by name (global, no API key needed). The World Bank has no text-search endpoint: without topic_id only the first 1,000 of ~29,500 indicators are scanned and the response's note says so; with topic_id (from wb_list_topics) the whole topic is scanned. Returns indicator IDs with name, unit, source, and topics. Example: query='GDP per capita' topic_id=3 (Economy & Growth)."
    )]
    pub async fn wb_search_indicators(
        &self,
        Parameters(req): Parameters<WbSearchIndicatorsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wb_search_indicators", async {
            self.record_call("wb_search_indicators");
            let result =
                worldbank::search_indicators(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Fetch time series observations from the World Bank.
    /// Returns date-value pairs for a country + indicator.
    #[tool(
        description = "Fetch World Bank time series observations by indicator ID and country code. Returns date-value pairs. Example: indicator_id='SP.POP.TOTL' country_code='USA' for US population, indicator_id='NY.GDP.PCAP.PP.KD' country_code='CHN' for China GDP per capita PPP."
    )]
    pub async fn wb_get_observations(
        &self,
        Parameters(req): Parameters<WbGetObservationsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wb_get_observations", async {
            self.record_call("wb_get_observations");
            let result =
                worldbank::get_observations(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// List all countries with ISO codes, regions, and income levels.
    #[tool(
        description = "List World Bank countries with ISO3 codes, regions, income levels, and capital cities. Optional income_group filter: 'hic' (high income), 'mic' (middle income: lower and upper), 'lmc', 'umc', 'lic' (low income). limit applies after filtering; matched reports the total."
    )]
    pub async fn wb_list_countries(
        &self,
        Parameters(req): Parameters<WbListCountriesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wb_list_countries", async {
            self.record_call("wb_list_countries");
            let result =
                worldbank::list_countries(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Browse the World Bank topic tree.
    #[tool(
        description = "Browse World Bank topics (e.g., Poverty, Education, Health, Trade, Climate Change). Returns topic IDs and names for use with wb_search_indicators topic_id filter."
    )]
    pub async fn wb_list_topics(
        &self,
        Parameters(req): Parameters<WbListTopicsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wb_list_topics", async {
            self.record_call("wb_list_topics");
            let result = worldbank::list_topics(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Get metadata for a single World Bank indicator.
    #[tool(
        description = "Get World Bank indicator metadata: name, unit, source, description, source organization, and topics. Example: indicator_id='SP.POP.TOTL' for total population."
    )]
    pub async fn wb_get_indicator_info(
        &self,
        Parameters(req): Parameters<WbGetIndicatorInfoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wb_get_indicator_info", async {
            self.record_call("wb_get_indicator_info");
            let result =
                worldbank::get_indicator_info(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    // ═══════════════════ DBnomics economic data tools ═══════════════════

    /// Search DBnomics series by full-text query across all providers.
    /// DBnomics aggregates 1.7B+ series from 700+ providers (IMF, OECD, ECB,
    /// INSEE, World Bank, FRED mirrors, etc.) — the global superset of FRED
    /// and the World Bank Indicators API. No API key required.
    #[tool(
        description = "Search DBnomics by full-text query across all providers (IMF, OECD, ECB, INSEE, World Bank, FRED mirrors, etc.; 1.7B+ series, no API key needed). Returns the DATASETS containing matching series (provider_code, dataset_code, dataset_name, nb_matching_series) — pass those codes to dbnomics_get_dataset. Example: query='GDP'."
    )]
    pub async fn dbnomics_search(
        &self,
        Parameters(req): Parameters<DbnomicsSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "dbnomics_search", async {
            self.record_call("dbnomics_search");
            let result = dbnomics::search(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// List DBnomics statistical providers (IMF, OECD, ECB, INSEE, etc.).
    #[tool(
        description = "List DBnomics statistical providers (700+ institutions: IMF, OECD, ECB, INSEE, World Bank, etc.). Returns provider code, name, region, and website. No API key needed."
    )]
    pub async fn dbnomics_list_providers(
        &self,
        Parameters(req): Parameters<DbnomicsListProvidersRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "dbnomics_list_providers", async {
            self.record_call("dbnomics_list_providers");
            let result = dbnomics::list_providers(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Get DBnomics dataset metadata. Supports the `:latest` release alias.
    #[tool(
        description = "Get DBnomics dataset metadata (name, description, dimensions, last update). Supports the `:latest` release alias (e.g., dataset_code='WEO:latest'). Example: provider_code='IMF' dataset_code='WEO:latest' for the latest World Economic Outlook dataset."
    )]
    pub async fn dbnomics_get_dataset(
        &self,
        Parameters(req): Parameters<DbnomicsGetDatasetRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "dbnomics_get_dataset", async {
            self.record_call("dbnomics_get_dataset");
            let result = dbnomics::get_dataset(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    /// Get DBnomics series observation (period + value pairs).
    #[tool(
        description = "Get DBnomics series observations by provider/dataset/series code. Returns series metadata + observations array [{period, value}] (the most recent `limit`; total_observations reports the full count). An unknown series is not_found. Example: provider_code='IMF' dataset_code='WEO:latest' series_code='ABW.BCA.us_dollars' for Aruba's current account balance."
    )]
    pub async fn dbnomics_get_series(
        &self,
        Parameters(req): Parameters<DbnomicsGetSeriesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "dbnomics_get_series", async {
            self.record_call("dbnomics_get_series");
            let result = dbnomics::get_series(&EconomicDataClient::new(&self.http), &req).await;
            result.map_err(McpToolError::from)
        })
        .await
    }

    // ═══════════════════ EQM rationale scoring ═══════════════════

    /// Score a forecast rationale against Explanation Quality Markers (EQMs).
    ///
    /// Based on Karvetski et al. (2026), Forecasting Research Institute.
    /// Scores 12 key reasoning patterns on a 0/1/2 scale using an LLM,
    /// then computes a composite score. EQMs flag weak reasoning more
    /// reliably than they identify excellent reasoning (asymmetric signal).
    #[tool(
        description = "Score a forecast rationale against Explanation Quality Markers (EQMs). Returns composite score, per-marker scores, red flags (warning signs), and green flags (good habits). Based on Karvetski et al. (2026). One decision-model call per rationale; the response's usage block reports the tokens billed."
    )]
    pub async fn market_score_rationale(
        &self,
        Parameters(req): Parameters<ScoreRationaleRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_score_rationale", async {
            self.record_call("market_score_rationale");
            let result = eqm::score_rationale(self.inference_port.as_ref(), &req).await;
            result.map_err(McpToolError::from).and_then(|eqm_result| {
                serde_json::to_value(&eqm_result)
                    .map_err(|e| McpToolError::internal(format!("eqm serialization failed: {e}")))
            })
        })
        .await
    }

    // ═══════════════════ MCDA weight-and-score ═══════════════════

    /// Weight decision criteria and score alternatives via the decision
    /// model — the mcda skill's weight-and-score semantic halves (card
    /// c8a2ffaf). One typed Score question per criterion weight and per
    /// (alternative, criterion) raw score over the fixed 1-10 answer space
    /// (cost criteria pre-inverted: 1 = most costly, 10 = least costly).
    #[tool(
        description = "Weight decision criteria and score alternatives via the decision model (kask.models.decision_model, served over the Decisions API — never a chat endpoint). One typed Score question per criterion weight and per alternative×criterion raw score over the fixed 1-10 answer space (cost criteria pre-inverted: 1 = most costly/worst, 10 = least costly/best). Returns {\"weights\": [{criterion, weight}], \"raw_scores\": [{alternative, criterion, raw_score}]} — the shape the mcda skill's lisp gates consume. An unset decision model is a typed refusal naming the setting."
    )]
    pub async fn mcda_weight_score(
        &self,
        Parameters(req): Parameters<WeightScoreRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "mcda_weight_score", async {
            self.record_call("mcda_weight_score");
            let result = mcda::weight_score(self.inference_port.as_ref(), &req).await;
            result.map_err(McpToolError::from).and_then(|mcda_result| {
                serde_json::to_value(&mcda_result)
                    .map_err(|e| McpToolError::internal(format!("mcda serialization failed: {e}")))
            })
        })
        .await
    }
}
