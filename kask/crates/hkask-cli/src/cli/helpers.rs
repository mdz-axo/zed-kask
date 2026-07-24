//! CLI helper utilities

use hkask_types::template_type::TemplateType as Type;

/// Parse a template type string into a TemplateType enum
///
/// pre:  type_str is any string
/// post: returns Some(Type) if type_str matches a known template type
/// post: returns None if type_str is unrecognized
pub fn parse_template_type(type_str: &str) -> Option<Type> {
    Type::parse_str(type_str)
}

/// Initialize tracing subscriber with optional verbose and JSON logging.
///
/// pre:  verbose and json_logs are boolean flags
/// post: if verbose → EnvFilter::new("debug")
/// post: if not verbose → EnvFilter::from_default_env()
/// post: if json_logs → subscriber uses JSON format
/// post: global tracing subscriber is set (panics if already set)
pub fn init_logging(verbose: bool, json_logs: bool) {
    use tracing_subscriber::{EnvFilter, FmtSubscriber};

    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::from_default_env()
    };
    let subscriber = FmtSubscriber::builder().with_env_filter(filter);
    if json_logs {
        tracing::subscriber::set_global_default(subscriber.json().finish())
            .expect("setting default subscriber failed");
    } else {
        tracing::subscriber::set_global_default(subscriber.finish())
            .expect("setting default subscriber failed");
    }
}
