//! Live smoke test for the DBnomics provider module.
//!
//! DBnomics is fully anonymous (no API key), so this test hits the live API.
//! It is `#[ignore]` by default to keep the test suite hermetic; run with
//! `cargo test -p hkask-mcp-prediction-markets --test dbnomics_smoke -- --ignored`.

use hkask_mcp_prediction_markets::economic_data::EconomicDataClient;
use hkask_mcp_prediction_markets::economic_data::dbnomics::{
    DbnomicsGetDatasetRequest, DbnomicsGetSeriesRequest, DbnomicsListProvidersRequest,
    DbnomicsSearchRequest, get_dataset, get_series, list_providers, search,
};

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client builds")
}

#[tokio::test]
#[ignore = "hits the live DBnomics API (network)"]
async fn search_gdp_returns_results() {
    let http = http_client();
    let client = EconomicDataClient::new(&http);
    let request = DbnomicsSearchRequest {
        query: "GDP".to_string(),
        limit: Some(5),
        offset: None,
    };
    let result = search(&client, &request).await.expect("search succeeds");
    let num_found = result
        .get("num_found")
        .and_then(|value| value.as_u64())
        .expect("num_found present");
    assert!(num_found > 0, "DBnomics should have GDP series");
    let results = result
        .get("results")
        .and_then(|value| value.as_array())
        .expect("results array present");
    assert!(!results.is_empty(), "search should return results");
    let first = &results[0];
    assert!(
        first
            .get("provider_code")
            .and_then(|value| value.as_str())
            .map(|code| !code.is_empty())
            .unwrap_or(false),
        "first result should have a non-empty provider_code"
    );
}

#[tokio::test]
#[ignore = "hits the live DBnomics API (network)"]
async fn list_providers_returns_imf() {
    let http = http_client();
    let client = EconomicDataClient::new(&http);
    let request = DbnomicsListProvidersRequest {
        limit: Some(50),
        offset: None,
    };
    let result = list_providers(&client, &request)
        .await
        .expect("list succeeds");
    let num_found = result
        .get("num_found")
        .and_then(|value| value.as_u64())
        .expect("num_found present");
    assert!(num_found > 0, "DBnomics should list providers");
    let providers = result
        .get("providers")
        .and_then(|value| value.as_array())
        .expect("providers array present");
    assert!(!providers.is_empty(), "should return providers");
    let codes: Vec<&str> = providers
        .iter()
        .filter_map(|provider| provider.get("code").and_then(|value| value.as_str()))
        .collect();
    assert!(
        codes.iter().any(|code| code.eq_ignore_ascii_case("IMF")),
        "IMF should be in the provider list: {codes:?}"
    );
}

#[tokio::test]
#[ignore = "hits the live DBnomics API (network)"]
async fn get_dataset_imf_weo_returns_metadata() {
    let http = http_client();
    let client = EconomicDataClient::new(&http);
    let request = DbnomicsGetDatasetRequest {
        provider_code: "IMF".to_string(),
        dataset_code: "WEO".to_string(),
    };
    let result = get_dataset(&client, &request)
        .await
        .expect("dataset fetch succeeds");
    let name = result
        .get("name")
        .and_then(|value| value.as_str())
        .expect("dataset name present");
    assert!(!name.is_empty(), "WEO dataset should have a name");
}

#[tokio::test]
#[ignore = "hits the live DBnomics API (network)"]
async fn get_series_returns_observations() {
    let http = http_client();
    let client = EconomicDataClient::new(&http);
    let request = DbnomicsGetSeriesRequest {
        provider_code: "IMF".to_string(),
        dataset_code: "WEO".to_string(),
        series_code: "NGDP".to_string(),
        observations: Some(true),
        limit: Some(10),
    };
    let result = get_series(&client, &request)
        .await
        .expect("series fetch succeeds");
    let observations = result
        .get("observations")
        .and_then(|value| value.as_array())
        .expect("observations array present");
    assert!(
        !observations.is_empty(),
        "NGDP series should have observations"
    );
    let first = &observations[0];
    assert!(
        first.get("period").is_some(),
        "first observation should have a period"
    );
    assert!(
        first.get("value").is_some(),
        "first observation should have a value"
    );
}
