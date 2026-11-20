//! MT19937 with CPython's seeding and shuffle semantics, so the Rust
//! instrument reproduces the retired Python harness's GSM8K problem order
//! exactly (rows stay comparable with the archived receipts).
//!
//! PORTED from the data-dir harness's validated mt.rs (the P4 "kept
//! components port in" list): `init_genrand` + `init_by_array` with
//! CPython's exact formulas, the unrolled twist, and — critically —
//! the shuffle via `_randbelow_with_getrandbits` rejection sampling,
//! which is CPython's ACTUAL `random.Random.shuffle` path. The previous
//! in-repo draft was a broken rewrite: its `init_by_array` second loop
//! (`while i < 624` with the CPython wrap-reset inside) never
//! terminated, and neither its seeding formulas nor its
//! `int(random() * (i + 1))` shuffle matched CPython — found live at
//! consolidation P4 when the unit test spun at 100% CPU and the order
//! oracle test had never run. The order is pinned by
//! `tests/mt_order.rs` against the archived JSONL `problem_id`
//! sequences (skips visibly when the archive is absent).

pub struct PyRandom {
    st: [u32; 624],
    idx: usize,
}

fn mag01(y: u32) -> u32 {
    if y & 1 != 0 { 0x9908_b0df } else { 0 }
}

impl PyRandom {
    /// CPython `random.Random(seed)`: the seed's 32-bit words feed
    /// `init_by_array` (little-endian, CPython's int-to-key conversion).
    pub fn new(seed: u64) -> Self {
        let mut key = vec![seed as u32];
        if seed >> 32 != 0 {
            key.push((seed >> 32) as u32);
        }
        let mut r = PyRandom {
            st: [0u32; 624],
            idx: 624,
        };
        r.init_by_array(&key);
        r
    }

    fn init_genrand(&mut self, s: u32) {
        self.st[0] = s;
        for i in 1..624usize {
            self.st[i] = 1812433253u32
                .wrapping_mul(self.st[i - 1] ^ (self.st[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.idx = 624;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19650218);
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = if 624 > key.len() { 624 } else { key.len() };
        while k > 0 {
            self.st[i] = (self.st[i]
                ^ (self.st[i - 1] ^ (self.st[i - 1] >> 30)).wrapping_mul(1664525))
            .wrapping_add(key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                self.st[0] = self.st[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        // CPython's second loop is COUNTED (for k = N-1; k; k--) — the
        // wrap-reset belongs INSIDE it; a `while i < N` head with this
        // reset never terminates (the bug this port replaces).
        k = 623;
        while k > 0 {
            self.st[i] = (self.st[i]
                ^ (self.st[i - 1] ^ (self.st[i - 1] >> 30)).wrapping_mul(1566083941))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                self.st[0] = self.st[623];
                i = 1;
            }
            k -= 1;
        }
        self.st[0] = 0x8000_0000;
    }

    fn gen_u32(&mut self) -> u32 {
        if self.idx >= 624 {
            for kk in 0..(624 - 397) {
                let y = (self.st[kk] & 0x8000_0000) | (self.st[kk + 1] & 0x7fff_ffff);
                self.st[kk] = self.st[kk + 397] ^ (y >> 1) ^ mag01(y);
            }
            for kk in (624 - 397)..623 {
                let y = (self.st[kk] & 0x8000_0000) | (self.st[kk + 1] & 0x7fff_ffff);
                self.st[kk] = self.st[kk - (624 - 397)] ^ (y >> 1) ^ mag01(y);
            }
            let y = (self.st[623] & 0x8000_0000) | (self.st[0] & 0x7fff_ffff);
            self.st[623] = self.st[396] ^ (y >> 1) ^ mag01(y);
            self.idx = 0;
        }
        let mut y = self.st[self.idx];
        self.idx += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// CPython `getrandbits(k)` for 1 <= k <= 32: the high k bits of one
    /// word.
    fn getrandbits(&mut self, k: u32) -> u32 {
        let k = k.clamp(1, 32);
        self.gen_u32() >> (32 - k)
    }

    /// CPython `_randbelow_with_getrandbits`: k = n.bit_length(), reject
    /// r >= n.
    fn randbelow(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let k = 32 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    /// CPython `random.Random.shuffle` over 0..n: reversed Fisher-Yates
    /// via `_randbelow` — the exact path the retired instrument's problem
    /// order came from (pinned by tests/mt_order.rs against the
    /// archived rows).
    pub fn shuffle_order(&mut self, n: usize) -> Vec<usize> {
        let mut x: Vec<usize> = (0..n).collect();
        if n < 2 {
            return x;
        }
        for i in (1..n).rev() {
            let j = self.randbelow((i + 1) as u32) as usize;
            x.swap(i, j);
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt_is_deterministic_and_terminates() {
        // Self-consistency + termination (the previous rewrite never
        // returned from construction — this is the regression floor).
        // The CPython-value pin is tests/mt_order.rs against the
        // archived problem_id sequences — the real oracle.
        let mut a = PyRandom::new(20261106);
        let mut b = PyRandom::new(20261106);
        assert_eq!(a.shuffle_order(100), b.shuffle_order(100));
        assert_eq!(a.shuffle_order(7473), b.shuffle_order(7473));
        // Deterministic across constructions.
        let c = PyRandom::new(20261106).shuffle_order(100);
        assert_eq!(c, PyRandom::new(20261106).shuffle_order(100));
    }
}
