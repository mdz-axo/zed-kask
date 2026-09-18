//! Property layer for the kask-core small pins
//! (`kask/docs/reference/testing-protocol.md`). Batch 6 of the propagation
//! plan (`tasks/kask-testing-propagation-plan.md`): the unit pins for these
//! seams exist (`sanitize_name_blocks_path_traversal`,
//! `cell_coordinate_round_trips_exactly`,
//! `equal_versions_retain_the_first_candidate`); the properties generalize
//! them over generated input domains. A shrunk counterexample is a finding
//! to report, never a signal to weaken a property.

use crate::agent_paths::{agent_dir, sanitize_name};
use crate::spreadsheet::{CellCoordinate, MAX_SHEET_COLS, MAX_SHEET_ROWS};
use crate::ytdlp::{candidate_is_preferred, parse_version};
use proptest::prelude::*;

proptest! {
    /// Hypothesis: sanitization is traversal-proof over any generated name —
    /// the result never contains a path separator, never collapses to "." or
    /// "..", never carries leading/trailing dashes, and always lands exactly
    /// one level under the agents tree.
    #[test]
    fn sanitize_name_is_traversal_proof_over_generated_names(
        name in "[a-zA-Z0-9./\\\\:*?\"<>|() -]{0,40}",
    ) {
        let sanitized = sanitize_name(&name);
        prop_assert!(
            !sanitized.contains('/') && !sanitized.contains('\\'),
            "sanitized {:?} carries a path separator",
            sanitized
        );
        prop_assert_ne!(sanitized.as_str(), ".");
        prop_assert_ne!(sanitized.as_str(), "..");
        if !sanitized.is_empty() {
            prop_assert!(!sanitized.starts_with('-'));
            prop_assert!(!sanitized.ends_with('-'));
        }
        let dir = agent_dir(&name);
        prop_assert!(
            sanitized.is_empty() || dir.ends_with(&sanitized),
            "agent_dir must join the sanitized name, got {:?}",
            dir
        );
    }

    /// Hypothesis: every in-bounds generated coordinate validates and
    /// round-trips exactly through serde — sheet, row, and col survive.
    #[test]
    fn cell_coordinates_round_trip_within_the_sheet_ceiling(
        sheet in "[A-Za-z][A-Za-z0-9 ]{0,15}",
        row in 0usize..MAX_SHEET_ROWS,
        col in 0usize..MAX_SHEET_COLS,
    ) {
        let coordinate =
            CellCoordinate::new(sheet.clone(), row, col).expect("in-bounds coordinate validates");
        let round: CellCoordinate = serde_json::from_value(
            serde_json::to_value(&coordinate).expect("coordinate serializes"),
        )
        .expect("coordinate deserializes");
        prop_assert_eq!(round.sheet, coordinate.sheet);
        prop_assert_eq!(round.row, coordinate.row);
        prop_assert_eq!(round.col, coordinate.col);
    }

    /// Hypothesis: the sheet ceiling rejects, never clamps — rows or columns
    /// at or beyond the bound and blank sheets fail validation for every
    /// generated overhang.
    #[test]
    fn cell_coordinates_reject_beyond_the_sheet_ceiling(
        sheet in "[A-Za-z][A-Za-z0-9 ]{0,15}",
        over in 0usize..1000,
    ) {
        prop_assert!(CellCoordinate::new(sheet.clone(), MAX_SHEET_ROWS + over, 0).is_err());
        prop_assert!(CellCoordinate::new(sheet.clone(), 0, MAX_SHEET_COLS + over).is_err());
        prop_assert!(CellCoordinate::new(String::new(), 0, 0).is_err());
    }

    /// Hypothesis: version preference is a strict order — irreflexive,
    /// antisymmetric, and transitive — so retention folds over candidate
    /// lists are deterministic and cannot cycle.
    #[test]
    fn version_preference_is_a_strict_order(
        a in proptest::collection::vec(0u64..=100, 0..6),
        b in proptest::collection::vec(0u64..=100, 0..6),
        c in proptest::collection::vec(0u64..=100, 0..6),
    ) {
        prop_assert!(!candidate_is_preferred(&a, &a));
        if candidate_is_preferred(&a, &b) {
            prop_assert!(!candidate_is_preferred(&b, &a));
        }
        if candidate_is_preferred(&a, &b) && candidate_is_preferred(&b, &c) {
            prop_assert!(candidate_is_preferred(&a, &c));
        }
    }

    /// Hypothesis: a retention fold (replace only on strictly newer, as the
    /// media ytdlp integration drives it) always ends on the lexicographic
    /// maximum of the candidate list.
    #[test]
    fn retention_folds_end_on_the_maximum(
        versions in proptest::collection::vec(proptest::collection::vec(0u64..=100, 1..5), 1..40),
    ) {
        let mut current: Option<Vec<u64>> = None;
        for version in &versions {
            let take = current
                .as_deref()
                .map_or(true, |held| candidate_is_preferred(version, held));
            if take {
                current = Some(version.clone());
            }
        }
        let retained = current.expect("a nonempty candidate list retains one");
        let maximum = versions.iter().max().expect("nonempty").clone();
        prop_assert_eq!(retained, maximum);
    }

    /// Hypothesis: dotted-numeric version strings parse to exactly their
    /// generated segments.
    #[test]
    fn parse_version_recovers_generated_segments(
        major in 0u64..=1000,
        minor in 0u64..=1000,
        patch in 0u64..=1000,
    ) {
        let text = format!("{major}.{minor}.{patch}");
        prop_assert_eq!(parse_version(&text), Some(vec![major, minor, patch]));
    }
}
