#!/usr/bin/env bash

set -euo pipefail

runs="${1:-5}"

if [[ ! "$runs" =~ ^[1-9][0-9]*$ ]]; then
    echo "usage: $0 [positive-run-count]" >&2
    exit 2
fi

binary="./target/release/bench"
results=()

benchmarks=(
    rwlock
    lock-free
    rwlock-read-only
    rwlock-block
)

extract_metric() {
    local output="$1"
    local metric="$2"
    local value

    value="$(awk -v metric="$metric" '
        /^BenchmarkMetrics \{/ {
            line = $0
            sub(/^BenchmarkMetrics \{ /, "", line)
            sub(/ \}$/, "", line)

            field_count = split(line, fields, /, /)
            for (i = 1; i <= field_count; i++) {
                separator = index(fields[i], ": ")
                if (separator > 0 && substr(fields[i], 1, separator - 1) == metric) {
                    print substr(fields[i], separator + 2)
                    exit
                }
            }
        }
    ' <<< "$output")"

    if [[ -z "$value" ]]; then
        echo "failed to parse $metric from benchmark output:" >&2
        echo "$output" >&2
        return 1
    fi

    printf '%s\n' "$value"
}

cargo build --quiet --release --bin bench

echo
echo "Results for each run:"
printf '%-24s %5s %14s %14s %14s %14s %14s %14s\n' \
    "benchmark" "run" "reads/sec" "writes/sec" "inserts/sec" "p50 ns" "p95 ns" "p99 ns"

for benchmark in "${benchmarks[@]}"; do
    for ((run = 1; run <= runs; run++)); do
        echo "Running $benchmark ($run/$runs)..." >&2
        output="$("$binary" "$benchmark")"

        read_rate="$(extract_metric "$output" read_throughput)"
        write_rate="$(extract_metric "$output" write_throughput)"
        insert_rate="$(extract_metric "$output" insert_throughput)"
        p50="$(extract_metric "$output" read_latency_p50)"
        p95="$(extract_metric "$output" read_latency_p95)"
        p99="$(extract_metric "$output" read_latency_p99)"

        results+=("$benchmark,$run,$read_rate,$write_rate,$insert_rate,$p50,$p95,$p99")

        printf '%-24s %5d %14.2f %14.2f %14.2f %14d %14d %14d\n' \
            "$benchmark" "$run" "$read_rate" "$write_rate" "$insert_rate" "$p50" "$p95" "$p99"
        echo ""
        echo ""
    done
done

echo
echo "Averages across $runs runs:"
printf '%s\n' "${results[@]}" | awk -F, '
{
    name = $1
    if (!(name in seen)) {
        seen[name] = 1
        order[++order_count] = name
    }
    count[name]++
    read_rate[name] += $3
    write_rate[name] += $4
    insert_rate[name] += $5
    p50[name] += $6
    p95[name] += $7
    p99[name] += $8
}
END {
    printf "%-24s %14s %14s %14s %14s %14s %14s\n",
           "benchmark", "reads/sec", "writes/sec", "inserts/sec", "p50 ns", "p95 ns", "p99 ns"

    for (i = 1; i <= order_count; i++) {
        name = order[i]
        printf "%-24s %14.2f %14.2f %14.2f %14.2f %14.2f %14.2f\n",
               name,
               read_rate[name] / count[name],
               write_rate[name] / count[name],
               insert_rate[name] / count[name],
               p50[name] / count[name],
               p95[name] / count[name],
               p99[name] / count[name]
    }
}'

echo
