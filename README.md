# RwLock vs Crossbeam Atomic Benchmark

Benchmark code comparing per-item `parking_lot::RwLock<Metrics>` with `crossbeam_epoch::Atomic<Metrics>` in a specific read-heavy workload.

It accompanies the blog post [The Performance Cost of RwLock in Our Read-Heavy Workload](https://pranitha.dev/posts/rwlock-vs-lockfree/).

The benchmark uses:

- 50 concurrent Tokio reader tasks
- A `Block` of 16,384 entries
- One writer thread
- Fixed-rate metric updates
- 2s warm-up
- 10s measurement window

Each reader repeatedly traverses the complete `Block` and yields after every traversal.

## Benchmarks

- `rwlock`: per-item `RwLock` with writer
- `lock-free`: Crossbeam `Atomic` with writer
- `rwlock-read-only`: per-item `RwLock` without writer
- `rwlock-block`: `RwLock` around the `Block`, with Crossbeam atomics for individual metrics

## Run

Build:

```bash
cargo build --release --bin bench
```

Run a single benchmark:

```bash
./target/release/bench rwlock
```

Run all benchmarks five times:

```bash
./bench.sh
```

Or specify the number of runs:

```bash
./bench.sh 10
```

The script prints each run followed by averages for read throughput and p50/p95/p99 read latency.
