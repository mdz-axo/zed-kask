//! Financial data tools — profile, quote, statements, metrics, history, search.
use crate::{
    CompaniesServer, fibo, providers,
    types::{HistoricalRequest, SearchRequest, SymbolLimitRequest, SymbolRequest},
    validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = financial_data_router, vis = "pub")]
impl CompaniesServer {
    #[tool(description = "Get company profile")]
    pub async fn company_profile(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "company_profile",
            Self::ontology_anchor("company_profile"),
            async {
                validate_symbol(&symbol)?;
                let result = self.fetch("company_profile", &symbol, &[]).await?;
                Ok(fibo::enrich_with_ontology(result, "company_profile"))
            },
        )
        .await
    }

    #[tool(description = "Get stock quote")]
    pub async fn stock_quote(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "stock_quote",
            Self::ontology_anchor("stock_quote"),
            async {
                validate_symbol(&symbol)?;
                let result = self.fetch("stock_quote", &symbol, &[]).await?;
                Ok(fibo::enrich_with_ontology(result, "stock_quote"))
            },
        )
        .await
    }

    #[tool(description = "Get income statement")]
    pub async fn income_statement(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "income_statement",
            Self::ontology_anchor("income_statement"),
            async {
                validate_symbol(&symbol)?;
                let limit_str = limit.unwrap_or(5).to_string();
                self.fetch("income_statement", &symbol, &[("limit", &limit_str)])
                    .await
            },
        )
        .await
    }

    #[tool(description = "Get balance sheet")]
    pub async fn balance_sheet(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "balance_sheet",
            Self::ontology_anchor("balance_sheet"),
            async {
                validate_symbol(&symbol)?;
                let limit_str = limit.unwrap_or(5).to_string();
                self.fetch("balance_sheet", &symbol, &[("limit", &limit_str)])
                    .await
            },
        )
        .await
    }

    #[tool(description = "Get cash flow statement")]
    pub async fn cash_flow_statement(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "cash_flow_statement",
            Self::ontology_anchor("cash_flow_statement"),
            async {
                validate_symbol(&symbol)?;
                let limit_str = limit.unwrap_or(5).to_string();
                self.fetch("cash_flow_statement", &symbol, &[("limit", &limit_str)])
                    .await
            },
        )
        .await
    }

    #[tool(description = "Get key metrics")]
    pub async fn key_metrics(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "key_metrics",
            Self::ontology_anchor("key_metrics"),
            async {
                validate_symbol(&symbol)?;
                let limit_str = limit.unwrap_or(5).to_string();
                let result = self
                    .fetch("key_metrics", &symbol, &[("limit", &limit_str)])
                    .await?;
                Ok(fibo::enrich_with_ontology(result, "key_metrics"))
            },
        )
        .await
    }

    #[tool(description = "Get historical price data")]
    pub async fn historical_price(
        &self,
        Parameters(HistoricalRequest { symbol, from, to }): Parameters<HistoricalRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "historical_price",
            Self::ontology_anchor("historical_price"),
            async {
                validate_symbol(&symbol)?;
                let result = self
                    .fetch("historical_price", &symbol, &[("from", &from), ("to", &to)])
                    .await?;
                Ok(fibo::enrich_with_ontology(result, "historical_price"))
            },
        )
        .await
    }

    #[tool(description = "Search for symbols")]
    pub async fn symbol_search(
        &self,
        Parameters(SearchRequest { query, limit }): Parameters<SearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "symbol_search",
            Self::ontology_anchor("symbol_search"),
            async {
                if query.is_empty() {
                    return Err(McpToolError::invalid_argument("query must not be empty"));
                }
                let limit_str = limit.unwrap_or(10).to_string();
                // Search is special: it doesn't use a symbol, it uses a query.
                // Route to FMP first (better US coverage), fall back to EODHD.
                let fmp_result =
                    providers::fmp_search_get(&self.client, &query, &limit_str, &self.fmp_api_key)
                        .await;

                match fmp_result {
                    Ok(v) => Ok(v),
                    Err(_fmp_err) => {
                        providers::eodhd_search_get(
                            &self.client,
                            &query,
                            &limit_str,
                            &self.eodhd_api_key,
                        )
                        .await
                    }
                }
            },
        )
        .await
    }

    #[tool(
        description = "Resolve a company name or plain ticker to its primary exchange symbol. Searches EODHD for the primary common stock listing (isPrimary=true, Type=Common Stock) and returns the symbol in {CODE}.{EXCHANGE} format, the company name, and whether it's a US listing (FMP primary) or international (EODHD primary). Use this before calling company_profile, income_statement, etc. when you only have the company name. If the input already contains a dot (e.g., VOD.LSE), it's returned as-is."
    )]
    pub async fn resolve_symbol(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "resolve_symbol",
            Self::ontology_anchor("symbol_search"),
            async {
                if symbol.is_empty() {
                    return Err(McpToolError::invalid_argument("symbol must not be empty"));
                }
                let resolved = providers::resolve_symbol(
                    &self.client,
                    &symbol,
                    &self.eodhd_api_key,
                )
                .await?;

                Ok(serde_json::json!({
                    "query": symbol,
                    "symbol": resolved.symbol,
                    "companyName": resolved.company_name,
                    "isUS": resolved.is_us,
                    "primaryProvider": if resolved.is_us { "FMP" } else { "EODHD" },
                    "framework": "EODHD symbol resolution. Searches for the primary common stock listing (isPrimary=true) and returns the canonical symbol for data fetching."
                }))
            },
        )
        .await
    }
}
