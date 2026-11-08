;; pre-emit.lisp — the DME-0-pattern decision-time gate over a dme-harness
;; `convert` manifest (kask/crates/dme-harness, the embedding-to-edit
;; converter). Run: lisp_eval with this form and env = the
;; convert-manifest.json object (its top-level keys arrive as Lisp
;; bindings; the projection MUST also bind projected_weights_touched and
;; projected_edited_share — bind null for a manifest that predates the
;; DME-2 fields; the gate refuses, naming the absence, never errors).
;; ACCEPT only a well-shaped emit; refuse naming the failed check — the
;; instrument measures, the gate decides.
;;
;; Checks (each names its anchor): dimensions (u.dim == dims.hidden; the
;; trigger's declared shape must match its dim — the dense k or the
;; explicit column list, both == dims.trigger), budgets (anchors
;; reconcile; vectors rows == anchors; at least 2 anchors), the rank-one
;; scale (s = alpha/r with r = 1 — the edit spec's alpha equals the
;; manifest's), the constant-steering discriminant (u variance > 0 —
;; `no_linear_edit_is_constant`), DME-2 (projected_edited_share <=
;; 0.001 — the skill's 0.1% surface cap, SKILL.md:193, operationalizing
;; the anchor's 0.016% surface-minimization lesson; mechanically
;; enforced from the corrective phase; the edit leg carries the same
;; hard refusal against the parsed GGUF's real param_count — belt and
;; braces), the trigger-kind declaration (dense or columns — a manifest
;; declaring neither predates the amendment), and hashes (every required
;; sha256 present, per trigger shape).
;;
;; Shape-safety: the trigger-kind-specific lookups (trigger.k,
;; trigger.covdiag, trigger.columns) live INSIDE cond clause bodies
;; guarded by the kind checks, so a manifest of any generation renders a
;; clean verdict — a pre-amendment manifest (no kind) refuses at the
;; declaration clause before any per-kind key is read, and a columns
;; manifest never reads the dense keys.
(let ((get (lambda (m k) (assoc k m))))
  (let ((u_dim (get u "dim"))
        (h (get dims "hidden"))
        (t_dim (get dims "trigger"))
        (trig_kind (get trigger "kind"))
        (train (get anchors "train"))
        (held (get anchors "held_out"))
        (count (get anchors "count"))
        (rows (get (get inputs "vectors") "rows"))
        (uvar (get u "variance"))
        (sha_u (get u "sha256"))
        (sha_cfg (get (get inputs "config") "sha256"))
        (sha_vec (get (get inputs "vectors") "sha256"))
        (sha_man (get (get inputs "capture_manifest") "sha256"))
        (sha_gguf (get (get inputs "input_gguf") "sha256"))
        (spec_alpha (get edit_spec "alpha"))
        (touched projected_weights_touched)
        (share projected_edited_share))
    (cond
      ((not (= u_dim h)) "REFUSE: u.dim != dims.hidden")
      ((not (stringp trig_kind)) "REFUSE: trigger.kind is not declared — a pre-amendment manifest; re-run convert (dense and columns are the declared trigger shapes)")
      ((not (or (string= trig_kind "dense") (string= trig_kind "columns"))) "REFUSE: trigger.kind must be \"dense\" or \"columns\"")
      ((and (string= trig_kind "dense") (not (= (get (get trigger "k") "dim") t_dim))) "REFUSE: k.dim != dims.trigger (the dense trigger must match the edit tensor's input dim)")
      ((and (string= trig_kind "columns") (not (= (get (get trigger "columns") "dim") t_dim))) "REFUSE: columns.dim != dims.trigger (the explicit column list must live on the edit tensor's input dim)")
      ((not (= (+ train held) count)) "REFUSE: anchors do not reconcile (train + held_out != count)")
      ((not (= rows count)) "REFUSE: vectors rows != anchors count")
      ((< count 2) "REFUSE: fewer than 2 anchors")
      ((not (= rank 1)) "REFUSE: rank != 1 (the pinned rank-one family)")
      ((not (= alpha spec_alpha)) "REFUSE: edit_spec.alpha != alpha (s = alpha/r, r = 1)")
      ((<= uvar 0) "REFUSE: constant direction (u variance <= 0) — constant steering is not a linear edit (no_linear_edit_is_constant)")
      ((or (not (numberp touched)) (not (numberp share))) "REFUSE: the projected edit surface is not stated (projected_weights_touched / projected_edited_share absent) — a pre-DME-2 manifest; re-run convert with total_weights declared")
      ((> share 0.001) "REFUSE: DME-2 — projected_edited_share > 0.001 (the 0.1% cap; the anchor's 131,072-weight 0.016% surface is the reference point)")
      ((or (string= sha_u "") (string= sha_cfg "") (string= sha_vec "") (string= sha_man "") (string= sha_gguf "")) "REFUSE: a required sha256 is absent")
      ((and (string= trig_kind "dense") (or (string= (get (get trigger "k") "sha256") "") (string= (get (get trigger "covdiag") "sha256") ""))) "REFUSE: the dense trigger's k or covdiag sha256 is absent")
      ((and (string= trig_kind "columns") (string= (get (get trigger "columns") "sha256") "")) "REFUSE: the columns file sha256 is absent")
      (t "ACCEPT: dims, budgets, s=alpha/r, DME-2 surface, hashes, and the constant-steering discriminant all pass"))))
