/-
Negative control for `spillover_noninterference.lean` (PRF-03).

Omits the `e.publicSeen` gate in `readPayloadBuggy` and attempts to prove
`private_put_never_grants_public_read`. Must FAIL `lean_check`.
-/

namespace SpilloverNoninterferenceNegControl

inductive Privacy where
  | pub_
  | priv
deriving DecidableEq

structure Entry where
  hash : String
  payload : String
  publicSeen : Bool

structure Store where
  entries : List Entry

def readPayloadBuggy (s : Store) (h : String) (_caller : Privacy) : Option String :=
  match s.entries.find? (fun e => e.hash == h) with
  | none => none
  | some e => some e.payload

def putPriv (s : Store) (h payload : String) : Store :=
  { entries := { hash := h, payload := payload, publicSeen := false } :: s.entries }

theorem buggy_read_hides_private_must_fail (s : Store) (h payload : String) :
    readPayloadBuggy (putPriv s h payload) h .pub_ = none := by
  simp [readPayloadBuggy, putPriv]

end SpilloverNoninterferenceNegControl
