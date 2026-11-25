/-
Negative control for `nostr_fe5_packing.lean` (PRF-05).

Omits the `last % 16 = 0` canonical-padding hypothesis (modeling the pre-S3 bug)
and attempts to prove injectivity. Must FAIL `lean_check`.
-/

namespace NostrFe5PackingNegControl

def fromBase (b : Nat) : List Nat → Nat
  | [] => 0
  | w :: ws => w * b ^ ws.length + fromBase b ws

def decodedHashNat (ws : List Nat) : Nat :=
  fromBase 32 ws / 16

theorem unpadded_injective_must_fail
    (ws1 ws2 : List Nat)
    (hlen1 : ws1.length = 52) (hlen2 : ws2.length = 52)
    (hvalid1 : ∀ w ∈ ws1, w < 32) (hvalid2 : ∀ w ∈ ws2, w < 32)
    (hdec : decodedHashNat ws1 = decodedHashNat ws2) :
    ws1 = ws2 := by
  omega

end NostrFe5PackingNegControl
