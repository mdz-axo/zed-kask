//! Economic-data and EQM tool methods for `PredictionMarketsServer`.
//!
//! These are the 14 economic-data tool wrappers (FRED, World Bank, DBnomics)
//! and the EQM rationale-scoring tool. Each is a thin forwarder to the
//! `economic_data` provider modules or `eqm` — the server struct holds the
//! state (HTTP client, FRED key, inference port), the provider modules hold
//! the implementation. Extracted from the lib root to shrink the god-file.

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
use super::{McpToolError, PredictionMarketsServer, execute_tool_semantic};
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
    ) -> String {
        execute_tool_semantic(
            self,
            "fred_search_series",
            Self::ontology_anchor("fred_search_series"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("fred_search_series".to_string());
                let result = fred::search_series(
                    &EconomicDataClient::new(&self.http),
                    self.fred_api_key.as_deref(),
                    &req,
                )
                .await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Fetch time series observations from FRED.
    /// Returns date-value pairs, most recent first. Supports date range
    /// filtering, frequency transformation, and units transformation.
    #[tool(
        description = "Fetch FRED time series observations by series ID. Returns date-value pairs (most recent first). Supports date range, frequency, and units transformations. Example: series_id='PAYEMS' for nonfarm payrolls, series_id='FEDFUNDS' for Fed funds rate."
    )]
    pub async fn fred_get_observations(
        &self,
        Parameters(req): Parameters<FredGetObservationsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "fred_get_observations",
            Self::ontology_anchor("fred_get_observations"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("fred_get_observations".to_string());
                let result = fred::get_observations(
                    &EconomicDataClient::new(&self.http),
                    self.fred_api_key.as_deref(),
                    &req,
                )
                .await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Get metadata for a single FRED series.
    #[tool(
        description = "Get FRED series metadata: title, units, frequency, seasonal adjustment, date range, notes. Example: series_id='CPIAUCSL' for CPI All Items."
    )]
    pub async fn fred_get_series_info(
        &self,
        Parameters(req): Parameters<FredGetSeriesInfoRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "fred_get_series_info",
            Self::ontology_anchor("fred_get_series_info"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("fred_get_series_info".to_string());
                let result = fred::get_series_info(
                    &EconomicDataClient::new(&self.http),
                    self.fred_api_key.as_deref(),
                    &req,
                )
                .await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Browse the FRED category tree.
    #[tool(
        description = "Browse FRED category tree. Returns child categories for a given parent (default: root). Use to discover economic data by domain (e.g., Employment, Prices, Interest Rates)."
    )]
    pub async fn fred_list_categories(
        &self,
        Parameters(req): Parameters<FredListCategoriesRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "fred_list_categories",
            Self::ontology_anchor("fred_list_categories"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("fred_list_categories".to_string());
                let result = fred::list_categories(
                    &EconomicDataClient::new(&self.http),
                    self.fred_api_key.as_deref(),
                    &req,
                )
                .await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Get FRED release metadata and its series list.
    #[tool(
        description = "Get FRED release metadata (name, description, last_updated, next_release) and its series list. Use to track data release schedules. Example: release_id=50 for 'Employment Situation Summary' (the monthly jobs report)."
    )]
    pub async fn fred_get_release(
        &self,
        Parameters(req): Parameters<FredGetReleaseRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "fred_get_release",
            Self::ontology_anchor("fred_get_release"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("fred_get_release".to_string());
                let result = fred::get_release(
                    &EconomicDataClient::new(&self.http),
                    self.fred_api_key.as_deref(),
                    &req,
                )
                .await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    // ═══════════════════ World Bank economic data tools ═══════════════════

    /// Search World Bank indicators by text.
    /// Returns indicator IDs with name, unit, source, and topics.
    /// The World Bank API covers ~29,500 indicators across 45+ databases
    /// for all countries — the global complement to FRED's US-centric data.
    #[tool(
        description = "Search World Bank indicators by text. Returns indicator IDs with name, unit, source, and topics. Covers ~29,500 indicators (global, no API key needed). Example: query='employment' to find labor indicators, query='GDP per capita' for economic indicators."
    )]
    pub async fn wb_search_indicators(
        &self,
        Parameters(req): Parameters<WbSearchIndicatorsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "wb_search_indicators",
            Self::ontology_anchor("wb_search_indicators"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("wb_search_indicators".to_string());
                let result =
                    worldbank::search_indicators(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
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
    ) -> String {
        execute_tool_semantic(
            self,
            "wb_get_observations",
            Self::ontology_anchor("wb_get_observations"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("wb_get_observations".to_string());
                let result =
                    worldbank::get_observations(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// List all countries with ISO codes, regions, and income levels.
    #[tool(
        description = "List World Bank countries with ISO3 codes, regions, income levels, and capital cities. Optional income_group filter: 'hic' (high income), 'mic' (middle income), 'lic' (low income)."
    )]
    pub async fn wb_list_countries(
        &self,
        Parameters(req): Parameters<WbListCountriesRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "wb_list_countries",
            Self::ontology_anchor("wb_list_countries"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("wb_list_countries".to_string());
                let result =
                    worldbank::list_countries(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Browse the World Bank topic tree.
    #[tool(
        description = "Browse World Bank topics (e.g., Poverty, Education, Health, Trade, Climate Change). Returns topic IDs and names for use with wb_search_indicators topic_id filter."
    )]
    pub async fn wb_list_topics(&self, Parameters(req): Parameters<WbListTopicsRequest>) -> String {
        execute_tool_semantic(
            self,
            "wb_list_topics",
            Self::ontology_anchor("wb_list_topics"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("wb_list_topics".to_string());
                let result =
                    worldbank::list_topics(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Get metadata for a single World Bank indicator.
    #[tool(
        description = "Get World Bank indicator metadata: name, unit, source, description, source organization, and topics. Example: indicator_id='SP.POP.TOTL' for total population."
    )]
    pub async fn wb_get_indicator_info(
        &self,
        Parameters(req): Parameters<WbGetIndicatorInfoRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "wb_get_indicator_info",
            Self::ontology_anchor("wb_get_indicator_info"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("wb_get_indicator_info".to_string());
                let result =
                    worldbank::get_indicator_info(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    // ═══════════════════ DBnomics economic data tools ═══════════════════

    /// Search DBnomics series by full-text query across all providers.
    /// DBnomics aggregates 1.7B+ series from 700+ providers (IMF, OECD, ECB,
    /// INSEE, World Bank, FRED mirrors, etc.) — the global superset of FRED
    /// and the World Bank Indicators API. No API key required.
    #[tool(
        description = "Search DBnomics economic time series by full-text query across all providers (IMF, OECD, ECB, INSEE, World Bank, FRED mirrors, etc.). 1.7B+ series, no API key needed. Example: query='GDP' for gross domestic product series."
    )]
    pub async fn dbnomics_search(
        &self,
        Parameters(req): Parameters<DbnomicsSearchRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "dbnomics_search",
            Self::ontology_anchor("dbnomics_search"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("dbnomics_search".to_string());
                let result = dbnomics::search(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// List DBnomics statistical providers (IMF, OECD, ECB, INSEE, etc.).
    #[tool(
        description = "List DBnomics statistical providers (700+ institutions: IMF, OECD, ECB, INSEE, World Bank, etc.). Returns provider code, name, region, and website. No API key needed."
    )]
    pub async fn dbnomics_list_providers(
        &self,
        Parameters(req): Parameters<DbnomicsListProvidersRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "dbnomics_list_providers",
            Self::ontology_anchor("dbnomics_list_providers"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("dbnomics_list_providers".to_string());
                let result =
                    dbnomics::list_providers(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Get DBnomics dataset metadata. Supports the `:latest` release alias.
    #[tool(
        description = "Get DBnomics dataset metadata (name, description, dimensions, last update). Supports the `:latest` release alias (e.g., dataset_code='WEO:latest'). Example: provider_code='IMF' dataset_code='WEO:latest' for the latest World Economic Outlook dataset."
    )]
    pub async fn dbnomics_get_dataset(
        &self,
        Parameters(req): Parameters<DbnomicsGetDatasetRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "dbnomics_get_dataset",
            Self::ontology_anchor("dbnomics_get_dataset"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("dbnomics_get_dataset".to_string());
                let result =
                    dbnomics::get_dataset(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
        .await
    }

    /// Get DBnomics series observation (period + value pairs).
    #[tool(
        description = "Get DBnomics series observations by provider/dataset/series code. Returns series metadata + observations array [{period, value}]. Example: provider_code='IMF' dataset_code='WEO:latest' series_code='NGDP' for nominal GDP."
    )]
    pub async fn dbnomics_get_series(
        &self,
        Parameters(req): Parameters<DbnomicsGetSeriesRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "dbnomics_get_series",
            Self::ontology_anchor("dbnomics_get_series"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("dbnomics_get_series".to_string());
                let result = dbnomics::get_series(&EconomicDataClient::new(&self.http), &req).await;
                result.map_err(McpToolError::from)
            },
        )
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
        description = "Score a forecast rationale against Explanation Quality Markers (EQMs). Returns composite score, per-marker scores, red flags (warning signs), and green flags (good habits). Based on Karvetski et al. (2026). Cost: ~$0.007 per rationale."
    )]
    pub async fn market_score_rationale(
        &self,
        Parameters(req): Parameters<ScoreRationaleRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_score_rationale",
            Self::ontology_anchor("market_score_rationale"),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_score_rationale".to_string());
                let result = eqm::score_rationale(self.inference_port.as_ref(), &req).await;
                result.map_err(McpToolError::from).and_then(|eqm_result| {
                    serde_json::to_value(&eqm_result).map_err(|e| {
                        McpToolError::internal(format!("eqm serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                    })
                })
            },
        )
        .await
    }
}
