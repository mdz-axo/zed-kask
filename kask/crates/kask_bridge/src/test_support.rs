//! Shared cfg(test) fixtures for kask_bridge's test modules (R5 batch-2).
//! One canonical copy — test modules import from here instead of
//! re-declaring.

use credentials_provider::CredentialsProvider;

/// A mock `CredentialsProvider` that returns a canned secret for a specific
/// URL and `None` for everything else — the composed env-building and
/// credential-resolution tests run against it instead of the real keychain.
pub(crate) struct MockCredentialsProvider {
    pub(crate) secrets: std::collections::HashMap<String, Vec<u8>>,
}

impl CredentialsProvider for MockCredentialsProvider {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>,
    > {
        let result = self
            .secrets
            .get(url)
            .cloned()
            .map(|pw| ("user".to_string(), pw));
        Box::pin(async move { Ok(result) })
    }

    fn write_credentials<'a>(
        &'a self,
        _url: &'a str,
        _username: &'a str,
        _password: &'a [u8],
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn delete_credentials<'a>(
        &'a self,
        _url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
        Box::pin(async { Ok(()) })
    }
}
