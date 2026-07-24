//! REPL /feedback handler — append a timestamped comment to feedback.md.
//!
//! Writes to `~/.local/share/hkask/feedback.md` (same directory as the REPL
//! history file). Each entry records the UTC timestamp, the active userpod
//! name, and the user's free-text comment.
//!
//! Scope: REPL-only. Not exposed via CLI or API.

use std::io::Write as _;

use crate::ReplState;

/// Handle the `/feedback` slash command.
///
/// Prompts for a comment, then appends one Markdown entry to feedback.md.
/// Prints a confirmation path so the user knows where their feedback landed.
pub fn handle_feedback(state: &ReplState) {
    println!();
    println!("  \x1b[1mSubmit feedback\x1b[0m");
    println!("  Your comment is appended to a local file — nothing is sent anywhere.");
    println!("  Type your feedback and press Enter (empty line cancels):");
    println!();
    print!("  > ");
    let _ = std::io::stdout().flush();

    let mut comment = String::new();
    if std::io::stdin().read_line(&mut comment).is_err() {
        println!("  \x1b[31mCould not read input.\x1b[0m");
        println!();
        return;
    }

    let comment = comment.trim();
    if comment.is_empty() {
        println!("  \x1b[2mFeedback cancelled.\x1b[0m");
        println!();
        return;
    }

    let path = feedback_path();

    match append_feedback(&path, &state.current_agent, comment) {
        Ok(()) => {
            println!("  \x1b[32m✓\x1b[0m Feedback recorded — {}", path.display());
        }
        Err(e) => {
            println!("  \x1b[31m✗\x1b[0m Could not write feedback: {}", e);
        }
    }
    println!();
}

/// Returns the path to the feedback file, creating its parent directory if needed.
fn feedback_path() -> std::path::PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("hkask");
    let _ = std::fs::create_dir_all(&path);
    path.push("feedback.md");
    path
}

/// Append one Markdown entry to the feedback file.
fn append_feedback(
    path: &std::path::Path,
    userpod: &str,
    comment: &str,
) -> Result<(), std::io::Error> {
    // Initialize the file with a header on first write.
    let is_new = !path.exists();

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    if is_new {
        writeln!(
            file,
            "# hKask Feedback Ledger\n\
             \n\
             User-submitted onboarding and usability notes.\n\
             Each entry: UTC timestamp — userpod — free-text comment.\n"
        )?;
    }

    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

    writeln!(
        file,
        "## {timestamp} — {userpod}\n\
         \n\
         > {comment}\n\
         \n\
         ---\n"
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a unique temp path for a test. Uses the test name as a suffix
    /// so parallel test runs don't collide. Caller is responsible for cleanup.
    fn tmp_path(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hkask_feedback_test_{}.md", tag));
        p
    }

    // standard header ("# hKask Feedback Ledger") before the first entry.
    #[test]
    fn append_feedback_creates_file_with_header_on_first_write() {
        let path = tmp_path("header");
        let _ = std::fs::remove_file(&path); // ensure clean state

        append_feedback(&path, "Curator", "onboarding was confusing").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents.starts_with("# hKask Feedback Ledger"),
            "file must start with the standard header"
        );
        let _ = std::fs::remove_file(&path);
    }

    // the header appears exactly once even after multiple writes.
    #[test]
    fn append_feedback_omits_header_on_subsequent_writes() {
        let path = tmp_path("no_dup_header");
        let _ = std::fs::remove_file(&path);

        append_feedback(&path, "Curator", "first note").unwrap();
        append_feedback(&path, "Curator", "second note").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let header = "# hKask Feedback Ledger";
        let count = contents.matches(header).count();
        assert_eq!(count, 1, "header must appear exactly once");
        let _ = std::fs::remove_file(&path);
    }

    // and the verbatim comment text in Markdown blockquote form.
    #[test]
    fn append_feedback_entry_contains_userpod_and_comment() {
        let path = tmp_path("entry_content");
        let _ = std::fs::remove_file(&path);

        append_feedback(&path, "Atlas", "model list was hard to read").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("Atlas"),
            "entry must include the userpod name"
        );
        assert!(
            contents.contains("> model list was hard to read"),
            "entry must include the comment as a blockquote"
        );
        let _ = std::fs::remove_file(&path);
    }
}
