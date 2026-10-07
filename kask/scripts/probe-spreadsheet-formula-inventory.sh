#!/usr/bin/env bash
# SP-10 (spreadsheet-capability-redesign-proposals.md): probe the
# logisheets-rs formula inventory, admission-gate style.
#
# Builds a scratch harness OUTSIDE the workspace (a temp dir), pinned to
# the admitted release (logisheets-rs =1.15.1, plan §10 Phase 0 record),
# evaluates each candidate function under simple invocation shapes, and
# prints one line per (function, shape):
#
#     FN|shape|parse=<check_formula result>|value=<evaluated cell value>
#
# The verified results land in
# kask/docs/reference/mcp-servers/spreadsheet.md (the formula primitive's
# documented inventory — closing evidence gap F11 of the from-scratch
# design review). Parse acceptance alone does NOT distinguish known from
# unknown functions (an unknown function parses and then evaluates to a
# visible `#` error value — pinned by `unsupported_formula_surfaces_as_
# error`), so the probe classifies by the EVALUATED value.
#
# Usage: kask/scripts/probe-spreadsheet-formula-inventory.sh
set -euo pipefail

SCRATCH="$(mktemp -d /tmp/spreadsheet-formula-probe.XXXXXX)"
trap 'rm -rf "$SCRATCH"' EXIT

cd "$SCRATCH"
cargo init --name formula_probe --bin >/dev/null 2>&1

cat > Cargo.toml <<'EOF'
[package]
name = "formula_probe"
version = "0.0.0"
edition = "2021"

[dependencies]
logisheets-rs = "=1.15.1"
EOF

cat > src/main.rs <<'EOF'
//! Formula-inventory probe: one workbook, B1:B3 seeded with 1.0/2.0/3.0,
//! one candidate function per row under five simple invocation shapes,
//! every evaluated value printed as `FN|shape|parse=<bool>|value=<Debug>`.
fn main() {
    let functions: &[&str] = &[
        // Math & aggregation
        "SUM", "SUMIF", "SUMIFS", "SUMPRODUCT", "AVERAGE", "AVERAGEIF", "AVERAGEIFS",
        "COUNT", "COUNTA", "COUNTIF", "COUNTIFS", "COUNTBLANK", "MIN", "MAX", "MEDIAN",
        "MODE", "LARGE", "SMALL", "RANK", "PERCENTILE", "STDEV", "STDEVP", "VAR", "VARP",
        "ABS", "SIGN", "SQRT", "POWER", "EXP", "LN", "LOG", "LOG10", "MOD", "INT", "TRUNC",
        "ROUND", "ROUNDUP", "ROUNDDOWN", "CEILING", "FLOOR", "RAND", "RANDBETWEEN",
        // Logic
        "IF", "IFERROR", "AND", "OR", "NOT", "TRUE", "FALSE", "ISBLANK", "ISNUMBER",
        "ISTEXT", "ISERROR", "NA",
        // Lookup & reference
        "VLOOKUP", "HLOOKUP", "INDEX", "MATCH", "LOOKUP", "OFFSET", "INDIRECT", "ROW",
        "COLUMN", "TRANSPOSE",
        // Text
        "LEN", "LEFT", "RIGHT", "MID", "UPPER", "LOWER", "TRIM", "CONCAT", "CONCATENATE",
        "SUBSTITUTE", "REPLACE", "FIND", "SEARCH", "TEXT", "VALUE", "EXACT",
        // Date & time
        "DATE", "TODAY", "NOW", "YEAR", "MONTH", "DAY", "EOMONTH",
        // Financial
        "NPV", "IRR", "PMT", "FV", "PV", "RATE",
        // Statistics
        "CORREL", "SLOPE", "INTERCEPT",
    ];
    let shapes: &[&str] = &["B1", "B1:B3", "B1,B1", "B1,B1,B1", "B1,B1,B1,B1"];

    let mut workbook = logisheets_rs::Workbook::default();
    let mut seed = logisheets_rs::PayloadsAction::new();
    for (row, value) in [1.0f64, 2.0, 3.0].iter().enumerate() {
        seed = seed.add_payload(logisheets_rs::CellInput {
            sheet_idx: 0,
            row,
            col: 1,
            content: value.to_string(),
        });
    }
    let _ = workbook.handle_action(logisheets_rs::EditAction::Payloads(seed.set_init(true)));

    let mut row = 5usize;
    let mut placements: Vec<(String, String, usize)> = Vec::new();
    for function in functions {
        for shape in shapes {
            let formula = format!("={function}({shape})");
            let parsed = workbook.check_formula(formula.clone());
            let mut action = logisheets_rs::PayloadsAction::new();
            action = action.add_payload(logisheets_rs::CellInput {
                sheet_idx: 0,
                row,
                col: 2,
                content: formula.clone(),
            });
            let _ = workbook.handle_action(logisheets_rs::EditAction::Payloads(action.set_init(true)));
            placements.push((function.to_string(), shape.to_string(), row));
            println!("{function}|{shape}|parse={parsed}|pending");
            row += 1;
        }
    }

    let end = row.saturating_sub(1);
    let infos = match workbook
        .get_sheet_by_idx(0)
        .and_then(|sheet| sheet.get_cell_infos(5, 2, end, 2))
    {
        Ok(infos) => infos,
        Err(error) => {
            eprintln!("read failed: {error:?}");
            return;
        }
    };
    for (function, shape, at) in placements {
        let value = &infos[at - 5].value;
        println!("{function}|{shape}|value={value:?}");
    }
}
EOF

# The pinned release is in the local cargo cache (the workspace depends on
# it), so the scratch build runs offline.
cargo run --offline --quiet