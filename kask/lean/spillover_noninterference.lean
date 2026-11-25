/-
zed-kask spec pin (PRF-03 — D100 `hkask-condenser::SpilloverStore` payload
confidentiality non-interference & capacity preservation).

The Rust side: `kask/crates/hkask-condenser/src/spillover.rs` (`SpilloverStore`,
`put`, `receipt_for_uri`, `read_slice`).

Explicit scope / assumption note:
The current Rust implementation returns distinct `NotFound` vs
`PrivateBoundaryRefused` error variants on `Public` lookups of privately-stored
hashes (`spillover.rs:306-315,339-346`). This proof models **payload
confidentiality non-interference** (a `Public` read never returns the payload of
a hash that was only inserted under `Private`) and **entry capacity
preservation** (`entries.length ≤ maxEntries`), matching the current Rust
contract without assuming the existence-channel error variants are collapsed.

Check: `lean_check` on this file (pinned Lean 4.34.0); the paired
`spillover_noninterference_negative_control.lean` must FAIL the same check.
-/

namespace SpilloverNoninterference

inductive Privacy where
  | pub_
  | priv
deriving DecidableEq

structure Entry where
  hash : String
  payload : String
  publicSeen : Bool
deriving DecidableEq

structure Store where
  entries : List Entry
  maxEntries : Nat

/-- Lookup payload for `h` as `caller`. A `pub_` caller may read an entry only
when `e.publicSeen = true`. -/
def readPayload (s : Store) (h : String) (caller : Privacy) : Option String :=
  match s.entries.find? (fun e => e.hash == h) with
  | none => none
  | some e =>
      if caller == .priv || e.publicSeen then some e.payload else none

/-- Insert or update `h` with `payload` under `priv`. Existing entries for `h`
are removed; `publicSeen` becomes `true` iff `priv = .pub_` OR `h` was already
`publicSeen`. Truncates to `maxEntries` when full. -/
def put (s : Store) (h payload : String) (p : Privacy) : Store :=
  let wasPublic :=
    match s.entries.find? (fun e => e.hash == h) with
    | some e => e.publicSeen
    | none => false
  let rest := s.entries.filter (fun e => e.hash != h)
  let trimmed := rest.take (s.maxEntries - 1)
  let entry : Entry := { hash := h, payload := payload, publicSeen := wasPublic || (p == .pub_) }
  { s with entries := entry :: trimmed }

/-- Payload confidentiality non-interference (PRF-03):
Inserting a hash under `Privacy.priv` when it was not previously `publicSeen`
never exposes its payload to a `Privacy.pub_` reader. -/
theorem private_put_never_grants_public_read
    (s : Store) (h payload : String)
    (hnotpub : readPayload s h .pub_ = none) :
    readPayload (put s h payload .priv) h .pub_ = none := by
  unfold readPayload put at *
  cases hfind : s.entries.find? (fun e => e.hash == h)
  · simp
  · next e =>
      simp only [hfind] at hnotpub ⊢
      cases hpub : e.publicSeen
      · simp
      · simp [hpub] at hnotpub

/-- Capacity preservation: if `0 < s.maxEntries`, `put` always produces a store
with `entries.length ≤ s.maxEntries`. -/
theorem put_preserves_capacity (s : Store) (h payload : String) (p : Privacy)
    (hcap : 0 < s.maxEntries) :
    (put s h payload p).entries.length ≤ s.maxEntries := by
  unfold put
  simp only [List.length_cons, List.length_take]
  omega

end SpilloverNoninterference

#print axioms SpilloverNoninterference.private_put_never_grants_public_read
#print axioms SpilloverNoninterference.put_preserves_capacity
