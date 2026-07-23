# hkask-templates

Registry, vocabulary, and template execution for hKask.

The "thread" in hKask's loom-and-thread architecture: YAML manifests + Jinja2 templates define the mutable behavioral surface.

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Registry** | CRUD for template manifests (`manifest.yaml`) |
| **Vocabulary** | 120-term controlled vocabulary for template validation |
| **Cascade** | Template resolution and inheritance |
| **Resolver** | Template selection based on context and constraints |
| **Jinja2** | `minijinja` engine — safe, sandboxed template execution |
