---
title: "Swarm Panel Modes (State)"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, lifecycle]
---

# Swarm Panel Modes (State)

The `SwarmPanel` (`crates/swarm_panel/src/swarm_panel.rs:574`) has four
`PanelMode` states (`:289`), exposed as a header tab bar. The user can switch
freely between any of the four at any time via `set_mode` (`:1798`), which
also moves focus to the target mode's first field (the M5 fix). Entering
`Steer` lazily constructs the curator `ConversationView` via
`ensure_steer_conversation` (`:1870`), baking the current backend mode into its
system prompt. A backend toggle (`set_swarm_mode`, `:1834`) drops any open
Steer conversation so the next entry rebuilds it with the new mode — otherwise
the curator would pass a stale `context.mode` to the skill cascade. The panel
opens to `Browse` via the `Toggle` action (`:230`); `ToggleFocus` only focuses
an existing item. See the [Swarm Systems How-to](../diataxis/swarm_system/how-to.md) and the [Swarm Cybernetics/Semantics Audit](../audits/swarm-cybernetics-semantics-audit.md).

```mermaid
stateDiagram-v2
    direction TD
    [*] --> Browse : Toggle action deploys panel
    Browse --> Browse : filter All Swarms Agents
    Browse --> Author : New Agent tab
    Browse --> Compose : New Swarm tab
    Browse --> Steer : Steer tab builds conversation
    Author --> Browse : cancel or save
    Author --> Author : edit name prompt type
    Compose --> Browse : cancel or create
    Compose --> Compose : ask Xaman add agents
    Steer --> Browse : switch tab
    Steer --> Steer : curator PDCA turn
    Browse --> [*] : close item
    Author --> [*]
    Compose --> [*]
    Steer --> [*]

    state BackendToggle {
        [*] --> Abw
        Abw --> Local : set_swarm_mode Local
        Local --> Abw : set_swarm_mode Abw
    }
    note right of BackendToggle
        kask.swarm.mode setting
        persists to settings.json
        MCP server restarts on change
        toggling drops open Steer convo
        so curator bakes the new mode
    end note
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-007
verified_date: 2026-08-04
verified_against: crates/swarm_panel/src/swarm_panel.rs:230,261,289,1798,1834,1870; crates/swarm_panel/src/panel_button.rs:13
status: VERIFIED
-->