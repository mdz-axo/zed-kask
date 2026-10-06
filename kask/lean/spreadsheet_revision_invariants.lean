/-
zed-kask spec pin: spreadsheet revision-chain invariants for
hkask-spreadsheet / hkask-mcp-spreadsheet (from-scratch design review,
Phase 4).

The Rust side:
- `handle_apply` (kask/crates/hkask-spreadsheet/src/service.rs:537-616)
  orders every persisted mutation: idempotency lookup → base-digest
  verification → engine edit → `write_revision` → `record_operation`.
- `ArtifactStore::write_revision` (artifact_store.rs:115-152) publishes
  atomically (temp + fsync + rename) and refuses an existing revision id
  ("revisions are immutable").
- `find_operation` / `record_operation` (artifact_store.rs:201-236) key
  completed operations by the SHA-256 of the caller's idempotency key; a
  record carries {key, base, result} (artifact_store.rs:46-50).
- `handle_open` (service.rs:504-535) verifies the stored digest at open —
  a mismatch is Conflict, never silent acceptance.

STATED ASSUMPTIONS (the model abstracts these; the Rust side pins them):
1. Atomic transition: one apply is one atomic step. The real crash windows
   (revision durably published, completion record not yet written) are
   deliberately OUTSIDE this model: the orphan window reconciles as
   `unknown` and re-applies fresh, pinned by
   `property_tests.rs:216-271` (`orphan_revision_window_reconciles_unknown_
   then_reapplies_fresh`) and ruled deliberate in
   `kask/docs/loop-register.md` register row L22.
2. Digest identity: a revision's content digest identifies its bytes exactly
   (the Phase 0 admission record's byte-determinism finding). Digests are
   opaque here (Nat stand-ins); no hash-collision case is modeled.
3. Storage reflects the store: reading a revision returns the stored bytes;
   storage failures are out of scope.
4. Fresh revision ids: a successful publication supplies a revision id not
   already stored for that artifact (uuid v4 at service.rs:588). The
   store-side refusal of a colliding id (artifact_store.rs:126-132, pinned
   by `same_id_rewrite_is_rejected_and_bytes_unchanged`) is the Rust-side
   backstop, not modeled here.

A proof here establishes the invariant under these assumptions — not
fitness to the capability plan, and not the Rust implementation (which is
pinned by the service.rs test suite, property_tests.rs, and the server's
tests/tool_behavior.rs). Core Lean 4 only (no Mathlib); checked with the
project's pinned Lake toolchain via lean_check. The paired
`spreadsheet_revision_negative_control.lean` must FAIL the same check.
-/

namespace SpreadsheetRevisionSpec

/-- Opaque identities — only decidability of equality matters (Nat stand-ins). -/
abbrev ArtifactId := Nat
abbrev RevisionId := Nat
abbrev Digest := Nat
abbrev IdemKey := Nat

/-- One completed operation record — `OperationRecord` (artifact_store.rs:46-50). -/
structure OpRecord where
  key : IdemKey
  baseRev : RevisionId
  baseDigest : Digest
  resultRev : RevisionId
  resultDigest : Digest
deriving DecidableEq

/-- The store: immutable revisions and completed-operation records
    (the artifact_store.rs:8-20 layout, abstracted). -/
structure Store where
  revisions : List (ArtifactId × RevisionId × Digest)
  ops : List (ArtifactId × OpRecord)
deriving DecidableEq

/-- The first stored digest of (artifact, revision), if any. -/
def lookupRevision (revs : List (ArtifactId × RevisionId × Digest))
    (a : ArtifactId) (r : RevisionId) : Option Digest :=
  match revs with
  | [] => none
  | (a', r', d) :: rest => if a' = a ∧ r' = r then some d else lookupRevision rest a r

/-- The completed operation recorded under (artifact, key), if any. -/
def lookupOp (ops : List (ArtifactId × OpRecord))
    (a : ArtifactId) (k : IdemKey) : Option OpRecord :=
  match ops with
  | [] => none
  | (a', rec) :: rest => if a' = a ∧ rec.key = k then some rec else lookupOp rest a k

def Store.baseDigestOf (s : Store) (a : ArtifactId) (r : RevisionId) : Option Digest :=
  lookupRevision s.revisions a r

def Store.opOf (s : Store) (a : ArtifactId) (k : IdemKey) : Option OpRecord :=
  lookupOp s.ops a k

inductive ApplyOutcome where
  | applied (resultRev : RevisionId)
  | replay (resultRev : RevisionId)
  | conflict
  | keyReuse
  | unknownBase
deriving DecidableEq

/-- One apply step, mirroring `handle_apply`'s order exactly
    (service.rs:537-616): idempotency lookup → base-digest verification →
    publish (extend revisions) → record (extend ops). -/
def applyStep (s : Store) (a : ArtifactId) (baseRev : RevisionId) (baseDigest : Digest)
    (key : IdemKey) (newRev : RevisionId) (newDigest : Digest) : ApplyOutcome × Store :=
  match s.opOf a key with
  | some rec =>
      if rec.baseRev = baseRev ∧ rec.baseDigest = baseDigest then
        (ApplyOutcome.replay rec.resultRev, s)
      else (ApplyOutcome.keyReuse, s)
  | none =>
      match s.baseDigestOf a baseRev with
      | none => (ApplyOutcome.unknownBase, s)
      | some stored =>
          if stored = baseDigest then
            (ApplyOutcome.applied newRev,
             { revisions := (a, newRev, newDigest) :: s.revisions,
               ops := (a, { key := key, baseRev := baseRev, baseDigest := baseDigest,
                            resultRev := newRev, resultDigest := newDigest }) :: s.ops })
          else (ApplyOutcome.conflict, s)

-- ── Branch characterizations (the definition's case analysis, as lemmas) ──

theorem applyStep_replay (s : Store) (a baseRev baseDigest key newRev newDigest : Nat)
    (rec : OpRecord) (hrec : s.opOf a key = some rec)
    (hsame : rec.baseRev = baseRev ∧ rec.baseDigest = baseDigest) :
    applyStep s a baseRev baseDigest key newRev newDigest
      = (ApplyOutcome.replay rec.resultRev, s) := by
  obtain ⟨h1, h2⟩ := hsame
  simp [applyStep, hrec, h1, h2]

theorem applyStep_keyReuse (s : Store) (a baseRev baseDigest key newRev newDigest : Nat)
    (rec : OpRecord) (hrec : s.opOf a key = some rec)
    (hdiff : ¬ (rec.baseRev = baseRev ∧ rec.baseDigest = baseDigest)) :
    applyStep s a baseRev baseDigest key newRev newDigest
      = (ApplyOutcome.keyReuse, s) := by
  simp [applyStep, hrec, hdiff]

theorem applyStep_unknownBase (s : Store) (a baseRev baseDigest key newRev newDigest : Nat)
    (hrec : s.opOf a key = none) (hnone : s.baseDigestOf a baseRev = none) :
    applyStep s a baseRev baseDigest key newRev newDigest
      = (ApplyOutcome.unknownBase, s) := by
  simp [applyStep, hrec, hnone]

theorem applyStep_conflict (s : Store) (a baseRev baseDigest key newRev newDigest stored : Nat)
    (hrec : s.opOf a key = none) (hstored : s.baseDigestOf a baseRev = some stored)
    (hne : stored ≠ baseDigest) :
    applyStep s a baseRev baseDigest key newRev newDigest
      = (ApplyOutcome.conflict, s) := by
  simp [applyStep, hrec, hstored, hne]

theorem applyStep_applied (s : Store) (a baseRev baseDigest key newRev newDigest stored : Nat)
    (hrec : s.opOf a key = none) (hstored : s.baseDigestOf a baseRev = some stored)
    (heq : stored = baseDigest) :
    applyStep s a baseRev baseDigest key newRev newDigest
      = (ApplyOutcome.applied newRev,
         { revisions := (a, newRev, newDigest) :: s.revisions,
           ops := (a, { key := key, baseRev := baseRev, baseDigest := baseDigest,
                        resultRev := newRev, resultDigest := newDigest }) :: s.ops }) := by
  simp [applyStep, hrec, hstored, heq]

-- ── The revision invariants ─────────────────────────────────────────────────

/-- Lookup skips a non-matching cons head (used by INV-5). -/
theorem lookupRevision_cons_ne (a' r' d : Nat)
    (rest : List (Nat × Nat × Nat)) (a r : Nat)
    (h : ¬ (a' = a ∧ r' = r)) :
    lookupRevision ((a', r', d) :: rest) a r = lookupRevision rest a r := by
  simp only [lookupRevision]
  split
  · next hcond => exact absurd hcond h
  · rfl

/-- INV-1: a failed apply (conflict, key reuse, or unknown base) leaves the
store unchanged — every previously stored revision, the base included,
stays exactly as it was. -/
theorem failed_apply_leaves_store_unchanged
    (s : Store) (a baseRev baseDigest key newRev newDigest : Nat) :
    ((applyStep s a baseRev baseDigest key newRev newDigest).1 = ApplyOutcome.conflict ∨
     (applyStep s a baseRev baseDigest key newRev newDigest).1 = ApplyOutcome.keyReuse ∨
     (applyStep s a baseRev baseDigest key newRev newDigest).1 = ApplyOutcome.unknownBase) →
    (applyStep s a baseRev baseDigest key newRev newDigest).2 = s := by
  intro h
  cases hOp : s.opOf a key with
  | none =>
    cases hRev : s.baseDigestOf a baseRev with
    | none => simp [applyStep, hOp, hRev]
    | some stored =>
      by_cases hEq : stored = baseDigest
      · simp [applyStep, hOp, hRev, hEq] at h
      · simp [applyStep, hOp, hRev, hEq]
  | some rec =>
    by_cases hSame : rec.baseRev = baseRev ∧ rec.baseDigest = baseDigest
    · obtain ⟨h1, h2⟩ := hSame
      simp [applyStep, hOp, h1, h2] at h
    · simp [applyStep, hOp, hSame]

/-- INV-1 corollary: the base revision stays readable with its exact digest
after a failed apply. -/
theorem failed_apply_keeps_base_readable
    (s : Store) (a baseRev baseDigest key newRev newDigest stored : Nat)
    (hstored : s.baseDigestOf a baseRev = some stored)
    (hfail : (applyStep s a baseRev baseDigest key newRev newDigest).1 = ApplyOutcome.conflict) :
    lookupRevision (applyStep s a baseRev baseDigest key newRev newDigest).2.revisions a baseRev
      = some stored := by
  have hunchanged : (applyStep s a baseRev baseDigest key newRev newDigest).2 = s := by
    apply failed_apply_leaves_store_unchanged
    left
    exact hfail
  rw [hunchanged]
  exact hstored

/-- INV-2: a digest mismatch is a conflict, never a silent overwrite — the
store is unchanged and nothing is published under the new revision id.
(The unrecorded-key hypothesis mirrors `handle_apply`'s branch order:
the idempotency lookup runs before digest verification.) -/
theorem digest_mismatch_is_conflict_never_overwrite
    (s : Store) (a baseRev baseDigest key newRev newDigest stored : Nat)
    (hnokey : s.opOf a key = none)
    (hstored : s.baseDigestOf a baseRev = some stored) (hne : stored ≠ baseDigest)
    (hfresh : lookupRevision s.revisions a newRev = none) :
    (applyStep s a baseRev baseDigest key newRev newDigest).1 = ApplyOutcome.conflict ∧
    (applyStep s a baseRev baseDigest key newRev newDigest).2 = s ∧
    lookupRevision (applyStep s a baseRev baseDigest key newRev newDigest).2.revisions a newRev
      = none := by
  have h := applyStep_conflict s a baseRev baseDigest key newRev newDigest stored
    hnokey hstored hne
  rw [h]
  exact ⟨rfl, rfl, hfresh⟩

/-- INV-3: an idempotent replay returns the recorded result revision and
mints nothing — the store is unchanged. -/
theorem idempotent_replay_returns_recorded_result
    (s : Store) (a baseRev baseDigest key newRev newDigest : Nat)
    (rec : OpRecord) (hrec : s.opOf a key = some rec)
    (hsame : rec.baseRev = baseRev ∧ rec.baseDigest = baseDigest) :
    (applyStep s a baseRev baseDigest key newRev newDigest).1
      = ApplyOutcome.replay rec.resultRev ∧
    (applyStep s a baseRev baseDigest key newRev newDigest).2 = s ∧
    lookupRevision (applyStep s a baseRev baseDigest key newRev newDigest).2.revisions a newRev
      = lookupRevision s.revisions a newRev := by
  have h := applyStep_replay s a baseRev baseDigest key newRev newDigest rec hrec hsame
  rw [h]
  exact ⟨rfl, rfl, rfl⟩

/-- INV-4: a key reused against a different base is rejected and the store
is unchanged. -/
theorem key_reuse_against_different_base_is_rejected
    (s : Store) (a baseRev baseDigest key newRev newDigest : Nat)
    (rec : OpRecord) (hrec : s.opOf a key = some rec)
    (hdiff : ¬ (rec.baseRev = baseRev ∧ rec.baseDigest = baseDigest)) :
    (applyStep s a baseRev baseDigest key newRev newDigest).1 = ApplyOutcome.keyReuse ∧
    (applyStep s a baseRev baseDigest key newRev newDigest).2 = s := by
  have h := applyStep_keyReuse s a baseRev baseDigest key newRev newDigest rec hrec hdiff
  rw [h]
  exact ⟨rfl, rfl⟩

/-- INV-5: a successful apply extends the revision history by exactly one
cons — every previously stored revision remains stored (immutability), and
the base stays readable with its exact digest. -/
theorem applied_extends_history_and_preserves_base
    (s : Store) (a baseRev baseDigest key newRev newDigest stored : Nat)
    (hnokey : s.opOf a key = none)
    (hstored : s.baseDigestOf a baseRev = some stored) (heq : stored = baseDigest)
    (hne : newRev ≠ baseRev) :
    (applyStep s a baseRev baseDigest key newRev newDigest).2.revisions
      = (a, newRev, newDigest) :: s.revisions ∧
    ((∀ a'' r'' d'', (a'', r'', d'') ∈ s.revisions →
      (a'', r'', d'') ∈ (applyStep s a baseRev baseDigest key newRev newDigest).2.revisions) ∧
    lookupRevision (applyStep s a baseRev baseDigest key newRev newDigest).2.revisions a baseRev
      = some stored) := by
  have happlied := applyStep_applied s a baseRev baseDigest key newRev newDigest stored
    hnokey hstored heq
  rw [happlied]
  refine ⟨rfl, fun a'' r'' d'' hmem => List.Mem.tail _ hmem, ?_⟩
  · have hskip : ¬ (a = a ∧ newRev = baseRev) := by
      intro hc
      exact hne hc.2
    rw [lookupRevision_cons_ne a newRev newDigest s.revisions a baseRev hskip]
    exact hstored

-- ── Concrete sanity instances (closed, decidable) ──────────────────────────

/-- One stored base revision (artifact 0, revision 0, digest 1); no records. -/
def sanityStore : Store :=
  { revisions := [(0, 0, 1)], ops := [] }

/-- A matching digest applies; a mismatching digest conflicts — computed. -/
example : (applyStep sanityStore 0 0 1 0 1 2).1 = ApplyOutcome.applied 1 := by decide

example : (applyStep sanityStore 0 0 0 0 1 2).1 = ApplyOutcome.conflict := by decide

example : (applyStep sanityStore 0 5 1 0 1 2).1 = ApplyOutcome.unknownBase := by decide

end SpreadsheetRevisionSpec

#print axioms SpreadsheetRevisionSpec.digest_mismatch_is_conflict_never_overwrite
#print axioms SpreadsheetRevisionSpec.failed_apply_leaves_store_unchanged
#print axioms SpreadsheetRevisionSpec.idempotent_replay_returns_recorded_result
#print axioms SpreadsheetRevisionSpec.key_reuse_against_different_base_is_rejected
#print axioms SpreadsheetRevisionSpec.applied_extends_history_and_preserves_base
