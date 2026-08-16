#[test]
fn test_structural_check_form_parses() {
    let form = r#"
(let ((manifest target_manifest))
  (let ((steps (assoc "steps" manifest)))
    (let ((n (length steps)))
      (if (= n 0)
          (list "empty_manifest")
          (begin
            (define count-action
              (lambda (ss action-name acc)
                (if (is_null ss)
                    acc
                    (let ((s (car ss)))
                      (let ((a (assoc "action" s)))
                        (if (string= a action-name)
                            (count-action (cdr ss) action-name (+ acc 1))
                            (count-action (cdr ss) action-name acc)))))))
            (define n-execute (count-action steps "execute" 0))
            (define n-select (count-action steps "select" 0))
            (define n-compute (count-action steps "compute" 0))
            (define structural-defects
              (append
                (if (and (= n-select 0) (> n 0))
                    (list "zero_select_steps_ceiling_violation")
                    (list))
                (if (and (= n-execute 0) (= n-compute 0))
                    (list "no_deterministic_steps_below_floor")
                    (list))))
            structural-defects)))))
"#;
    match hkask_lisp::parse(form) {
        Ok(parsed) => assert!(!parsed.is_empty(), "should parse at least one form"),
        Err(e) => panic!("Parse error: {e}"),
    }
}
