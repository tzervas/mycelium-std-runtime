//! Mechanized `SelectionPolicy` capture and setting (M-963; DN-78 §3 B-1/B-2; the M-828
//! capture-and-set tail, decided under the 2026-07-02 delegation recorded in DN-78 §1).
//!
//! Two surfaces, both riding the existing RFC-0005 machinery in `mycelium-select` — **no new
//! selection mechanism** (KC-3; DN-63 §3.5 "the third application of the existing one"):
//!
//! - **Capture (B-1):** [`capture`] materializes the policy that decided a recorded
//!   [`Explanation`] back into a nameable, diffable, inspectable [`SelectionPolicy`] value via
//!   the [`PolicyRegistry`] — never an opaque handle (ADR-006). [`replay`] re-runs the recorded
//!   inputs (honoring the recorded override state) and requires the same decision; divergence
//!   is an explicit [`ReplayError::Diverged`], never a silent pass (G2).
//! - **Setting (B-2):** [`PolicySlot`] binds the active policy for one RFC-0005 site
//!   ([`PolicySite`]); every [`PolicySlot::set`] appends a [`PolicySetRecord`] to a
//!   **capped**, append-only transition log — a mechanized set is never a silent override
//!   (research/27-dn64-ergonomics-rnd-RECORD.md §2.2), and EXPLAIN stays answerable afterward
//!   (the slot holds the full policy value). Selection through the slot records the mandatory
//!   [`Explanation`] into an extractable, **capped** trace (the "runtime records which policy it
//!   applied" half of mechanized capture).
//!
//! # CC-B6 — capped logs (G-8 / assessment F13; `docs/spec/Language-Retention-Policy.md` §5)
//!
//! `transitions`/`trace` were originally unbounded `Vec`s — a long-running colony that sets
//! policies or selects through a slot indefinitely would grow them without limit (G-8: no
//! unbounded accumulator before caps exist). Both logs are now capped per the
//! `LanguageRetentionPolicy` §5 `on_overflow = drop_oldest` shape (mirroring the W-A
//! [`crate`]-external precedent, `mycelium-cert::store::CertStore`): each is a bounded ring that
//! evicts its oldest entry on overflow and increments a never-silent drop counter
//! ([`PolicySlot::transitions_dropped`]/[`PolicySlot::trace_dropped`] — the EXPLAIN-of-drop
//! discipline, §8 of that spec) rather than growing without bound. **Seq stays monotonic across
//! drops** (backward compat): [`PolicySetRecord::seq`] is generated from an internal counter that
//! only ever increments, independent of the (possibly-truncated) log's current length — a dropped
//! record's `seq` is never reused, and [`PolicySlot::transitions_total`]/
//! [`PolicySlot::trace_total`] give the true historical count even once entries have been evicted
//! (G2: a truncated log that *looks* complete, with no way to tell how much is missing, would be a
//! silent lie).
//!
//! **Cap values (`Declared`, not (yet) `CertMode`-gated — flagged, not silently assumed).** Unlike
//! `CertStore::insert`, which takes `mode: CertMode` per call because its callers (the mode-gated
//! swap engine) always have a live `CertMode` in hand, **no current construction site of
//! [`PolicySlot`] carries a `CertMode`** (verified: `PolicySlot::new` is called only from this
//! module's own tests today — mitigation #14/G2, checked before assuming otherwise). Mode-gating a
//! parameter nothing would ever vary would be speculative generality (YAGNI), not honesty, so
//! [`DECLARED_POLICY_TRANSITION_CAP`]/[`DECLARED_POLICY_TRACE_CAP`] are flat `Declared` constants
//! rather than mode-dispatched functions — genuinely threading `CertMode` through once a caller
//! exists to supply one is a flagged residual, not implemented here. The two numbers are anchored
//! to *existing* `LanguageRetentionPolicy` §5 figures rather than invented fresh (the same style of
//! judgment call `CertStore::declared_cert_handle_cap`'s own doc comment makes for `Balanced`):
//! `transitions` (rare, coarse policy-set events) reuses `fast`'s `hot_first_fault_cap` record
//! count (64); `trace` (frequent, per-selection events) reuses `certified`'s (1024) — see
//! `docs/spec/Language-Retention-Policy.md` §5's amended table for the row recording this choice.
//!
//! # Guarantee tags (VR-5; rows in [`crate::guarantee_matrix::MATRIX`])
//!
//! - Transition-record append (one record per `set`, monotonic `seq`, subject to the retention
//!   cap above — an old record may be evicted, never a duplicate/lost `seq`): **`Exact`** —
//!   by construction.
//! - Selection without an active policy is an explicit [`SlotError::NoActivePolicy`]: **`Exact`**
//!   — fail-closed by construction (G2).
//! - Capture resolution of an unknown `policy_ref` is an explicit
//!   [`CaptureError::UnknownPolicyRef`]: **`Exact`** — fail-closed by construction (G2).
//! - Replay-reaches-the-recorded-decision: **`Empirical`** — the record-vs-replay differential
//!   is property-tested (`src/tests/policy_mech.rs`); determinism grounds in RFC-0005 `select`
//!   purity but carries no mechanized theorem, so it is not `Proven` (M-964 audit, DN-78
//!   appendix).
//! - Drop accounting (`transitions_dropped`/`trace_dropped` count exactly the evicted records):
//!   **`Exact`** — by construction (mirrors `CertStore::dropped`).

use mycelium_core::ContentHash;
use mycelium_select::{
    select, Candidate, Explanation, PolicyRegistry, SelectError, SelectionInputs, SelectionPolicy,
};

/// The `LanguageRetentionPolicy` §5 **Declared placeholder** cap for [`PolicySlot::transitions`]
/// (a per-`set` event — rare/coarse). Anchored to `fast`'s `hot_first_fault_cap` record count (64)
/// rather than invented fresh (see the module doc comment's cap-values note). Revisit once the
/// Phase-2 sizing pass or a real `CertMode`-carrying caller informs a better figure.
pub const DECLARED_POLICY_TRANSITION_CAP: usize = 64;

/// The `LanguageRetentionPolicy` §5 **Declared placeholder** cap for [`PolicySlot::trace`] (a
/// per-`select` event — frequent/fine-grained). Anchored to `certified`'s `hot_first_fault_cap`
/// record count (1024) rather than invented fresh (see the module doc comment's cap-values note).
pub const DECLARED_POLICY_TRACE_CAP: usize = 1024;

/// The RFC-0005 policy sites (§4: swap-target, packing; RFC-0008 RT3 adds placement as the
/// third). A [`PolicySlot`] is keyed by site so a set/select is always attributed to the site
/// it governs (provenance, RFC-0001 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySite {
    /// The RFC-0002 swap-target site.
    SwapTarget,
    /// The RFC-0004 §5 packing site.
    Packing,
    /// The RFC-0008 RT3 placement site (single-node in Phase I — DN-78 §3; the multi-node
    /// candidate set is deferred, see [`crate::r2_residual`]).
    Placement,
}

impl core::fmt::Display for PolicySite {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PolicySite::SwapTarget => write!(f, "swap-target"),
            PolicySite::Packing => write!(f, "packing"),
            PolicySite::Placement => write!(f, "placement"),
        }
    }
}

/// A reified policy-set transition record (G2: a mechanized set is never a silent override —
/// the transition itself is inspectable). Guarantee: **`Exact`** — every [`PolicySlot::set`]
/// appends exactly one record, with a per-slot monotonic sequence number, by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySetRecord {
    /// The site whose active policy changed.
    pub site: PolicySite,
    /// Per-slot monotonic sequence number (0 for the first set). Stays monotonic across the
    /// transition log's retention cap (CC-B6): a `seq` is generated once per `set` call and never
    /// reused, even once the record it named has been evicted under cap pressure.
    pub seq: u64,
    /// The previous active policy's content address, `None` on the first set.
    pub previous: Option<ContentHash>,
    /// The new active policy's content address ([`SelectionPolicy::policy_ref`]).
    pub new_policy: ContentHash,
    /// The new policy's display name (for the EXPLAIN/teaching surface).
    pub new_policy_name: String,
}

/// Why a slot operation failed — always explicit (G2), never a silent default choice.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotError {
    /// Selection was requested but no policy has been set for this site. Guarantee: **`Exact`**
    /// — fail-closed by construction; there is no built-in fallback policy (a silent default
    /// would be a black box, ADR-006).
    NoActivePolicy {
        /// The site that has no active policy.
        site: PolicySite,
    },
    /// The underlying RFC-0005 selection refused (e.g. an out-of-range override).
    Select(SelectError),
}

impl core::fmt::Display for SlotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SlotError::NoActivePolicy { site } => {
                write!(
                    f,
                    "no active policy is set for the {site} site — set one explicitly \
                     (PolicySlot::set); there is no silent default (G2/ADR-006)"
                )
            }
            SlotError::Select(e) => write!(f, "selection refused: {e}"),
        }
    }
}

impl std::error::Error for SlotError {}

impl From<SelectError> for SlotError {
    fn from(e: SelectError) -> Self {
        SlotError::Select(e)
    }
}

/// A runtime slot binding the **active** [`SelectionPolicy`] for one RFC-0005 site, with a
/// **capped**, append-only transition log and a **capped** selection trace (DN-78 §3 B-2;
/// capping: CC-B6, `docs/spec/Language-Retention-Policy.md` §5).
///
/// The slot is the mechanized *setter surface*: `set` swaps the active policy and records the
/// transition; `select` decides through the active policy and records the mandatory
/// [`Explanation`]. Both logs are read-only views (`transitions`/`trace`) — append-only by
/// construction (no public mutation besides the appending operations) and bounded by
/// [`DECLARED_POLICY_TRANSITION_CAP`]/[`DECLARED_POLICY_TRACE_CAP`] — exceeding the cap evicts the
/// oldest entry (`drop_oldest`, the `LanguageRetentionPolicy` §5 `on_overflow` shape) and
/// increments a never-silent drop counter ([`PolicySlot::transitions_dropped`]/
/// [`PolicySlot::trace_dropped`]) rather than growing without bound (G-8).
#[derive(Debug)]
pub struct PolicySlot {
    site: PolicySite,
    active: Option<SelectionPolicy>,
    transitions: Vec<PolicySetRecord>,
    trace: Vec<Explanation>,
    /// Every `set` ever made on this slot, independent of `transitions`' current (possibly
    /// capped) length — the seq generator; monotonic, never resets on eviction.
    transitions_total: u64,
    /// Every `select` ever made on this slot, independent of `trace`'s current length.
    trace_total: u64,
    /// How many `transitions` entries this slot has evicted under cap pressure (EXPLAIN-of-drop).
    transitions_dropped: u64,
    /// How many `trace` entries this slot has evicted under cap pressure (EXPLAIN-of-drop).
    trace_dropped: u64,
}

impl PolicySlot {
    /// An empty slot for `site` — no active policy, no transitions, no trace.
    #[must_use]
    pub fn new(site: PolicySite) -> Self {
        PolicySlot {
            site,
            active: None,
            transitions: Vec::new(),
            trace: Vec::new(),
            transitions_total: 0,
            trace_total: 0,
            transitions_dropped: 0,
            trace_dropped: 0,
        }
    }

    /// The site this slot governs.
    #[must_use]
    pub fn site(&self) -> PolicySite {
        self.site
    }

    /// Set the active policy, appending a [`PolicySetRecord`] (returned by reference).
    ///
    /// Guarantee: **`Exact`** — exactly one record is appended per call, `seq` is the per-slot
    /// monotonic count (generated from [`PolicySlot::transitions_total`], never from the possibly-
    /// capped log's current length — stable across evictions), and `previous` is the outgoing
    /// policy's content address (`None` on the first set). The transition is never silent (G2).
    /// If the log is at [`DECLARED_POLICY_TRANSITION_CAP`], the oldest record is evicted first
    /// (`drop_oldest`) and [`PolicySlot::transitions_dropped`] increments — the record just
    /// appended is always retained and returned (the cap is a fixed, non-zero constant here — a
    /// `0` cap, meaning "retain nothing," would need `set` to return `Option<&PolicySetRecord>`
    /// instead; that is a different, not-yet-needed contract, not silently half-supported here).
    pub fn set(&mut self, policy: SelectionPolicy) -> &PolicySetRecord {
        const { assert!(DECLARED_POLICY_TRANSITION_CAP > 0) };
        let record = PolicySetRecord {
            site: self.site,
            seq: self.transitions_total,
            previous: self.active.as_ref().map(SelectionPolicy::policy_ref),
            new_policy: policy.policy_ref(),
            new_policy_name: policy.name().to_owned(),
        };
        self.transitions_total += 1;
        self.active = Some(policy);
        while self.transitions.len() >= DECLARED_POLICY_TRANSITION_CAP {
            self.transitions.remove(0);
            self.transitions_dropped += 1;
        }
        self.transitions.push(record);
        self.transitions
            .last()
            .expect("push above guarantees a last element (cap > 0, asserted at compile time)")
    }

    /// The active policy, if one has been set. `None` is not a fallback state — selection
    /// through an unset slot refuses explicitly ([`SlotError::NoActivePolicy`]).
    #[must_use]
    pub fn active(&self) -> Option<&SelectionPolicy> {
        self.active.as_ref()
    }

    /// The transition log — every **retained** `set`, in order. **Capped**: once
    /// [`DECLARED_POLICY_TRANSITION_CAP`] is reached, the oldest entries are evicted; the log's
    /// length is `min(transitions_total(), DECLARED_POLICY_TRANSITION_CAP)`. A truncated view is
    /// always disclosed, never silent — see [`PolicySlot::transitions_dropped`]/
    /// [`PolicySlot::transitions_total`] for the honest full picture (G2).
    #[must_use]
    pub fn transitions(&self) -> &[PolicySetRecord] {
        &self.transitions
    }

    /// How many `set` calls this slot has ever recorded, INCLUDING evicted ones — the true
    /// historical count (`transitions().len() + transitions_dropped()` when the log is non-empty
    /// and the cap is non-zero; more simply, the next `seq` this slot will assign).
    #[must_use]
    pub fn transitions_total(&self) -> u64 {
        self.transitions_total
    }

    /// How many transition records this slot has evicted under cap pressure since construction —
    /// the never-silent drop counter (EXPLAIN-of-drop; `LanguageRetentionPolicy` §8). Never
    /// silently reset or hidden.
    #[must_use]
    pub fn transitions_dropped(&self) -> u64 {
        self.transitions_dropped
    }

    /// The selection trace: the mandatory [`Explanation`] of every **retained** selection made
    /// through this slot, in order — extractable for capture/diffing (DN-78 §3 B-2). **Capped**:
    /// once [`DECLARED_POLICY_TRACE_CAP`] is reached, the oldest entries are evicted — see
    /// [`PolicySlot::trace_dropped`]/[`PolicySlot::trace_total`] for the honest full picture (G2).
    /// (`capture`/`replay` never depend on this log — they consume the [`Explanation`]
    /// [`PolicySlot::select`] returns directly, so eviction here never affects their correctness.)
    #[must_use]
    pub fn trace(&self) -> &[Explanation] {
        &self.trace
    }

    /// How many `select` calls this slot has ever recorded, INCLUDING evicted ones.
    #[must_use]
    pub fn trace_total(&self) -> u64 {
        self.trace_total
    }

    /// How many trace entries this slot has evicted under cap pressure since construction — the
    /// never-silent drop counter (EXPLAIN-of-drop; `LanguageRetentionPolicy` §8).
    #[must_use]
    pub fn trace_dropped(&self) -> u64 {
        self.trace_dropped
    }

    /// Decide through the active policy (RFC-0005 `select`), recording the mandatory
    /// [`Explanation`] into the slot's trace.
    ///
    /// Errors are explicit: an unset slot is [`SlotError::NoActivePolicy`] (never a silent
    /// default — G2/ADR-006); an underlying selection refusal passes through as
    /// [`SlotError::Select`]. The returned [`Explanation`] is always the one just decided,
    /// regardless of whether the trace log had room to retain it (capture/replay never depend on
    /// `trace()` — see its doc comment).
    pub fn select(
        &mut self,
        inputs: &SelectionInputs,
        forced: Option<usize>,
    ) -> Result<(Candidate, Explanation), SlotError> {
        let site = self.site;
        let policy = self
            .active
            .as_ref()
            .ok_or(SlotError::NoActivePolicy { site })?;
        const { assert!(DECLARED_POLICY_TRACE_CAP > 0) };
        let (candidate, explanation) = select(policy, inputs, forced)?;
        self.trace_total += 1;
        while self.trace.len() >= DECLARED_POLICY_TRACE_CAP {
            self.trace.remove(0);
            self.trace_dropped += 1;
        }
        self.trace.push(explanation.clone());
        Ok((candidate, explanation))
    }
}

/// A captured policy: the RFC-0005-conformant [`SelectionPolicy`] value that decided a recorded
/// [`Explanation`], materialized for reuse/diffing (DN-78 §3 B-1). Not an opaque handle — the
/// full policy value is inspectable (ADR-006).
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedPolicy {
    /// The policy's content address (equal to `policy.policy_ref()` — verified at capture).
    pub policy_ref: ContentHash,
    /// The policy value itself.
    pub policy: SelectionPolicy,
}

/// Why a capture failed — always explicit (G2), never a silent reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// The recorded `policy_ref` resolves to nothing in the given registry. Guarantee:
    /// **`Exact`** — fail-closed by construction; capture never fabricates a policy.
    UnknownPolicyRef {
        /// The unresolvable content address.
        policy_ref: ContentHash,
    },
    /// The registry returned a policy whose own content address differs from the requested one
    /// (a corrupted registry). Never silently accepted.
    RefMismatch {
        /// The content address the capture asked for.
        requested: ContentHash,
        /// The content address the resolved policy actually hashes to.
        resolved: ContentHash,
    },
}

impl core::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CaptureError::UnknownPolicyRef { policy_ref } => {
                write!(
                    f,
                    "policy_ref {policy_ref:?} is not in the registry — capture refuses rather \
                     than reconstructing a policy (G2/ADR-006)"
                )
            }
            CaptureError::RefMismatch {
                requested,
                resolved,
            } => {
                write!(
                    f,
                    "registry corruption: requested {requested:?} but the stored policy hashes \
                     to {resolved:?}"
                )
            }
        }
    }
}

impl std::error::Error for CaptureError {}

/// Materialize the policy that decided `explanation` from `registry` (DN-78 §3 B-1).
///
/// Guarantee: **`Exact`** for the resolution contract — an unknown ref is an explicit
/// [`CaptureError::UnknownPolicyRef`] and a hash mismatch an explicit
/// [`CaptureError::RefMismatch`]; a returned [`CapturedPolicy`] always satisfies
/// `policy.policy_ref() == policy_ref` (checked here, not assumed).
pub fn capture(
    registry: &PolicyRegistry,
    explanation: &Explanation,
) -> Result<CapturedPolicy, CaptureError> {
    let requested = explanation.policy.clone();
    let policy = registry
        .get(&requested)
        .ok_or_else(|| CaptureError::UnknownPolicyRef {
            policy_ref: requested.clone(),
        })?;
    let resolved = policy.policy_ref();
    if resolved != requested {
        return Err(CaptureError::RefMismatch {
            requested,
            resolved,
        });
    }
    Ok(CapturedPolicy {
        policy_ref: requested,
        policy: policy.clone(),
    })
}

/// Why a replay failed — always explicit (G2), never a silent pass.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayError {
    /// The captured policy is not the one the record claims decided it (`policy_ref`
    /// mismatch) — replaying against the wrong policy would be a silent apples-to-oranges
    /// comparison, so it refuses up front.
    PolicyMismatch {
        /// The record's policy content address.
        recorded: ContentHash,
        /// The captured policy's content address.
        captured: ContentHash,
    },
    /// Re-running the recorded inputs refused (e.g. a recorded override index that no longer
    /// fits the policy — impossible for a faithful capture, but never silently swallowed).
    Select(SelectError),
    /// The replayed decision differs from the recorded one. With a validated policy and
    /// identical inputs this indicates non-determinism or a record from different code —
    /// either way it is surfaced, never absorbed.
    Diverged {
        /// The original record.
        recorded: Box<Explanation>,
        /// The replayed record that differs from it.
        replayed: Box<Explanation>,
    },
}

impl core::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReplayError::PolicyMismatch { recorded, captured } => write!(
                f,
                "replay refused: the record was decided by {recorded:?} but the captured \
                 policy is {captured:?}"
            ),
            ReplayError::Select(e) => write!(f, "replay selection refused: {e}"),
            ReplayError::Diverged { recorded, replayed } => write!(
                f,
                "replay diverged: recorded chose index {} (rule {:?}), replay chose index {} \
                 (rule {:?})",
                recorded.chosen_index,
                recorded.matched_rule,
                replayed.chosen_index,
                replayed.matched_rule
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<SelectError> for ReplayError {
    fn from(e: SelectError) -> Self {
        ReplayError::Select(e)
    }
}

/// Replay a recorded decision against its captured policy (DN-78 §3 B-1): re-run the recorded
/// inputs — honoring the recorded override state — and require the identical [`Explanation`].
///
/// Guarantee: **`Empirical`** for "replay reaches the recorded decision" — the record-vs-replay
/// differential is property-tested over randomized policies/inputs; RFC-0005 `select` is
/// deterministic (same `(policy, inputs, forced)` → same result) but that determinism carries
/// no mechanized theorem, so the claim is not `Proven` (VR-5; M-964 audit). A divergence is an
/// explicit [`ReplayError::Diverged`] carrying both records for inspection (G2).
pub fn replay(
    captured: &CapturedPolicy,
    recorded: &Explanation,
) -> Result<Explanation, ReplayError> {
    if captured.policy_ref != recorded.policy {
        return Err(ReplayError::PolicyMismatch {
            recorded: recorded.policy.clone(),
            captured: captured.policy_ref.clone(),
        });
    }
    let forced = recorded.overridden.then_some(recorded.chosen_index);
    let (_, replayed) = select(&captured.policy, &recorded.inputs, forced)?;
    if replayed != *recorded {
        return Err(ReplayError::Diverged {
            recorded: Box::new(recorded.clone()),
            replayed: Box::new(replayed),
        });
    }
    Ok(replayed)
}
