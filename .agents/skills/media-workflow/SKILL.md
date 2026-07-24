---
name: media-workflow
visibility: public
description: "Multi-step media pipeline skill. Teaches agents to compose Fal.ai workflow DAGs from natural-language intent and execute them as atomic operations. Bridges the gap between single-call media tools and complex multi-step pipelines (e.g., generate → remove background → upscale). Uses media/workflow-composer to build the DAG and media.execute_workflow to run it.
"
---

# Media Workflow

Multi-step media pipeline skill. Teaches agents to compose Fal.ai workflow DAGs from natural-language intent and execute them as atomic operations. Bridges the gap between single-call media tools and complex multi-step pipelines (e.g., generate → remove background → upscale). Uses media/workflow-composer to build the DAG and media.execute_workflow to run it.


## When to Use

- When the user wants multiple media operations chained together (e.g., "generate a logo, remove its background, and upscale to 4K").
- When a user describes a multi-step media pipeline that needs to be composed into a Fal.ai workflow DAG and executed as a single atomic operation.

## Instructions

### compose-and-execute

1. Detect multi-step intent: if the user wants more than one media operation (e.g., "generate X then remove background then upscale"), use this workflow; direct single operations to individual media tools directly.
2. Compose the DAG by using the `media/workflow-composer` template with the user's intent as the `intent` parameter to produce a valid Fal workflow JSON.
3. Execute the workflow by calling `media.execute_workflow` with the workflow JSON string to run all steps in dependency order, handle `$references` between nodes, and return output URLs.
4. Report results by presenting the output URLs and any metadata to the user.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `compose-and-execute.j2` | KnowAct | Compose a Fal.ai workflow DAG from a natural-language media intent, then execute it as a single atomic operation. Use this when the user wants multiple media operations chained together (e.g., "generate a logo, remove its background, and upscale to 4K"). Delegates to media/workflow-composer for DAG construction and media.execute_workflow for execution.  |

## Constraints

- `compose-and-execute.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
