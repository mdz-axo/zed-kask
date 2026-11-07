;; pre-emit.lisp — the DME-0-pattern decision-time gate over a dme-harness
;; `convert` manifest (kask/crates/dme-harness, the embedding-to-edit
;; converter). Run: lisp_eval with this form and env = the
;; convert-manifest.json object (its top-level keys arrive as Lisp
;; bindings). ACCEPT only a well-shaped emit; refuse naming the failed
;; check — the instrument measures, the gate decides.
;;
;; Checks (each names its anchor): dimensions (u.dim == dims.hidden,
;; k.dim == dims.trigger — the dense trigger must match the edit tensor's
;; input dim), budgets (anchors reconcile; vectors rows == anchors; at
;; least 2 anchors), the rank-one scale (s = alpha/r with r = 1 — the
;; edit spec's alpha equals the manifest's), hashes (every input and
;; output sha256 present), and the constant-steering discriminant
;; (u variance > 0 — `no_linear_edit_is_constant`).
(let ((get (lambda (m k) (assoc k m))))
  (let ((u_dim (get u "dim"))
        (h (get dims "hidden"))
        (k_dim (get (get trigger "k") "dim"))
        (t_dim (get dims "trigger"))
        (train (get anchors "train"))
        (held (get anchors "held_out"))
        (count (get anchors "count"))
        (rows (get (get inputs "vectors") "rows"))
        (uvar (get u "variance"))
        (sha_u (get u "sha256"))
        (sha_k (get (get trigger "k") "sha256"))
        (sha_cov (get (get trigger "covdiag") "sha256"))
        (sha_cfg (get (get inputs "config") "sha256"))
        (sha_vec (get (get inputs "vectors") "sha256"))
        (sha_man (get (get inputs "capture_manifest") "sha256"))
        (sha_gguf (get (get inputs "input_gguf") "sha256"))
        (spec_alpha (get edit_spec "alpha")))
    (cond
      ((not (= u_dim h)) "REFUSE: u.dim != dims.hidden")
      ((not (= k_dim t_dim)) "REFUSE: k.dim != dims.trigger (the dense trigger must match the edit tensor's input dim)")
      ((not (= (+ train held) count)) "REFUSE: anchors do not reconcile (train + held_out != count)")
      ((not (= rows count)) "REFUSE: vectors rows != anchors count")
      ((< count 2) "REFUSE: fewer than 2 anchors")
      ((not (= rank 1)) "REFUSE: rank != 1 (the pinned rank-one family)")
      ((not (= alpha spec_alpha)) "REFUSE: edit_spec.alpha != alpha (s = alpha/r, r = 1)")
      ((<= uvar 0) "REFUSE: constant direction (u variance <= 0) — constant steering is not a linear edit (no_linear_edit_is_constant)")
      ((or (string= sha_u "") (string= sha_k "") (string= sha_cov "")
           (string= sha_cfg "") (string= sha_vec "") (string= sha_man "")
           (string= sha_gguf ""))
       "REFUSE: a required sha256 is absent")
      (t "ACCEPT: dims, budgets, s=alpha/r, hashes, and the constant-steering discriminant all pass"))))
