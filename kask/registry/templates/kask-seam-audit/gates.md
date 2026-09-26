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

## Converge — open count

Bind `findings` to all tracks' findings and `adjudicated_ids` to the `id` list of
adjudicate's `annotated_findings`. The result is the count of findings that are
uncited or not adjudicated; 0 means clean. Pass it to `final-report` as
`convergence_score`; the model never supplies it.

```
(begin (define n (lambda (fs adj) (if (= (length fs) 0) 0 (+ (if (or (and (= (length (assoc "file_line" (car fs))) 0) (not (assoc "deferred" (car fs)))) (not (member (assoc "id" (car fs)) adj))) 1 0) (n (cdr fs) adj))))) (n findings adjudicated_ids))
```
