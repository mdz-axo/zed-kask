# Preserve raw JSON identity before the lossy JSON -> Lisp bridge.
# No review echo, scoring, call counter or shadow admission state.
# Digest binding (operator ruling 2026-10-30): prompt identity crosses the
# seam as a lowercase-hex SHA-256 digest; the full revision texts live in
# the canonical revision files and the delivered output, never in the env.
def digest:
  type == "string" and test("^[0-9a-f]{64}$");
def request_shape:
  type == "object" and
  keys == ["attempt_id","kind","level","reviewed_prompt_digest","reviewed_revision"] and
  (.attempt_id | type == "number" and floor == .) and
  (.level | type == "number" and floor == .) and
  (.reviewed_revision | type == "number" and floor == .) and
  (.reviewed_prompt_digest | digest) and
  (.kind | IN("initial","escalate","reprobe","correction","retry"));
type == "object" and has("effort_tier") and has("request") and
(.prompt_revisions | type == "array" and all(.[]; digest)) and
(.critique_runs | type == "array") and
(.request == null or (.request | request_shape)) and
all(.critique_runs[];
  type == "object" and has("status") and has("outcome") and
  has("reviewed_prompt_digest") and (.reviewed_prompt_digest | digest) and
  (if .status == "pending" then .outcome == null
   elif .status == "completed" then
    (.outcome | type == "object") and
    (.outcome.ratings | type == "array") and
    (.outcome.findings | type == "array") and
    all(.outcome.ratings[]; type == "object") and
    all(.outcome.findings[]; type == "object")
   else true end))
