# kask-seam-audit gates (pinned lisp_eval forms)

Each form was tested on passing and failing cases on 2026-09-26. Pass the named
template outputs as env bindings; each gate returns `true` to pass.

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

## KSA-1 — measured-layout 5-gate check

Evaluate the 5 Fagan hard gates deterministically over `layout-sense`'s outputs (`overflow_delta`, `fitts_violations`, `primary_action_visible`, `text_column_residual`, `has_text_column`, `on_grid_spacing`, `action_count`, `action_budget`). Returns `(list all_gates_passed failing_gates)`.

```
(let ((g1 (and (<= overflow_delta 0) (= (length fitts_violations) 0))) (g2 primary_action_visible) (g3 (or (not has_text_column) (>= text_column_residual 160))) (g4 on_grid_spacing) (g5 (<= action_count action_budget))) (let ((failed (append (if g1 (list) (list "no_overflow")) (append (if g2 (list) (list "primary_action_visible")) (append (if g3 (list) (list "text_column_min_width")) (append (if g4 (list) (list "on_grid_spacing")) (if g5 (list) (list "action_count_budget")))))))) (list (= (length failed) 0) failed)))
```

## KSA-2 — MCDA weighted score verification

Compute each candidate's weighted score `Σ(score × weight)` deterministically from `scores` (`[s_sec, s_code, s_ui, s_cost]`) and `weights` (`[w_sec, w_code, w_ui, w_cost]`).

```
(+ (* (nth 0 scores) (nth 0 weights)) (* (nth 1 scores) (nth 1 weights)) (* (nth 2 scores) (nth 2 weights)) (* (nth 3 scores) (nth 3 weights)))
```

## KSA-3 — final-report track, severity, and defense-layer counts

Compute `findings_per_track`, `findings_per_severity`, and `defense_layer_coverage` deterministically from the three tracks' finding lists (`sec_findings`, `arch_findings`, `ui_findings`) and `missing_layers` instead of asking `final-report.j2` to count them.

```
(begin (define count-sev (lambda (fs s) (if (is_null fs) 0 (+ (if (string= (assoc "severity" (car fs)) s) 1 0) (count-sev (cdr fs) s))))) (let ((all (append sec_findings (append arch_findings ui_findings)))) (list (list (length sec_findings) (length arch_findings) (length ui_findings)) (list (count-sev all "critical") (count-sev all "high") (count-sev all "medium") (count-sev all "low") (count-sev all "info")) (list (- total_layers (length missing_layers)) (length missing_layers)))))
```
