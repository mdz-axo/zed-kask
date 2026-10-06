/-
Negative control for the spreadsheet revision-invariant spec pin: this file
must FAIL to check. It asserts the exact failure mode INV-2 forbids — a
digest mismatch silently applying — over the same model, with a concrete
closed instance `decide` can evaluate. A checker that accepts it is not an
oracle. Companion: `spreadsheet_revision_invariants.lean`.
-/

namespace SpreadsheetRevisionNegativeControl

abbrev ArtifactId := Nat
abbrev RevisionId := Nat
abbrev Digest := Nat
abbrev IdemKey := Nat

structure OpRecord where
  key : IdemKey
  baseRev : RevisionId
  baseDigest : Digest
  resultRev : RevisionId
  resultDigest : Digest
deriving DecidableEq

structure Store where
  revisions : List (ArtifactId × RevisionId × Digest)
  ops : List (ArtifactId × OpRecord)
deriving DecidableEq

def lookupRevision (revs : List (ArtifactId × RevisionId × Digest))
    (a : ArtifactId) (r : RevisionId) : Option Digest :=
  match revs with
  | [] => none
  | (a', r', d) :: rest => if a' = a ∧ r' = r then some d else lookupRevision rest a r

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

/-- The stored base revision (artifact 0, revision 0) has digest 1. -/
def store : Store := { revisions := [(0, 0, 1)], ops := [] }

/-- MUST FAIL: the caller names digest 0 for a base stored with digest 1 —
a mismatch. INV-2 (spreadsheet_revision_invariants.lean) says the outcome
is `conflict`; this false claim says the mismatch silently applies. `decide`
evaluates the concrete instance and must reject the equation. -/
example : (applyStep store 0 0 0 0 1 2).1 = ApplyOutcome.applied 1 := by decide

end SpreadsheetRevisionNegativeControl
