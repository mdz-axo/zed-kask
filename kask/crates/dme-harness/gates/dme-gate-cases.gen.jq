include "dme-measure";
# dme-gate-cases.gen.jq: writes the declared cases of the direct-model-edit gates.
# Every case is built at test time by the measuring module itself:
#   jq -c -n -L . --slurpfile fx dme-gate-fixture.json -f dme-gate-cases.gen.jq
# dme-gate-fixture.json holds verbatim excerpts of archived harness output (probe rows, edit receipts, the saved card).
# Synthetic rows carry the harness RunRow shape (receipt.rs:17-67); every env below is built by the
# same dme-measure.jq builders the skill names, then tampered with only where a case says so.
def CFG: {name:"B",label:"card non-thinking VL",enable_thinking:false,temperature:0.7,top_p:0.8,top_k:20,min_p:0.0,presence_penalty:1.5,repetition_penalty:1.0};
def CARDQ(n; t; p; pp): "- \(n): temperature=\(t), top_p=\(p), top_k=20, min_p=0.0, presence_penalty=\(pp), repetition_penalty=1.0";
def FOOT: "> Under the non-thinking mode, the sampling parameters were set to top_p=0.8, top_k=20, presence_penalty=1.5, and temperature=0.7.";
def TWO: "> temperature=1.0 were used. All other benchmarks adopted temperature=0.6 under the thinking mode.";
def TRAP: "config-A.json (anchor-style trap): thinking, temperature 0.6, top_p 0.95";
def ONE: "> All other benchmarks adopted the same sampling configuration but with temperature=0.6 under the thinking mode.";
def NOTES: [CARDQ("Non-thinking, text tasks";1.0;1.0;2.0), CARDQ("Non-thinking, VL tasks";0.7;0.8;1.5), CARDQ("Thinking, text tasks";1.0;0.95;1.5), CARDQ("Thinking, VL/precise coding";0.6;0.95;0.0), FOOT, TWO, TRAP, ONE] | join("\n");
def CARD(n; et; t; p; pp; cap): {row:n,source_quote:CARDQ(n;t;p;pp),default_enable_thinking:false,enable_thinking:et,cap:cap};
def VLN: CARD("Non-thinking, VL tasks"; false; 0.7; 0.8; 1.5; 32768);
def ECHO: {temperature:0.699999988079071,top_p:0.800000011920929,top_k:20,min_p:0.0,presence_penalty:1.5,repeat_penalty:1.0,repeat_last_n:36864,n_predict:32768};
def FX: {config:"B",source:"gsm8k-train",kind:"numeric",cap:32768,enable_thinking_applied:true,presence_penalty_applied:1.5,dtype:"BF16",device:"server",threads:0,kernelize:false,param_count:752393024,dropped_gen_kwargs:[],config_file:(CFG + {card_row:VLN}),harness:"dme-harness-rust/0.1.0",backend:"llama.cpp (b1-9e2a04a)",penalty_last_n:36864,n_ctx:36864,engine_version:"b1-9e2a04a"};
def HC: {"H":[true,false,"hit_limit"],"W":[false,false,"wrong_complete"],"C":[false,true,"correct_complete"]};
def RROW: FX + {ts:1.5,model:"m.gguf",gold:"1",prompt_tokens:9,new_tokens:9,wall_secs:1.0,tok_per_sec:9.0,extracted:null,extraction_via:"none",tail_distinct_ratio:1.0};
def R(segs; start; sha): (RROW + {gguf_sha256:sha}) as $base | [segs[] | . as $s | HC[$s[1]] as $v | range(0; $s[0]) | $v] | to_entries | map((.key + start) as $i | $base + {problem_id:"p\($i)",seed:(1000 + $i),hit_limit:.value[0],correct:.value[1],label:.value[2],server_generation_settings:(ECHO + {seed:(1000 + $i)})});
def R(segs; sha): R(segs; 1; sha);
# what `dme-harness summary` prints for a rows file: the fields the measuring module reads
def SM(r): if (r | type) != "array" then null else {n:(r|length),gguf_sha256:(r[0].gguf_sha256? // ""),counts:{correct_complete:([r[]|select(type == "object" and .label=="correct_complete")]|length),wrong_complete:([r[]|select(type == "object" and .label=="wrong_complete")]|length),hit_limit:([r[]|select(type == "object" and .label=="hit_limit")]|length)}} end;
def TRIG: {kind:"columns",dim:3584,columns:64,weight_min:1.0,weight_max:1.0,k_sha256:"cols",covdiag_sha256:null};
def DT: {source:"file",dim:1024,sha256:"u-sha",mean:0.0121};
def DC: {source:"seeded",dim:1024,sha256:"r-sha",mean:0.0006};
def rc(i; o; a; d; l2): {input_sha256:i,output_sha256:o,alpha:a,layers:[15,19],tensor:"blk.{L}.ffn_down.weight",trigger:TRIG,direction:d,per_layer:[{layer:15,edit_delta_l2:l2,unedited_elements_changed:0},{layer:19,edit_delta_l2:l2,unedited_elements_changed:0}]};
def RC1: [rc("base";"ed";6;DT;4.0)];
def S(n; nt; nr; d; tt; tc): {answers:n,targeted_net:nt,control_net:nr,targeted_over_control_net:d,targeted_truncation_drop:tt,control_truncation_drop:tc};
def T(n; bh; bw; ch; cw; hw; h0; h1): {answers:n,truncated_to_correct:bh,finished_wrong_to_correct:bw,correct_to_truncated:ch,correct_to_finished_wrong:cw,truncated_to_finished_wrong:hw,truncated_before:h0,truncated_after:h1};
def c(id; gate; env; expect): {id:id,gate:gate,env:env,expect:expect};
def ECHOKEY: {temperature:"temperature",top_p:"top_p",top_k:"top_k",min_p:"min_p",presence_penalty:"presence_penalty",repetition_penalty:"repeat_penalty"};
def setrun(cfgmod; card; cap): .cap = cap | .config_file = ((CFG + cfgmod) + (if card == null then {} else {card_row:card} end))
  | .server_generation_settings = (reduce (cfgmod | to_entries[] | select(ECHOKEY[.key] != null)) as $kv (.server_generation_settings; .[ECHOKEY[$kv.key]] = $kv.value));
def d0x(f): (R([[14,"C"],[8,"W"],[2,"H"]]; "base") | map(f)) as $r | gate_dme0($r; SM($r); NOTES);
def d0(cfgmod; card; cap): d0x(setrun(cfgmod; card; cap));
def d0(cfgmod; card): d0(cfgmod; card; 32768);
def RUNA: {enable_thinking:true,temperature:0.6,top_p:0.95};
def e3(b; e; rcs): gate_dme3(b; SM(b); e; SM(e); rcs);
def a3(bs; es): e3(R(bs; "base"); R(es; "ed"); RC1);
# explicit transitions: [[count, before, after], ...]
def tr(xs): e3(R([xs[] | [.[0], .[1]]]; "base"); R([xs[] | [.[0], .[2]]]; "ed"); RC1);
def c3(bs; ts; cs; crc): R(bs;"base") as $b | R(ts;"tg") as $t | R(cs;"ct") as $c | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;DT;4.0)]; $c; SM($c); crc);
def c3(bs; ts; cs): c3(bs; ts; cs; [rc("base";"ct";6;DC;4.0)]);
def sa($a; segs): R(segs; "s\($a)") as $r | {rows:$r,summary:SM($r),receipts:[rc("base";"s\($a)";$a;DT;($a * 1.0))]};
def ser(bs; arms): R(bs; "base") as $b | gate_dme2d($b; SM($b); arms);
def P96: R([[60,"C"],[30,"W"],[6,"H"]]; 100; "base");
def T2: R([[2,"H"]]; 5000; "base");
def pres(p; t): gate_dme2b(p; SM(p); t; SM(t));
def D4B: [[235,"H"],[21,"C"]]; def D4T: [[102,"C"],[133,"H"],[21,"C"]]; def D4C: [[4,"C"],[231,"H"],[21,"C"]];
def OKB: [[85,"H"],[11,"C"]]; def OKE: [[30,"C"],[55,"H"],[11,"C"]];
def OK3: a3(OKB; OKE);
def MINUS: "uniform-minus-direction";
$fx[0].old_rows as $old | $fx[0].new_rows as $new | $fx[0].receipts as $rcp | $fx[0].card as $card
| $fx[0].pairs_old as $ro | $fx[0].pairs_new as $rn |
[
 c("A1";"dme-0";d0({}; VLN);{equals:["settings-match-card-row","Non-thinking, VL tasks",[],["penalty_last_n"],[]]}),
 c("A2";"dme-0";d0(RUNA; VLN);{at:[[0,"settings-differ-from-card-row"],[2,["enable_thinking","temperature","top_p"]],[4,["mode-is-not-the-card-default"]]]}),
 c("A3";"dme-0";d0(RUNA; CARD("Thinking, text tasks";true;1.0;0.95;1.5;81920));{at:[[0,"settings-differ-from-card-row"],[2,["temperature","cap"]]]}),
 c("A4";"dme-0";d0(RUNA; CARD("Non-thinking, text tasks";false;1.0;1.0;2.0;32768));{at:[[0,"settings-differ-from-card-row"],[2,["enable_thinking","temperature","top_p","presence_penalty"]]]}),
 c("A5";"dme-0";d0({repetition_penalty:1.1}; VLN);{at:[[0,"settings-differ-from-card-row"],[2,["repetition_penalty"]]]}),
 c("A6";"dme-0";d0({}; VLN + {cap:null});{at:[[0,"settings-match-card-row"],[2,[]],[3,["cap","penalty_last_n"]]]}),
 c("A7";"dme-0";d0x(del(.server_generation_settings.top_k));{equals:["run-setting-missing-or-mistyped",["top_k"]]}),
 c("A8";"dme-0";d0({enable_thinking:"false"}; VLN);{equals:["run-setting-missing-or-mistyped",["enable_thinking"]]}),
 c("A9";"dme-0";d0({temperature:"0.7"}; VLN);{equals:["run-setting-missing-or-mistyped",["temperature"]]}),
 c("A10";"dme-0";d0({}; VLN | del(.row));{equals:"card-row-not-named-or-unquoted"}),
 c("A10b";"dme-0";d0({}; null);{equals:"card-row-missing"}),
 c("A11";"dme-0";d0({enable_thinking:true,temperature:1.0,top_p:0.95}; CARD("Thinking, text tasks";true;1.0;0.95;1.5;81920); 81920);{at:[[0,"settings-match-card-row"],[4,["mode-is-not-the-card-default"]]]}),
 c("A12";"dme-0";d0x(.server_generation_settings.temperature = 0.7 | .server_generation_settings.top_p = 0.8);{equals:["settings-match-card-row","Non-thinking, VL tasks",[],["penalty_last_n"],[]]}),
 c("A13";"dme-0";((R([[14,"C"],[8,"W"],[2,"H"]]; "base") | .[3].config_file.temperature = 0.6) as $r | gate_dme0($r; SM($r); NOTES));{equals:["run-facts-mixed-within-a-run",["config_file.temperature"]]}),
 c("A14";"dme-0";d0({}; VLN + {source_quote:TWO});{equals:["card-row-value-ambiguous-or-mistyped",["temperature"]]}),
 c("A14b";"dme-0";d0({}; VLN + {cap:"32768"});{equals:["card-row-value-ambiguous-or-mistyped",["cap"]]}),
 c("A15";"dme-0";d0({}; VLN + {source_quote:""});{equals:"card-row-not-named-or-unquoted"}),
 c("A16";"dme-0";d0({}; VLN | del(.default_enable_thinking));{at:[[0,"settings-match-card-row"],[4,["card-default-mode-not-stated"]]]}),
 c("A17";"dme-0";d0({}; VLN + {penalty_last_n:64});{at:[[0,"settings-differ-from-card-row"],[2,["penalty_last_n"]]]}),
 c("A20";"dme-0";(d0({}; VLN) | .in.run.counts.hit_limit = -2 | .in.run.counts.correct_complete = 18);{equals:"measurement-inconsistent"}),
 c("A21";"dme-0";(d0({}; VLN) | .in.run.summary_agrees = "true");{equals:"rows-disagree-with-harness-summary"}),
 c("A22";"dme-0";d0(RUNA; {row:"anchor-style trap",source_quote:TRAP,default_enable_thinking:false,enable_thinking:true,cap:32768});{equals:"card-quote-states-no-sampling-setting"}),
 c("A23";"dme-0";d0({}; VLN + {source_quote:"a sentence the card does not contain"});{equals:"card-row-quote-not-found-in-card"}),
c("A33";"dme-0";(d0({}; VLN) | .in.run.counts.hit_limit=2.5 | .in.run.counts.wrong_complete=7.5);{equals:"measurement-inconsistent"}),
 c("A24";"dme-0";d0({}; VLN + {source_quote:FOOT});{equals:["settings-match-on-stated-settings-only","Non-thinking, VL tasks",[],["min_p","repetition_penalty","penalty_last_n"],[]]}),
 c("A25";"dme-0";d0(RUNA; VLN + {enable_thinking:[]});{equals:["card-row-value-ambiguous-or-mistyped",["enable_thinking"]]}),
 c("A26";"dme-0";d0({}; VLN + {cap:-2}; 9223372036854775807);{equals:["run-setting-missing-or-mistyped",["cap"]]}),
 c("A27";"dme-0";d0(RUNA; VLN + {source_quote:"e"});{equals:"card-quote-states-no-sampling-setting"}),
 c("A28";"dme-0";d0({temperature:0.7007}; VLN);{at:[[0,"settings-differ-from-card-row"],[2,["temperature"]]]}),
 c("A32";"dme-0";d0(RUNA; {row:"thinking-mode benchmarks",source_quote:ONE,default_enable_thinking:false,enable_thinking:true});{equals:["settings-match-on-stated-settings-only","thinking-mode benchmarks",[],["top_p","top_k","min_p","presence_penalty","repetition_penalty","cap","penalty_last_n"],["mode-is-not-the-card-default"]]}),
 c("X13";"dme-3";(OK3 | .in.edited.edits = "x");{equals:"edit-trigger-weights-not-recorded"}),
 c("X14";"dme-3";(OK3 | .in.edited.edits = 5);{equals:"edit-trigger-weights-not-recorded"}),
 c("A29";"dme-0";(d0({}; VLN) | del(.in.quote_in_card));{equals:"card-row-quote-not-found-in-card"}),
 c("A30";"dme-0";((R([[14,"C"],[8,"W"],[2,"H"]]; "base")) as $r | gate_dme0($r; (SM($r) | .n = 500); NOTES));{equals:"rows-disagree-with-harness-summary"}),
 c("A31";"dme-0";((R([[14,"C"],[8,"W"],[2,"H"]]; "base")) as $r | gate_dme0($r; (SM($r) | .gguf_sha256 = "other"); NOTES));{equals:"rows-disagree-with-harness-summary"}),
 c("A18";"dme-0";gate_dme0($new; SM($new); $card);{equals:"card-row-missing"}),
 c("A19";"dme-0";(($new | map(.config_file.card_row = {row:"Non-thinking, VL tasks",source_quote:"- Non-thinking, VL tasks: temperature=0.7, top_p=0.80, top_k=20, min_p=0.0, presence_penalty=1.5, repetition_penalty=1.0",default_enable_thinking:false,enable_thinking:false,cap:32768})) as $r | gate_dme0($r; SM($r); $card));{equals:["settings-match-card-row","Non-thinking, VL tasks",[],["penalty_last_n"],[]]}),

 c("B1";"dme-3";gate_dme3($old; SM($old); $new; SM($new); []);{head:"run-facts-mixed-within-a-run",contains:["engine_version","server_generation_settings.repeat_last_n","penalty_last_n","harness","dropped_gen_kwargs"]}),
c("B1b";"dme-3";gate_dme3($ro; SM($ro); $rn; SM($rn); []);{equals:["base-run","rows-disagree-with-harness-summary"]}),
 c("B2";"dme-3";OK3;{at:[[0,"mostly-unblocking"],[3,MINUS]]}),
 c("B3";"dme-3";e3(R(OKB;"base"); R(OKE;"base"); RC1);{equals:"edited-rows-carry-the-base-model-file"}),
 c("B4";"dme-3";e3(R(OKB;"base") | map(.cap = 4096); R(OKE;"ed"); RC1);{equals:["runs-differ-beyond-the-model-file",["cap"]]}),
 c("B5";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | .[2].engine_version = "other"); RC1);{equals:["run-facts-mixed-within-a-run",[],[],["engine_version"]]}),
 c("B6";"dme-3";e3([]; []; []);{equals:["base-run","rows-missing"]}),
 c("B7";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("other";"ed";6;DT;4.0)]);{equals:"receipts-do-not-link-base-to-edited"}),
 c("B7b";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"elsewhere";6;DT;4.0)]);{equals:"receipts-do-not-link-base-to-edited"}),
 c("B8";"dme-3";e3(R(OKB;"base") | map(del(.penalty_last_n)); R(OKE;"ed") | map(del(.penalty_last_n)); RC1);{equals:["base-run",["run-facts-incomplete",["penalty_last_n"]]]}),
 c("B9";"dme-3";e3(R(OKB;"base"); R(OKE;"ed") | map(.config_file.repetition_penalty = 1.1); RC1);{equals:["runs-differ-beyond-the-model-file",["config_file.repetition_penalty"]]}),
 c("B10";"dme-3";e3(R(OKB;"base"); R(OKE;"ed") | map(.server_generation_settings.repeat_last_n = 64); RC1);{equals:["runs-differ-beyond-the-model-file",["server_generation_settings.repeat_last_n"]]}),
 c("B11";"dme-3";e3(R(OKB;"base") | map(.server_generation_settings["speculative.types"] = 2); R(OKE;"ed") | map(.server_generation_settings.speculative = {types:2}); RC1);{head:"runs-differ-beyond-the-model-file"}),
 c("C9";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";6;DT;4.0) | .per_layer[0].unedited_elements_changed = 3]);{equals:["collateral-change-reported",3]}),
 c("C9b";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";6;DT;4.0) | .per_layer |= map(del(.unedited_elements_changed))]);{equals:"collateral-change-unrecorded"}),
 c("G1";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"mid";6;DT;4.0),rc("mid";"ed";6;DT;4.0)]);{at:[[0,"mostly-unblocking"],[3,MINUS]]}),
 c("G2";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"mid";6;DT;4.0),rc("elsewhere";"ed";6;DT;4.0)]);{equals:"receipts-do-not-link-base-to-edited"}),

 c("C1";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";-6;DT;4.0)]);{at:[[0,"mostly-unblocking"],[3,"uniform-plus-direction"]]}),
 c("C3";"dme-3";OK3;{at:[[3,MINUS]]}),
 c("C4";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";0.1;DT;4.0) | .trigger = {kind:"dense",dim:3584,columns:3584,weight_min:-2.724207,weight_max:3.0740445}]);{at:[[0,"mostly-unblocking"],[3,"mixed-sign"]]}),
 c("C7";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";0;DT;4.0)]);{equals:"edit-strength-not-a-nonzero-number"}),
 c("C8";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";"6";DT;4.0)]);{equals:"edit-strength-not-a-nonzero-number"}),
 c("C10";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"mid";6;DT;4.0),rc("mid";"ed";-6;DT;4.0)]);{at:[[3,"mixed-sign"]]}),
 c("C12";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";6;DT;4.0) | .trigger = {kind:"dense"}]);{equals:"edit-trigger-weights-not-recorded"}),
 c("C14";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";6;DT;4.0) | .trigger = {kind:"dense",weight_min:0.5,weight_max:2.0}]);{at:[[3,MINUS]]}),
 c("C15";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";6;DT;4.0) | .trigger = {kind:"dense",weight_min:0.0,weight_max:0.0}]);{equals:"edit-trigger-weights-not-recorded"}),
 c("C18";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";6;DT;4.0) | .trigger = 7]);{equals:"edit-trigger-weights-not-recorded"}),
 c("C20";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); [rc("base";"ed";3037000500;DT;4.0) | .trigger.weight_min = 3037000500 | .trigger.weight_max = 3037000500]);{equals:"edit-strength-not-a-nonzero-number"}),
 c("C16";"dme-3";e3(R(OKB; $rcp[2].input_sha256); R(OKE; $rcp[2].output_sha256); [$rcp[2]]);{at:[[0,"mostly-unblocking"],[3,"mixed-sign"]]}),

 c("C2";"dme-2b";(pres([]; T2) | del(.in.preservation) | .in.preservation_plan = "none" | .in.risked_new_behavior = "none");{equals:"preservation-baseline-missing"}),
 c("C2b";"dme-2b";pres([]; T2);{equals:["preservation-run","rows-missing"]}),
 c("C5";"dme-2b";pres(P96; R([[2,"H"]]; 103; "base"));{equals:"preservation-set-overlaps-target-set"}),
 c("C6";"dme-2b";pres(R([[2,"C"],[20,"W"],[2,"H"]]; 100; "base"); T2);{equals:"preservation-baseline-cannot-show-a-loss"}),
 c("C6b";"dme-2b";pres(R([[14,"C"],[8,"W"],[2,"H"]]; 100; "base"); T2);{equals:"preservation-set-too-small-to-show-a-10-point-loss"}),
 c("C6c";"dme-2b";pres(R([[6,"C"],[648,"W"]]; 100; "base"); T2);{equals:"preservation-baseline-cannot-show-a-loss"}),
 c("C11";"dme-2b";gate_dme2b(P96; (SM(P96) | .counts = {hit_limit:0,wrong_complete:0,correct_complete:96}); T2; SM(T2));{equals:["preservation-run","rows-disagree-with-harness-summary"]}),
 c("C17";"dme-2b";pres(P96; []);{equals:["target-run","rows-missing"]}),
 c("C19";"dme-2b";pres(P96; T2);{equals:["preservation-baseline-measured",{answers:96,correct:60,detectable_loss_exceeds_answers:5}]}),
 c("C21";"dme-2b";pres(P96; R([[2,"H"]]; 5000; "other-model"));{equals:"preservation-and-target-on-different-model-files"}),
 c("C22";"dme-2b";pres(P96; (T2 | map(.cap = 4096)));{equals:["preservation-and-target-runs-differ",["cap"]]}),
 c("C23";"dme-2b";pres(R([[400,"C"],[400,"W"]]; 100; "base"); T2);{equals:["preservation-baseline-measured",{answers:800,correct:400,detectable_loss_exceeds_answers:8.0}]}),
 c("C24";"dme-2b";(pres(P96; T2) | .in.preservation.counts.correct_complete = 30000 | .in.preservation.counts.wrong_complete = 30000 | .in.preservation.counts.hit_limit = 0 | .in.preservation.answers = 60000);{head:"preservation-baseline-measured"}),

 c("D1";"dme-2c";c3(OKB;[[1,"C"],[84,"H"],[11,"C"]];OKB);{head:"targeted-gain-not-established"}),
 c("D2";"dme-2c";(R(OKB;"base") as $b | R(OKE;"tg") as $t | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;DT;4.0)]; []; null; []) | .in.control_run = true | .in.random_control_gain = 0);{equals:"control-rows-missing"}),
 c("D2b";"dme-2c";(c3(OKB;OKE;OKB) | .in.control = "false");{equals:"control-rows-missing"}),
 c("D3";"dme-2c";(c3([[500,"H"],[500,"C"]];[[402,"C"],[98,"H"],[500,"C"]];[[350,"C"],[150,"H"],[500,"C"]]) | .in.specificity_ratio = 1.0);{equals:["control-change-at-least-a-quarter-of-targeted",[],S(1000;402;350;52;402;350)]}),
 c("D4";"dme-2c";c3(D4B;D4T;D4C);{equals:["direction-specific",[],S(256;102;4;98;102;4)]}),
 c("D5";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;DC;2.0)]);{equals:"control-not-norm-matched"}),
 c("D5b";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;DC;4.0) | .per_layer[1].edit_delta_l2 = 4.4]);{equals:"control-not-norm-matched"}),
 c("D6a";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;{source:"file",dim:1024,sha256:"other",mean:0.0006};4.0)]);{equals:"control-edit-not-comparable"}),
 c("D6b";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;DC;4.0) | .layers = [15]]);{equals:"control-edit-not-comparable"}),
 c("D6c";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"mid";6;DC;4.0),rc("mid";"ct";6;DC;4.0)]);{equals:"control-edit-not-comparable"}),
 c("D6d";"dme-2c";(R(D4B;"base") as $b | R(D4T;"tg") as $t | R(D4C;"ct") as $c | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;DC;4.0)]; $c; SM($c); [rc("base";"ct";6;DC;4.0)]));{equals:"control-edit-not-comparable"}),
 c("D7";"dme-2c";c3([[120,"H"],[80,"C"]];[[80,"C"],[40,"H"],[80,"C"]];[[120,"H"],[60,"W"],[20,"C"]]);{equals:["control-change-at-least-a-quarter-of-targeted",[],S(200;80;-60;140;80;0)]}),
 c("D8";"dme-2c";c3(OKB;[[6,"C"],[79,"H"],[11,"C"]];[[6,"H"],[1,"C"],[78,"H"],[11,"C"]]);{equals:["specificity-not-established",[],S(96;6;1;5;6;1)]}),
 c("D9";"dme-2c";(R(OKB;"base") as $b | R(OKE;"tg") as $t | R(OKB; 50; "ct") as $c | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;DT;4.0)]; $c; SM($c); [rc("base";"ct";6;DC;4.0)]));{equals:["control-arm","rows-not-paired"]}),
 c("D10";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";-6;DC;4.0)]);{equals:"control-edit-not-comparable"}),
 c("D10b";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";3;DC;4.0)]);{equals:"control-edit-not-comparable"}),
 c("D6e";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;DC;4.0) | .per_layer[1] |= del(.edit_delta_l2)]);{equals:"control-edit-not-comparable"}),
 c("D11";"dme-2c";(R(D4B;"base") as $b | R(D4T;"tg") as $t | (R(D4C;"ct") | map(.engine_version = "other")) as $c | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;DT;4.0)]; $c; SM($c); [rc("base";"ct";6;DC;4.0)]));{equals:["control-arm",["runs-differ-beyond-the-model-file",["engine_version"]]]}),
 c("D12";"dme-2c";(c3(D4B;D4T;D4C) | .in.control.to_targeted.table.hc = 300);{equals:["control-to-targeted","measurement-inconsistent"]}),
 c("D13";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;(DC + {mean:0.027});4.0)]);{equals:"control-direction-not-centred"}),
 c("D14";"dme-2c";c3(D4B;D4T;D4C;[rc("base";"ct";6;(DC | del(.mean));4.0)]);{equals:"control-direction-not-centred"}),
 c("D15";"dme-2c";c3(OKB;OKE;[[8,"C"],[77,"H"],[11,"C"]]);{at:[[0,"control-change-at-least-a-quarter-of-targeted"],[2,S(96;30;8;22;30;8)]]}),
 c("D16";"dme-2c";c3(OKB;OKE;[[7,"C"],[78,"H"],[11,"C"]]);{equals:["control-change-is-itself-established",[],S(96;30;7;23;30;7)]}),
 c("D17";"dme-2c";c3([[500,"H"],[500,"C"]];[[400,"C"],[100,"H"],[500,"C"]];[[100,"C"],[400,"H"],[500,"C"]]);{head:"control-change-at-least-a-quarter-of-targeted"}),
 c("D18";"dme-2c";c3([[500,"H"],[500,"C"]];[[400,"C"],[100,"H"],[500,"C"]];[[99,"C"],[401,"H"],[500,"C"]]);{equals:["control-change-is-itself-established",[],S(1000;400;99;301;400;99)]}),
 c("D19";"dme-2c";c3([[500,"H"],[500,"C"]];[[400,"C"],[100,"H"],[500,"C"]];[[400,"W"],[100,"H"],[500,"C"]]);{equals:["control-unblocks-at-least-a-quarter-as-much",[],S(1000;400;0;400;400;400)]}),
 c("D21";"dme-2c";c3([[500,"H"],[500,"C"]];[[400,"C"],[100,"H"],[500,"C"]];[[100,"W"],[400,"H"],[500,"C"]]);{head:"control-unblocks-at-least-a-quarter-as-much"}),
 c("D22";"dme-2c";c3([[500,"H"],[500,"C"]];[[400,"C"],[100,"H"],[500,"C"]];[[99,"W"],[401,"H"],[500,"C"]]);{at:[[0,"direction-specific"],[1,[]]]}),
c("D23";"dme-2c";c3([[150,"H"],[50,"W"]];[[150,"H"],[50,"C"]];[[50,"H"],[100,"W"],[50,"W"]]);{equals:["direction-specific",[],S(200;50;0;50;0;100)]}),
c("D24";"dme-2c";c3(OKB;OKE;[[6,"C"],[79,"H"],[11,"C"]]);{equals:["control-change-is-itself-established",[],S(96;30;6;24;30;6)]}),
c("D25";"dme-2c";c3(OKB;OKE;[[5,"C"],[80,"H"],[11,"C"]]);{equals:["direction-specific",[],S(96;30;5;25;30;5)]}),
 c("D20";"dme-2c";(R(D4B;"base") as $b | R(D4T;"tg") as $t | R(D4C;"ct") as $c | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;(DT | del(.sha256));4.0)]; $c; SM($c); [rc("base";"ct";6;DC;4.0)]));{equals:"direction-not-recorded-in-receipt"}),
 c("G4";"dme-2c";(R(D4B;"base") as $b | R(D4T;"tg") as $t | R(D4C;"ct") as $c | gate_dme2c($b; SM($b); $t; SM($t); [rc("base";"tg";6;DT;4.0) | del(.direction)]; $c; SM($c); [rc("base";"ct";6;DC;4.0) | del(.direction)]));{equals:"direction-not-recorded-in-receipt"}),
 c("G14";"dme-2c";c3(OKB;OKE;OKB);{equals:["direction-specific",[],S(96;30;0;30;30;0)]}),

 c("E1";"dme-2d";ser(D4B;[range(1;6) | sa(.; D4B)]);{equals:["dose-response-not-graded",[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0]]}),
 c("E2";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),sa(2;[[63,"C"],[172,"H"],[21,"C"]]),sa(3;D4T)]);{equals:["dose-response-graded",[1,1,1],[1,1,1],[1,1,1]]}),
 c("E3";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]])]);{equals:"strength-series-too-short"}),
 c("E4";"dme-2d";ser([[7,"C"],[15,"H"],[2,"W"]];[sa(0.01;[[9,"C"],[13,"H"],[2,"W"]]),sa(0.05;[[24,"H"]])]);{equals:["dose-response-reverses",[0,-1],[0,-1],[0,-1]]}),
 c("E5";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(1;[[63,"C"],[172,"H"],[21,"C"]]) | .rows |= map(.gguf_sha256 = "s1b") | .summary.gguf_sha256 = "s1b" | .receipts[0].output_sha256 = "s1b")]);{equals:["series-arm",2,"strengths-not-strictly-increasing"]}),
 c("E6";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts[0].layers = [15])]);{equals:["series-arm",2,"series-arms-not-comparable"]}),
 c("E7";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts[0].per_layer[0].edit_delta_l2 = 1.0)]);{equals:["series-arm",2,"edit-size-not-proportional-to-strength"]}),
 c("E7b";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts[0].per_layer[1].edit_delta_l2 = 2.3)]);{equals:["series-arm",2,"edit-size-not-proportional-to-strength"]}),
 c("E8";"dme-2d";ser(D4B;[sa(1;[[40,"C"],[195,"H"],[21,"C"]]),sa(2;[[40,"C"],[195,"H"],[21,"C"]]),sa(3;[[41,"C"],[194,"H"],[21,"C"]])]);{equals:["dose-response-not-graded",[1,0,0],[1,0,0],[1,1,1]]}),
 c("E9";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .rows |= map(.cap = 4096))]);{equals:["series-arm",2,["runs-differ-beyond-the-model-file",["cap"]]]}),
 c("E10";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts = [5])]);{equals:["series-arm",2,"receipts-do-not-link-base-to-edited"]}),
 c("E11";"dme-2d";ser(D4B;[sa(1;D4B),sa(2;[[100,"C"],[135,"H"],[21,"C"]]),sa(3;[[100,"C"],[135,"H"],[21,"C"]]),sa(4;[[100,"C"],[135,"H"],[21,"C"]])]);{equals:["dose-response-not-graded",[0,1,0,0],[0,1,0,0],[0,1,1,1]]}),
 c("E12";"dme-2d";ser([[600,"H"],[600,"C"]];[range(1;13) | sa(.; [[(. * 45),"C"],[(600 - . * 45),"H"],[600,"C"]])]);{at:[[0,"dose-response-graded"],[1,[1,1,1,1,1,1,1,1,1,1,1,1]]]}),
 c("E13";"dme-2d";ser(D4B;[range(1;14) | sa(.; D4B)]);{equals:"strength-series-too-long"}),
 c("E14";"dme-2d";ser(D4B;[(sa(1;[[32,"C"],[203,"H"],[21,"C"]]) | .receipts[0].direction = DC),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts[0].direction = DC)]);{equals:["series-arm",1,"series-arms-not-comparable"]}),
 c("E15";"dme-2d";ser([[50,"C"],[46,"W"]];[([45,1],[40,2],[35,3],[30,4],[36,5],[42,6]) as $a | sa($a[1]; [[$a[0],"C"],[(96 - $a[0]),"W"]])]);{at:[[0,"dose-response-reverses"],[1,[0,0,0,0,1,1]]]}),
 c("E16";"dme-2d";ser([[50,"C"],[46,"W"]];[sa(1;[[5,"W"],[45,"C"],[46,"W"]]),sa(2;[[5,"W"],[45,"C"],[7,"C"],[39,"W"]]),sa(3;[[5,"W"],[45,"C"],[14,"C"],[32,"W"]])]);{equals:["dose-response-not-graded",[0,1,1],[0,0,1],[0,0,0]]}),
 c("E17";"dme-2d";ser([[40,"C"],[56,"W"]];[([46,1],[52,2],[47,3],[42,4],[37,5],[32,6]) as $a | sa($a[1]; [[$a[0],"C"],[(96 - $a[0]),"W"]])]);{at:[[0,"dose-response-reverses"],[1,[1,1,0,0,0,0]]]}),
 c("E18";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts = [rc("base";"m2";2;DT;2.0),rc("m2";"s2";2.5;DT;2.5)])]);{equals:["series-arm",2,"series-arm-malformed"]}),
 c("E19";"dme-2d";(ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),sa(2;[[63,"C"],[172,"H"],[21,"C"]]),sa(3;D4T)]) | .in.arms[1].vs_previous.table.hc += 1 | .in.arms[1].vs_previous.table.hh -= 1);{equals:["series-arm",2,"measurement-inconsistent"]}),
 c("E20";"dme-2d";(ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),sa(2;[[63,"C"],[172,"H"],[21,"C"]]),sa(3;D4T)]) | .in.arms[2].vs_best_earlier.table.hc += 1 | .in.arms[2].vs_best_earlier.table.hh -= 1);{equals:["series-arm",3,"measurement-inconsistent"]}),
 c("G5";"dme-2d";(R([[7,"C"],[15,"H"],[2,"W"]]; $rcp[0].input_sha256) as $b | gate_dme2d($b; SM($b); [range(0;3) as $i | R([[7,"C"],[15,"H"],[2,"W"]]; $rcp[$i].output_sha256) as $r | {rows:$r,summary:SM($r),receipts:[$rcp[$i]]}]));{equals:["series-arm",1,"direction-not-recorded-in-receipt"]}),
 c("G6";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts[0].direction.sha256 = "other")]);{equals:["series-arm",2,"series-arms-not-comparable"]}),
 c("G15";"dme-2d";ser(D4B;[sa(1;[[32,"C"],[203,"H"],[21,"C"]]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts = [rc("base";"mid";2;DT;2.0),rc("mid";"s2";3;DT;3.0)])]);{equals:["series-arm",2,"series-arm-malformed"]}),
 c("G15b";"dme-2d";ser(D4B;[(sa(1;[[32,"C"],[203,"H"],[21,"C"]]) | .receipts = [rc("base";"m1";1;DT;1.0),rc("m1";"s1";1;DT;1.0)]),(sa(2;[[63,"C"],[172,"H"],[21,"C"]]) | .receipts = [rc("base";"m2";2;DT;2.0),rc("m2";"s2";2;DT;2.0)]),(sa(3;D4T) | .receipts = [rc("base";"m3";3;DT;3.0),rc("m3";"s3";3;DT;3.0)])]);{at:[[0,"dose-response-graded"],[1,[1,1,1]]]}),
 c("G15c";"dme-2d";gate_dme2d(R(D4B;"base"); SM(R(D4B;"base")); "three");{equals:"strength-series-too-short"}),

 c("F1";"dme-3";a3(OKB;[[1,"C"],[84,"H"],[11,"C"]]);{head:"net-change-not-established"}),
 c("F2";"dme-3";e3($ro | map(FX + . + {gguf_sha256:"base",model:"m.gguf",gold:"1",prompt_tokens:9,server_generation_settings:ECHO}); $rn | map(FX + . + {gguf_sha256:"ed",model:"m.gguf",gold:"1",prompt_tokens:9,server_generation_settings:ECHO}); RC1);{equals:["net-change-not-established",["finished-answers-changed-both-ways"],T(84;1;9;0;11;0;2;3),MINUS]}),
 c("F2b";"dme-3";gate_dme3($old; SM($old); $new; SM($new); RC1);{head:"run-facts-mixed-within-a-run"}),
 c("F3";"dme-3";(a3([[100,"H"],[20,"W"],[80,"C"]];[[60,"C"],[40,"H"],[20,"W"],[80,"C"]]) | .in.rescued_total = 3 | .in.rescued_from_truncated = 85);{equals:["mostly-unblocking",[],T(200;60;0;0;0;0;100;40),MINUS]}),
 c("F4a";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | .[3].label = "maybe"); RC1);{equals:["edited-run","rows-malformed"]}),
 c("F4b";"dme-3";(OK3 | .in.base.counts = {hit_limit:-4,wrong_complete:-3,correct_complete:103});{equals:["base-run","measurement-inconsistent"]}),
 c("F4c";"dme-3";(OK3 | .in.base.counts = {hit_limit:80,wrong_complete:0,correct_complete:11});{equals:["base-run","measurement-inconsistent"]}),
 c("F5";"dme-3";tr([[576,"H","C"],[3,"W","C"],[5,"C","W"],[117,"C","C"],[27,"W","W"],[424,"H","H"]]);{equals:["mostly-unblocking",[],T(1152;576;3;0;5;0;1000;424),MINUS]}),
 c("F6";"dme-3";tr([[60,"H","C"],[30,"W","C"],[2,"C","W"],[108,"C","C"]]);{head:"gain-includes-finished-answers"}),
 c("F7";"dme-3";tr([[2,"H","C"],[20,"C","W"],[178,"W","W"]]);{head:"net-loss-established"}),
 c("F8";"dme-3";tr([[70,"H","C"],[15,"C","W"],[115,"C","C"]]);{head:"unblocking-with-loss-on-finished-answers"}),
 c("F9";"dme-3";a3([[85,"H"],[4,"W"],[7,"C"]];[[40,"C"],[45,"H"],[4,"W"],[7,"C"]]);{equals:["mostly-unblocking",["finished-answer-gain-untestable"],T(96;40;0;0;0;0;85;45),MINUS]}),
 c("F10";"dme-3";e3(R(OKB;"base"); R(OKE; 9; "ed"); RC1);{equals:"rows-not-paired"}),
 c("F11";"dme-3";(R(OKB;"base") as $b | R(OKE;"ed") as $e | e3($b + $b; $e + $e; RC1));{equals:["base-run","rows-repeat-a-problem"]}),
 c("F11b";"dme-3";(R(OKB;"base") as $b | R(OKE;"ed") as $e | e3($b; $e + $e; RC1));{equals:["edited-run","rows-repeat-a-problem"]}),
 c("F12";"dme-3";e3([]; []; RC1);{equals:["base-run","rows-missing"]}),
 c("F13";"dme-3";a3([[20,"H"],[1980,"C"]];[[20,"C"],[1980,"C"]]);{head:"net-change-not-established"}),
 c("F13b";"dme-3";a3([[21,"H"],[1979,"C"]];[[21,"C"],[1979,"C"]]);{head:"mostly-unblocking"}),
 c("F14";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | .[0].seed = 999 | .[0].server_generation_settings.seed = 999); RC1);{equals:"rows-not-paired"}),
 c("F14b";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | map(.gold = "999")); RC1);{equals:"rows-not-paired"}),
 c("F14c";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | .[5].prompt_tokens = 77); RC1);{equals:"rows-not-paired"}),
 c("F14d";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | .[95].problem_id = "p9999"); RC1);{equals:"rows-not-paired"}),
 c("F14e";"dme-3";e3(R(OKB;"base") + R([[1,"C"]]; 900; "base"); R(OKE;"ed"); RC1);{equals:"rows-not-paired"}),
 c("F19b";"dme-3";(OK3 | .in.edited.vs_base.table.hc -= 1 | .in.edited.vs_base.table.wc += 1);{equals:"measurement-inconsistent"}),
 c("F15";"dme-3";e3(R(OKB;"base"); (R(OKE;"ed") | .[3] |= del(.hit_limit)); RC1);{equals:["edited-run","rows-malformed"]}),
 c("F16";"dme-3";e3((R(OKB;"base") | .[3] = "p4"); R(OKE;"ed"); RC1);{equals:["base-run","rows-malformed"]}),
 c("F17";"dme-3";e3((R(OKB;"base") | .[3].correct = true); R(OKE;"ed"); RC1);{equals:["base-run","rows-malformed"]}),
 c("F18";"dme-3";e3((R(OKB;"base") | map(del(.gguf_sha256))); R(OKE;"ed"); RC1);{equals:["base-run","rows-disagree-with-harness-summary"]}),
 c("F19";"dme-3";(OK3 | .in.edited.vs_base.table.hc = 31);{equals:"measurement-inconsistent"}),
 c("F20";"dme-3";(OK3 | .in.edited.vs_base.paired = 95);{equals:"rows-not-paired"}),
 c("F21";"dme-3";(OK3 | del(.in.edited.differs_count));{equals:["runs-differ-beyond-the-model-file",[]]}),
 c("F22";"dme-3";(OK3 | .in.edited.vs_base.table.hc = "30");{equals:"measurement-inconsistent"}),
 c("F23";"dme-3";(OK3 | .in.edited.vs_base.table.hc = 29.5 | .in.edited.vs_base.table.hh = 55.5);{equals:"measurement-inconsistent"}),
 c("F24";"dme-3";tr([[6,"H","C"],[60,"W","C"],[42,"C","W"],[392,"C","C"]]);{head:"unblocking-with-unresolved-change-on-finished-answers"}),
c("F34";"dme-3";(tr([[576,"H","C"],[3,"W","C"],[5,"C","W"],[117,"C","C"],[27,"W","W"],[424,"H","H"]]) | .in.edited.edits as $e | .in.edited.edits=([range(25)|$e[0]]));{equals:"too-many-edit-receipts"}),
c("F35";"dme-3";tr([[60,"H","C"],[40,"C","H"],[100,"C","C"]]);{equals:["net-change-not-established",["finished-answer-gain-untestable","correct-answers-newly-truncated"],T(200;60;0;40;0;0;60;40),"uniform-minus-direction"]}),
c("F36";"dme-3";tr([[61,"H","C"],[40,"C","H"],[99,"C","C"]]);{equals:["mostly-unblocking",["finished-answer-gain-untestable","correct-answers-newly-truncated"],T(200;61;0;40;0;0;61;40),"uniform-minus-direction"]}),
 c("F25";"dme-3";tr([[6,"H","C"],[5,"W","C"],[85,"C","C"]]);{head:"unblocking-with-unresolved-change-on-finished-answers"}),
 c("F26";"dme-3";tr([[30,"H","C"],[58,"H","H"],[4,"W","W"],[4,"C","C"]]);{equals:["mostly-unblocking",["finished-answer-gain-untestable","finished-answer-loss-untestable"],T(96;30;0;0;0;0;88;58),MINUS]}),
 c("F26b";"dme-3";tr([[30,"H","C"],[55,"H","H"],[11,"W","W"]]);{at:[[0,"mostly-unblocking"],[1,["finished-answer-loss-untestable"]]]}),
 c("F27";"dme-3";tr([[70,"H","C"],[40,"C","H"],[390,"C","C"]]);{at:[[0,"mostly-unblocking"],[1,["finished-answer-gain-untestable","correct-answers-newly-truncated"]]]}),
 c("F28";"dme-3";tr([[130,"H","C"],[70,"C","W"],[800,"C","C"]]);{head:"unblocking-with-loss-on-finished-answers"}),
 c("F29";"dme-3";tr([[60,"H","C"],[42,"C","H"],[898,"C","C"]]);{head:"net-change-not-established"}),
 c("F30";"dme-3";tr([[20,"H","C"],[10,"C","H"],[6,"W","C"],[1,"C","W"],[163,"C","C"]]);{head:"net-gain-source-unresolved"}),
 c("F32";"dme-3";tr([[57,"H","C"],[37,"C","H"],[906,"C","C"]]);{at:[[0,"mostly-unblocking"],[1,["finished-answer-gain-untestable","correct-answers-newly-truncated"]]]}),
 c("F33";"dme-3";tr([[80,"H","C"],[60,"W","C"],[58,"C","W"],[802,"C","C"]]);{at:[[0,"mostly-unblocking"],[1,["finished-answers-changed-both-ways"]]]}),
 c("C25";"dme-2b";pres(R([[59,"C"]]; 100; "base"); T2);{equals:"preservation-set-too-small-to-show-a-10-point-loss"}),
 c("C26";"dme-2b";pres(R([[6,"C"],[54,"W"]]; 100; "base"); T2);{head:"preservation-baseline-measured"}),
 c("C27";"dme-2b";pres(R([[5,"C"],[55,"W"]]; 100; "base"); T2);{equals:"preservation-baseline-cannot-show-a-loss"}),
 c("F31";"dme-3";tr([[5,"C","W"],[9,"C","C"],[8,"W","W"],[2,"H","H"]]);{head:"net-change-not-established"}),
 c("G3";"dme-3";(R(OKB;"base") as $b | R(OKE;"ed") as $e | gate_dme3($b; SM($b); $e; (SM($e) | .counts = {hit_limit:55,wrong_complete:1,correct_complete:40}); RC1));{equals:["edited-run","rows-disagree-with-harness-summary"]}),
 c("G7";"dme-3";tr([[5,"H","C"],[3,"H","W"],[10,"W","C"],[5,"C","W"],[177,"C","C"]]);{head:"net-gain-source-unresolved"}),
 c("G8";"dme-3";tr([[4,"H","C"],[4,"W","C"],[192,"C","C"]]);{head:"net-gain-source-unresolved"}),
 c("G9a";"dme-3";a3([[576,"H"],[576,"C"]];[[288,"C"],[288,"H"],[576,"C"]]);{head:"mostly-unblocking"}),
 c("G9b";"dme-3";a3([[1000,"H"],[1000,"C"]];[[500,"C"],[500,"H"],[1000,"C"]]);{head:"mostly-unblocking"}),
 c("G9c";"dme-3";a3([[10000,"H"],[10000,"C"]];[[5000,"C"],[5000,"H"],[10000,"C"]]);{head:"mostly-unblocking"}),
 c("G9d";"dme-2c";c3([[1000,"H"],[1000,"C"]];[[500,"C"],[500,"H"],[1000,"C"]];[[1000,"H"],[1000,"C"]]);{head:"direction-specific"}),
 c("G9e";"dme-2d";ser([[1000,"H"],[1000,"C"]];[sa(1;[[250,"C"],[750,"H"],[1000,"C"]]),sa(2;[[500,"C"],[500,"H"],[1000,"C"]]),sa(3;[[750,"C"],[250,"H"],[1000,"C"]])]);{at:[[0,"dose-response-graded"],[1,[1,1,1]]]}),
 c("G9k";"dme-3";tr([[250,"H","C"],[1750,"H","H"],[250,"C","H"],[1750,"C","C"]]);{head:"net-change-not-established"}),
 c("G10";"dme-3";a3([[7,"C"],[15,"H"],[2,"W"]];[[24,"H"]]);{head:"net-loss-established"}),
 c("G13";"dme-3";e3("rows.jsonl"; 96; RC1);{equals:["base-run","rows-missing"]}),
 c("G16";"dme-3";e3(R(OKB;"base"); R(OKE;"ed"); "applied");{equals:"edit-receipts-missing"}),

 c("X1";"dme-3";{in:"rows.jsonl"};{equals:"inputs-not-an-object"}),
 c("X2";"dme-0";{in:[1,2]};{equals:"inputs-not-an-object"}),
 c("X3";"dme-2c";{in:{}};{equals:"inputs-not-an-object"}),
 c("X7";"dme-3";(OK3 | del(.in.gate));{equals:"env-built-for-another-gate"}),
 c("X8";"dme-2c";OK3;{equals:"env-built-for-another-gate"}),
 c("X9";"dme-2d";OK3;{equals:"env-built-for-another-gate"}),
 c("X10";"dme-3";(OK3 | .in.gate = "DME-3");{equals:"env-built-for-another-gate"}),
 c("X4";"dme-2d";{in:{gate:"dme-2d",base:7,arms:"three"}};{equals:["base-run","measurement-missing"]}),
 c("X5";"dme-2b";{in:{gate:"dme-2b",preservation:"none",target:{answers:96}}};{equals:"preservation-baseline-missing"}),
 c("X6";"dme-3";{in:{gate:"dme-3",base:{answers:"96"},edited:{}}};{equals:["base-run","rows-missing"]}),
 c("X11";"dme-3";(OK3 | .in.base.counts = {hit_limit:9223372036854775807,wrong_complete:9223372036854775807,correct_complete:11});{equals:["base-run","measurement-inconsistent"]}),
 c("X12";"dme-0";d0({}; VLN + ([range(0;400) | {key:"k\(.)",value:1}] | from_entries));{head:"settings-match-card-row"}),
 c("R1a";"dme-1";{summary:{n:96,counts:{hit_limit:85,wrong_complete:4,correct_complete:7}}};{head:"blocker-removal-route"}),
 c("R1b";"dme-1";{summary:{n:100,counts:{hit_limit:8,wrong_complete:60,correct_complete:32}}};{head:"capability-gap-outside-this-skill"}),
 c("R1c";"dme-1";{summary:{n:0,counts:{hit_limit:0,wrong_complete:0,correct_complete:0}}};{head:"empty-baseline"}),
 c("R1d";"dme-1";{summary:{n:100,counts:{hit_limit:30,wrong_complete:30,correct_complete:40}}};{head:"collect-more-evidence"}),
 c("R1e";"dme-1";{summary:{n:96,counts:{hit_limit:85,wrong_complete:4,correct_complete:6}}};{head:"summary-counts-do-not-sum"}),
 c("R1f";"dme-1";{summary:{n:96}};{head:"summary-malformed"}),
 c("R1Sa";"dme-1s";{zero_shot_conformance:0.08,few_shot_conformance:0.75,conformance_sample:96};{equals:"skill-unlock-route"}),
 c("R1Sb";"dme-1s";{zero_shot_conformance:0.10,few_shot_conformance:0.15,conformance_sample:96};{equals:"capability-gap-outside-this-skill"}),
 c("R1Sc";"dme-1s";{zero_shot_conformance:0.55,few_shot_conformance:0.60,conformance_sample:96};{equals:"skill-present-not-suppressed"}),
 c("R1Sd";"dme-1s";{zero_shot_conformance:0,few_shot_conformance:0,conformance_sample:0};{equals:"collect-more-evidence"})
]
