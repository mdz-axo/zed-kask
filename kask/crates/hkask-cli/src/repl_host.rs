//! CliHost — implements hkask_repl::ReplHost by delegating to CLI subsystems.

use hkask_repl::host::{OnboardingError, OnboardingOutcome, ReplHost};

/// Host implementation bridging the REPL crate to CLI subsystems.
pub struct CliHost;

impl ReplHost for CliHost {
    fn resolve_user_webid(&self) -> hkask_types::WebID {
        crate::commands::helpers::resolve_user_webid()
    }

    fn run_onboarding(
        &self,
        rt: &tokio::runtime::Handle,
    ) -> Result<OnboardingOutcome, OnboardingError> {
        match rt.block_on(crate::onboarding::run_onboarding()) {
            Ok(outcome) => Ok(outcome),
            Err(crate::onboarding::OnboardingError::Cancelled) => Err(OnboardingError::Cancelled),
            Err(e) => Err(OnboardingError::Failed(e.to_string())),
        }
    }

    fn list_templates_local(&self) -> Vec<hkask_types::RegistryEntry> {
        crate::commands::helpers::list_templates_local()
    }

    #[cfg(feature = "tui")]
    fn open_transcript_viewer(&self, path: &std::path::Path) -> anyhow::Result<()> {
        crate::transcript_viewer::TranscriptViewer::from_file(path).and_then(|mut v| v.run())
    }

    fn run_sovereignty_status(&self) {
        // Inline sovereignty status — the CLI no longer exposes this as a
        // command (it's admin-only now), but the REPL still needs it.
        let webid = crate::commands::helpers::resolve_user_webid();
        let svc = crate::commands::helpers::build_agent_service();
        let cm = svc.governance().consent.clone();
        let boundary = hkask_types::curation::DataSovereigntyBoundary::load_or_default();

        println!("Sovereignty Status");
        println!("==================");
        println!();
        println!("Consent State:");
        println!("  WebID: {}", webid);

        let categories = hkask_types::DataCategory::all_known();
        for cat in categories {
            match cm.has_consent(&webid.to_string(), cat) {
                Ok(true) => println!("  • {}: GRANTED", cat.as_str()),
                _ => println!("  • {}: DENIED", cat.as_str()),
            }
        }
        println!();
        println!("Data Boundaries:");
        if boundary.sovereign_data.is_empty()
            && boundary.shared_data.is_empty()
            && boundary.public_data.is_empty()
        {
            println!("  • No boundary data stored yet");
        } else {
            if !boundary.sovereign_data.is_empty() {
                println!(
                    "  • Sovereign: {}",
                    boundary
                        .sovereign_data
                        .iter()
                        .map(|c| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !boundary.shared_data.is_empty() {
                println!(
                    "  • Shared: {}",
                    boundary
                        .shared_data
                        .iter()
                        .map(|c| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !boundary.public_data.is_empty() {
                println!(
                    "  • Public: {}",
                    boundary
                        .public_data
                        .iter()
                        .map(|c| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        println!();
        println!("Affirmative Consent:");
        let store = &svc.storage().sovereignty.clone();
        match store.get(&webid.to_string()) {
            Ok(Some(entry)) => println!(
                "  • Requires Affirmative Consent: {}",
                entry.requires_affirmative_consent
            ),
            Ok(None) => println!(
                "  • Requires Affirmative Consent: {}",
                boundary.requires_affirmative_consent()
            ),
            Err(_) => println!(
                "  • Requires Affirmative Consent: {}",
                boundary.requires_affirmative_consent()
            ),
        }
    }
}
