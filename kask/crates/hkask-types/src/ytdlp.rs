//! Runtime-independent yt-dlp selection and diagnostic policy.

pub const PATH_CANDIDATE: &str = "yt-dlp";
pub const USER_CANDIDATE_SUFFIX: &str = ".local/bin/yt-dlp";
pub const SYSTEM_CANDIDATES: &[&str] = &["/usr/local/bin/yt-dlp", "/usr/bin/yt-dlp"];

/// Parsed dotted yt-dlp version. Malformed components are rejected rather than
/// silently becoming version zero.
pub fn parse_version(output: &str) -> Option<Vec<u64>> {
    let mut version = Vec::new();
    for part in output.trim().split('.') {
        let digits = part
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        if digits.is_empty() {
            return None;
        }
        version.push(digits.parse().ok()?);
    }
    (!version.is_empty()).then_some(version)
}

/// Candidate priority is stable: replace the current candidate only for a
/// strictly newer version; equal versions retain the earlier path.
pub fn candidate_is_preferred(candidate: &[u64], current: &[u64]) -> bool {
    candidate > current
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtDlpIssue {
    MissingJavascriptRuntime,
    ExtractorFailure,
    AuthorizationFailure,
    UnavailableVideo,
    Other,
}

impl YtDlpIssue {
    pub const fn actionable_message(self) -> &'static str {
        match self {
            Self::MissingJavascriptRuntime => {
                "yt-dlp completed without a supported JavaScript runtime; install a supported runtime such as Deno or Node.js because some formats may be missing"
            }
            Self::ExtractorFailure => {
                "yt-dlp could not extract this platform page; update yt-dlp and retry"
            }
            Self::AuthorizationFailure => {
                "yt-dlp was denied access; authenticate outside hKask or choose a public video"
            }
            Self::UnavailableVideo => {
                "the selected video is unavailable, private, removed, or region-restricted"
            }
            Self::Other => "yt-dlp failed; verify the URL and update yt-dlp before retrying",
        }
    }
}

/// Classify stderr without returning signed URLs, cookies, credentials, or the
/// complete subprocess body to callers or logs.
pub fn classify_stderr(stderr: &str) -> Option<YtDlpIssue> {
    let normalized = stderr.to_ascii_lowercase();
    if normalized.trim().is_empty() {
        return None;
    }
    if normalized.contains("no supported javascript runtime")
        || normalized.contains("javascript runtime")
            && normalized.contains("formats may be missing")
    {
        Some(YtDlpIssue::MissingJavascriptRuntime)
    } else if normalized.contains("sign in")
        || normalized.contains("http error 401")
        || normalized.contains("http error 403")
        || normalized.contains("authentication")
        || normalized.contains("cookies")
    {
        Some(YtDlpIssue::AuthorizationFailure)
    } else if normalized.contains("video unavailable")
        || normalized.contains("not available")
        || normalized.contains("private video")
        || normalized.contains("removed")
    {
        Some(YtDlpIssue::UnavailableVideo)
    } else if normalized.contains("unable to extract")
        || normalized.contains("extractor error")
        || normalized.contains("extractorerror")
        || normalized.contains("nsig extraction failed")
    {
        Some(YtDlpIssue::ExtractorFailure)
    } else {
        Some(YtDlpIssue::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_versions_retain_the_first_candidate() {
        let version = parse_version("2026.08.19\n").expect("version parses");
        assert!(!candidate_is_preferred(&version, &version));
        assert!(candidate_is_preferred(&[2026, 8, 20], &version));
        assert!(parse_version("release-candidate").is_none());
    }

    #[test]
    fn diagnostics_are_actionable_and_do_not_echo_stderr() {
        for (stderr, issue) in [
            (
                "WARNING: No supported JavaScript runtime could be found; some formats may be missing",
                YtDlpIssue::MissingJavascriptRuntime,
            ),
            (
                "ERROR: Sign in to confirm your age",
                YtDlpIssue::AuthorizationFailure,
            ),
            ("ERROR: Video unavailable", YtDlpIssue::UnavailableVideo),
            (
                "ERROR: Unable to extract player response",
                YtDlpIssue::ExtractorFailure,
            ),
        ] {
            assert_eq!(classify_stderr(stderr), Some(issue));
            assert!(!issue.actionable_message().contains(stderr));
        }
    }
}
