# Analytical placement: model, PCB adaptations, and a path for Synth

Research note, 2026-10-29. Not a spec. Sources are listed at the end; every
formula below is traceable to one of them.

## Bottom line

Analytical placement is the strongest *global* placement model in VLSI, and two
PCB-specific papers (NS-Place, PA-ICM) show it transfers. But the published PCB
results also say the direct port fails: NS-Place reports that vanilla RePlAce
"fails to find good solutions for multi-layer designs with diversely sized
components", and a 2025 PCB paper notes analytical placers "do not easily
accommodate changes in component orientations".

So the useful question for Synth is not "adopt RePlAce/DREAMPlace". It is:

> Which part of the analytical model buys us quality without giving up the
> constraints that make a PCB placement human-quality (edge connectors,
> decoupling, functional clusters, keepouts, 90-degree rotations)?

The smallest defensible answer is a **wirelength-driven continuous seed**
(quadratic or weighted-average), density-spread on a board bin grid, then the
existing legalizer/compactor. Full Poisson + Nesterov is a later step, and only
if the seed measurably improves routed quality on our corpus.

## 1. The standard model

Global placement is written as an unconstrained relaxation:

```
min  W(x, y) + λ · D(x, y)
```

- `W` = wirelength, a smooth surrogate for HPWL.
- `D` = density / overlap penalty.
- `λ` = density weight, increased over the run.

Positions `x, y` are continuous. Overlaps are allowed during the solve and
removed by legalization afterwards.

### 1.1 Wirelength: HPWL to weighted average (WA)

HPWL for net `e` is the bounding-box half-perimeter:

```
HPWL(e) = (max_{i∈e} x_i − min_{i∈e} x_i)
        + (max_{i∈e} y_i − min_{i∈e} y_i)
```

It is piecewise linear, so its gradient is undefined where pins tie. The
weighted-average model replaces max/min with softmax-like averages over a
smoothing parameter `γ` (called `c` in the NS-Place paper):

```
Wa_x(e) = Σ_i x_i·exp(x_i/γ) / Σ_i exp(x_i/γ)
        − Σ_i x_i·exp(−x_i/γ) / Σ_i exp(−x_i/γ)

Wa(e)   = Wa_x(e) + Wa_y(e)
```

As `γ → 0`, `Wa_x → max x − min x`. The gradient for pin `j` is

```
p_j = exp(x_j/γ) / Σ_i exp(x_i/γ)
q_j = exp(−x_j/γ) / Σ_i exp(−x_i/γ)
P   = Σ_i x_i p_i
Q   = Σ_i x_i q_i

∂Wa_x/∂x_j = p_j·(1 + (x_j − P)/γ)
           − q_j·(1 − (x_j − Q)/γ)
```

which is `+1` at the max pin and `−1` at the min pin in the `γ → 0` limit.
Typical schedule: start with `γ` on the order of the board size (or half of
it), reduce by 2× each placement stage, stop at a few track pitches. Too small
too early creates a rough, local-minimum-prone objective; too large leaves the
placement spread out.

DREAMPlace implements the same idea as log-sum-exp (LSE):

```
LSE_x(e) = γ·ln Σ_i exp(x_i/γ) + γ·ln Σ_i exp(−x_i/γ)
```

`WA` and `LSE` differ by a constant offset for a fixed `γ`; both are used in
practice.

### 1.2 Quadratic placement (the cheap seed)

Before any nonlinear solve, classic placers solve a quadratic approximation.
Model each `k`-pin net as a clique with pair weight `1/(k−1)` (or as a star with
a virtual anchor). That gives a weighted graph Laplacian `Q` and a linear term
`c` from fixed pins:

```
min  ½ xᵀQx + cᵀx
```

With free/fixed partition:

```
Q_ff · x_f = −c_f − Q_fx · x_x
```

`Q_ff` is sparse, symmetric, positive definite once the graph is anchored to
fixed parts. Solve with conjugate gradient (Jacobi-preconditioned); tens of
iterations are enough for a few hundred components. RePlAce calls this the
"initial place" (BiCGSTAB) and reports ~5% HPWL improvement on large designs;
OpenROAD exposes it as `-skip_initial_place`.

This is the lowest-risk entry point: no density field, no FFT, no line search,
and it produces a seed rather than a final placement.

### 1.3 Density: two options

**Option A — bin overlap (simplest, and what NS-Place uses).** Divide the usable
board into `B` bins. For component `i` with width `w_i`, height `h_i`, bin `b`
with center `x_b`, width `w_b`:

```
d_x = |x_i − x_b|

Θ_x(b,i) = 1 − 2·d_x²/w_b²             if 0 ≤ d_x ≤ w_b/2
         = 2·(d_x − w_b)²/w_b²         if w_b/2 ≤ d_x ≤ w_b
         = 0                            if w_b ≤ d_x
```

`Θ_y` is the same in `y`. The bin density is

```
D_b = Σ_i C_i · Θ_x(b,i) · Θ_y(b,i)
```

with `C_i` normalized so a component's kernel sums to its area `w_i·h_i` over
the bins. The penalty is

```
D(x,y) = Σ_b (D_b − A_total/B)²
```

The gradient is local and cheap. The catch: bins assume a rectangular region;
outside-outline bins must be masked and the outline handled as a hard boundary
penalty (PA-ICM does this with a `B(x,y,r)` term).

**Option B — Poisson / electrostatic (ePlace, RePlAce, DREAMPlace).** Treat each
component as charge equal to its area. The potential `ψ` solves

```
∇²ψ = −ρ
∂ψ/∂n = 0            on the boundary   (Neumann: parts cannot leave)
∫ρ = ∫ψ = 0          (compatibility)
ρ(x,y) = Σ_i θ_i(x−x_i)·θ_i(y−y_i) − (1/WH)·Σ_i w_i·h_i
```

The last term makes `ρ` zero-mean. The system energy is

```
N = ½ · Σ_i q_i · ψ(x_i),      q_i = w_i·h_i
```

and the force on component `i` is `q_i·ξ(x_i)` with `ξ = −∇ψ`. Minimizing `N`
pushes parts out of dense regions.

On an `m × m` bin grid with `ω_u = 2πu/m`, the discrete cosine transform gives

```
ρ̂_{u,p} = (1/m²)·Σ_l Σ_j ρ(l,j)·cos(ω_u·l)·cos(ω_p·j)
ψ̂_{u,p} = ρ̂_{u,p} / (ω_u² + ω_p²)          (set ψ̂_{0,0} = 0)
ψ(l,j)  = Σ_u Σ_p ψ̂_{u,p}·cos(ω_u·l)·cos(ω_p·j)
```

`ξ` is the inverse DCT of `i·ω·ψ̂`. Cost is `O(m² log m)` with an FFT; Pplace
gives an `O(n log n)` analytic-series alternative that avoids the bin grid for
the potential at component centers.

**Which one for Synth:** bin overlap first. It is a few dozen lines, has no FFT
or boundary-condition subtleties, works with a masked irregular outline, and is
what the open PCB comparative study (NS-Place) actually uses. Poisson is a
second-stage upgrade if bin density shows grid artifacts.

### 1.4 Optimizer

RePlAce/ePlace use Nesterov's accelerated gradient:

```
t_0 = 1
y_k   = x_k + ((t_{k−1} − 1)/t_k)·(x_k − x_{k−1})
x_{k+1} = y_k − α_k·∇f(y_k)
t_{k+1} = (1 + √(1 + 4·t_k²))/2
```

- `α_k` from a line search, a dynamic step schedule (RePlAce), or
  Barzilai–Borwein (DREAMPlace 4.1).
- Precondition with a diagonal approximation of the Hessian (RePlAce uses
  vertex and net degrees).
- Raise `λ` when progress on density stalls.
- Stop when overflow (the fraction of area above target density) drops below a
  threshold, typically `0.1`.

L-BFGS is the alternative used by several academic placers. For ≤200
components, either is fine; Nesterov is simpler to make deterministic because
it has no history matrix.

## 2. What changes on a PCB

The VLSI formulation assumes movable, same-height, row-alignable cells and a
rectangular die. A PCB has none of those. The literature's adaptations:

### Fixed parts and edges
Connectors, mounting holes, switches, and displays are anchors. Keep them out
of the variables and put their pin coordinates into `c` (quadratic) or keep
their positions clamped (nonlinear). NS-Place's benchmark table explicitly
counts "locked" components (1–947 per board) and does not move them.

### Rotation
Only 0/90/180/270 degrees are legal. Three approaches appear:

1. Enumerate four orientations per component (4× the variables, or four solves).
2. Alternate: optimize `(x,y)` with orientations fixed, then re-choose
   orientation, repeat.
3. Treat orientation as a discrete variable. PA-ICM does this with a
   distribution-evolution algorithm (DEA-PPM) over orientations, interleaved
   with a conjugate-subgradient solve over coordinates.

For Synth, (2) is the cheapest: the existing placer already reasons about
`Rotation`, and a per-part orientation choice can be scored by the same
wirelength objective after the continuous solve.

### Board outline and keepouts
Replace the rectangular Neumann boundary with a mask on density bins plus a
boundary penalty `B` (PA-ICM eq. 10). Keepouts are zero-density, zero-placement
regions; connectors' courtyards and mounting holes must be excluded before the
solve.

### Two layers / layer assignment
NS-Place and PA-ICM split top and bottom into separate subproblems with
different objectives (top = centered module layout, bottom = pin-oriented).
Layer assignment is discrete and is not part of the continuous model; it has to
be fixed or enumerated outside it.

### Wirelength is not routability
NS-Place's main result is that adding a **net-separation** term beats pure
wirelength on routed metrics: up to 25% shorter routed wirelength, up to 50%
fewer vias, and 79% fewer DRVs versus manual and HPWL-minimal placements on 14
boards. The term is a max-margin / SVM-like penalty on the convex hulls of
neighboring nets, added to the WA + density objective. It is optional for a
first pass but is the strongest evidence that a pure-HPWL analytical placer is
not the end state for PCB.

### Legalization is the hard part
Abacus is the standard VLSI legalizer (row-based, dynamic programming, minimal
displacement), but it assumes rows of equal-height cells. PCB components are
arbitrary rectangles, so the published options are:

- **MILP** (NS-Place): binary variables for left/right and above/below for each
  pair, minimize HPWL + HPWL variance, subject to non-overlap and boundary
  constraints; relative-position constraints from global placement prune the
  variables. Exact, but only practical on tens of modules — the 1570-component
  board in their table produced no feasible MILP solution within 4 hours.
- **Tetris-like push + min-cost flow** (DREAMPlace mixed-size): legalize macros
  by pushing them out of overlap, then refine.
- **Heuristic four-direction push** (PA-ICM): sort by coordinate, shift one
  grid unit until clear, enforce spacing, move out-of-bounds parts into the
  largest corner void, then re-place close-to-pin parts.

For Synth, the MILP is out. A greedy displacement-minimizing legalizer over the
existing courtyard geometry is the realistic target; the current compaction and
overlap-separation code already contains most of the primitives.

## 3. What this means for Synth

### Where it plugs in

`synth_place::place()` already computes a continuous-ish seed: CEM region hints
(`cem.rs`) feed `target_point` and the greedy scanner finds the nearest legal
position (`lib.rs` around the `region_hints.hints.get(&id)` branch). So there
is a clean seam:

```
analytic seed (this work)  →  existing target-point search + legalization
                          →  existing compaction / refine / visual review
```

This is much smaller than replacing the placer, and it keeps the constraint
machinery that already passes the human-quality gate.

### Recommended first slice

1. Build `crates/synth-place/src/analytic.rs`:
   - Net list → clique-weighted sparse Laplacian (`f64`), fixed parts anchored.
   - Jacobi-preconditioned CG for `Q_ff x = b`.
   - Optional WA refinement: fixed `γ` schedule, Nesterov with a fixed step,
     50–200 iterations, and the bin-density term.
   - Return `RegionAssignment`-shaped hints (same type CEM returns) plus a
     score, so the rest of `place()` is untouched.
2. Replace only the CEM hint source behind a flag; run A/B on the reference
   fixtures and examples.
3. Gate on the metrics we already trust: `hpwl_total`, `routability()`,
   unresolved/unrouted nets from the router, DRC, and the visual-review
   `requires_revision` flag. A lower HPWL with more unrouted nets is a loss.
4. Only if step 3 holds, add the density term and legalize the continuous
   placement directly (instead of snapping to the greedy scanner).

### Determinism

Synth's contract is byte-identical output. Analytical placement is `f64` and
uses `exp`, `sin`, `cos`, whose last bits can differ across libm/CPU. RePlAce
explicitly claims deterministic results across compilers and OS, and DREAMPlace
added a deterministic mode, so it is achievable, but not automatic:

- fixed iteration counts, never "converged when the residual is < ε" at the
  last bit;
- deterministic reduction order (no `HashMap` iteration, no parallel
  reductions unless order-fixed);
- tie-break every choice on `ComponentId`;
- snap to integer nanometres once, at the boundary, with an explicit rounding
  rule;
- add a same-input, same-output test and a cross-platform check if CI allows.

### Cost

For our size class (≤200 components, ≤500 nets, bins ≤128×128), plain Rust
with no new dependency is enough: sparse CG is milliseconds, a 128×128 DCT is
sub-millisecond with a hand-rolled FFT or a small `rustfft` dependency. GPU and
PyTorch are not justified; DREAMPlace's speedup is for million-cell designs.

### What not to do

- Do not port DREAMPlace/RePlAce wholesale. Their machinery is built around
  rows, fillers, and cell libraries we do not have.
- Do not use MILP legalization. It does not scale to our boards and would add
  a solver dependency.
- Do not replace the functional/edge constraints with a wirelength objective.
  The existing placer's human-quality rules are the product; analytical
  placement should feed them, not override them.
- Do not assume a lower HPWL means a better board. Route it.

## 4. Open questions for the design review

1. **Seed or full placement?** Seed-only preserves the current gates and is
   reversible; full placement is where the literature's gains come from.
2. **Bin density or Poisson?** Start with bins; Poisson only if bin artifacts
   show up on the visual gate.
3. **Rotation policy.** Alternate orientation selection after each solve, or
   enumerate four orientations for the small set of parts where it matters?
4. **Which objective?** HPWL only, or WA + net separation (NS-Place's
   routability term) from the start? The evidence says net separation is what
   improves routed metrics.
5. **Benchmark.** Which fixtures/examples become the acceptance corpus, and
   what is the pass threshold (HPWL %, unrouted nets, vias, DRC)?

## Experiment 007 tuning (opt-in, local)

The RePlAce path behind `SYNTH_PLACE_REPLACE` was tuned on the 007 corpus. On
top of the prototype it adds:

- **Target-density bin force**: the target is `target_density × bin area`, and
  only bins above it push (`max(0, coverage − target)`), instead of the
  average-coverage target that never fires on a sparse PCB.
- **Courtyard overlap repulsion** and a density weight scaled once from the
  ratio of the wirelength and density gradient norms, so `lambda_start` is
  dimensionless.
- **Envelope compaction**: a squeeze plus band-slide pass after legalization,
  accepted only when the courtyard envelope shrinks without growing wire or
  entering a keepout.
- **Coverage pull and dead-pocket fill**: empty cells inside the envelope pull
  the nearest free parts, then small parts move one at a time into pockets that
  strictly gain covered cells.
- **Rail-net filter**: nets with more than `max_net_degree` pins stay out of the
  wirelength objective.

Defaults: `lambda_start 0.1`, `max_step_mm 1.5`, `target_density 0.8`,
`overlap_weight 0.06`, `compact_weight 0.12`, `coverage_weight 0.5`,
`max_net_degree 10`, `legalizer_gap_mm 0.25`.

The seam is not a default flip. On the export path the tuned placement is kept
only when its own routed board has 0 open nets and KiCad DRC 0 errors / 0
unconnected pads; otherwise the export re-runs with the default placement. The
Synth return-via rule is advisory for this gate because the default placement
carries the same finding on these boards.

Measured on the 007 corpus: tuning score 1517.22 → 996.75 (−34.3 %), held-out
454.88 → 441.06 (−3.0 %), tuned wire 1022.2 → 1147.0 mm. Against
`upstream/main`, the seven clean corpus boards score 4354 → 1438 with wire
1448.5 → 1297.0 mm; `irrigation_controller` is the board the route gate exists
for.

## References

1. C.-K. Cheng, A. B. Kahng, I. Kang, L. Wang. *RePlAce: Advancing Solution
   Quality and Routability Validation in Global Placement.* IEEE TCAD 38(9),
   2019. <https://vlsicad.ucsd.edu/Publications/Journals/j126.pdf>
2. J. Lu, P. Chen, C.-C. Chang, L. Sha, D. J.-H. Huang, C.-C. Teng, C.-K. Cheng.
   *ePlace: Electrostatics-Based Placement Using Fast Fourier Transform and
   Nesterov's Method.* ACM TODAES 20(2), 2015.
   <https://cseweb.ucsd.edu/~jlu/papers/eplace-todaes14/paper.pdf>
3. Y. Lin, S. Dhar, W. Li, H. Ren, B. Khailany, D. Z. Pan. *DREAMPlace: Deep
   Learning Toolkit-Enabled GPU Acceleration for Modern VLSI Placement.*
   DAC 2019 / IEEE TCAD 2020. <https://github.com/limbo018/DREAMPlace>
4. W. Zhu, Z. Huang, J. Chen, Y.-W. Chang. *Analytical Solution of Poisson's
   Equation with Application to VLSI Global Placement.* arXiv:2307.12041.
   <https://arxiv.org/html/2307.12041v1> (eqs. 2–5, 14–23 reproduce the ePlace
   Poisson/DCT model and the analytic series)
5. C.-K. Cheng, C.-T. Ho, C. Holtz. *Net Separation-Oriented Printed Circuit
   Board Placement via Margin Maximization (NS-Place).* ASP-DAC 2022 /
   arXiv:2210.14259. <https://arxiv.org/html/2210.14259v1> (WA formula,
   bin-density formula, MILP legalizer, PCB results)
6. H. Li, Z. Yang, H. Pang, N. Xu, Y. Chen. *A double-layer placement algorithm
   for integrated circuit-based modules on printed circuit board (PA-ICM).*
   arXiv:2502.14012. <https://arxiv.org/html/2502.14012v1> (mixed-variable
   x/y/r model, CSA + DEA-PPM, heuristic legalizer, spacing/outline terms)
7. P. Spindler, U. Schlichtmann, F. M. Johannes. *Abacus: Fast Legalization of
   Standard Cell Circuits with Minimal Movement.* ISPD 2008.
   <https://dl.acm.org/doi/10.1145/1353629.1353640>
8. OpenROAD. *Global Placement (gpl) documentation.*
   <https://openroad.readthedocs.io/en/latest/main/src/gpl/README.html>
   (RePlAce options, initial place, overflow/termination, density penalty)
9. C.-H. Tsou, S.-Y. Lin, W.-C. Hung, Y.-W. Chang. *Late Breaking Results:
   Modern Automatic PCB Placement with Complex Constraints.* DAC 2024.
   <https://dl.acm.org/doi/10.1145/3649329.3663495>
10. S. Li et al. *HiePlace: Efficient Hierarchical PCB Placement.* IEEE TCAD
    2026. <https://www.cse.cuhk.edu.hk/~byu/papers/J156-TCAD2026-HiePlace.pdf>
11. H. Zhao et al. *An analytical approach and fine-tuning strategy for PCB
    placement.* 2025. <https://www.sciencedirect.com/science/article/abs/pii/S016792602500224X>
12. Y. Lin et al. *DREAMPlace 4.1* release notes (BB step, two-stage macro
    flow, Tetris-like macro legalization, deterministic mode).
    <https://github.com/limbo018/DREAMPlace>
