//! UI helper functions for onboarding prompts.

use std::io::{BufRead, Write};

use super::OnboardingError;

/// Read a line from stdin. Token for reproducing the exact terminal interaction.
pub(crate) fn read_line() -> Result<String, std::io::Error> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line)
}

fn is_cancel_input(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.eq_ignore_ascii_case("exit")
        || trimmed.eq_ignore_ascii_case("quit")
        || trimmed.eq_ignore_ascii_case("/exit")
        || trimmed.eq_ignore_ascii_case("/quit")
}

/// Prompt the user and return their response (trims whitespace).
/// Returns `OnboardingError::Cancelled` if the user types exit, quit, /exit, or /quit.
pub(crate) fn prompt_line(prompt: &str) -> Result<String, OnboardingError> {
    print!("{prompt} ");
    std::io::stdout().flush()?;
    let raw = read_line()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_cancel_input(trimmed) {
        println!("  Onboarding cancelled.");
        return Err(OnboardingError::Cancelled);
    }
    Ok(trimmed.to_string())
}

/// Prompt for a passphrase (no echo).
/// Returns `OnboardingError::Cancelled` on Ctrl+C (which produces a `std::io::Error`
/// with `ErrorKind::Interrupted` from rpassword).
pub(crate) fn prompt_passphrase(prompt: &str) -> Result<String, OnboardingError> {
    print!("{prompt} ");
    std::io::stdout().flush()?;
    match rpassword::read_password() {
        Ok(pw) => Ok(pw),
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Err(OnboardingError::Cancelled),
        Err(e) => Err(OnboardingError::Io(e)),
    }
}

/// Evaluate passphrase strength and return a label + color code.
pub(crate) fn passphrase_strength(pass: &str) -> (&'static str, &'static str) {
    let len = pass.len();
    let has_upper = pass.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = pass.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = pass.chars().any(|c| c.is_ascii_digit());
    let has_special = pass.chars().any(|c| !c.is_alphanumeric());
    let variety = [has_upper, has_lower, has_digit, has_special]
        .iter()
        .filter(|&&x| x)
        .count();

    if len >= 16 && variety >= 3 {
        ("strong", "\x1b[32m") // green
    } else if len >= 12 && variety >= 2 {
        ("good", "\x1b[33m") // yellow
    } else if len >= 8 {
        ("fair", "\x1b[33m") // yellow
    } else {
        ("weak", "\x1b[31m") // red
    }
}

/// Prompt for passphrase with confirmation and strength feedback.
/// Returns `OnboardingError::Cancelled` if the user interrupts with Ctrl+C (which
/// produces a std::io::Error from rpassword).
pub(crate) fn prompt_passphrase_with_confirm() -> Result<String, OnboardingError> {
    loop {
        let pass = prompt_passphrase("  Master passphrase:")?;
        if pass.is_empty() {
            println!("  \x1b[31mPassphrase cannot be empty.\x1b[0m Please try again.\n");
            continue;
        }
        if pass.len() < 8 {
            println!(
                "  \x1b[31mPassphrase must be at least 8 characters.\x1b[0m Please try again.\n"
            );
            continue;
        }
        // Show strength feedback
        let (label, color) = passphrase_strength(&pass);
        println!("  Passphrase strength: {color}{label}\x1b[0m");

        let confirm = prompt_passphrase("  Confirm passphrase:")?;
        if pass == confirm {
            return Ok(pass);
        }
        println!("  \x1b[31mPassphrases don't match.\x1b[0m Please try again.\n");
    }
}

/// Prompt for a numeric choice within a range.
/// Returns `OnboardingError::Cancelled` if the user types exit, quit, /exit, or /quit.
pub(crate) fn prompt_choice(
    prompt: &str,
    range: std::ops::RangeInclusive<usize>,
) -> Result<usize, OnboardingError> {
    loop {
        let input = prompt_line(prompt)?;
        if input.trim().is_empty() {
            // Default to first option on empty input
            return Ok(*range.start());
        }
        match input.parse::<usize>() {
            Ok(n) if range.contains(&n) => return Ok(n),
            _ => println!(
                "  Please enter a number between {} and {}.\n  (or type 'exit' to cancel)",
                range.start(),
                range.end()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::passphrase_strength;

    // regardless of character variety.
    #[test]
    fn passphrase_strength_weak_below_8() {
        assert_eq!(passphrase_strength("Ab1!").0, "weak");
        assert_eq!(passphrase_strength("abcdefg").0, "weak"); // exactly 7
        assert_eq!(passphrase_strength("").0, "weak");
    }

    // letters) is classified "fair" — meets the minimum length but lacks variety.
    #[test]
    fn passphrase_strength_fair_at_8_single_variety() {
        // 8 chars, lowercase only → variety = 1 → fair
        assert_eq!(passphrase_strength("abcdefgh").0, "fair");
        // 11 chars, still only one class → still fair (not enough variety for "good")
        assert_eq!(passphrase_strength("abcdefghijk").0, "fair");
    }

    // classified "strong".
    #[test]
    fn passphrase_strength_strong_at_16_high_variety() {
        // 16 chars: upper + lower + digit + special → variety = 4 → strong
        assert_eq!(passphrase_strength("Abcdefgh1!xyz123").0, "strong");
        // 16 chars: upper + lower + digit (3 classes) → also strong
        assert_eq!(passphrase_strength("Abcdefgh1zzz1234").0, "strong");
    }
}
