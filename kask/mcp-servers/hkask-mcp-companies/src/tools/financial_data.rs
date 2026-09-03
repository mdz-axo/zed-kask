//! Financial data tools — profile, quote, statements, metrics, history, search.
use crate::{
    CompaniesServer, fibo, providers,
    types::{
        HistoricalRequest, ResolveSymbolRequest, SearchRequest, SymbolLimitRequest, SymbolRequest,
    },
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "company_profile", async {
            validate_symbol(&symbol)?;
            let result = self.fetch("company_profile", &symbol, &[]).await?;
            Ok(fibo::enrich_with_ontology(result, "company_profile"))
        })
        .await
    }

    #[tool(description = "Get stock quote")]
    pub async fn stock_quote(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "stock_quote", async {
            validate_symbol(&symbol)?;
            let result = self.fetch("stock_quote", &symbol, &[]).await?;
            Ok(fibo::enrich_with_ontology(result, "stock_quote"))
        })
        .await
    }

    #[tool(description = "Get income statement")]
    pub async fn income_statement(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "income_statement", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            self.fetch("income_statement", &symbol, &[("limit", &limit_str)])
                .await
        })
        .await
    }

    #[tool(description = "Get balance sheet")]
    pub async fn balance_sheet(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "balance_sheet", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            self.fetch("balance_sheet", &symbol, &[("limit", &limit_str)])
                .await
        })
        .await
    }

    #[tool(description = "Get cash flow statement")]
    pub async fn cash_flow_statement(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "cash_flow_statement", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            self.fetch("cash_flow_statement", &symbol, &[("limit", &limit_str)])
                .await
        })
        .await
    }

    #[tool(description = "Get key metrics")]
    pub async fn key_metrics(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "key_metrics", async {
            validate_symbol(&symbol)?;
            let limit_str = limit.unwrap_or(5).to_string();
            let result = self
                .fetch("key_metrics", &symbol, &[("limit", &limit_str)])
                .await?;
            Ok(fibo::enrich_with_ontology(result, "key_metrics"))
        })
        .await
    }

    #[tool(description = "Get historical price data")]
    pub async fn historical_price(
        &self,
        Parameters(HistoricalRequest { symbol, from, to }): Parameters<HistoricalRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "historical_price", async {
            validate_symbol(&symbol)?;
            let result = self
                .fetch("historical_price", &symbol, &[("from", &from), ("to", &to)])
                .await?;
            Ok(fibo::enrich_with_ontology(result, "historical_price"))
        })
        .await
    }

    #[tool(description = "Search for symbols")]
    pub async fn symbol_search(
        &self,
        Parameters(SearchRequest { query, limit }): Parameters<SearchRequest>,
    ) -> Result<String, McpToolError> {
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
        })
        .await
    }

    #[tool(
        description = "Resolve a company name and/or ticker to its primary exchange symbol. Pass the company name from the prompt (e.g. 'Capital One Financial Corp') and the ticker if the prompt gives one (e.g. 'COF'); optionally pass the exchange (e.g. 'NASDAQ', 'LSE', 'Toronto') or the country of domicile (e.g. 'US', 'Canada') to disambiguate the same ticker listed on several exchanges. An explicit exchange or country narrows the candidates; then the first exact ticker match, company-name match, or primary listing wins. Returns the symbol in {CODE}.{EXCHANGE} format, the company name, and whether it's a US listing (FMP primary) or international (EODHD primary). Use this before calling company_profile, income_statement, etc. when you only have the company name. A ticker that already includes an exchange suffix (e.g. VOD.LSE) is returned as-is."
    )]
    pub async fn resolve_symbol(
        &self,
        Parameters(ResolveSymbolRequest {
            company_name,
            ticker,
            exchange,
            country,
        }): Parameters<ResolveSymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "resolve_symbol", async {
            let company_name = non_empty(company_name);
            let ticker = non_empty(ticker);
            let exchange = non_empty(exchange);
            let country = non_empty(country);
            if company_name.is_none() && ticker.is_none() {
                return Err(McpToolError::invalid_argument(
                    "resolve_symbol requires the company name and/or the ticker; \
                         exchange and country are optional disambiguators",
                ));
            }
            let input = providers::ResolveSymbolInput {
                company_name,
                ticker,
                exchange,
                country,
            };
            let resolved =
                providers::resolve_symbol(&self.client, &input, &self.eodhd_api_key).await?;

            Ok(serde_json::json!({
                "query": {
                    "companyName": input.company_name,
                    "ticker": input.ticker,
                    "exchange": input.exchange,
                    "country": input.country,
                },
                "symbol": resolved.symbol,
                "companyName": resolved.company_name,
                "isUS": resolved.is_us,
                "primaryProvider": if resolved.is_us { "FMP" } else { "EODHD" },
                "framework": "Multi-signal EODHD symbol resolution: narrows to the \
                               given exchange/country, then prefers exact ticker match, \
                               company-name match, primary listing."
            }))
        })
        .await
    }
}

/// Trimmed input, or `None` when empty or blank.
fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
