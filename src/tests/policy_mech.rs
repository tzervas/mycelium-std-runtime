//! Tests for [`crate::policy_mech`] (M-963; DN-78 §3 B-1/B-2).
//!
//! Fixture-driven: policies come from [`policy`]/[`inputs`] builders and the property test's
//! generated cases; test bodies assert over a case (M-797 layout).

use crate::policy_mech::{
    capture, replay, CaptureError, PolicySite, PolicySlot, ReplayError, SlotError,
    DECLARED_POLICY_TRACE_CAP, DECLARED_POLICY_TRANSITION_CAP,
};
use mycelium_core::{GuaranteeStrength, Repr};
use mycelium_select::{
    Action, Candidate, CostModel, Explanation, PolicyRegistry, Predicate, Rule, SelectionInputs,
    SelectionPolicy,
};
use proptest::prelude::*;

/// A validated test policy over `Binary{width}` candidates.
fn policy(name: &str, widths: &[u32], rules: Vec<Rule>, default_choice: usize) -> SelectionPolicy {
    SelectionPolicy::new(
        name,
        widths
            .iter()
            .map(|w| Candidate::Repr(Repr::Binary { width: *w }))
            .collect(),
        rules,
        default_choice,
        CostModel {
            storage_weight: 1.0,
        },
    )
    .expect("test policy must validate")
}

/// Swap/packing-shaped inputs (no decode facts) over a `Binary` source.
fn inputs(width: u32) -> SelectionInputs {
    SelectionInputs {
        src: Repr::Binary { width },
        guarantee: GuaranteeStrength::Exact,
        bound: None,
        sparsity: None,
        decode: None,
    }
}

// ── B-2: the reified setter surface ──

#[test]
fn set_appends_one_record_with_monotonic_seq_and_previous_chain() {
    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    let a = policy("a", &[8], vec![], 0);
    let b = policy("b", &[8, 16], vec![], 1);
    let a_ref = a.policy_ref();
    let b_ref = b.policy_ref();

    let first = slot.set(a).clone();
    assert_eq!(first.seq, 0);
    assert_eq!(first.previous, None, "first set has no previous policy");
    assert_eq!(first.new_policy, a_ref);
    assert_eq!(first.site, PolicySite::SwapTarget);

    let second = slot.set(b).clone();
    assert_eq!(second.seq, 1, "seq is per-slot monotonic");
    assert_eq!(
        second.previous,
        Some(a_ref),
        "the transition records the outgoing policy — never a silent override (G2)"
    );
    assert_eq!(second.new_policy, b_ref);

    assert_eq!(slot.transitions().len(), 2, "exactly one record per set");
    assert_eq!(
        slot.active().expect("policy b is active").policy_ref(),
        b_ref
    );
    // Mutant witness: dropping the record push, or reusing seq 0, fails here.
}

#[test]
fn select_without_active_policy_refuses_explicitly() {
    let mut slot = PolicySlot::new(PolicySite::Placement);
    let err = slot
        .select(&inputs(8), None)
        .expect_err("an unset slot must refuse, never silently default (G2/ADR-006)");
    assert_eq!(
        err,
        SlotError::NoActivePolicy {
            site: PolicySite::Placement
        }
    );
    let msg = err.to_string();
    assert!(
        msg.contains("placement") && msg.contains("no silent default"),
        "the refusal must teach: site + no-silent-default; got: {msg}"
    );
}

#[test]
fn select_through_slot_records_trace() {
    let mut slot = PolicySlot::new(PolicySite::Packing);
    let p = policy("trace", &[8, 16], vec![], 0);
    let p_ref = p.policy_ref();
    slot.set(p);

    let (_, e1) = slot.select(&inputs(8), None).expect("selection succeeds");
    let (_, e2) = slot.select(&inputs(16), None).expect("selection succeeds");

    assert_eq!(slot.trace().len(), 2, "one Explanation per selection");
    assert_eq!(slot.trace()[0], e1);
    assert_eq!(slot.trace()[1], e2);
    assert!(
        slot.trace().iter().all(|e| e.policy == p_ref),
        "every trace entry names the policy that decided (RFC-0005 §3 provenance)"
    );
}

// ── CC-B6: capped logs (G-8 / assessment F13) ──

/// Exceeding [`DECLARED_POLICY_TRANSITION_CAP`] evicts the oldest transition record and increments
/// the never-silent drop counter — the log never grows past the cap, and `seq` stays monotonic
/// (never reused) across the eviction (backward-compat requirement, CC-B6).
#[test]
fn transitions_log_evicts_oldest_past_cap_seq_stays_monotonic() {
    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    let n = DECLARED_POLICY_TRANSITION_CAP + 5;
    for i in 0..n {
        slot.set(policy(&format!("p{i}"), &[8], vec![], 0));
    }
    assert_eq!(
        slot.transitions().len(),
        DECLARED_POLICY_TRANSITION_CAP,
        "the log never grows past the cap"
    );
    assert_eq!(
        slot.transitions_dropped(),
        5,
        "exactly (n - cap) records were evicted"
    );
    assert_eq!(
        slot.transitions_total(),
        n as u64,
        "the total count survives eviction — never silently lost (G2)"
    );
    // The retained window is exactly the most recent `cap` records, and seq is monotonic and
    // never reused across the boundary: the oldest RETAINED record's seq equals the drop count.
    assert_eq!(slot.transitions()[0].seq, slot.transitions_dropped());
    for w in slot.transitions().windows(2) {
        assert_eq!(
            w[1].seq,
            w[0].seq + 1,
            "seq is strictly monotonic in the retained window"
        );
    }
    assert_eq!(
        slot.transitions().last().unwrap().seq,
        n as u64 - 1,
        "the most recent record's seq is (total - 1)"
    );
}

/// Exceeding [`DECLARED_POLICY_TRACE_CAP`] evicts the oldest [`mycelium_select::Explanation`] and
/// increments the never-silent drop counter, mirroring the transitions-log behavior above —
/// capture/replay is unaffected since it consumes `select`'s own return value, never `trace()`.
#[test]
fn trace_log_evicts_oldest_past_cap_and_capture_replay_still_works() {
    let mut slot = PolicySlot::new(PolicySite::Packing);
    let p = policy("trace-cap", &[8, 16], vec![], 0);
    let p_ref = p.policy_ref();
    let mut registry = PolicyRegistry::new();
    registry.register(p.clone());
    slot.set(p);

    let n = DECLARED_POLICY_TRACE_CAP + 3;
    let mut last_recorded = None;
    for i in 0..n {
        let width = if i % 2 == 0 { 8 } else { 16 };
        let (_, e) = slot
            .select(&inputs(width), None)
            .expect("selection succeeds");
        last_recorded = Some(e);
    }
    assert_eq!(
        slot.trace().len(),
        DECLARED_POLICY_TRACE_CAP,
        "the trace never grows past the cap"
    );
    assert_eq!(
        slot.trace_dropped(),
        3,
        "exactly (n - cap) explanations were evicted"
    );
    assert_eq!(
        slot.trace_total(),
        n as u64,
        "the total count survives eviction"
    );
    assert!(
        slot.trace().iter().all(|e| e.policy == p_ref),
        "every RETAINED trace entry still names the deciding policy"
    );

    // capture/replay never depend on trace() — they use select()'s own return value, so eviction
    // above does not break them (the doc comment's claim, checked here).
    let recorded = last_recorded.expect("at least one selection was made");
    let captured = capture(&registry, &recorded).expect("registered ref must capture");
    let replayed = replay(&captured, &recorded).expect("replay must reach the recorded decision");
    assert_eq!(replayed, recorded);
}

// Property test (SC-2 — the bound + its property test): for ANY number of `set` calls, the
// transition log's retained length never exceeds the cap, the drop count plus the retained
// length always equals the true total, and seq is strictly monotonic across the whole retained
// window regardless of how many records were evicted.
proptest! {
    #[test]
    fn prop_transitions_cap_bound_holds(n in 0usize..300) {
        let mut slot = PolicySlot::new(PolicySite::SwapTarget);
        for i in 0..n {
            slot.set(policy(&format!("p{i}"), &[8], vec![], 0));
        }
        prop_assert!(slot.transitions().len() <= DECLARED_POLICY_TRANSITION_CAP);
        prop_assert_eq!(slot.transitions_total(), n as u64);
        prop_assert_eq!(
            slot.transitions_dropped() + slot.transitions().len() as u64,
            n as u64,
            "dropped + retained == total, always (no record is ever silently unaccounted)"
        );
        for w in slot.transitions().windows(2) {
            prop_assert_eq!(w[1].seq, w[0].seq + 1);
        }
    }
}

// ── B-1: capture and replay ──

#[test]
fn capture_unknown_ref_refuses() {
    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    let p = policy("unregistered", &[8], vec![], 0);
    let p_ref = p.policy_ref();
    slot.set(p);
    let (_, record) = slot.select(&inputs(8), None).expect("selection succeeds");

    let empty = PolicyRegistry::new();
    let err = capture(&empty, &record)
        .expect_err("capture must refuse an unknown ref, never reconstruct (G2)");
    assert_eq!(err, CaptureError::UnknownPolicyRef { policy_ref: p_ref });
}

#[test]
fn capture_round_trip_replay_matches() {
    let mut registry = PolicyRegistry::new();
    let p = policy(
        "round-trip",
        &[8, 16, 32],
        vec![Rule {
            when: Predicate::Always,
            action: Action::Cheapest,
        }],
        2,
    );
    registry.register(p.clone());

    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    slot.set(p);
    let (_, recorded) = slot.select(&inputs(8), None).expect("selection succeeds");

    let captured = capture(&registry, &recorded).expect("registered ref must capture");
    assert_eq!(
        captured.policy.policy_ref(),
        captured.policy_ref,
        "the captured value is the policy the record names — checked, not assumed"
    );
    let replayed = replay(&captured, &recorded).expect("replay must reach the recorded decision");
    assert_eq!(replayed, recorded, "replay reproduces the full Explanation");
}

#[test]
fn replay_honors_recorded_override() {
    let mut registry = PolicyRegistry::new();
    // Cheapest would pick index 0 (8 bits); force index 1 so the override path is exercised.
    let p = policy("override", &[8, 16], vec![], 0);
    registry.register(p.clone());

    let mut slot = PolicySlot::new(PolicySite::Packing);
    slot.set(p);
    let (_, recorded) = slot
        .select(&inputs(8), Some(1))
        .expect("in-range override succeeds");
    assert!(recorded.overridden, "the override is recorded first-class");

    let captured = capture(&registry, &recorded).expect("capture succeeds");
    let replayed = replay(&captured, &recorded).expect("replay re-applies the recorded override");
    assert_eq!(replayed, recorded);
}

#[test]
fn replay_against_wrong_policy_refuses() {
    let mut registry = PolicyRegistry::new();
    let a = policy("policy-a", &[8], vec![], 0);
    let b = policy("policy-b", &[8, 16], vec![], 1);
    registry.register(a.clone());
    registry.register(b.clone());

    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    slot.set(a);
    let (_, recorded_by_a) = slot.select(&inputs(8), None).expect("selection succeeds");

    let captured_b = crate::policy_mech::CapturedPolicy {
        policy_ref: b.policy_ref(),
        policy: b,
    };
    let err = replay(&captured_b, &recorded_by_a)
        .expect_err("replaying a record against a different policy must refuse up front");
    assert!(
        matches!(err, ReplayError::PolicyMismatch { .. }),
        "expected PolicyMismatch, got {err:?}"
    );
}

#[test]
fn replay_divergence_is_explicit_not_silent() {
    let mut registry = PolicyRegistry::new();
    let p = policy("diverge", &[8, 16], vec![], 0);
    registry.register(p.clone());

    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    slot.set(p);
    let (_, mut recorded) = slot.select(&inputs(8), None).expect("selection succeeds");

    // Tamper with the record (a record from different code / a corrupted store): the replay
    // recomputes the true decision and must surface the difference, never absorb it.
    recorded.chosen_index = 1;
    recorded.chosen = Candidate::Repr(Repr::Binary { width: 16 });

    let captured = capture(&registry, &recorded).expect("capture succeeds");
    let err = replay(&captured, &recorded).expect_err("a diverging record must be surfaced (G2)");
    match err {
        ReplayError::Diverged { recorded, replayed } => {
            assert_eq!(recorded.chosen_index, 1, "the tampered record is carried");
            assert_eq!(replayed.chosen_index, 0, "the true decision is carried");
        }
        other => panic!("expected Diverged, got {other:?}"),
    }
}

// ── The record-vs-replay differential (property test; the `Empirical` basis for the
//    "Policy capture replay reaches the recorded decision" matrix row — VR-5) ──

proptest! {
    #[test]
    fn prop_capture_replay_differential(
        // 1..=4 candidate widths, distinct-enough for a real choice space.
        widths in proptest::collection::vec(1u32..512, 1..4),
        default_ix in 0usize..4,
        use_cheapest_rule in any::<bool>(),
        input_width in 1u32..512,
        force in proptest::option::of(0usize..4),
    ) {
        let default_choice = default_ix % widths.len();
        let rules = if use_cheapest_rule {
            vec![Rule { when: Predicate::Always, action: Action::Cheapest }]
        } else {
            vec![]
        };
        let p = policy("prop", &widths, rules, default_choice);

        let mut registry = PolicyRegistry::new();
        registry.register(p.clone());

        let mut slot = PolicySlot::new(PolicySite::SwapTarget);
        slot.set(p);

        let forced = force.map(|f| f % widths.len());
        let (_, recorded) = slot
            .select(&inputs(input_width), forced)
            .expect("in-range (possibly forced) selection on a validated policy succeeds");

        let captured = capture(&registry, &recorded).expect("registered ref captures");
        let replayed = replay(&captured, &recorded)
            .expect("replay must reach the recorded decision (record-vs-replay differential)");
        prop_assert_eq!(replayed, recorded);
    }
}

// ── Sizing pass (course-correction W-D item 2): Declared -> Empirical where measured ───────────
//
// `PolicySetRecord`/`Explanation` (the records behind the CC-B6 `transitions`/`trace` caps above)
// have no existing `LanguageRetentionPolicy` §5 row at all — the caps themselves
// (`DECLARED_POLICY_TRANSITION_CAP`/`DECLARED_POLICY_TRACE_CAP`) are new (this crate's own CC-B6
// change). This measures their real per-record footprint the same way the L1/L4 measurements do
// (`mycelium-cert/tests/sizing.rs`, `mycelium-diag/src/tests.rs`): static `size_of` plus a
// heap-estimate (`.capacity()` of owned `String`/`Vec` fields) over a representative,
// non-trivial instance, then a synthetic load at the actual cap. `Empirical`, method stated —
// never `Exact` (heap-estimate ignores allocator overhead) and never asserted bare (VR-5).

/// A rough (never `Exact`) byte estimate of an [`Explanation`]'s heap allocations: the
/// `policy_name`'s capacity plus each `CandidateCost`'s `Candidate` heap contribution (only
/// `Candidate::Node` carries an owned `String` today; `Repr`/`PackScheme`/`DecodeMethod` are
/// fixed-size) — the dominant variable-size cost is `costs: Vec<CandidateCost>`'s own allocation.
fn explanation_heap_estimate(e: &Explanation) -> usize {
    let mut bytes = e.policy_name.capacity();
    bytes += e.costs.capacity() * std::mem::size_of::<mycelium_select::CandidateCost>();
    bytes
}

/// A rough byte estimate of a [`crate::policy_mech::PolicySetRecord`]'s heap allocations: just
/// `new_policy_name`'s capacity (its only owned `String`).
fn transition_heap_estimate(r: &crate::policy_mech::PolicySetRecord) -> usize {
    r.new_policy_name.capacity()
}

/// Static stack footprint — sanity-bounded, not pinned to an exact byte count. The actual measured
/// figures (obtained via `cargo test -p mycelium-std-runtime --lib policy_mech -- --nocapture`) are
/// recorded in `docs/spec/Language-Retention-Policy.md` §5, with this test named as the method.
#[test]
fn transition_and_explanation_stack_sizes_are_sane() {
    let transition_size = std::mem::size_of::<crate::policy_mech::PolicySetRecord>();
    let explanation_size = std::mem::size_of::<Explanation>();
    assert!(
        transition_size > 0 && transition_size < 512,
        "PolicySetRecord stack size {transition_size}"
    );
    assert!(
        explanation_size > 0 && explanation_size < 512,
        "Explanation stack size {explanation_size}"
    );
}

/// Synthetic-load measurement: fill a [`PolicySlot`]'s `transitions`/`trace` logs past their caps
/// with representative records, and report the retained set's total estimated bytes — the figure
/// `docs/spec/Language-Retention-Policy.md` §5 records for these two NEW rows. Also re-exercises
/// the cap bound from a sizing angle: the retained byte total is bounded by
/// `cap * (stack_size + representative_heap_estimate)`, never unbounded.
#[test]
fn synthetic_load_at_caps_reports_estimated_bytes() {
    let mut slot = PolicySlot::new(PolicySite::SwapTarget);
    let transition_stack = std::mem::size_of::<crate::policy_mech::PolicySetRecord>();
    let n_transitions = DECLARED_POLICY_TRANSITION_CAP * 2;
    for i in 0..n_transitions {
        slot.set(policy(&format!("p{i}"), &[8], vec![], 0));
    }
    let transitions_bytes: usize = slot
        .transitions()
        .iter()
        .map(|r| transition_stack + transition_heap_estimate(r))
        .sum();
    assert!(transitions_bytes > 0);
    assert!(
        transitions_bytes
            <= DECLARED_POLICY_TRANSITION_CAP
                * (transition_stack + transition_heap_estimate(&slot.transitions()[0]) + 64),
        "retained transitions bytes must stay bounded by cap * a per-record ceiling"
    );

    // 3 candidates + a matched rule: the widest field set `select` populates, not a minimal
    // 2-candidate/no-rule shape — a conservative (not optimistic) representative instance.
    let p = policy(
        "trace-sizing",
        &[8, 16, 32],
        vec![Rule {
            when: Predicate::Always,
            action: Action::Cheapest,
        }],
        0,
    );
    slot.set(p);
    let explanation_stack = std::mem::size_of::<Explanation>();
    let n_trace = DECLARED_POLICY_TRACE_CAP * 2;
    for i in 0..n_trace {
        let width = if i % 2 == 0 { 8 } else { 16 };
        slot.select(&inputs(width), None)
            .expect("selection succeeds");
    }
    let trace_bytes: usize = slot
        .trace()
        .iter()
        .map(|e| explanation_stack + explanation_heap_estimate(e))
        .sum();
    assert!(trace_bytes > 0);

    eprintln!(
        "synthetic_load_at_caps_reports_estimated_bytes: \
         transitions cap={DECLARED_POLICY_TRANSITION_CAP} retained_bytes={transitions_bytes} \
         per_record_avg={} | trace cap={DECLARED_POLICY_TRACE_CAP} retained_bytes={trace_bytes} \
         per_record_avg={}",
        transitions_bytes / DECLARED_POLICY_TRANSITION_CAP,
        trace_bytes / DECLARED_POLICY_TRACE_CAP
    );
}
