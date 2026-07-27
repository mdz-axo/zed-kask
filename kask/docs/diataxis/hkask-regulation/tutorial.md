---
title: "hkask-regulation — Tutorial: Reading a Regulation Span"
audience: [operators, developers]
last_updated: 2026-07-27
version: "0.1.0"
status: "Active"
domain: "Regulation"
mds_categories: [lifecycle]
---

# hkask-regulation — Tutorial: Reading a Regulation Span

This tutorial shows how to read and interpret a Regulation observable span.
Regulation spans are the telemetry that the cybernetic loop consumes to
monitor agent behavior.

## Learning path

```mermaid
flowchart TD
    A[Step 1: Identify the span namespace] --> B[Step 2: Read the span fields]
    B --> C[Step 3: Trace the span to its source]
    C --> D[Step 4: Check the variety monitor]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-REG-003
verified_date: 2026-07-27
verified_against: kask/crates/hkask-regulation/src/runtime.rs:405,276; kask/crates/hkask-regulation/src/qa_span.rs:13
status: VERIFIED
-->

## Steps 1-2: Identify the namespace and read the fields

Regulation spans use the `reg.*` namespace. The `RegulationLedger`
(`runtime.rs:405`) records each span as a `RegulationCycleEntry`
(`runtime.rs:343`). Each entry has a `span_category`, `span_path`, `phase`,
and `observer_webid`.

## Steps 3-4: Trace the source and check variety

Trace the span to its emission point in the source code. Check the
`VarietyMonitor` (`runtime.rs:276`) to see whether the span's domain has
sufficient variety. A variety deficit triggers an algedonic alert.

## See also

- [hkask-regulation Reference](./reference.md): class diagram of the
  ledger and loop.
- [hkask-regulation How-to](./how-to.md): adding a new span namespace.

---

[^conant-ashby]: Conant, R. C., & Ashby, W. R. (1970). *Every good regulator of a control system must be a model of that system.* International Journal of Systems Science, 1(2), 89-97. <https://www.tandfonline.com/doi/abs/10.1080/00207727008902020>.
