//! Improv mode — interaction posture types and system-prompt generation.
//!
//! Five improv modes (Plussing, Yes And, Yes But, Freestyling, Riffing) plus
//! Cascade for sequential composition. When active, a mode-specific instruction
//! is prepended to the effective input so the model adopts the interaction
//! posture. The modes are prompt-injection directives — the LLM does the work.
//!
//! Cascade composes modes sequentially, bounded by the matryoshka limit (7).

use std::time::Duration;

/// Matryoshka limit — maximum total mode applications in a cascade (including nested).
pub const MATRYOSHKA_LIMIT: usize = 7;

/// Policy for how a Riff resolves back to the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiffReturn {
    /// Return to the group context after the riff.
    ReturnToGroup,
    /// Spawn a new thread for the riff's findings.
    SpawnThread,
    /// Return after a fixed number of exploration steps.
    ReturnAfterSteps { max_steps: usize },
}

/// The five improv modes plus Cascade for recursive composition.
///
/// Exhaustive enum — no `Other` or `Custom` fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImprovMode {
    /// Plussing (Catmull): Extract agreeable components, silently discard remainder.
    /// Build constructively on selected seeds. Never explicitly negate.
    Plussing,

    /// Yes And: Accept the whole contribution, extend with a novel layer.
    YesAnd,

    /// Yes But: Accept the whole contribution, append a constraint or redirect.
    YesBut,

    /// Freestyling: Rapid collaborative short-response cycling.
    /// Time-bounded, no single owner.
    Freestyling {
        /// Maximum duration for the freestyle session.
        time_bound: Duration,
    },

    /// Riffing: Solo divergent exploration from a seed contribution.
    Riffing {
        /// Policy for how the riff resolves back to the group.
        return_policy: RiffReturn,
    },

    /// Cascade: Sequential composition of sub-modes.
    /// Bounded by the matryoshka limit (7 total applications).
    Cascade(Vec<ImprovMode>),
}

impl ImprovMode {
    /// Human-readable label for this mode.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            ImprovMode::Plussing => "plussing",
            ImprovMode::YesAnd => "yes-and",
            ImprovMode::YesBut => "yes-but",
            ImprovMode::Freestyling { .. } => "freestyling",
            ImprovMode::Riffing { .. } => "riffing",
            ImprovMode::Cascade(_) => "cascade",
        }
    }

    /// Count total mode applications, recursing into nested cascades.
    pub fn total_applications(modes: &[ImprovMode]) -> usize {
        modes
            .iter()
            .map(|m| match m {
                ImprovMode::Cascade(inner) => 1 + Self::total_applications(inner),
                _ => 1,
            })
            .sum()
    }
}

/// Generate a system-prompt instruction for the active improv mode.
///
/// Prepended to the effective input before inference so the model
/// adopts the specified interaction posture. Each mode has a concise
/// instruction that encodes its core constraint.
#[must_use]
pub fn improv_system_prompt(mode: &ImprovMode) -> String {
    match mode {
        ImprovMode::Plussing => {
            "[Improv mode: Plussing]\n\
             Find what you can agree with in the user's message. Build constructively on those points.\n\
             Silently omit anything you disagree with — never explicitly negate or reject.\n\
             If nothing is agreeable, redirect constructively: \"Let's explore this from a different angle.\"".to_string()
        }
        ImprovMode::YesAnd => {
            "[Improv mode: Yes And]\n\
             Accept the user's entire message as valid. Extend it with a novel, additive layer.\n\
             Your extension must build on their contribution, not replace or contradict it.\n\
             Start with \"Yes, and also:\" or equivalent acceptance language.".to_string()
        }
        ImprovMode::YesBut => {
            "[Improv mode: Yes But]\n\
             Accept the user's entire message as valid. Then append a constructive constraint\n\
             or boundary condition that narrows scope without contradicting.\n\
             Frame as additive guidance: \"Yes, and let's also consider...\" not \"No, because...\"\n\
             Never use rejecting language (no, wrong, can't, impossible).".to_string()
        }
        ImprovMode::Freestyling { .. } => {
            "[Improv mode: Freestyling]\n\
             Engage in rapid, associative, creative response. Keep responses short (1-3 sentences).\n\
             Build on the energy of the conversation — this is creative exploration, not careful analysis.\n\
             Take creative leaps. Connect ideas associatively. Don't over-think.".to_string()
        }
        ImprovMode::Riffing { .. } => {
            "[Improv mode: Riffing]\n\
             Take one idea from the user's message and explore it independently as a solo tangent.\n\
             Go deep, go wide, go creative — this is your independent exploration space.\n\
             When done, either return to the main topic with a synthesis of your findings,\n\
             or signal that this tangent deserves its own thread.".to_string()
        }
        ImprovMode::Cascade(steps) => {
            let step_labels: Vec<String> = steps.iter().map(|m| m.label().to_string()).collect();
            format!(
                "[Improv mode: Cascade — {}]\n\
                 Apply these improv modes in sequence to your response:\n\
                 {}\n\
                 Each step's output feeds into the next. Stay within the matryoshka limit of 7 total applications.",
                step_labels.join(" → "),
                step_labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| format!("  {}. {}", i + 1, label))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_stable() {
        assert_eq!(ImprovMode::Plussing.label(), "plussing");
        assert_eq!(ImprovMode::YesAnd.label(), "yes-and");
        assert_eq!(ImprovMode::YesBut.label(), "yes-but");
        assert_eq!(
            ImprovMode::Freestyling {
                time_bound: Duration::from_secs(60)
            }
            .label(),
            "freestyling"
        );
        assert_eq!(
            ImprovMode::Riffing {
                return_policy: RiffReturn::ReturnToGroup
            }
            .label(),
            "riffing"
        );
        assert_eq!(ImprovMode::Cascade(vec![]).label(), "cascade");
    }

    #[test]
    fn total_applications_counts_nested() {
        assert_eq!(ImprovMode::total_applications(&[ImprovMode::Plussing]), 1);
        assert_eq!(
            ImprovMode::total_applications(&[ImprovMode::Plussing, ImprovMode::YesAnd]),
            2
        );
        // Cascade([YesAnd, YesBut]) counts as 1 (cascade) + 2 (inner) = 3.
        // Plus the outer Plussing = 4 total.
        assert_eq!(
            ImprovMode::total_applications(&[
                ImprovMode::Plussing,
                ImprovMode::Cascade(vec![ImprovMode::YesAnd, ImprovMode::YesBut]),
            ]),
            4
        );
    }

    #[test]
    fn system_prompt_includes_mode_name() {
        let prompt = improv_system_prompt(&ImprovMode::Plussing);
        assert!(prompt.contains("Plussing"));
        let prompt = improv_system_prompt(&ImprovMode::YesAnd);
        assert!(prompt.contains("Yes And"));
    }
}
