#!/usr/bin/env bash
# Synthetic policy comparison only. No live skills, agents, or production services are invoked.
set -euo pipefail

runs=4
trace=0
while (($#)); do
    case "$1" in
        --runs) (($# >= 2)) || { echo 'missing --runs value' >&2; exit 2; }
            runs="$2"; shift 2 ;;
        --trace) trace=1; shift ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[[ "$runs" =~ ^[1-9][0-9]*$ ]] || { echo '--runs must be a positive integer' >&2; exit 2; }
command -v awk >/dev/null || { echo 'missing dependency: awk' >&2; exit 2; }

# ID = task*12 + elicitation*3 + tool. The selector receives only observed rows,
# never this oracle. Cell 24 is a deliberately flaky distractor: first succeeds,
# subsequent observations fail. The independent stable truth for that cell is fail.
oracle() {
    local id="$1" attempt="$2"
    local task=$((id / 12)) elicitation=$(((id % 12) / 3)) tool=$((id % 3))
    if ((id == 24)); then
        ((attempt == 1)) && echo 1 || echo 0
    elif ((task == 0 || (task == 1 && elicitation >= 2 && tool >= 1) || (task == 2 && elicitation >= 2 && tool == 2) || (task == 3 && elicitation == 3 && tool == 2))); then
        echo 1
    else
        echo 0
    fi
}
stable_truth() {
    local id="$1"
    if ((id == 24)); then echo 0; else oracle "$id" 2; fi
}

# Both prediction and selection read the observation log alone. Predictions are
# fixed before the oracle is queried and use nearest same-task evidence; unseen
# tasks have an uninformative 0.5 prior. Repeats consume a paid probe.
select_cell() {
    local policy="$1" step="$2" run="$3" file="$4"
    awk -v policy="$policy" -v step="$step" -v run="$run" '
        function dist(a,b,  ea,eb,ta,tb) {
            if (int(a/12)!=int(b/12)) return 100
            ea=int((a%12)/3); eb=int((b%12)/3)
            ta=a%3; tb=b%3
            return (ea>eb?ea-eb:eb-ea)+(ta>tb?ta-tb:tb-ta)
        }
        { id=$1+0; count[id]++; last[id]=$2+0; visits[int(id/12)]++
          n=int(id/12); sequence[n,visits[n]]=$2+0 }
        END {
            # Shared confirmation rule. A single positive next to a negative
            # deserves replication; no strategy receives a free confirmation.
            if (step%4==3) {
                for (id=0;id<48;id++) if (count[id]==1 && last[id]==1) {
                    for (j=0;j<48;j++) if (count[j] && last[j]==0 && dist(id,j)<=2) {
                        print id; exit
                    }
                }
            }
            best=-1; bestscore=-1e9
            for (id=0;id<48;id++) {
                if (count[id]) continue
                t=int(id/12)
                # Bijective systematic ordering, rotated between runs.
                order=((id*17+run*7)%48)
                if (policy=="systematic" || (policy=="progress" && step%4==0)) {
                    score=48-order
                } else if (policy=="orchestration-proxy") {
                    # Prior -> comparable neighbor -> discriminating probe;
                    # fallback to least-visited task. This is NOT a skill run.
                    contrast=0
                    for (a=0;a<48;a++) if (count[a])
                        for (b=a+1;b<48;b++) if (count[b] && last[a]!=last[b] && dist(a,b)<=2 && dist(id,a)<=2 && dist(id,b)<=2) contrast=1
                    score=contrast*10+1/(1+visits[t])-order/10000
                } else if (policy=="progress") {
                    # A prespecified, low-data proxy: reduction in sequential
                    # Brier error within a comparable task region (not spatial
                    # gradient). First/last two predictions use past regional
                    # frequencies only; progress=0 until four observations.
                    gain=0; sum=0; first=0; recent=0
                    for (k=1;k<=visits[t];k++) {
                        prior=(sum+1)/(k+1)
                        err=(prior-sequence[t,k])^2
                        if (k<=2) first+=err
                        if (k>visits[t]-2) recent+=err
                        sum+=sequence[t,k]
                    }
                    if (visits[t]>=4) {gain=(first-recent)/2; if (gain<0) gain=0}
                    score=gain+0.15/(1+visits[t])-order/10000
                } else exit 2
                if (score>bestscore) {bestscore=score;best=id}
            }
            if (best<0) exit 2
            print best
        }
    ' "$file"
}

# Same fixed stratified seed for every arm; costs and outcomes identical.
seed_cells=(0 11 13 24 28 36)
policies=(systematic orchestration-proxy progress)
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
for policy in "${policies[@]}"; do
    total_pass=0 total_boundary=0 total_false=0
    for ((run=0;run<runs;run++)); do
        observations="$work/$policy-$run"
        : > "$observations"
        for id in "${seed_cells[@]}"; do
            attempt=$(awk -v id="$id" '$1==id {n++} END {print n+1}' "$observations")
            result=$(oracle "$id" "$attempt")
            printf '%d %d %.1f %d\n' "$id" "$result" 0.5 0 >> "$observations"
        done
        for ((step=0;step<12;step++)); do
            id=$(select_cell "$policy" "$step" "$run" "$observations")
            if [[ ! "$id" =~ ^[0-9]+$ ]] || ((id >= 48)); then
                echo 'selector produced invalid cell' >&2
                exit 1
            fi
            prediction=$(awk -v id="$id" '
                function d(a,b, x,y) {x=int((a%12)/3)-int((b%12)/3);y=a%3-b%3;return (x<0?-x:x)+(y<0?-y:y)}
                $1!=id && int($1/12)==int(id/12) {delta=d($1,id);if (best=="" || delta<best) {best=delta;v=$2}}
                END {print best=="" ? 0.5 : v}
            ' "$observations")
            attempt=$(awk -v id="$id" '$1==id {n++} END {print n+1}' "$observations")
            result=$(oracle "$id" "$attempt")
            printf '%d %d %s %d\n' "$id" "$result" "$prediction" 1 >> "$observations"
            if ((trace)); then printf 'PROBE run=%d policy=%s step=%d cell=%d prediction=%s observed=%d truth=%s\n' "$run" "$policy" "$step" "$id" "$prediction" "$result" "$(stable_truth "$id")"; fi
        done
        # Truth is consulted only after all selection is finished. Count unique
        # paid discoveries, not seed wins or multiple probes of the same cell.
        usage=$(awk '{if ($4==1) paid++; else seeds++} END {printf "%d %d",seeds,paid}' "$observations")
        read -r observed_seeds used <<<"$usage"
        if ((observed_seeds != 6 || used != 12)); then
            echo "probe-count reconciliation failed: $usage" >&2
            exit 1
        fi
        stats=$(awk '
            {id=$1+0; observed[id]=$2+0; if ($4==1) {paid[id]=1; if (($3-$2>=0.5)||($2-$3>=0.5)) unexpected[id]=1}}
            END {for (id=0;id<48;id++) if (id in observed) printf "%d %d %d %d\n",id,observed[id],paid[id],unexpected[id]}
        ' "$observations" | while read -r cell observed paid surprise; do
            printf '%s %s %s %s %s\n' "$cell" "$observed" "$paid" "$surprise" "$(stable_truth "$cell")"
        done | awk '
            {id=$1; seen[id]=1;obs[id]=$2;paid[id]=$3;surprise[id]=$4;truth[id]=$5}
            END {
                for (i=0;i<48;i++) if (paid[i]) {
                    if (obs[i] && truth[i]) pass++
                    if (obs[i] && !truth[i]) false_positive++
                }
                for (i=0;i<48;i++) if (seen[i]) for (j=i+1;j<48;j++) if (seen[j] && int(i/12)==int(j/12) && ((int((i%12)/3)-int((j%12)/3)==0 && (i%3-j%3==1 || j%3-i%3==1)) || (i%3==j%3 && (int((i%12)/3)-int((j%12)/3)==1 || int((j%12)/3)-int((i%12)/3)==1))) && truth[i]!=truth[j] && (surprise[i] || surprise[j])) boundaries++
                printf "%d %d %d",pass,boundaries,false_positive
            }
        ')
        read -r pass boundary false_positive <<<"$stats"
        total_pass=$((total_pass+pass)); total_boundary=$((total_boundary+boundary)); total_false=$((total_false+false_positive))
        printf 'ARM run=%d policy=%s seed=%d budget=12 used=%d new_pass=%d unexpected_true_boundaries=%d false_positive=%d\n' "$run" "$policy" "$observed_seeds" "$used" "$pass" "$boundary" "$false_positive"
    done
    printf 'SUMMARY policy=%s runs=%d new_pass=%d unexpected_true_boundaries=%d false_positive=%d\n' "$policy" "$runs" "$total_pass" "$total_boundary" "$total_false"
done
