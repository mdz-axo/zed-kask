/-
zed-kask spec pin (PRF-05 — `hkask-nostr-core` NIP-57 `fe5_to_32_bytes` canonical
5-bit-to-8-bit bit-packing injectivity).

The Rust side: `kask/crates/hkask-nostr-core/src/zaps.rs` (`fe5_to_32_bytes`).
52 5-bit bech32 words encode 260 bits = 32 bytes (256 bits) + 4 trailing padding
bits (`words[51] & 0x0f`). Slice S3 enforced `(words[51] & 0x0f) == 0` so that
every 32-byte `description_hash` has a unique canonical 52-word encoding.

This file proves:
1. Big-endian base-32 value (`fromBase 32`) is injective over fixed-length lists
   of 5-bit words (`∀ w ∈ ws, w < 32`).
2. Dividing a 260-bit value by `16` (stripping 4 trailing bits) is injective on
   words whose last word satisfies `w_last % 16 = 0` — hence `fe5_to_32_bytes`
   with the zero-padding check is injective onto 256-bit hashes.
3. Without the zero-padding check, distinct 52-word encodings collide (proven as
   a concrete witness theorem `non_zero_padding_collides`).

Check: `lean_check` on this file (pinned Lean 4.34.0); the paired
`nostr_fe5_packing_negative_control.lean` must FAIL the same check.
-/

namespace NostrFe5Packing

/-- Big-endian positional value in base `b` (MSB first). -/
def fromBase (b : Nat) : List Nat → Nat
  | [] => 0
  | w :: ws => w * b ^ ws.length + fromBase b ws

theorem fromBase_lt (b : Nat) (hb : 0 < b) :
    ∀ (ws : List Nat), (∀ w ∈ ws, w < b) → fromBase b ws < b ^ ws.length
  | [], _ => by simp [fromBase]
  | w :: ws, hall => by
      have hw : w < b := hall w (by simp)
      have hws : ∀ x ∈ ws, x < b := fun x hx => hall x (by simp [hx])
      have ih := fromBase_lt b hb ws hws
      simp only [fromBase, List.length_cons, Nat.pow_succ]
      have hmul : (w + 1) * b ^ ws.length ≤ b * b ^ ws.length :=
        Nat.mul_le_mul_right (b ^ ws.length) hw
      have hdist : (w + 1) * b ^ ws.length = w * b ^ ws.length + b ^ ws.length := by
        rw [Nat.add_mul, Nat.one_mul]
      have hcomm : b * b ^ ws.length = b ^ ws.length * b := Nat.mul_comm _ _
      omega

/-- Positional encoding in base `b > 0` is injective on equal-length digit lists
whose digits are strictly below `b`. -/
theorem fromBase_injective (b : Nat) (hb : 0 < b) :
    ∀ (xs ys : List Nat),
      xs.length = ys.length →
      (∀ x ∈ xs, x < b) →
      (∀ y ∈ ys, y < b) →
      fromBase b xs = fromBase b ys →
      xs = ys
  | [], [], _, _, _, _ => rfl
  | x :: xs, y :: ys, hlen, hxs, hys, heq => by
      have hlen' : xs.length = ys.length := by simpa using hlen
      have hx : x < b := hxs x (by simp)
      have hy : y < b := hys y (by simp)
      have hxs' : ∀ a ∈ xs, a < b := fun a ha => hxs a (by simp [ha])
      have hys' : ∀ a ∈ ys, a < b := fun a ha => hys a (by simp [ha])
      have hlt_x : fromBase b xs < b ^ xs.length := fromBase_lt b hb xs hxs'
      have hlt_y : fromBase b ys < b ^ ys.length := fromBase_lt b hb ys hys'
      simp only [fromBase] at heq
      rw [hlen'] at heq hlt_x
      have hpow_pos : 0 < b ^ ys.length := Nat.pow_pos hb
      have hxy : x = y := by
        rcases Nat.lt_trichotomy x y with hlt | heq_xy | hgt
        · have hstep : (x + 1) * b ^ ys.length ≤ y * b ^ ys.length :=
            Nat.mul_le_mul_right (b ^ ys.length) hlt
          have hdist : (x + 1) * b ^ ys.length = x * b ^ ys.length + b ^ ys.length := by
            rw [Nat.add_mul, Nat.one_mul]
          omega
        · exact heq_xy
        · have hstep : (y + 1) * b ^ ys.length ≤ x * b ^ ys.length :=
            Nat.mul_le_mul_right (b ^ ys.length) hgt
          have hdist : (y + 1) * b ^ ys.length = y * b ^ ys.length + b ^ ys.length := by
            rw [Nat.add_mul, Nat.one_mul]
          omega
      subst hxy
      have htail : fromBase b xs = fromBase b ys := by omega
      have hrest := fromBase_injective b hb xs ys hlen' hxs' hys' htail
      rw [hrest]

/-- Value of a non-empty list ending in `last`: `fromBase b (init ++ [last]) = fromBase b init * b + last`. -/
theorem fromBase_append_singleton (b : Nat) (init : List Nat) (last : Nat) :
    fromBase b (init ++ [last]) = fromBase b init * b + last := by
  induction init with
  | nil => simp [fromBase]
  | cons x xs ih =>
      simp only [List.cons_append, fromBase, List.length_append, List.length_singleton, ih]
      rw [Nat.pow_succ, Nat.add_mul]
      have hassoc : x * (b ^ xs.length * b) = x * b ^ xs.length * b := by
        rw [Nat.mul_assoc]
      omega

/-- The 256-bit decoded value of a 52-word 5-bit bech32 payload is `fromBase 32 ws / 16`
(dropping the 4 trailing padding bits). -/
def decodedHashNat (ws : List Nat) : Nat :=
  fromBase 32 ws / 16

/-- Canonical-encoding injectivity (PRF-05):
Two 52-word 5-bit sequences whose final 4 padding bits are zero (`last % 16 = 0`)
and that decode to the same 256-bit hash are identical. -/
theorem fe5_canonical_injective
    {init1 init2 : List Nat} {last1 last2 : Nat}
    (hlen1 : init1.length = 51) (hlen2 : init2.length = 51)
    (hvalid1 : ∀ w ∈ (init1 ++ [last1]), w < 32)
    (hvalid2 : ∀ w ∈ (init2 ++ [last2]), w < 32)
    (hpad1 : last1 % 16 = 0) (hpad2 : last2 % 16 = 0)
    (hdec : decodedHashNat (init1 ++ [last1]) = decodedHashNat (init2 ++ [last2])) :
    init1 ++ [last1] = init2 ++ [last2] := by
  have hv1 := fromBase_append_singleton 32 init1 last1
  have hv2 := fromBase_append_singleton 32 init2 last2
  unfold decodedHashNat at hdec
  rw [hv1, hv2] at hdec
  have hmod1 : (fromBase 32 init1 * 32 + last1) % 16 = 0 := by omega
  have hmod2 : (fromBase 32 init2 * 32 + last2) % 16 = 0 := by omega
  have hfull : fromBase 32 (init1 ++ [last1]) = fromBase 32 (init2 ++ [last2]) := by
    rw [hv1, hv2]
    omega
  have hlen : (init1 ++ [last1]).length = (init2 ++ [last2]).length := by
    simp [hlen1, hlen2]
  exact fromBase_injective 32 (by decide) (init1 ++ [last1]) (init2 ++ [last2])
    hlen hvalid1 hvalid2 hfull

/-- Witness that dropping the 4-bit padding check breaks injectivity:
`[0, ..., 0, 0]` and `[0, ..., 0, 1]` are distinct 52-word sequences that decode
to the exact same 256-bit hash `0`. -/
theorem non_zero_padding_collides :
    ∃ (ws1 ws2 : List Nat),
      ws1.length = 52 ∧ ws2.length = 52 ∧
      (∀ w ∈ ws1, w < 32) ∧ (∀ w ∈ ws2, w < 32) ∧
      ws1 ≠ ws2 ∧
      decodedHashNat ws1 = decodedHashNat ws2 := by
  refine ⟨List.replicate 51 0 ++ [0], List.replicate 51 0 ++ [1], ?_, ?_, ?_, ?_, ?_, ?_⟩
  · simp
  · simp
  · intro w hw
    simp only [List.mem_append, List.mem_replicate, List.mem_singleton] at hw
    rcases hw with ⟨_, rfl⟩ | rfl <;> omega
  · intro w hw
    simp only [List.mem_append, List.mem_replicate, List.mem_singleton] at hw
    rcases hw with ⟨_, rfl⟩ | rfl <;> omega
  · intro h
    have := congrArg List.getLast? h
    simp at this
  · unfold decodedHashNat
    rw [fromBase_append_singleton, fromBase_append_singleton]
    have hzero : fromBase 32 (List.replicate 51 0) = 0 := by decide
    rw [hzero]

end NostrFe5Packing

#print axioms NostrFe5Packing.fromBase_injective
#print axioms NostrFe5Packing.fe5_canonical_injective
#print axioms NostrFe5Packing.non_zero_padding_collides
