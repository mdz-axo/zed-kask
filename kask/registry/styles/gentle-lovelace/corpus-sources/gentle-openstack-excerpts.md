# OpenStack Writing Guidelines — Key Excerpts with Section-Type Annotation
# Source: OpenStack Documentation Contributor Guide (Anne Gentle's team)
# Primary dimension: Gentle (Agent-Correctness)
# Tags: Statement, Evidence, Implications

## Excerpt 1: General Writing Guidelines — Active Voice
### Section type: Statement + Evidence + Implications

[Statement]
Write in active voice rather than passive voice. Active voice identifies the
agent of action as the subject of the verb — usually the user.

[Evidence]
Passive: "The command can be executed by the user."
Active:   "Run the command."

[Implications]
Active voice usually requires fewer words than passive voice. Users read
documentation to perform tasks or gather information. For users, these
activities take place in their present, so the present tense is appropriate
in most cases.

## Excerpt 2: General Writing Guidelines — Second Person
### Section type: Statement + Implications

[Statement]
Users are more engaged with documentation when you use second person (that is,
you address the user as "you").

[Implications]
Write in second person for all task-based documentation. Third person is
appropriate for reference material and specifications.

## Excerpt 3: API Documentation Guidelines — Structure
### Section type: Statement + Evidence + Diagram

[Statement]
All API reference jobs publish from master as soon as a change lands in the
respective project repository. This publishing practice means that you must
write inline information when an API has a change release-to-release.

[Evidence — file structure]
Create an `api-ref/source` directory in your project repository.
Create a `conf.py` for the project with the openstackdocstheme.
Create RST files for each operation.
Create sample JSON requests and responses in a directory.
Add the `api-ref-jobs` template to your project.

[Diagram — API reference structure]
api-ref/
  source/
    conf.py
    index.rst
    parameters.yaml
    samples/
      request.json
      response.json

[Implications]
After the source files and build jobs exist, the docs are built to
docs.openstack.org. If your document is completely new, add links from the
API landing page and the OpenStack Governance reference document.

## Excerpt 4: Docs Like Code — Core Loop
### Section type: Statement + Implications

[Statement]
That's the core loop: write a file, commit it, and it publishes automatically.

[Implications]
Version every change — use Git to track who changed what and why, for docs,
not just code. CI/CD pipelines publish your docs on every merge, no manual
steps. Writers and developers collaborate in the same workflow they already
know. Broken docs block the build — a stale package count carries the same
severity as a compilation error.

## Excerpt 5: Docs-as-Code Definition
### Section type: Statement

Docs as code is taking developer techniques and applying them to documentation:
using GitHub for collaborative version control, test automation (running docs
through linters), and continuous integration/continuous deployment so that you
have source for your docs and it builds something that can be published
immediately.
