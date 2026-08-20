# Interdisciplinary Constraint-Forces Skills — Architecture

This diagram shows the relationship between the two scaffolded skills (GSR, CFR), their delegate skills, and the ontology-source providers. It serves as a **reference** document in the Diataxis framework — austere and factual, for architects and developers who need to understand the skill composition.

```mermaid
graph TD
    subgraph "Ontology Sources (multi-provider)"
        OBO["OBO Foundry\n(no auth, ~200+ ontologies)"]
        ONTOBEE["OntoBee\n(SPARQL mirror)"]
        BIOPORTAL["BioPortal\n(apikey, 1288 ontologies)"]
    end

    subgraph "GSR: gradient-seeded-recombination"
        GSR_INV["1. Inventory\n(enum + license check)"]
        GSR_PRIOR["2. Prior\n(K_n complete graph)"]
        GSR_MAP["3. Map\n(actual recombination field)"]
        GSR_DETECT["4. Detect\n(8-shape taxonomy)"]
        GSR_HYP["5. Hypothesize\n(7-class reason taxonomy)"]
        GSR_PRIO["6. Prioritize\n(reason class ordering)"]
        GSR_SEED["7. Select Seeds\n(most central concept)"]
        GSR_CONV["8. Converge\n(Cauchy on field coverage)"]
        GSR_LOOP["9. Loop\n(feedback to Prior)"]

        GSR_INV --> GSR_PRIOR --> GSR_MAP --> GSR_DETECT --> GSR_HYP --> GSR_PRIO --> GSR_SEED --> GSR_CONV --> GSR_LOOP
        GSR_LOOP --> GSR_PRIOR
    end

    subgraph "CFR: constraint-forces-recast"
        CFR_REP["1. Represent\n(axiom graph)"]
        CFR_VIOL["2. Violate\n(B's axiom violations)"]
        CFR_PROJ["3. Project\n(min-sat projection)"]
        CFR_CTRL["4. Control\n(relabel control)"]
        CFR_3CRIT["5. Three-Criterion\n(expressible/absent/consistent)"]
        CFR_CMP["6. Compare\n(mutant delta vs relabel delta)"]
        CFR_FRON["7. Frontier\n(Pareto on novelty/validity/cost)"]
        CFR_CONV["8. Converge\n(lisp.eval: frontier stability)"]
        CFR_LOOP["9. Loop\n(advance seed_index)"]

        CFR_REP --> CFR_VIOL --> CFR_PROJ --> CFR_CTRL --> CFR_3CRIT --> CFR_CMP --> CFR_FRON --> CFR_CONV --> CFR_LOOP
        CFR_LOOP --> CFR_REP
    end

    subgraph "Delegate Skills"
        FALS["falsifiability\n(Popper/Platt/Pearl)"]
        GHUNT["gradient-hunter\n(8-shape taxonomy)"]
        GPA["gpa-evolution\n(Pareto frontier)"]
        PCYB["pragmatic-cybernetics\n(variety engineering)"]
        META["metacognition\n(perspective rotation)"]
    end

    OBO --> GSR_INV
    ONTOBEE --> GSR_INV
    BIOPORTAL --> GSR_INV

    GSR_HYP -.->|delegates| FALS
    GSR_HYP -.->|delegates| META
    GSR_PRIOR -.->|delegates| PCYB
    GSR_MAP -.->|delegates| GRAPH
    GSR_DETECT -.->|inherits taxonomy| GHUNT

    GSR_SEED -->|seed_concepts| CFR_REP

    CFR_3CRIT -.->|methodological anchor| FALS
    CFR_FRON -.->|methodological anchor| GPA
```

## Cross-Links

- [MDS](../architecture/core/MDS.md) — Minimal Domain Specification categories
