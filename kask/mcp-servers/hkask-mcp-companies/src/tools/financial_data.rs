//! Financial data tools — profile, quote, statements, metrics, history, search.
use crate::{
    CompaniesServer, providers,
    types::{HistoricalRequest, SearchRequest, SymbolLimitRequest, SymbolRequest},
    validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = financial_data_router, vis = "pub")]
impl CompaniesServer {
    #[tool(description = "Get company profile")]
    pub async fn company_profile(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> String {
        execute_tool(self, "company_profile", async {
            validate_symbol(&symbol)?;
            let result = self.fetch("company_profile", &symbol, &[]).await;
            self.record_fetch_outcome("company_profile", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Get stock quote")]
    pub async fn stock_quote(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> String {
        execute_tool(self, "stock_quote", async {
            validate_symbol(&symbol)?;
            let result = self.fetch("stock_quote", &symbol, &[]).await;
            self.record_fetch_outcome("stock_quote", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Get income statement")]
    pub async fn income_statement(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> String {
        execute_tool(self, "income_statement", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            let result = self
                .fetch("income_statement", &symbol, &[("limit", &limit_str)])
                .await;
            self.record_fetch_outcome("income_statement", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Get balance sheet")]
    pub async fn balance_sheet(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> String {
        execute_tool(self, "balance_sheet", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            let result = self
                .fetch("balance_sheet", &symbol, &[("limit", &limit_str)])
                .await;
            self.record_fetch_outcome("balance_sheet", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Get cash flow statement")]
    pub async fn cash_flow_statement(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> String {
        execute_tool(self, "cash_flow_statement", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            let result = self
                .fetch("cash_flow_statement", &symbol, &[("limit", &limit_str)])
                .await;
            self.record_fetch_outcome("cash_flow_statement", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Get key metrics")]
    pub async fn key_metrics(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> String {
        execute_tool(self, "key_metrics", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            let result = self
                .fetch("key_metrics", &symbol, &[("limit", &limit_str)])
                .await;
            self.record_fetch_outcome("key_metrics", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Get historical price data")]
    pub async fn historical_price(
        &self,
        Parameters(HistoricalRequest { symbol, from, to }): Parameters<HistoricalRequest>,
    ) -> String {
        execute_tool(self, "historical_price", async {
            validate_symbol(&symbol)?;
            let result = self
                .fetch("historical_price", &symbol, &[("from", &from), ("to", &to)])
                .await;
            self.record_fetch_outcome("historical_price", &symbol, &result);
            result
        })
        .await
    }

    #[tool(description = "Search for symbols")]
    pub async fn symbol_search(
        &self,
        Parameters(SearchRequest { query, limit }): Parameters<SearchRequest>,
    ) -> String {
        execute_tool(self, "symbol_search", async {
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
                Ok(v) => {
                    self.record_experience(
                        "symbol_search",
                        &format!("query={}, provider=fmp", query),
                        "success",
                        v.clone(),
                    );
                    Ok(v)
                }
                Err(_fmp_err) => {
                    let eodhd_result = providers::eodhd_search_get(
                        &self.client,
                        &query,
                        &limit_str,
                        &self.eodhd_api_key,
                    )
                    .await;
                    match &eodhd_result {
                        Ok(v) => {
                            self.record_experience(
                                "symbol_search",
                                &format!("query={}, provider=eodhd", query),
                                "success",
                                v.clone(),
                            );
                        }
                        Err(e) => {
                            self.record_experience(
                                "symbol_search",
                                &format!("query={}", query),
                                "error",
                                serde_json::json!({"error": e.to_json_string()}),
                            );
                        }
                    }
                    eodhd_result
                }
            }
        })
        .await
    }
}
