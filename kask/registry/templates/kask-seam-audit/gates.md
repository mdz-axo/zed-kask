# kask-seam-audit gates (pinned lisp_eval forms)

Each form was tested on passing and failing cases on 2026-09-26; `KSA-1` and
`KSA-2` were retested on passing and failing cases on 2026-10-16. Pass the named
template outputs as env bindings; each section names its own pass condition (a
returned `true`, a count of 0, or a `true` field in the returned object).

## Gate A — seam-map `prior_verdicts`

Every verdict is exactly `live` or `phantom`.

```
(begin (define g (lambda (vs) (if (= (length vs) 0) t (and (or (string= (assoc "verdict" (car vs)) "live") (string= (assoc "verdict" (car vs)) "phantom")) (g (cdr vs)))))) (g prior_verdicts))
```

## Gate B — each audit's `findings`

Every finding has a non-empty `file_line` or `deferred: true`, and a severity in
`critical|high|medium|low|info`.

```
(begin (define sev (lambda (s) (or (string= s "critical") (string= s "high") (string= s "medium") (string= s "low") (string= s "info")))) (define g (lambda (fs) (if (= (length fs) 0) t (and (or (not (= (length (assoc "file_line" (car fs))) 0)) (assoc "deferred" (car fs))) (sev (assoc "severity" (car fs))) (g (cdr fs)))))) (g findings))
```

## Gate C — remediate

Every applied remediation has `test_added` equal to the string `"true"`, every
`touched_files` entry is `within_kask`, and `hard_stop.triggered` (passed as
`triggered`) is false.

```
(begin (define ok (lambda (fs) (if (= (length fs) 0) t (and (assoc "within_kask" (car fs)) (ok (cdr fs)))))) (define g (lambda (rs) (if (= (length rs) 0) t (and (string= (assoc "test_added" (car rs)) "true") (ok (assoc "touched_files" (car rs))) (g (cdr rs)))))) (and (g applied_remediations) (not triggered)))
```

## Gate FA — adjudicate's `annotated_findings` (functional authority)

Every annotated finding has a non-empty `current_function` and an
`authority_status` in `supported|revoked|deprecated|not_adjudicated`. The
authority verdict itself is judgment (P, the operator's); this gate only
enforces that every finding carries one.

```
(begin (define st (lambda (s) (or (string= s "supported") (string= s "revoked") (string= s "deprecated") (string= s "not_adjudicated")))) (define g (lambda (fs) (if (= (length fs) 0) t (and (not (= (length (assoc "current_function" (car fs))) 0)) (st (assoc "authority_status" (car fs))) (g (cdr fs)))))) (g annotated_findings))
```

## Converge — open count

Bind `findings` to all tracks' findings and `adjudicated_ids` to the `id` list of
adjudicate's `annotated_findings`. The result is the count of findings that are
uncited or not adjudicated; 0 means clean. Pass it to `final-report` as
`convergence_score`; the model never supplies it.

```
(begin (define n (lambda (fs adj) (if (= (length fs) 0) 0 (+ (if (or (and (= (length (assoc "file_line" (car fs))) 0) (not (assoc "deferred" (car fs)))) (not (member (assoc "id" (car fs)) adj))) 1 0) (n (cdr fs) adj))))) (n findings adjudicated_ids))
```

## KSA-1 — measured-layout derived quantities + 5-gate check

Derive the layout quantities deterministically from `layout-sense`'s primitive
measurements and evaluate the 5 Fagan hard gates (this replaced `layout-decide.j2`'s
LLM evaluation). Bind every value from `layout-sense`'s output:
`container_available_width`, `child_min_widths` (each `{element, min_width_px, kind}`
with kind `fixed|text_column|flex`), `total_gap_width`, `fitts_min_target`,
`action_count`, `action_budget` (5 per Hick's Law), `primary_action_visible`,
`on_grid_spacing`. The form derives `sum_min_widths` (Σ child min-widths + gaps),
`overflow_delta` (sum − container), `text_column_residual` (container − fixed
children − gaps), and `fitts_violations` (fixed children below the Fitts floor —
flex children shrink, so the floor does not apply to their minimum), then evaluates
the 5 gates (`overflow_delta <= 0`, `primary_action_visible`,
`text_column_residual >= 160`, `on_grid_spacing`, `action_count <= action_budget`)
and returns the gate result object `{all_gates_passed, failing_gates, sum_min_widths,
overflow_delta, text_column_residual, fitts_violations}` — bind it as `decide_result`
for `layout-act` and `layout-review`.

```
(begin (define sum-all (lambda (cs) (if (is_null cs) 0 (+ (assoc "min_width_px" (car cs)) (sum-all (cdr cs)))))) (define sum-fixed (lambda (cs) (if (is_null cs) 0 (+ (if (string= (assoc "kind" (car cs)) "fixed") (assoc "min_width_px" (car cs)) 0) (sum-fixed (cdr cs)))))) (define fitts-bad (lambda (cs) (if (is_null cs) (list) (append (if (and (string= (assoc "kind" (car cs)) "fixed") (< (assoc "min_width_px" (car cs)) fitts_min_target)) (list (assoc "element" (car cs))) (list)) (fitts-bad (cdr cs)))))) (let ((sum_min_widths (+ (sum-all child_min_widths) total_gap_width))) (let ((overflow_delta (- sum_min_widths container_available_width))) (let ((text_column_residual (- (- container_available_width (sum-fixed child_min_widths)) total_gap_width))) (let ((fitts_violations (fitts-bad child_min_widths))) (let ((failed (append (if (<= overflow_delta 0) (list) (list "no_overflow")) (append (if primary_action_visible (list) (list "primary_action_visible")) (append (if (>= text_column_residual 160) (list) (list "text_column_min_width")) (append (if on_grid_spacing (list) (list "on_grid_spacing")) (if (<= action_count action_budget) (list) (list "action_count_budget")))))))) (list (list "all_gates_passed" (= (length failed) 0)) (list "failing_gates" failed) (list "sum_min_widths" sum_min_widths) (list "overflow_delta" overflow_delta) (list "text_column_residual" text_column_residual) (list "fitts_violations" fitts_violations))))))))
```

## KSA-2 — MCDA weighted totals, top-3, and ±20% perturbation stability

Compute every candidate's weighted total `Σ(score × weight)` deterministically,
derive the baseline top-3 by weighted total, and recompute the top-3 under each
single-axis +20% weight emphasis (`security`, `codebase`, `ui`, `cost`) — four
perturbation sets (this replaced the LLM-computed `weighted_total`, `top3`, and
`sensitivity` outputs in `mcda.j2`'s contract). Bind `candidates` (each `{id, scores}`
with `scores` = `[security, codebase, ui, cost_inverted]`, from `mcda`'s
`ranked_remediations`) and `weights` (`[w_security, w_codebase, w_ui, w_cost]`, from
`mcda`'s `weight_vector` in the same order). Returns `{weighted_totals, baseline_top3,
perturbations: [{axis, top3, flipped}...], stable_under_perturbation}` — stable iff
the baseline top-3 set is unchanged under ALL four perturbations.

```
(begin (define wt (lambda (s w) (+ (* (nth 0 s) (nth 0 w)) (* (nth 1 s) (nth 1 w)) (* (nth 2 s) (nth 2 w)) (* (nth 3 s) (nth 3 w))))) (define totals (lambda (cs w) (if (is_null cs) (list) (cons (wt (assoc "scores" (car cs)) w) (totals (cdr cs) w))))) (define find-max (lambda (cs w) (if (= (length cs) 1) (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w)) (let ((hd (list (assoc "id" (car cs)) (wt (assoc "scores" (car cs)) w))) (tl (find-max (cdr cs) w))) (if (>= (nth 1 tl) (nth 1 hd)) tl hd))))) (define remove-id (lambda (cs id) (if (is_null cs) (list) (if (string= (assoc "id" (car cs)) id) (remove-id (cdr cs) id) (cons (car cs) (remove-id (cdr cs) id)))))) (define top3 (lambda (cs w n) (if (or (= n 0) (is_null cs)) (list) (let ((m (find-max cs w))) (cons (nth 0 m) (top3 (remove-id cs (nth 0 m)) w (- n 1))))))) (define pert-w (lambda (w a i) (if (= i 4) (list) (cons (if (= i a) (* (nth i w) 1.2) (nth i w)) (pert-w w a (+ i 1)))))) (define pert-top3 (lambda (a) (top3 candidates (pert-w weights a 0) 3))) (define subset (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (subset (cdr xs) ys))))) (define same-set (lambda (a b) (and (= (length a) (length b)) (subset a b)))) (let ((base (top3 candidates weights 3))) (let ((p0 (pert-top3 0)) (p1 (pert-top3 1)) (p2 (pert-top3 2)) (p3 (pert-top3 3))) (list (list "weighted_totals" (totals candidates weights)) (list "baseline_top3" base) (list "perturbations" (list (list "security" p0 (not (same-set base p0))) (list "codebase" p1 (not (same-set base p1))) (list "ui" p2 (not (same-set base p2))) (list "cost" p3 (not (same-set base p3))))) (list "stable_under_perturbation" (and (same-set base p0) (same-set base p1) (same-set base p2) (same-set base p3)))))))
```

## KSA-3 — final-report track, severity, and defense-layer counts

Compute `findings_per_track`, `findings_per_severity`, and `defense_layer_coverage` deterministically from the three tracks' finding lists (`sec_findings`, `arch_findings`, `ui_findings`) and `missing_layers` instead of asking `final-report.j2` to count them.

```
(begin (define count-sev (lambda (fs s) (if (is_null fs) 0 (+ (if (string= (assoc "severity" (car fs)) s) 1 0) (count-sev (cdr fs) s))))) (let ((all (append sec_findings (append arch_findings ui_findings)))) (list (list (length sec_findings) (length arch_findings) (length ui_findings)) (list (count-sev all "critical") (count-sev all "high") (count-sev all "medium") (count-sev all "low") (count-sev all "info")) (list (- total_layers (length missing_layers)) (length missing_layers)))))
```
