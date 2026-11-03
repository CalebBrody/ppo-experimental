//! Deterministic pseudo-random numbers, with the helpers Showdown's battle
//! code uses: `random(n)`, `random_range(a, b)`, `chance(num, den)`, `sample`
//! and partial `shuffle`.
//!
//! Sampled play uses two generators:
//! - xoshiro256++, fast, for self-play and search (fast mode);
//! - Showdown's own `SodiumRNG` (ChaCha20, as libsodium's
//!   `randombytes_buf_deterministic`), so that a battle seeded like Showdown's
//!   draws the same numbers (reference mode).
//!
//! Enumeration uses a third backend, `Precise`, which replays weighted integer
//! decisions without invoking either generator.
//!
//! Both sampled generators produce 32-bit draws that the helpers scale exactly as Showdown's
//! `PRNG.random` does, so a helper's result depends only on the draw.

use crate::EnumerationMode;

#[derive(Clone, Debug)]
pub struct Rng {
    gen: Gen,
    /// When on, every draw is recorded with its call site (for comparing the
    /// order of draws with Showdown's).
    pub trace: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
enum Gen {
    Xoshiro([u64; 4]),
    /// Showdown's sodium seed: a 32-byte ChaCha20 key, replaced at each draw.
    Sodium([u8; 32]),
    Precise(PreciseState),
}

const DRAW_COUNT: u64 = 1u64 << 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecisionKind {
    Integer(u32),
    Chance(u32, u32),
    Damage(u32),
}

#[derive(Clone, Debug)]
struct Decision {
    kind: DecisionKind,
    alternatives: u32,
    selected: u32,
}

/// A depth-first path through conditional chance decisions. Suffixes belong
/// only to their prefix and are discarded when that prefix changes.
#[derive(Clone, Debug)]
pub(crate) struct PreciseState {
    mode: EnumerationMode,
    path: Vec<Decision>,
    cursor: usize,
    pub(crate) probability: f64,
}

impl PreciseState {
    pub(crate) fn new(mode: EnumerationMode) -> Self {
        Self {
            mode,
            path: Vec::new(),
            cursor: 0,
            probability: 1.0,
        }
    }

    fn choose(&mut self, kind: DecisionKind, alternatives: u32) -> (u32, f64) {
        if self.cursor == self.path.len() {
            self.path.push(Decision {
                kind,
                alternatives,
                selected: 0,
            });
        }
        let decision = &self.path[self.cursor];
        assert_eq!(decision.kind, kind, "chance replay diverged");
        assert_eq!(
            decision.alternatives, alternatives,
            "chance replay bounds diverged"
        );
        let k = decision.selected;
        let probability = match kind {
            DecisionKind::Integer(n) => {
                let lo = (u64::from(k) * DRAW_COUNT).div_ceil(u64::from(n));
                let hi = ((u64::from(k) + 1) * DRAW_COUNT).div_ceil(u64::from(n));
                (hi - lo) as f64 / DRAW_COUNT as f64
            }
            DecisionKind::Chance(num, den) => {
                let yes = (u64::from(num.min(den)) * DRAW_COUNT).div_ceil(u64::from(den));
                if alternatives == 1 {
                    1.0
                } else if k == 0 {
                    yes as f64 / DRAW_COUNT as f64
                } else {
                    (DRAW_COUNT - yes) as f64 / DRAW_COUNT as f64
                }
            }
            DecisionKind::Damage(_) => 1.0 / f64::from(alternatives),
        };
        self.cursor += 1;
        self.probability *= probability;
        (k, probability)
    }

    pub(crate) fn advance(&mut self) -> bool {
        self.path.truncate(self.cursor);
        while let Some(last) = self.path.last_mut() {
            if last.selected + 1 < last.alternatives {
                last.selected += 1;
                self.cursor = 0;
                self.probability = 1.0;
                return true;
            }
            self.path.pop();
        }
        false
    }
}

// Thread-local so the no-PRNG regression test can run alongside sampled tests.
#[cfg(test)]
std::thread_local! {
    static FORBID_PRNG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn assert_prng_allowed() {
    FORBID_PRNG.with(|forbidden| assert!(!forbidden.get(), "enumeration invoked a PRNG"));
}

fn splitmix64(state: &mut u64) -> u64 {
    #[cfg(test)]
    assert_prng_allowed();
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Nonce of libsodium's deterministic random bytes ("LibsodiumDRG").
const SODIUM_NONCE: &[u8; 12] = b"LibsodiumDRG";

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// The first 64-byte ChaCha20 (RFC 8439) keystream block for this key, the
/// sodium nonce and block counter 0.
fn chacha20_block(key: &[u8; 32]) -> [u8; 64] {
    #[cfg(test)]
    assert_prng_allowed();
    let mut init = [0u32; 16];
    init[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for i in 0..8 {
        init[4 + i] = u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap());
    }
    init[12] = 0;
    for i in 0..3 {
        init[13 + i] = u32::from_le_bytes(SODIUM_NONCE[4 * i..4 * i + 4].try_into().unwrap());
    }
    let mut s = init;
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&s[i].wrapping_add(init[i]).to_le_bytes());
    }
    out
}

impl Rng {
    /// Fast mode: xoshiro256++ from a 64-bit seed.
    pub fn new(seed: u64) -> Rng {
        let mut st = seed;
        let state = [
            splitmix64(&mut st),
            splitmix64(&mut st),
            splitmix64(&mut st),
            splitmix64(&mut st),
        ];
        Rng {
            gen: Gen::Xoshiro(state),
            trace: None,
        }
    }

    /// Reference mode: Showdown's PRNG for a seed like "sodium,0123abcd...".
    pub fn from_showdown_seed(seed: &str) -> Result<Rng, String> {
        let hex = seed
            .strip_prefix("sodium,")
            .ok_or_else(|| format!("not a sodium seed: {seed}"))?;
        if hex.len() > 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("bad sodium seed: {seed}"));
        }
        // Showdown pads the hex to 32 bytes with zeros.
        let padded = format!("{hex:0<64}");
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = u8::from_str_radix(&padded[2 * i..2 * i + 2], 16).unwrap();
        }
        Ok(Rng {
            gen: Gen::Sodium(key),
            trace: None,
        })
    }

    /// The seed as Showdown prints it (`battle.prng.getSeed()`), in reference mode.
    pub fn showdown_seed(&self) -> Option<String> {
        match &self.gen {
            Gen::Sodium(key) => Some(format!(
                "sodium,{}",
                key.iter().map(|b| format!("{b:02x}")).collect::<String>()
            )),
            Gen::Xoshiro(_) | Gen::Precise(_) => None,
        }
    }

    fn next_xoshiro(s: &mut [u64; 4]) -> u64 {
        #[cfg(test)]
        assert_prng_allowed();
        let result = (s[0].wrapping_add(s[3])).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// A 32-bit draw (Showdown's `rng.next()`).
    pub fn next_u32(&mut self) -> u32 {
        match &mut self.gen {
            Gen::Precise(_) => panic!("raw PRNG draw during enumeration"),
            Gen::Xoshiro(s) => (Rng::next_xoshiro(s) >> 32) as u32,
            Gen::Sodium(key) => {
                let block = chacha20_block(key);
                key.copy_from_slice(&block[..32]);
                u32::from_be_bytes(block[32..36].try_into().unwrap())
            }
        }
    }

    /// 64 random bits, for seeding (not used by battle code).
    pub fn next_u64(&mut self) -> u64 {
        match &mut self.gen {
            Gen::Precise(_) => panic!("raw PRNG draw during enumeration"),
            Gen::Xoshiro(s) => Rng::next_xoshiro(s),
            Gen::Sodium(_) => ((self.next_u32() as u64) << 32) | self.next_u32() as u64,
        }
    }

    /// Uniform integer in `[0, n)`: Showdown's `random(n)`, floor(draw * n / 2^32).
    #[track_caller]
    pub fn random(&mut self, n: u32) -> u32 {
        assert!(n > 0);
        if let Gen::Precise(state) = &mut self.gen {
            return state.choose(DecisionKind::Integer(n), n).0;
        }
        let r = ((self.next_u32() as u64 * n as u64) >> 32) as u32;
        if let Some(t) = &mut self.trace {
            let at = std::panic::Location::caller();
            t.push(format!("random({n}) = {r} @ {}:{}", at.file(), at.line()));
        }
        r
    }

    /// Damage variance: only AlmostPrecise replaces the distribution by endpoints.
    /// Delegation preserves ordinary play's draw count and caller trace.
    #[track_caller]
    pub fn random_damage(&mut self, n: u32) -> u32 {
        assert!(n > 0);
        if let Gen::Precise(state) = &mut self.gen {
            if state.mode == EnumerationMode::AlmostPrecise {
                let (selected, _) =
                    state.choose(DecisionKind::Damage(n), if n == 1 { 1 } else { 2 });
                return if selected == 0 { 0 } else { n - 1 };
            }
        }
        self.random(n)
    }

    pub(crate) fn precise(state: PreciseState) -> Self {
        Self {
            gen: Gen::Precise(state),
            trace: None,
        }
    }

    pub(crate) fn into_precise(self) -> PreciseState {
        match self.gen {
            Gen::Precise(state) => state,
            _ => unreachable!("enumeration lost its precise generator"),
        }
    }

    /// Uniform integer in `[lo, hi)`, like Showdown's `random(lo, hi)`.
    #[track_caller]
    pub fn random_range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.random(hi - lo)
    }

    /// True with probability `num / den`, like `randomChance`.
    #[track_caller]
    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        assert!(den > 0);
        if let Gen::Precise(state) = &mut self.gen {
            let count = (u64::from(num.min(den)) * DRAW_COUNT).div_ceil(u64::from(den));
            let alternatives = if count == 0 || count == DRAW_COUNT {
                1
            } else {
                2
            };
            let (selected, _) = state.choose(DecisionKind::Chance(num, den), alternatives);
            return count == DRAW_COUNT || (count != 0 && selected == 0);
        }
        self.random(den) < num
    }

    #[track_caller]
    pub fn sample<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.random(items.len() as u32) as usize]
    }

    /// Fisher–Yates shuffle of `items[start..end]`, like Showdown's `shuffle`.
    #[track_caller]
    pub fn shuffle<T>(&mut self, items: &mut [T], start: usize, end: usize) {
        let mut start = start;
        while start < end - 1 {
            let next = self.random_range(start as u32, end as u32) as usize;
            if start != next {
                items.swap(start, next);
            }
            start += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chacha20_matches_rfc8439_keystream() {
        // RFC 8439 2.4.2 uses another nonce; check the block function on the
        // all-zero key and nonce (test vector 1 of A.1) through a local copy.
        let key = [0u8; 32];
        let mut init = [0u32; 16];
        init[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
        let mut s = init;
        for _ in 0..10 {
            quarter(&mut s, 0, 4, 8, 12);
            quarter(&mut s, 1, 5, 9, 13);
            quarter(&mut s, 2, 6, 10, 14);
            quarter(&mut s, 3, 7, 11, 15);
            quarter(&mut s, 0, 5, 10, 15);
            quarter(&mut s, 1, 6, 11, 12);
            quarter(&mut s, 2, 7, 8, 13);
            quarter(&mut s, 3, 4, 9, 14);
        }
        let first = s[0].wrapping_add(init[0]).to_le_bytes();
        assert_eq!(first, [0x76, 0xb8, 0xe0, 0xad]);
        let _ = key;
    }

    #[test]
    fn sodium_matches_showdown() {
        // Showdown: new PRNG('sodium,0123456789abcdef0123456789abcdef').rng.next() x 5.
        let mut r = Rng::from_showdown_seed("sodium,0123456789abcdef0123456789abcdef").unwrap();
        let draws: Vec<u32> = (0..5).map(|_| r.next_u32()).collect();
        assert_eq!(
            draws,
            [1564598223, 2628916691, 3021397871, 1916167142, 4157841405]
        );
        assert_eq!(
            r.showdown_seed().unwrap(),
            "sodium,51b3d614ec21851a6419bc898430b29563817b1ec4fa00ea8c2cf63b160efc26"
        );
        // random(16), random(100), random(2, 5), randomChance(1, 3), random(6).
        let mut q = Rng::from_showdown_seed("sodium,0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            (
                q.random(16),
                q.random(100),
                q.random_range(2, 5),
                q.chance(1, 3),
                q.random(6)
            ),
            (5, 61, 4, false, 5)
        );
    }

    #[test]
    fn showdown_seed_round_trips() {
        let r = Rng::from_showdown_seed("sodium,0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            r.showdown_seed().unwrap(),
            "sodium,0123456789abcdef0123456789abcdef00000000000000000000000000000000"
        );
    }
}

#[cfg(test)]
mod enumeration_tests {
    use super::*;
    fn outcomes<T>(mode: EnumerationMode, f: impl Fn(&mut Rng) -> T) -> Vec<(T, f64)> {
        let mut path = PreciseState::new(mode);
        let mut out = Vec::new();
        loop {
            let mut rng = Rng::precise(path);
            let value = f(&mut rng);
            path = rng.into_precise();
            out.push((value, path.probability));
            if !path.advance() {
                return out;
            }
        }
    }
    struct NoPrng;
    impl NoPrng {
        fn enter() -> Self {
            FORBID_PRNG.with(|flag| flag.set(true));
            Self
        }
    }
    impl Drop for NoPrng {
        fn drop(&mut self) {
            FORBID_PRNG.with(|flag| flag.set(false));
        }
    }

    #[test]
    fn almost_precise_never_calls_a_prng() {
        use crate::{Battle, Dex};
        use std::sync::Arc;
        let dex =
            Arc::new(Dex::from_json(include_str!("../tests/fixtures/enumeration.json")).unwrap());
        let team = "Fast|||battlearmor|Tackle,Double Hit|Hardy|||||50|";
        let mut battle = Battle::new(
            dex,
            [team, "Slow|||battlearmor|Splash|Hardy|||||50|"],
            ["a", "b"],
            1,
        )
        .unwrap();
        battle.choose_team(0, vec![0]);
        battle.choose_team(1, vec![0]);
        let sampled = [
            Rng::new(19),
            Rng::from_showdown_seed("sodium,1234").unwrap(),
        ];
        // Tripwires are in the actual xoshiro, ChaCha and seed-expansion code,
        // so even accidentally using a fresh or separate RNG fails this test.
        let _guard = NoPrng::enter();
        let helper_paths = outcomes(EnumerationMode::AlmostPrecise, |r| {
            let mut shuffled = [0, 1, 2];
            r.shuffle(&mut shuffled, 0, 3);
            (
                r.random(3),
                r.random_range(2, 4),
                r.chance(3, 10),
                r.sample(&[10, 20]),
                r.random_damage(16),
                shuffled,
            )
        });
        assert!((helper_paths.iter().map(|x| x.1).sum::<f64>() - 1.0).abs() < 1e-12);
        for rng in sampled {
            battle.rng = rng;
            for choice in ["move 1", "move 2"] {
                let result = battle
                    .enumerate_outcomes_text(choice, "move 1", EnumerationMode::AlmostPrecise)
                    .unwrap();
                assert!(!result.is_empty());
                assert!((result.iter().map(|o| o.probability).sum::<f64>() - 1.0).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn no_prng_tripwire_detects_both_generators() {
        let mut generators = [
            Rng::new(19),
            Rng::from_showdown_seed("sodium,1234").unwrap(),
        ];
        let _guard = NoPrng::enter();
        for rng in &mut generators {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rng.next_u32())).is_err()
            );
        }
    }

    #[test]
    fn conditional_paths_have_their_own_bounds_and_lengths() {
        let out = outcomes(EnumerationMode::Precise, |r| {
            let a = r.random(3);
            (a, if a == 0 { None } else { Some(r.random(a + 1)) })
        });
        assert_eq!(out.len(), 6);
        assert_eq!(
            out.iter()
                .map(|x| x.0)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            6
        );
        assert!((out.iter().map(|x| x.1).sum::<f64>() - 1.0).abs() < 1e-14);
    }
    #[test]
    fn scaled_integer_masses_and_chance_aggregation_match() {
        let out = outcomes(EnumerationMode::Precise, |r| r.random(3));
        assert_eq!(
            out.iter()
                .map(|x| x.1 * DRAW_COUNT as f64)
                .collect::<Vec<_>>(),
            vec![1431655766.0, 1431655765.0, 1431655765.0]
        );
        let chance = outcomes(EnumerationMode::Precise, |r| r.chance(1, 3));
        assert_eq!(chance, vec![(true, out[0].1), (false, out[1].1 + out[2].1)]);
        assert_eq!(
            outcomes(EnumerationMode::Precise, |r| r.chance(0, 3)),
            vec![(false, 1.0)]
        );
        assert_eq!(
            outcomes(EnumerationMode::Precise, |r| r.chance(4, 3)),
            vec![(true, 1.0)]
        );
    }
    #[test]
    fn only_damage_is_approximated() {
        let precise = outcomes(EnumerationMode::Precise, |r| r.random_damage(16));
        assert_eq!(
            precise,
            (0..16).map(|n| (n, 1.0 / 16.0)).collect::<Vec<_>>()
        );
        let mode = EnumerationMode::AlmostPrecise;
        assert_eq!(
            outcomes(mode, |r| r.random_damage(16)),
            vec![(0, 0.5), (15, 0.5)]
        );
        assert_eq!(outcomes(mode, |r| r.random_damage(1)), vec![(0, 1.0)]);
        assert_eq!(outcomes(mode, |r| r.random(16)), precise);
        assert_eq!(outcomes(mode, |r| r.random_range(4, 20)).len(), 16);
        assert_eq!(outcomes(mode, |r| r.sample(&[0; 16])).len(), 16);
        assert_eq!(
            outcomes(mode, |r| r.chance(3, 100)),
            outcomes(EnumerationMode::Precise, |r| r.chance(3, 100))
        );
        assert_eq!(
            outcomes(mode, |r| (r.random_damage(16), r.random_damage(16))).len(),
            4
        );
    }
    #[test]
    fn shuffle_enumerates_all_permutations() {
        let out = outcomes(EnumerationMode::AlmostPrecise, |r| {
            let mut a = [0, 1, 2];
            r.shuffle(&mut a, 0, 3);
            a
        });
        assert_eq!(out.len(), 6);
        assert_eq!(
            out.iter()
                .map(|x| x.0)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            6
        );
        assert!((out.iter().map(|x| x.1).sum::<f64>() - 1.0).abs() < 1e-14);
    }
    #[test]
    fn damage_helper_preserves_sampled_draws() {
        for mut a in [
            Rng::new(13),
            Rng::from_showdown_seed("sodium,1234").unwrap(),
        ] {
            let mut b = a.clone();
            for _ in 0..100 {
                assert_eq!(a.random_damage(16), b.random(16));
            }
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
    #[test]
    #[should_panic(expected = "raw PRNG draw during enumeration")]
    fn raw_draws_are_forbidden() {
        Rng::precise(PreciseState::new(EnumerationMode::Precise)).next_u32();
    }
}
