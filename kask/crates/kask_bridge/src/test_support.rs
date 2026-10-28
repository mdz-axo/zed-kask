//! Shared cfg(test) fixtures for kask_bridge's test modules (R5 batch-2).
//! One canonical copy — test modules import from here instead of
//! re-declaring.

use credentials_provider::CredentialsProvider;

/// A mock `CredentialsProvider` that returns a canned secret for a specific
/// URL and `None` for everything else — the composed env-building and
/// credential-resolution tests run against it instead of the real keychain.
///
/// `failures` injects the failure states a real keychain can produce, so
/// tests can pin the distinguishable classification: `Err` for a locked or
/// unavailable keychain, and a non-UTF-8 byte value for a corrupt entry —
/// the two states the previous `MockCredentialsProvider` could never
/// produce (it always returned `Ok`), leaving them untestable. Construct
/// with `MockCredentialsProvider::new(secrets)` for the old always-Ok shape;
/// add failures via `MockCredentialsProvider::with_failure`.
pub(crate) struct MockCredentialsProvider {
    pub(crate) secrets: std::collections::HashMap<String, Vec<u8>>,
    /// URLs whose reads fail with `Err` (a locked/unavailable keychain).
    pub(crate) failures: std::collections::HashMap<String, String>,
}

impl MockCredentialsProvider {
    /// The always-`Ok` shape tests previously constructed by struct literal —
    /// same behavior, `failures` empty.
    pub(crate) fn new(secrets: std::collections::HashMap<String, Vec<u8>>) -> Self {
        Self {
            secrets,
            failures: std::collections::HashMap::new(),
        }
    }
}

impl CredentialsProvider for MockCredentialsProvider {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a gpui::AsyncApp,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>,
    > {
        if let Some(error) = self.failures.get(url) {
            let error = error.clone();
            return Box::pin(async move { Err(anyhow::anyhow!(error)) });
        }
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
