# Spec — `std.runtime` / `colony` (the fungal concurrency surface — RESERVED-not-active until RFC-0008 constructs land)

| Field | Value |
|---|---|
| **Status** | **Accepted** (2026-06-21 — maintainer-ratified the **v0 R1 surface**). Preconditions met: RFC-0016 Accepted, RFC-0008 Accepted, the Phase-7 v0 R1 slice landed (M-521; ADR-020 Enacted), and the DN-16 honesty re-audit (2026-06-21) found the crate ratification-ready (honest tags; thin empirical base noted as a V&V follow-up, not a blocker). **Further constructs activate construct-by-construct at the Phase-7 runtime gate** per ADR-020 — this ratifies the landed v0 surface, not the not-yet-landed constructs. Was **Draft (needs-design)** 2026-06-17. |
| **Module / Ring** | `std.runtime` (aka `colony`) · Ring 1/2 (RFC-0016 §4.2 — the supervision/composition surface is Ring 1 over the landed `mycelium_interp::supervise` capability; the broader concurrency lexicon is Ring 2) · Tier A (RFC-0016 §4.3, the differentiator runtime surface) |
| **Tracks** | `M-521` (#162) — the Phase-5 task this spec delivers; sequenced against the Phase-7 runtime track (M-355–357 already landed the supervision/composition base) |
| **Scope** | The library bindings for the RFC-0008 runtime lexicon — `hypha`/`fuse`/`colony`/`cyst`/`graft`/`forage`/`backbone`/`mesh`/`tier`/`reclaim` — plus **structured concurrency** (`Scope`, the `Colony` grouping) and **`reclaim` bounded-cascade supervision** (M-355–357). The module owns the concurrency/colony lexicon + supervision; it **presents the binding set design-first** and activates **construct-by-construct** as each RFC-0008 vocabulary item lands (no premature surface — VR-5). |
| **Boundary** | Out of scope: the **typed *read* surface** of the in-runtime logical clock (the `LogicalInstant` read — a monotonic/wall/logical typed distinction) is `std.time` (M-529). `runtime` **owns the logical clock's *advancement semantics*** (what a tick means, when it increments — the M-356 reclaim-windowing basis) and **consumes** `time`'s read surface, rather than owning the read (mirrors `std.time` §3 / its Boundary). The **recovery *policy*** (what to do on a failure value) is `std.recover` (M-520, RFC-0014); `runtime` owns the *cascade* that re-runs supervised units, not the per-error recovery DSL. **Structured diagnostics** (the failure record/trace surfaced) are `std.diag` (M-510, RFC-0013). A **representation** change at a wire boundary is `std.swap` (M-516), never a silent `xloc` recode (S1). Placement/`forage` is a reified `std.select` (M-519) policy (RFC-0005, the third selection site). |
| **Depends on** | **RFC-0008** (Accepted — the runtime/concurrency model; RT1–RT7; §4.5 the reserved lexicon; §4.7 the composition contract); RFC-0016 §4.1 (the contract), §4.2 (rings), §4.3 (Tier A), §8-Q4 (the std-vs-separate-phylum question this spec FLAGs); RFC-0001 (the value model — `Value`/`Repr`/`Meta`, the guarantee lattice §4.3, meet-composition §4.7). |
| **Grounds on** | M-356 `mycelium_interp::supervise::Supervisor` (the `reclaim` bounded-cascade supervisor — Done); M-357 `mycelium-mlir::runtime` (the deterministic fork/join `Scope`/`Colony` executor + the RT2 sequentialization differential — Done) and its follow-on typed SPSC channels (`mycelium-mlir::channel` — Done); RFC-0014 §4.8 / M-353 (the per-task `Budgets` ledger + `cascade` effect budget). KC-3: above the kernel — the scheduler/supervisor live **outside** the trusted base (RT2). |

---

## 1. Summary

`std.runtime` is the library home of Mycelium's **fungal concurrency surface** — the RFC-0008 runtime lexicon
(`hypha`/`fuse`/`colony`/`cyst`/`graft`/`forage`/`backbone`/`mesh`/`tier`/`reclaim`), **structured concurrency**
(the `Scope`/`Colony` grouping, RT7), and **`reclaim` bounded-cascade supervision** (M-355–357). Its **honesty
crux is C2 applied to *a planning surface*:** almost all of this vocabulary is **RESERVED-not-active** (Glossary
**⟂**) until the corresponding RFC-0008 construct lands on the Phase-7 runtime track — so the guarantee matrix
(§4) honestly marks each reserved binding **`Declared` / not-yet-active** with its **activation condition**, and
presents **no guaranteed, callable API** for a construct that does not exist yet. Presenting a confident active
surface here would be the planning analogue of G2 (a silent over-claim) — forbidden by VR-5. Its second crux is
C1/G2 on the *active* slice that *has* landed: a failed `hypha` and a supervised cascade are a **bounded,
observable `reclaim`** — an explicit `TaskOutcome` and a legible diagnostic trace, **never a silent hang, a
swallowed panic, or an unbounded restart storm** (RFC-0008 §4.7; RFC-0013/0014 I1). Ring 1 over the landed
`mycelium_interp::supervise` / `mycelium-mlir::runtime` capabilities, Ring 2 for the broader lexicon; it adds no
trusted code (KC-3) — the scheduler and supervisor live outside the kernel (RT2).

## 2. Scope & module boundary

- **In scope:**
  - **Structured concurrency** — the `Scope`/`Colony` grouping (RT7): fork a set of cooperating `hypha`, join
    every child, **no orphan/leaked task is expressible** (LR-9 lifted to runtime units). `Scope` is the
    *active, landed* core (M-357).
  - **`reclaim` bounded-cascade supervision** (M-355–357) — restart a failed child under a cascade bounded on
    **both axes**: a total `cascade` effect-budget cap **and** a windowed max-restart-intensity (≤ *N* restarts
    in *W* logical ticks). Exceeding either is an explicit **escalation** (RFC-0008 §4.7-C4).
  - **The composition primitives** (M-356) — per-task `Budgets`, cooperative cancellation (`CancelToken`),
    cross-task failure propagation as an explicit `TaskOutcome` (RFC-0008 §4.7-C1/C2/C3).
  - **The reserved lexicon bindings** (`hypha`/`fuse`/`cyst`/`graft`/`forage`/`backbone`/`mesh`/`tier` — and
    the `colony`/`reclaim` names) — **presented as the binding set**, each tagged with its activation condition;
    activated construct-by-construct as RFC-0008 vocabulary lands.
- **Out of scope (and who owns it):**
  - **The logical-clock / time source** the supervisor's intensity window advances on → `std.time` (M-529); the
    monotonic-vs-wall distinction is *their* typed surface (RFC-0008 §4.7 uses a deterministic logical counter
    for v0; physical/hybrid clocks are RFC-0008 R8-Q3, deferred). `runtime` consumes a clock, never defines one.
  - **The recovery *policy*** (the declarative "on this error, do X" DSL + bounded effects) → `std.recover`
    (M-520, RFC-0014). `runtime` owns the *cascade mechanism* (`reclaim`), not the per-failure recovery vocabulary.
  - **The structured failure record / trace** a refusal or escalation carries → `std.diag` (M-510, RFC-0013);
    `runtime` *emits into* the diagnostic surface (additive, I1), it does not define the record schema.
  - **Representation change** at a wire/channel boundary → `std.swap` (M-516); an `xloc` whose wire format
    differs from the value's `Repr` performs a *visible* `Swap` (S1), never a silent recode.
  - **Placement as a learned/ambient effect** → there is none; `forage` is a reified `std.select` (M-519)
    RFC-0005 policy with mandatory EXPLAIN (RT3 — the third selection site).
- **Ring & layering:** the supervision/composition surface (`Scope`/`Colony`/`reclaim`/budgets/cancellation) is
  **Ring 1** — an ergonomic library over the *landed* capability crates `mycelium_interp::supervise` and
  `mycelium-mlir::runtime`/`channel` (certificate/EXPLAIN **consumer**, no new trusted code). The broader lexicon
  (`fuse`/`cyst`/`graft`/`forage`/`backbone`/`mesh`/`tier`) is **Ring 2** — written to the contract over Ring 0/1
  as each construct lands. The scheduler and supervisor are **outside** the kernel (RT2; KC-3).

## 3. Exported-op surface (design sketch)

A DESIGN sketch — enough to fix the surface and feed the guarantee matrix, **not a committed grammar** and (for
the **⟂** rows) **not yet a callable API**. Value-semantic, immutable-by-default (RT1 — only `Value`s cross a
boundary). Fallible ops return `Result`; effectful ops declare their effect (C6). Each reserved binding is shown
with its **⟂ ACTIVATION** marker — it activates only when the named RFC-0008 construct lands.

```
// illustrative signatures (not a committed surface; ⟂ rows are RESERVED-not-active)

// === ACTIVE slice (landed: M-356/M-357 — structured concurrency + supervision) ===

// Structured concurrency — the Colony scope (RT7: every child joined, no orphan).
type Colony   // the dynamic grouping of cooperating hypha under shared cancel + supervision (DN-06)
type Task     // a cooperative unit carrying its own Budgets ledger + the shared CancelToken
enum TaskOutcome { Done(Value), Failed(Diag), BudgetExhausted(Budget), Cancelled }  // exactly one; no silent variant (C1)

scope_run<T>(body: fn(&Colony) -> T) -> Result<T, RuntimeErr>   // joins all children before return (RT7)
spawn(c: &Colony, t: Task) -> Handle                            // fork a child into the scope; never orphaned
cancel(c: &Colony) -> ()                                        // cooperative; propagates down the scope tree (RT7)

// reclaim — bounded-cascade supervision (M-356; RFC-0008 §4.7-C4).
type Supervisor   // restart policy + the two-axis cascade bound
supervise(c: &Colony, policy: ReclaimPolicy) -> Supervisor
struct ReclaimPolicy { cascade_budget: Budget, max_restarts: u32, window_ticks: u32 }  // both axes, reified (C3)
// Exceeding EITHER axis -> explicit Escalation (the supervisor's own Failed outcome), never a storm.

// === RESERVED-not-active lexicon (⟂ — bindings presented, activate construct-by-construct) ===

hypha(...)   // ⟂ ACTIVATION: the RFC-0008 §4.5 `hypha` surface construct (currently realized only as the
             //   landed Task/Scope primitive; the *named* binding is reserved — DN-03/RFC-0008 §4.5)
fuse(a, b, merge) -> Result<Value, ConflictPolicy>   // ⟂ ACTIVATION: RFC-0008 §4.5 `fuse` (RT6 lawful merge:
             //   join payload, meet guarantee; a non-semilattice merge is an explicit conflict, not an anastomosis)
cyst(k)      -> Cyst                                  // ⟂ ACTIVATION: RFC-0008 §4.4 encystment (content-addressed checkpoint)
graft(cap)   -> Result<Substrate, GraftErr>          // ⟂ ACTIVATION: RFC-0008 §4.5 capability contract (affine handle, LR-8)
forage(work, policy) -> Placement                    // ⟂ ACTIVATION: RFC-0008 §4.5 placement as an RFC-0005 policy (EXPLAIN-able, RT3)
backbone(path)                                       // ⟂ ACTIVATION: RFC-0008 §4.5 declared high-bandwidth path (semantics-free, RT3)
mesh(...)    -> ProbabilityBound                      // ⟂ ACTIVATION: RFC-0008 §4.3 gossip/pub-sub (probabilistic, δ-tagged, RT5)
tier(mode)                                            // ⟂ ACTIVATION: RFC-0008 §4.5 execution-mode switch (NFR-7-equivalent; NOT a swap, S1)

enum RuntimeErr { Cancelled, BudgetExhausted, Escalation, Deadlock, /* xloc/mesh failures: R2 */ }
```

> **Note (the honesty crux, FLAGGED §7-Q1/Q2):** the **⟂** rows are **not a committed surface and not yet
> callable** — they name the binding the module *will* own when the RFC-0008 construct lands. The Phase-7 gate
> (RFC-0016 §8-Q4) decides their sequencing *and* whether they live in `std.runtime` at all or in a separate
> `runtime` phylum. This spec deliberately ships **no guaranteed API for a construct that does not exist** (VR-5).

## 4. Guarantee matrix (the load-bearing deliverable — RFC-0016 §4.5)

Rows = the binding vocabulary this module exposes. To be encoded as a checked table (the RFC-0003 §4 template)
and asserted in tests **only for the ACTIVE rows**; the **⟂** rows carry their **activation condition** in place
of a guarantee and become assertable when their construct lands. **Tag legend:** `Exact` = exact, no accuracy
semantics; `Empirical` = established by a differential/trial; **`Declared (⟂ not-yet-active)`** = the binding is
*reserved* — asserted, always flagged, with the named activation condition (the honest floor for a
not-yet-constructed surface — VR-5).

| Op / binding | Guarantee tag | Fallibility (explicit error set) | Declared effects | EXPLAIN-able? |
|---|---|---|---|---|
| `scope_run` / `Colony` (structured concurrency) | `Exact` (the structuring is exact: every child joined, no orphan — RT7) | `Err(Escalation \| Cancelled \| Deadlock)` (no silent hang) | `concurrency` (scheduling — declared, KC-3-outside-kernel) | yes (the scope tree is reified/inspectable) |
| `spawn` | `Exact` (the child is owned by the scope by construction — RT7) | total (the fork itself; the child resolves to a `TaskOutcome`) | `concurrency` | yes (the parent/child edge is inspectable) |
| `cancel` (cooperative, RT7) | `Exact` (cooperative; never preemptive — no dropped in-flight outcome) | total → an additive `Cancelled` outcome (I1) | `concurrency` | yes |
| `TaskOutcome` resolution (C3) | `Exact` (exactly one explicit variant; **no silent/dropped variant**) | `Done \| Failed \| BudgetExhausted \| Cancelled` | none | yes (the outcome is the value the parent must act on) |
| `reclaim` / `supervise` (bounded cascade, C4) | `Exact` (the bound is exact on **both** axes; logical-clock window is deterministic) | `Err(Escalation)` on exceeding *either* the `cascade` budget *or* the intensity window | `cascade(budget)` + `time` (logical clock, from `std.time`) | yes (the `ReclaimPolicy` + the escalation trace are reified) |
| per-task `Budgets` (C1) | `Exact` (per-task ledger; one task cannot exhaust another's) | `Err(EffectBudgetExhausted)` *in that task* | `alloc/retry/cascade(budget)` | yes (the ledger is inspectable) |
| `hypha` (named binding) | **`Declared (⟂ not-yet-active)`** | — | (declares `concurrency` when active) | n/a until active |
| `fuse` (RT6 lawful merge) | **`Declared (⟂ not-yet-active)`** — when active, the *merge result* tags by **meet** of inputs (RFC-0001 §4.7); the merge op's intrinsic strength may be `Proven` only with the semilattice side-conditions *checked* (RT6) | — (when active: `Err(Conflict)` for a non-semilattice merge) | none | n/a until active |
| `cyst` (encystment) | **`Declared (⟂ not-yet-active)`** (content-addressed when active — ADR-003) | — | none | n/a until active |
| `graft` (capability contract) | **`Declared (⟂ not-yet-active)`** | — (when active: `Err(GraftErr)`; affine `substrate`, LR-8) | `io` (when active) | n/a until active |
| `forage` (placement) | **`Declared (⟂ not-yet-active)`** — semantics-free when active (RT3: changes performance, never meaning) | — | `concurrency` (placement) | yes-by-design (an RFC-0005 EXPLAIN policy) — once active |
| `backbone` (transport path) | **`Declared (⟂ not-yet-active)`** (semantics-free placement artifact, RT3) | — | none (perf-only) | once active |
| `mesh` (gossip/pub-sub) | **`Declared (⟂ not-yet-active)`** — when active, carries a `ProbabilityBound` (δ) with a basis, **never `Exact`/reliable** (RT5) | — (when active: lossy delivery is explicit) | `io` (when active) | once active |
| `tier` (exec-mode switch) | **`Declared (⟂ not-yet-active)`** — NFR-7 observable-equivalent when active; **not** a `Swap` (S1) | — | `tier` (when active) | once active |

**Tag justification (VR-5 — downgrade rather than overclaim):**
- **`Exact` rows** are the *active, landed* slice (M-356/M-357). They carry no *accuracy* semantics — the
  guarantee is structural: RT7 makes "every child joined, no orphan" an exact property, and §4.7-C4 makes the
  cascade bound exact on both axes (a deterministic logical-clock window, RFC-0008 §4.7). The RT2 *determinism*
  of the executor itself is separately tagged **`Empirical`** (the sequentialization / Kahn differential is the
  evidence, **not** a mechanized proof — RFC-0008 changelog, VR-5); this matrix's `Exact` rows are the
  *structuring/bounding* guarantees, not a determinism claim.
- **`Declared (⟂ not-yet-active)` rows** are the **honesty crux**. Each names a RFC-0008 §4.x construct that has
  **not landed as a surface binding**. They are tagged at the honest floor (`Declared`, always flagged) with an
  explicit **activation condition** — the spec asserts only *which* construct must land and *what tag it will
  then carry*, never a present guarantee for an absent surface. `fuse` and `mesh` annotate their *eventual*
  honest tag (meet-composed / δ-bounded) so the activation does not later sneak in an over-claim. **No `⟂` row is
  presented as callable today** (no premature surface — VR-5).
- **No silent hang / swallowed panic / unbounded storm anywhere** (C1/G2): a stalled dataflow is an explicit
  `Deadlock { parked }`; a failed child is an explicit `Failed` outcome the parent *must* act on; an over-budget
  cascade is an explicit `Escalation` — never an IEEE-NaN analogue of the runtime (a silent stop).

## 5. §4.1 contract conformance (C1–C6)

- **C1 — never-silent (G2).** On the active slice this is *enforced, not asserted*: `TaskOutcome` has **no
  silent/dropped variant** (every child resolves to `Done`/`Failed`/`BudgetExhausted`/`Cancelled` the parent
  must act on, RFC-0008 §4.7-C3); a stalled network is an explicit `Deadlock { parked }`, never a hang; an
  over-budget cascade is an explicit `Escalation`, never an unbounded restart storm; cancellation is cooperative,
  so no in-flight explicit outcome is dropped mid-step (C2). For the **⟂** rows, C1 is *deferred with its
  construct* — and the never-silent property is a **rejection criterion** for each construct's activation (RT4).
- **C2 — honest per-op tag (VR-5).** The module's crux. The active rows tag `Exact` for the structural/bounding
  guarantees and **`Empirical`** for executor determinism (the differential is the evidence, never `Proven` — no
  in-repo mechanized proof). Every reserved binding tags **`Declared (⟂ not-yet-active)`** with its activation
  condition — **the spec presents no guaranteed surface for a construct that does not exist** (the planning
  analogue of G2, forbidden). `fuse` will tag by **meet** of inputs (RFC-0001 §4.7); `mesh` will carry a δ
  `ProbabilityBound`, never `Exact`.
- **C3 — no black boxes / EXPLAIN (SC-3/G11).** The structured `Scope`/`Colony` tree is **reified and
  inspectable** (parent/child edges, join state). The `ReclaimPolicy` (both cascade axes) and every `Escalation`
  carry an inspectable, EXPLAIN-able trace — *why* a restart fired, *which* axis tripped (RFC-0008 §4.7-C4).
  `forage` placement, when active, is a reified RFC-0005 policy with **mandatory** EXPLAIN (RT3). No opaque
  scheduler decision is user-visible without its artifact.
- **C4 — content-addressed, value-semantic (ADR-003 / RFC-0001).** Only immutable `Value`s cross a
  `hypha`/channel/scope boundary, `Meta` intact (RT1; WF5 extended). A `cyst` checkpoint is a **content-addressed**
  artifact (ADR-003) when active. The `Meta` a value carries (its guarantee tag, provenance) is **not** identity
  (ADR-003) — supervision/placement metadata never changes a value's identity.
- **C5 — above the kernel (KC-3).** The scheduler (`mycelium-mlir::runtime`) and supervisor
  (`mycelium_interp::supervise`) live **outside** the trusted base — concurrency adds scheduling *outside* the
  kernel, never new meaning inside it (RT2; the §4.7 composition contract "adds no L0 node"). `std.runtime`
  consumes these landed capabilities; it enlarges no trusted code. A `graft`/`wild` FFI floor, when `graft`
  activates, is confined to an audited `wild` block and inventoried (LR-9) — FLAGGED §7-Q4.
- **C6 — declared, bounded effects (RFC-0014).** Concurrency/scheduling is a **declared** effect on the
  signature (the matrix's `concurrency` column). Supervision is a **bounded** effect: the `reclaim` cascade is
  bounded on both axes (a `cascade` effect budget — M-353 — *and* a windowed intensity bound), so it is a
  **declared, bounded cascade**, never unbounded (RFC-0008 §4.7-C4; M-355–357). Per-task `Budgets` are instanced
  per task (C1): one task's overrun is an in-that-task `EffectBudgetExhausted`, never global.

## 6. Grounding

- The runtime model, the lexicon, and the never-silent/honest-tag invariants: **RFC-0008** (Accepted
  2026-06-16) — RT1 (values move, state never shared), RT2 (deterministic fragment is the default; trusted base
  stays sequential, KC-3), RT3 (nondeterminism reified + EXPLAIN), RT4 (partial failure explicit), RT5 (runtime
  guarantees on the one lattice — δ for `mesh`), RT6 (lawful `fuse`: join payload, meet guarantee), RT7
  (structured lifetimes — no orphan task); **§4.5** (the reserved lexicon table — each term **⟂** until DN-03
  ratifies + the construct lands); **§4.7** (the composition contract: per-task budgets, cooperative cancellation,
  cross-task propagation, the two-axis `reclaim` bounded cascade).
- The landed surface this grounds on: **M-356** `mycelium_interp::supervise::Supervisor` (the bounded-cascade
  supervisor, both axes, explicit `Escalation`); **M-357** `mycelium-mlir::runtime` (the deterministic fork/join
  `Scope`/`Colony` executor + the RT2 sequentialization differential) and its typed SPSC channel follow-on
  (`mycelium-mlir::channel` — explicit `Deadlock`/`Closed`/`Disconnected`, never a silent hang); **M-353 /
  RFC-0014 §4.8** (the per-task `Budgets` ledger + the `cascade` effect budget). The `colony` name (the dynamic
  grouping) is **DN-06**; the `Colony` alias of the structured `Scope` is the M-357 realization.
- The contract, rings, tier placement, and the cross-phase FLAG: **RFC-0016** §4.1 (C1–C6), §4.2 (Ring 1/2),
  §4.3 (the `runtime`/`colony` row + the "reserved-not-active until RFC-0008 constructs land … sequenced against
  the Phase-7 track" framing), §4.5 (the guarantee-matrix obligation), **§8-Q4** (the `runtime`/`colony`
  sequencing + std-vs-separate-`runtime`-phylum question — FLAGGED, not decided here).
- The value model + honesty lattice + reserved-marker discipline: **RFC-0001** (the `Exact ⊐ Proven ⊐ Empirical
  ⊐ Declared` lattice §4.3; meet-composition §4.7; `Value`/`Repr`/`Meta`); **Glossary** (the **⟂**
  reserved-not-active lexicon: `hypha`/`fuse`/`colony`/`cyst`/`graft`/`forage`/`backbone`/`mesh`/`tier`/`reclaim`);
  **VR-5** (honest tags / downgrade), **G2** (never-silent), **KC-3** (small kernel), **LR-8/LR-9** (affine
  substrate / no leak — lifted to runtime units by RT7), **ADR-003** (metadata is not identity).

## 7. Open questions (FLAGGED — resolve before ratification)

- **(Q1) Does `runtime`/`colony` live in `std` at all — or in a separate `runtime` phylum?** RFC-0016 **§8-Q4**.
  The fungal runtime surface depends on the RFC-0008 constructs activating (Phase 7); it **may live in a separate
  `runtime` phylum gated on Phase 7** rather than in the `std` phylum. This spec **presents the binding set + the
  decision as a FLAGGED question — it does not silently decide placement** (the planning analogue of G2). —
  *Disposition: FLAGGED to §8-Q4; the maintainer decides `std.runtime` vs a `runtime` phylum at ratification.
  This spec's binding set is written to be relocatable wholesale either way.*
- **(Q2) Phase-7 sequencing — which constructs activate, in what order.** The **⟂** rows activate
  **construct-by-construct** as each RFC-0008 §4.5 vocabulary item lands (the §4.6 staging: R1 single-node →
  R2 distribution). The *active* slice today is the M-356/M-357 supervision + structured-concurrency base; the
  *next* R1 slice is typed channels + the full scheduler; `xloc`/`mesh`/`graft` are R2. — *Disposition: FLAGGED;
  the matrix's `⟂` rows flip to a real tag per-construct as it lands — no premature surface (VR-5). Ties to
  RFC-0008 §4.6 staging + R8-Q1 (the scheduler).*
- **(Q3) The supervision intensity clock.** The §4.7-C4 window uses a **deterministic logical clock** (a
  monotonic counter) for v0; physical/hybrid clocks for *real-time* intensity are RFC-0008 **R8-Q3**, deferred.
  This couples `runtime` to `std.time` (M-529): is the logical clock `runtime`-local, or a `std.time` logical-clock
  type `runtime` consumes? — *Disposition: FLAGGED; default to consuming a `std.time` clock type so the
  monotonic-vs-wall distinction stays where M-529 owns it. Boundary recorded for the orchestrator.*
- **(Q4) The `graft`/`wild` FFI floor.** When `graft` (a capability contract with external infrastructure)
  activates, its affine `substrate` handle bottoms out in OS facilities → a `wild` block (ADR-014, LR-9). That
  would narrow the C5 "no new trusted code" claim to "no new trusted code *outside an audited `wild` inventory*".
  — *Disposition: FLAGGED; ties to RFC-0016 §8-Q6 (the minimal audited FFI floor / `std-sys` split). Until
  `graft` activates, `runtime` asserts no `wild`.*
- **(Q5) Ergonomics vs the contract at the runtime call site (tension A).** How much of the
  scope/supervision/EXPLAIN/effect machinery is always-explicit at the spawn/supervise site vs
  implicit-but-inspectable (the RFC-0012 ambient lesson)? — *Disposition: FLAGGED to RFC-0016 §8-Q3; default to
  required-explicit (scope + `ReclaimPolicy` named at the call site) until the per-ring ergonomics pass.*

## Meta — changelog

- **2026-06-17 — Draft (needs-design).** Stands up the `std.runtime` / `colony` (M-521, #162) module spec under
  RFC-0016 (Draft) and RFC-0008 (Accepted): the Tier-A fungal concurrency surface — the RFC-0008 runtime lexicon
  (`hypha`/`fuse`/`colony`/`cyst`/`graft`/`forage`/`backbone`/`mesh`/`tier`/`reclaim`), structured concurrency
  (`Scope`/`Colony`, RT7), and `reclaim` bounded-cascade supervision (M-355–357). **Honesty crux:** most of this
  surface is **RESERVED-not-active** (Glossary **⟂**) until the RFC-0008 constructs land on the Phase-7 track, so
  the guarantee matrix honestly marks each reserved binding **`Declared` / not-yet-active** with its activation
  condition and presents **no guaranteed, callable API** for an absent construct (no premature surface — VR-5,
  the planning analogue of G2). The *active* slice (M-356 supervisor / M-357 fork-join `Scope`) is the
  never-silent floor: a failed `hypha` and a supervised cascade are a **bounded, observable `reclaim`** — explicit
  `TaskOutcome`/`Escalation`/`Deadlock` and a legible diagnostic trace, never a silent hang, swallowed panic, or
  unbounded restart storm (RFC-0008 §4.7; RFC-0013/0014 I1). Fixes the scope + boundary (time → `std.time`;
  recovery policy → `std.recover`; diagnostics → `std.diag`; repr change → `std.swap`; placement → `std.select`),
  the exported-op surface sketch (active structured-concurrency/supervision + the **⟂** reserved lexicon bindings),
  and — the load-bearing deliverable — the per-binding **guarantee matrix** (active `Exact`/`Empirical` rows vs
  `Declared (⟂ not-yet-active)` rows with activation conditions). §4.1 conformance (C1–C6) stated concretely;
  grounding traces to RFC-0008 (RT1–RT7, §4.5/§4.7), M-353/M-356/M-357, RFC-0016 §4.1/§4.2/§4.3/§4.5/§8-Q4,
  RFC-0001, the Glossary **⟂** lexicon, VR-5/G2/KC-3/LR-8/LR-9/ADR-003. Five questions FLAGGED — the
  std-vs-separate-`runtime`-phylum decision (§8-Q4), the Phase-7 construct-by-construct sequencing, the
  supervision clock boundary, the `graft`/`wild` FFI floor (§8-Q6), and ergonomics-vs-contract (§8-Q3) — **the
  phylum placement is presented, never silently decided**. No code; no kernel change (KC-3). Append-only.
