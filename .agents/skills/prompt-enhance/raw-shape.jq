# Preserve raw JSON identity before the lossy JSON -> Lisp bridge.
# No review echo, scoring, call counter or shadow admission state.
def request_shape:
  type == "object" and
  keys == ["attempt_id","kind","level","reviewed_prompt","reviewed_revision"] and
  (.attempt_id | type == "number" and floor == .) and
  (.level | type == "number" and floor == .) and
  (.reviewed_revision | type == "number" and floor == .) and
  (.reviewed_prompt | type == "string") and
  (.kind | IN("initial","escalate","reprobe","correction","retry"));
type == "object" and has("effort_tier") and has("request") and
(.prompt_revisions | type == "array") and
(.critique_runs | type == "array") and
(.request == null or (.request | request_shape)) and
all(.critique_runs[];
  type == "object" and has("status") and has("outcome") and
  (if .status == "pending" then .outcome == null
   elif .status == "completed" then
    (.outcome | type == "object") and
    (.outcome.ratings | type == "array") and
    (.outcome.findings | type == "array") and
    all(.outcome.ratings[]; type == "object") and
    all(.outcome.findings[]; type == "object")
   else true end))
