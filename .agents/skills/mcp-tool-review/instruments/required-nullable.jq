# Properties a tools/list capture lists under `required` although the property accepts null.
# Input: an array of {name, inputSchema}. Output: one line per property, then nothing if none.
def accepts_null: ((.type? // empty) | if type == "array" then any(. == "null") else . == "null" end) // false
  or ((.anyOf? // []) | any((.type? // empty) | if type == "array" then any(. == "null") else . == "null" end));
.[] | .name as $t | (.inputSchema.required // [])[] as $r
| select((.inputSchema.properties[$r] // {}) | accepts_null) | "\($t)\t\($r)"
