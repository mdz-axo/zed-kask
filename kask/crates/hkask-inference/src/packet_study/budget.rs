//! Study-local generation-only monetary accounting and separate count attempts.
//! Count attempts do not price the counting endpoint; its billing remains unknown.
//! The caller must fsync each count/reserve event before accessing its response;
//! this offline-replay ledger performs no I/O.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Limits {
    pub(super) max_attempts: u64,
    pub(super) max_input_tokens: u64,
    pub(super) max_output_tokens: u64,
    pub(super) provider_output_max: u64,
    pub(super) spending_ceiling_microusd: u64,
    pub(super) prompt_microusd_per_million_tokens: u64,
    pub(super) completion_microusd_per_million_tokens: u64,
}

impl Limits {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.max_attempts == 0
            || self.max_input_tokens == 0
            || self.max_output_tokens == 0
            || self.provider_output_max == 0
            || self.spending_ceiling_microusd == 0
        {
            return Err("attempt count, token caps and spending ceiling must be nonzero".into());
        }
        if self.max_output_tokens > self.provider_output_max {
            return Err("max_output_tokens exceeds provider_output_max".into());
        }
        self.cost(self.max_input_tokens, self.provider_output_max)?;
        Ok(())
    }

    fn cost(&self, input: u64, output: u64) -> Result<u64, String> {
        let total = u128::from(input)
            .checked_mul(u128::from(self.prompt_microusd_per_million_tokens))
            .and_then(|prompt| {
                u128::from(output)
                    .checked_mul(u128::from(self.completion_microusd_per_million_tokens))
                    .and_then(|completion| prompt.checked_add(completion))
            })
            .ok_or("token tariff arithmetic overflow")?;
        // Round the combined request cost, not each billable component.
        u64::try_from(total.div_ceil(1_000_000))
            .map_err(|_| "token tariff arithmetic overflow".into())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Usage {
    pub(super) prompt_tokens: Option<u64>,
    pub(super) completion_tokens: Option<u64>,
    /// A subset of completion_tokens, never an additional billable count.
    pub(super) reasoning_tokens: Option<u64>,
    pub(super) reported_cost_microusd: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Event {
    Count {
        id: String,
    },
    Reserve {
        id: String,
        input_upper_bound: Option<u64>,
        cap_check: bool,
    },
    Settle {
        id: String,
        usage: Option<Usage>,
    },
    Stop {
        reason: String,
    },
}

#[derive(Debug, Serialize)]
struct Allowance {
    input: u64,
    output: u64,
    settlement_attempted: bool,
    failed: bool,
    // Preserved even if adding exposure to incurred_microusd overflows.
    observed_microusd: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(super) struct Ledger {
    limits: Limits,
    attempts: u64,
    counted: BTreeSet<String>,
    incurred_microusd: u64,
    outstanding: BTreeMap<String, u64>,
    stopped: Option<String>,
    // Includes settled IDs so neither settlement nor replay can reuse an attempt.
    allowances: BTreeMap<String, Allowance>,
}

impl Ledger {
    pub(super) fn new(limits: Limits) -> Result<Self, String> {
        limits.validate()?;
        Ok(Self {
            limits,
            attempts: 0,
            counted: BTreeSet::new(),
            incurred_microusd: 0,
            outstanding: BTreeMap::new(),
            stopped: None,
            allowances: BTreeMap::new(),
        })
    }

    pub(super) fn count(&mut self, id: &str) -> Result<(), String> {
        if let Some(reason) = self.stopped() {
            return Err(format!("ledger stopped: {reason}"));
        }
        if self.counted.contains(id) {
            return Err(format!("count attempt ID already used: {id}"));
        }
        if self.count_attempts() >= self.limits.max_attempts {
            return Err("count attempt limit exhausted".into());
        }
        self.counted.insert(id.into());
        Ok(())
    }

    pub(super) fn reserve(
        &mut self,
        id: &str,
        input_upper_bound: Option<u64>,
        cap_check: bool,
    ) -> Result<u64, String> {
        if let Some(reason) = self.stopped() {
            return Err(format!("ledger stopped: {reason}"));
        }
        if self.allowances.contains_key(id) {
            return Err(format!("attempt ID already used: {id}"));
        }
        let input = input_upper_bound.ok_or("unknown input upper bound")?;
        if input > self.limits.max_input_tokens {
            return Err("input upper bound exceeds max_input_tokens".into());
        }
        let attempts = self
            .attempts
            .checked_add(1)
            .ok_or("attempt count overflow")?;
        if attempts > self.limits.max_attempts {
            return Err("attempt limit exhausted".into());
        }
        let output = if cap_check {
            self.limits.provider_output_max
        } else {
            self.limits.max_output_tokens
        };
        let reservation = self.limits.cost(input, output)?;
        let liability = self
            .incurred_microusd
            .checked_add(self.outstanding_microusd()?)
            .and_then(|total| total.checked_add(reservation))
            .ok_or("aggregate liability overflow")?;
        if liability > self.limits.spending_ceiling_microusd {
            return Err("spending ceiling exhausted".into());
        }
        self.attempts = attempts;
        self.outstanding.insert(id.into(), reservation);
        self.allowances.insert(
            id.into(),
            Allowance {
                input,
                output,
                settlement_attempted: false,
                failed: false,
                observed_microusd: None,
            },
        );
        Ok(reservation)
    }

    pub(super) fn settle(&mut self, id: &str, usage: Option<&Usage>) -> Result<(), String> {
        let allowance = self
            .allowances
            .get(id)
            .ok_or_else(|| format!("unknown attempt ID: {id}"))?;
        if allowance.settlement_attempted {
            return Err(format!("settlement already attempted: {id}"));
        }
        let reservation = *self
            .outstanding
            .get(id)
            .ok_or_else(|| format!("missing reservation: {id}"))?;
        let derived = usage.and_then(|usage| {
            usage
                .prompt_tokens
                .zip(usage.completion_tokens)
                .map(|(input, output)| self.limits.cost(input, output))
        });
        let observed = usage
            .and_then(|usage| usage.reported_cost_microusd)
            .into_iter()
            .chain(
                derived
                    .as_ref()
                    .and_then(|cost| cost.as_ref().ok())
                    .copied(),
            )
            .max();
        let validation = (|| {
            let usage = usage.ok_or("missing usage")?;
            let input = usage.prompt_tokens.ok_or("unknown prompt token count")?;
            let output = usage
                .completion_tokens
                .ok_or("unknown completion token count")?;
            if usage
                .reasoning_tokens
                .is_some_and(|reasoning| reasoning > output)
            {
                return Err("reasoning tokens exceed completion tokens".into());
            }
            if input > allowance.input {
                return Err("actual prompt tokens exceed reserved input upper bound".into());
            }
            // Cap checks reserve the provider maximum for safety, not permission
            // to exceed the study's requested output cap.
            if output > allowance.output || output > self.limits.max_output_tokens {
                return Err("actual completion tokens exceed output cap".into());
            }
            self.limits.cost(input, output)?;
            let cost = observed.ok_or("unknown observed cost")?;
            if cost > reservation {
                return Err("observed cost exceeds reservation".into());
            }
            Ok(cost)
        })();
        let accrued = self.incurred_microusd.checked_add(observed.unwrap_or(0));
        let validation = validation.and_then(|cost| {
            accrued.ok_or_else(|| "incurred cost overflow".to_string())?;
            Ok(cost)
        });
        let allowance = self
            .allowances
            .get_mut(id)
            .ok_or_else(|| format!("unknown attempt ID: {id}"))?;
        allowance.settlement_attempted = true;
        allowance.observed_microusd = observed;
        if let Some(accrued) = accrued {
            // On failure this deliberately counts exposure AND the held reserve:
            // uncertainty is never converted into credit. Each ID is counted once.
            self.incurred_microusd = accrued;
        }
        match validation {
            Ok(_) => {
                self.outstanding.remove(id);
                Ok(())
            }
            Err(reason) => {
                allowance.failed = true;
                self.stop(&format!("{id}: {reason}"));
                Err(reason)
            }
        }
    }

    pub(super) fn stop(&mut self, reason: &str) {
        if self.stopped.is_none() {
            self.stopped = Some(reason.into());
        }
    }

    pub(super) fn attempts(&self) -> u64 {
        self.attempts
    }

    pub(super) fn count_attempts(&self) -> u64 {
        self.counted.len() as u64
    }

    pub(super) fn incurred_microusd(&self) -> u64 {
        self.incurred_microusd
    }

    pub(super) fn outstanding_microusd(&self) -> Result<u64, String> {
        self.outstanding.values().try_fold(0_u64, |total, cost| {
            total
                .checked_add(*cost)
                .ok_or_else(|| "outstanding cost overflow".into())
        })
    }

    pub(super) fn stopped(&self) -> Option<&str> {
        self.stopped.as_deref()
    }

    pub(super) fn replay(limits: Limits, events: &[Event]) -> Result<Self, String> {
        let mut ledger = Self::new(limits)?;
        for event in events {
            match event {
                Event::Count { id } => ledger.count(id)?,
                Event::Reserve {
                    id,
                    input_upper_bound,
                    cap_check,
                } => {
                    ledger.reserve(id, *input_upper_bound, *cap_check)?;
                }
                Event::Settle { id, usage } => {
                    let first_settlement = ledger
                        .allowances
                        .get(id)
                        .is_some_and(|allowance| !allowance.settlement_attempted);
                    if let Err(reason) = ledger.settle(id, usage.as_ref()) {
                        // An expected failure is a durable transition, not a reason
                        // to discard the ledger. Malformed/repeated events still fail.
                        if !first_settlement
                            || !ledger
                                .allowances
                                .get(id)
                                .is_some_and(|allowance| allowance.failed)
                        {
                            return Err(reason);
                        }
                    }
                }
                Event::Stop { reason } => ledger.stop(reason),
            }
        }
        Ok(ledger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Explicit study fixture policy, not a production default.
    fn limits() -> Limits {
        Limits {
            max_attempts: 100,
            max_input_tokens: 8192,
            max_output_tokens: 1024,
            provider_output_max: 128_000,
            spending_ceiling_microusd: 6_000_000,
            prompt_microusd_per_million_tokens: 2_000_000,
            completion_microusd_per_million_tokens: 6_000_000,
        }
    }

    fn usage(input: u64, output: u64) -> Usage {
        Usage {
            prompt_tokens: Some(input),
            completion_tokens: Some(output),
            reasoning_tokens: None,
            reported_cost_microusd: None,
        }
    }

    fn snapshot(ledger: &Ledger) -> String {
        serde_json::to_string(ledger).expect("ledger serializes")
    }

    fn held(ledger: &Ledger, reservation: u64) {
        assert_eq!(
            ledger.outstanding_microusd().expect("bounded sum"),
            reservation
        );
        assert!(ledger.stopped().is_some());
    }

    #[test]
    fn count_exhaustion_and_duplicates_refuse_atomically() {
        let mut policy = limits();
        policy.max_attempts = 2;
        let mut ledger = Ledger::new(policy).expect("valid fixture");
        assert_eq!(ledger.count_attempts(), 0);
        ledger.count("one").expect("first count fits");
        let before = snapshot(&ledger);
        assert_eq!(
            ledger.count("one").expect_err("duplicate count"),
            "count attempt ID already used: one"
        );
        assert_eq!(snapshot(&ledger), before);
        ledger.count("two").expect("exact count limit fits");
        assert_eq!(ledger.count_attempts(), 2);
        let before = snapshot(&ledger);
        assert_eq!(
            ledger.count("three").expect_err("count limit exhausted"),
            "count attempt limit exhausted"
        );
        assert_eq!(snapshot(&ledger), before);
        assert!(ledger.stopped().is_none());
        assert_eq!(ledger.attempts(), 0);
        assert_eq!(ledger.incurred_microusd(), 0);
        assert_eq!(ledger.outstanding_microusd().expect("sum"), 0);
    }

    #[test]
    fn count_respects_sticky_stop_without_mutation() {
        let mut ledger = Ledger::new(limits()).expect("valid fixture");
        ledger.count("one").expect("first count");
        ledger.stop("count response failed");
        ledger.stop("later reason");
        for id in ["one", "two"] {
            let before = snapshot(&ledger);
            assert_eq!(
                ledger.count(id).expect_err("stopped ledger refuses counts"),
                "ledger stopped: count response failed"
            );
            assert_eq!(snapshot(&ledger), before);
        }
        assert_eq!(ledger.count_attempts(), 1);
    }

    #[test]
    fn count_and_generation_admission_are_independent() {
        let mut policy = limits();
        policy.max_attempts = 1;
        for count_first in [false, true] {
            let mut ledger = Ledger::new(policy.clone()).expect("valid fixture");
            if count_first {
                ledger.count("one").expect("count first");
            }
            let reservation = ledger
                .reserve("one", Some(10), false)
                .expect("generation admission is independent of counts");
            if !count_first {
                ledger.count("one").expect("count after generation reserve");
            }
            assert_eq!(ledger.attempts(), 1);
            assert_eq!(ledger.count_attempts(), 1);
            assert_eq!(ledger.outstanding_microusd().expect("sum"), reservation);
            assert_eq!(ledger.incurred_microusd(), 0);
            ledger.settle("one", Some(&usage(10, 1))).expect("settle");
            assert_eq!(ledger.count_attempts(), 1);
            assert_eq!(ledger.incurred_microusd(), 26);
        }
    }

    #[test]
    fn replay_preserves_count_only_attempts_without_generation_liability() {
        let mut policy = limits();
        policy.max_attempts = 2;
        let events = [
            Event::Count { id: "one".into() },
            Event::Count { id: "two".into() },
            Event::Stop {
                reason: "count response failed".into(),
            },
        ];
        let bytes = serde_json::to_string(&events).expect("journal serializes");
        let decoded: Vec<Event> = serde_json::from_str(&bytes).expect("journal parses");
        let replay = Ledger::replay(policy.clone(), &decoded).expect("replay count-only prefix");
        let mut live = Ledger::new(policy).expect("valid fixture");
        live.count("one").expect("first count");
        live.count("two").expect("second count");
        live.stop("count response failed");
        assert_eq!(snapshot(&live), snapshot(&replay));
        assert_eq!(replay.count_attempts(), 2);
        assert_eq!(replay.attempts(), 0);
        assert_eq!(replay.incurred_microusd(), 0);
        assert_eq!(replay.outstanding_microusd().expect("sum"), 0);
        assert_eq!(replay.stopped(), Some("count response failed"));
        let prefix =
            Ledger::replay(limits(), &decoded[..1]).expect("interrupted count-only prefix");
        assert_eq!(prefix.count_attempts(), 1);
        assert_eq!(prefix.attempts(), 0);
        assert!(prefix.stopped().is_none());
    }

    #[test]
    fn replay_refuses_invalid_count_events_immediately() {
        let mut policy = limits();
        policy.max_attempts = 1;
        for (events, expected) in [
            (
                vec![
                    Event::Count { id: "one".into() },
                    Event::Count { id: "one".into() },
                ],
                "count attempt ID already used: one",
            ),
            (
                vec![
                    Event::Count { id: "one".into() },
                    Event::Count { id: "two".into() },
                ],
                "count attempt limit exhausted",
            ),
            (
                vec![
                    Event::Stop {
                        reason: "stopped".into(),
                    },
                    Event::Count { id: "one".into() },
                ],
                "ledger stopped: stopped",
            ),
        ] {
            assert_eq!(
                Ledger::replay(policy.clone(), &events).expect_err("invalid count journal"),
                expected
            );
        }
    }

    #[test]
    fn reservation_tariffs_and_exact_boundary() {
        let mut ledger = Ledger::new(limits()).expect("valid fixture");
        assert_eq!(
            ledger.reserve("cap", Some(8192), true).expect("cap check"),
            784_384
        );
        assert_eq!(
            ledger.reserve("normal", Some(8192), false).expect("normal"),
            22_528
        );
        let mut policy = limits();
        policy.spending_ceiling_microusd = 22_528;
        let mut ledger = Ledger::new(policy).expect("valid boundary policy");
        ledger
            .reserve("boundary", Some(8192), false)
            .expect("exact ceiling fits");
        let before = snapshot(&ledger);
        assert!(ledger.reserve("extra", Some(0), false).is_err());
        assert_eq!(snapshot(&ledger), before);
    }

    #[test]
    fn fractional_tariffs_reserve_and_settle_exact_request_cost() {
        let mut policy = limits();
        policy.prompt_microusd_per_million_tokens = 750_000;
        policy.completion_microusd_per_million_tokens = 4_500_000;
        let mut ledger = Ledger::new(policy).expect("valid fractional tariffs");
        assert_eq!(
            ledger.reserve("exact", Some(8192), false).expect("reserve"),
            10_752
        );
        let actual = Usage {
            reasoning_tokens: Some(512),
            ..usage(8192, 1024)
        };
        ledger.settle("exact", Some(&actual)).expect("settle");
        assert_eq!(ledger.incurred_microusd(), 10_752);
        assert_eq!(ledger.outstanding_microusd().expect("sum"), 0);
        assert!(ledger.stopped().is_none());
    }

    #[test]
    fn fractional_components_round_once_per_request() {
        let mut policy = limits();
        policy.max_output_tokens = 1;
        policy.prompt_microusd_per_million_tokens = 250_000;
        policy.completion_microusd_per_million_tokens = 250_000;
        for (input, output, expected) in [(1, 0, 1), (0, 1, 1), (1, 1, 1), (3, 1, 1), (4, 1, 2)] {
            assert_eq!(policy.cost(input, output).expect("bounded cost"), expected);
        }
        let mut ledger = Ledger::new(policy).expect("valid fractional tariffs");
        for id in ["one", "two"] {
            assert_eq!(ledger.reserve(id, Some(1), false).expect("reserve"), 1);
            ledger.settle(id, Some(&usage(1, 1))).expect("settle");
        }
        // Separate requests each round up even when their exact costs sum to one.
        assert_eq!(ledger.incurred_microusd(), 2);
        assert_eq!(ledger.outstanding_microusd().expect("sum"), 0);
    }

    #[test]
    fn tariff_cost_keeps_wide_products_and_checks_final_conversion() {
        let mut policy = limits();
        policy.prompt_microusd_per_million_tokens = 1_000_000;
        policy.completion_microusd_per_million_tokens = 1;
        assert_eq!(
            policy.cost(u64::MAX, 0).expect("exact u64 maximum"),
            u64::MAX
        );
        assert_eq!(
            policy.cost(u64::MAX - 1, 1).expect("rounds to maximum"),
            u64::MAX
        );
        assert_eq!(
            policy.cost(u64::MAX, 1).expect_err("rounding exceeds u64"),
            "token tariff arithmetic overflow"
        );
        policy.prompt_microusd_per_million_tokens = u64::MAX;
        policy.completion_microusd_per_million_tokens = u64::MAX;
        assert_eq!(
            policy
                .cost(500_000, 500_000)
                .expect("wide sum fits after division"),
            u64::MAX
        );
        assert!(policy.cost(u64::MAX, 0).is_err());
        assert!(policy.cost(0, u64::MAX).is_err());
        assert_eq!(
            policy
                .cost(u64::MAX, u64::MAX)
                .expect_err("u128 sum overflow"),
            "token tariff arithmetic overflow"
        );
    }

    #[test]
    fn zero_tokens_and_zero_tariffs_cost_zero() {
        let mut policy = limits();
        policy.prompt_microusd_per_million_tokens = u64::MAX;
        policy.completion_microusd_per_million_tokens = u64::MAX;
        assert_eq!(policy.cost(0, 0).expect("zero usage"), 0);
        policy.prompt_microusd_per_million_tokens = 0;
        assert_eq!(policy.cost(u64::MAX, 0).expect("free input"), 0);
        policy.prompt_microusd_per_million_tokens = u64::MAX;
        policy.completion_microusd_per_million_tokens = 0;
        assert_eq!(policy.cost(0, u64::MAX).expect("free output"), 0);
        policy.prompt_microusd_per_million_tokens = 0;
        assert_eq!(policy.cost(u64::MAX, u64::MAX).expect("zero tariffs"), 0);
        let mut ledger = Ledger::new(policy).expect("zero tariffs are valid");
        assert_eq!(
            ledger.reserve("free", Some(8192), false).expect("reserve"),
            0
        );
        ledger
            .settle("free", Some(&usage(8192, 1024)))
            .expect("settle");
        assert_eq!(ledger.incurred_microusd(), 0);
        assert_eq!(ledger.outstanding_microusd().expect("sum"), 0);
        assert!(ledger.stopped().is_none());
    }

    #[test]
    fn aggregate_exhaustion_includes_spent_and_outstanding() {
        let mut policy = limits();
        policy.spending_ceiling_microusd = 45_056;
        let mut ledger = Ledger::new(policy).expect("valid fixture");
        ledger.reserve("spent", Some(8192), false).expect("first");
        ledger
            .settle("spent", Some(&usage(8192, 1024)))
            .expect("full actual cost");
        ledger.reserve("held", Some(8192), false).expect("second");
        assert_eq!(ledger.incurred_microusd(), 22_528);
        let before = snapshot(&ledger);
        assert!(ledger.reserve("third", Some(0), false).is_err());
        assert_eq!(snapshot(&ledger), before);
    }

    #[test]
    fn reserve_refusals_are_atomic_and_settled_ids_stay_used() {
        let mut policy = limits();
        policy.max_attempts = 1;
        let mut ledger = Ledger::new(policy).expect("valid fixture");
        for bound in [None, Some(8193)] {
            let before = snapshot(&ledger);
            assert!(ledger.reserve("bad", bound, false).is_err());
            assert_eq!(snapshot(&ledger), before);
        }
        ledger.reserve("one", Some(10), false).expect("one attempt");
        let before = snapshot(&ledger);
        assert!(ledger.reserve("one", Some(10), false).is_err());
        assert_eq!(snapshot(&ledger), before);
        ledger.settle("one", Some(&usage(10, 1))).expect("settles");
        for id in ["one", "two"] {
            let before = snapshot(&ledger);
            assert!(ledger.reserve(id, Some(10), false).is_err());
            assert_eq!(snapshot(&ledger), before);
        }
        assert_eq!(ledger.attempts(), 1);
    }

    #[test]
    fn policy_validation_and_tariff_overflow() {
        for field in 0..5 {
            let mut policy = limits();
            match field {
                0 => policy.max_attempts = 0,
                1 => policy.max_input_tokens = 0,
                2 => policy.max_output_tokens = 0,
                3 => policy.provider_output_max = 0,
                _ => policy.spending_ceiling_microusd = 0,
            }
            assert!(Ledger::new(policy).is_err());
        }
        let mut policy = limits();
        policy.max_output_tokens = policy.provider_output_max + 1;
        assert!(Ledger::new(policy).is_err());
        for prices in [(u64::MAX, 0), (0, u64::MAX), (u64::MAX, u64::MAX)] {
            let mut policy = limits();
            policy.max_input_tokens = u64::MAX;
            policy.provider_output_max = u64::MAX;
            policy.prompt_microusd_per_million_tokens = prices.0;
            policy.completion_microusd_per_million_tokens = prices.1;
            assert!(Ledger::new(policy).is_err());
        }
    }

    #[test]
    fn aggregate_overflow_is_atomic() {
        let mut policy = limits();
        policy.max_input_tokens = u64::MAX / 2;
        policy.max_output_tokens = 1;
        policy.provider_output_max = 1;
        policy.prompt_microusd_per_million_tokens = 1_000_000;
        policy.completion_microusd_per_million_tokens = 1_000_000;
        policy.spending_ceiling_microusd = u64::MAX;
        let mut ledger = Ledger::new(policy).expect("single reservation fits");
        ledger
            .reserve("one", Some(u64::MAX / 2), false)
            .expect("first fits");
        let before = snapshot(&ledger);
        assert!(ledger.reserve("two", Some(u64::MAX / 2), false).is_err());
        assert_eq!(snapshot(&ledger), before);
    }

    #[test]
    fn missing_usage_and_counts_never_refund_or_allow_retry() {
        for actual in [
            None,
            Some(Usage {
                prompt_tokens: None,
                ..usage(10, 1)
            }),
            Some(Usage {
                completion_tokens: None,
                reported_cost_microusd: Some(20),
                ..usage(10, 1)
            }),
        ] {
            let mut ledger = Ledger::new(limits()).expect("valid fixture");
            let reserve = ledger.reserve("unknown", Some(10), false).expect("reserve");
            assert!(ledger.settle("unknown", actual.as_ref()).is_err());
            held(&ledger, reserve);
            assert_eq!(
                ledger.incurred_microusd(),
                actual
                    .as_ref()
                    .and_then(|usage| usage.reported_cost_microusd)
                    .unwrap_or(0)
            );
            let before = snapshot(&ledger);
            assert!(ledger.settle("unknown", Some(&usage(1, 1))).is_err());
            assert!(ledger.reserve("next", Some(1), false).is_err());
            assert_eq!(snapshot(&ledger), before);
        }
    }

    #[test]
    fn settlement_overflow_preserves_exposure_and_all_held_reservations() {
        let events = [
            Event::Reserve {
                id: "one".into(),
                input_upper_bound: Some(10),
                cap_check: false,
            },
            Event::Reserve {
                id: "two".into(),
                input_upper_bound: Some(10),
                cap_check: false,
            },
            Event::Settle {
                id: "one".into(),
                usage: Some(Usage {
                    reported_cost_microusd: Some(u64::MAX),
                    ..usage(10, 1)
                }),
            },
            Event::Settle {
                id: "two".into(),
                usage: Some(usage(10, 1)),
            },
        ];
        let ledger = Ledger::replay(limits(), &events).expect("overflow remains auditable");
        held(&ledger, 2 * (20 + 6144));
        assert_eq!(ledger.incurred_microusd(), u64::MAX);
        assert_eq!(ledger.allowances["one"].observed_microusd, Some(u64::MAX));
        assert_eq!(ledger.allowances["two"].observed_microusd, Some(26));
        assert!(ledger.allowances["two"].failed);
    }

    #[test]
    fn reasoning_is_a_subset_and_reported_price_is_conservative() {
        for reported in [None, Some(1), Some(100)] {
            let mut ledger = Ledger::new(limits()).expect("valid fixture");
            ledger.reserve("ok", Some(10), false).expect("reserve");
            let actual = Usage {
                reasoning_tokens: Some(4),
                reported_cost_microusd: reported,
                ..usage(10, 5)
            };
            ledger
                .settle("ok", Some(&actual))
                .expect("reasoning included in completion");
            assert_eq!(ledger.incurred_microusd(), reported.unwrap_or(0).max(50));
            assert_eq!(ledger.outstanding_microusd().expect("sum"), 0);
            assert!(ledger.stopped().is_none());
        }
    }

    #[test]
    fn invalid_counts_caps_and_overreports_hold_reservations() {
        for actual in [
            Usage {
                reasoning_tokens: Some(6),
                ..usage(10, 5)
            },
            usage(11, 1),
            usage(10, 1025),
            Usage {
                reported_cost_microusd: Some(100_000),
                ..usage(10, 1)
            },
            usage(u64::MAX, u64::MAX),
        ] {
            let mut ledger = Ledger::new(limits()).expect("valid fixture");
            let reserve = ledger.reserve("bad", Some(10), false).expect("reserve");
            assert!(ledger.settle("bad", Some(&actual)).is_err());
            held(&ledger, reserve);
            if let Some(reported) = actual.reported_cost_microusd {
                assert!(ledger.incurred_microusd() >= reported);
            }
        }
    }

    #[test]
    fn cap_check_is_not_permission_to_exceed_requested_output() {
        let mut ledger = Ledger::new(limits()).expect("valid fixture");
        ledger
            .reserve("ok", Some(8192), true)
            .expect("cap reservation");
        ledger
            .settle("ok", Some(&usage(10, 1)))
            .expect("low declared actual succeeds");
        let reserve = ledger
            .reserve("bad", Some(8192), true)
            .expect("cap reservation");
        assert!(ledger.settle("bad", Some(&usage(10, 1025))).is_err());
        held(&ledger, reserve);
    }

    #[test]
    fn route_stop_is_sticky_but_inflight_usage_can_be_accounted() {
        let mut ledger = Ledger::new(limits()).expect("valid fixture");
        ledger
            .reserve("inflight", Some(10), false)
            .expect("reserve");
        ledger.stop("route mismatch");
        ledger.stop("later reason");
        let before = snapshot(&ledger);
        assert!(ledger.reserve("next", Some(10), false).is_err());
        assert_eq!(snapshot(&ledger), before);
        ledger
            .settle("inflight", Some(&usage(10, 1)))
            .expect("account inflight exposure");
        assert_eq!(ledger.stopped(), Some("route mismatch"));
        assert_eq!(ledger.incurred_microusd(), 26);
    }

    #[test]
    fn replay_preserves_crash_liability_and_failed_settlement() {
        let events = vec![Event::Reserve {
            id: "crashed".into(),
            input_upper_bound: Some(8192),
            cap_check: true,
        }];
        let bytes = serde_json::to_string(&events).expect("journal serializes");
        let mut decoded: Vec<Event> = serde_json::from_str(&bytes).expect("journal parses");
        let ledger = Ledger::replay(limits(), &decoded).expect("replay reserve");
        assert_eq!(ledger.attempts(), 1);
        assert_eq!(ledger.incurred_microusd(), 0);
        assert_eq!(ledger.outstanding_microusd().expect("sum"), 784_384);
        decoded.push(Event::Settle {
            id: "crashed".into(),
            usage: None,
        });
        let ledger = Ledger::replay(limits(), &decoded).expect("failed settle is durable state");
        held(&ledger, 784_384);
        decoded.push(Event::Settle {
            id: "crashed".into(),
            usage: Some(usage(1, 1)),
        });
        assert!(Ledger::replay(limits(), &decoded).is_err());
    }

    #[test]
    fn replay_matches_live_valid_and_invalid_exposure_and_stop() {
        for actual in [
            usage(10, 1),
            Usage {
                reported_cost_microusd: Some(100_000),
                ..usage(10, 1)
            },
        ] {
            let events = [
                Event::Reserve {
                    id: "a".into(),
                    input_upper_bound: Some(10),
                    cap_check: false,
                },
                Event::Settle {
                    id: "a".into(),
                    usage: Some(actual.clone()),
                },
                Event::Stop {
                    reason: "study complete".into(),
                },
            ];
            let replay = Ledger::replay(limits(), &events).expect("replay transitions");
            let mut live = Ledger::new(limits()).expect("valid fixture");
            live.reserve("a", Some(10), false).expect("reserve");
            let outcome = live.settle("a", Some(&actual));
            assert_eq!(outcome.is_err(), actual.reported_cost_microusd.is_some());
            live.stop("study complete");
            assert_eq!(snapshot(&live), snapshot(&replay));
        }
        let unknown = [Event::Settle {
            id: "missing".into(),
            usage: None,
        }];
        assert!(Ledger::replay(limits(), &unknown).is_err());
    }

    #[test]
    fn serde_rejects_invalid_counts_and_unknown_policy_fields() {
        let unknown: Usage = serde_json::from_str("{}").expect("absent counts stay unknown");
        assert!(unknown.prompt_tokens.is_none());
        assert!(unknown.completion_tokens.is_none());
        for json in [
            r#"{"prompt_tokens":-1}"#,
            r#"{"completion_tokens":1.5}"#,
            r#"{"prompt_tokens":18446744073709551616}"#,
        ] {
            assert!(serde_json::from_str::<Usage>(json).is_err());
        }
        let mut policy = serde_json::to_value(limits()).expect("policy serializes");
        policy["typo"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Limits>(policy).is_err());
    }
}
