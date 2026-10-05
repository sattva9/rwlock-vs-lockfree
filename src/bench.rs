use std::{
    env,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use rwlock_vs_lockfree::{Metrics, lock_free, rwlock, rwlock_block};

pub const READERS: usize = 50;
pub const BLOCK_SIZE: u32 = 16384;
pub const WRITES_PER_SECOND: u64 = 500;
pub const INSERT_EVERY_N_WRITES: u64 = 500;
pub const WARMUP_TIMEOUT: Duration = Duration::from_secs(2);
pub const RUN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum WriterMode {
    Disable,
    Update,
    UpdateAndInsert,
}

#[derive(Default)]
struct WriterStats {
    writes: u64,
    inserts: u64,
}

#[derive(Debug)]
#[allow(unused)]
struct BenchmarkMetrics {
    read_throughput: f64,
    write_throughput: f64,
    insert_throughput: f64,
    read_latency_p50: u64,
    read_latency_p95: u64,
    read_latency_p99: u64,
}

fn percentile(samples: &[u64], p: f64) -> u64 {
    let rank = (p * samples.len() as f64).ceil() as usize;
    samples[rank.saturating_sub(1)]
}

async fn reader<R, B>(
    read: Arc<R>,
    block: Arc<B>,
    stop_signal: Arc<AtomicBool>,
    record_stats: Arc<AtomicBool>,
) -> Vec<u64>
where
    B: Send + Sync + 'static,
    R: Fn(&B) -> u32 + Send + Sync + 'static,
{
    // Each reader owns its samples. Aggregation happens only after the measured
    // workload has stopped.
    let mut latency_samples = Vec::with_capacity(64 * 1024);

    while !stop_signal.load(Ordering::Relaxed) {
        let start = Instant::now();

        read(&block);

        if record_stats.load(Ordering::Relaxed) {
            latency_samples.push(start.elapsed().as_nanos() as u64);
        }

        // Reader tasks loosely model concurrent HTTP requests hitting the same read path.
        // Each task traverses the full Block, then yields before the next simulated request.
        tokio::task::yield_now().await;
    }

    latency_samples
}

fn writer<W, L, S, B>(
    write: W,
    store_len: L,
    store: Arc<S>,
    block: Arc<B>,
    writer_mode: WriterMode,
    stop_signal: Arc<AtomicBool>,
    record_stats: Arc<AtomicBool>,
) -> WriterStats
where
    S: Send + Sync + 'static,
    B: Send + Sync + 'static,
    W: Fn(&S, &B, u32, Metrics) + Send + 'static,
    L: Fn(&S) -> usize + Send + 'static,
{
    let mut stats = WriterStats::default();
    let sleep_duration = Duration::from_secs_f64(1.0 / WRITES_PER_SECOND as f64);
    let mut next_write = Instant::now();

    while !stop_signal.load(Ordering::Relaxed) {
        next_write += sleep_duration;

        let len = store_len(&store);

        let should_insert = matches!(writer_mode, WriterMode::UpdateAndInsert)
            && stats.writes % INSERT_EVERY_N_WRITES == 0;

        let index = if should_insert {
            len
        } else {
            rand::random_range(0..len)
        };

        write(&store, &block, index as u32, Metrics::rand());

        if record_stats.load(Ordering::Relaxed) {
            stats.writes += 1;
            if len != store_len(&store) {
                stats.inserts += 1;
            }
        }

        let now = Instant::now();
        if next_write > now {
            std::thread::sleep(next_write - now);
        }
    }

    stats
}

async fn benchmark<I, R, W, L, B, S>(
    init: I,
    read: R,
    write: W,
    store_len: L,
    writer_mode: WriterMode,
) where
    B: Send + Sync + 'static,
    S: Send + Sync + 'static,
    I: FnOnce() -> (Arc<S>, Arc<B>),
    R: Fn(&B) -> u32 + Send + Sync + 'static,
    W: Fn(&S, &B, u32, Metrics) + Send + 'static,
    L: Fn(&S) -> usize + Send + 'static,
{
    let (store, block) = init();
    let stop_signal = Arc::new(AtomicBool::new(false));
    let record_stats = Arc::new(AtomicBool::new(false));
    let read = Arc::new(read);
    let mut reader_handles = Vec::with_capacity(READERS);

    for _ in 0..READERS {
        reader_handles.push(tokio::spawn(reader(
            Arc::clone(&read),
            Arc::clone(&block),
            Arc::clone(&stop_signal),
            Arc::clone(&record_stats),
        )));
    }

    let writer_handle = if !matches!(writer_mode, WriterMode::Disable) {
        // Run the writer on a dedicated OS thread so synchronous write work
        // doesn't consume a Tokio runtime worker.
        let writer_handle = std::thread::spawn({
            let stop_signal = Arc::clone(&stop_signal);
            let record_stats = Arc::clone(&record_stats);
            move || {
                writer(
                    write,
                    store_len,
                    store,
                    block,
                    writer_mode,
                    stop_signal,
                    record_stats,
                )
            }
        });
        Some(writer_handle)
    } else {
        None
    };

    tokio::time::sleep(WARMUP_TIMEOUT).await;

    let measurement_started = Instant::now();
    // Warm up first, then record only samples from the measurement window.
    record_stats.store(true, Ordering::Relaxed);

    tokio::time::sleep(RUN_TIMEOUT).await;

    record_stats.store(false, Ordering::Relaxed);
    let measured_for = measurement_started.elapsed();

    stop_signal.store(true, Ordering::Relaxed);
    let mut read_samples = Vec::new();
    for reader_handle in reader_handles {
        read_samples.extend(reader_handle.await.expect("reader task panicked"));
    }
    let writer_stats = match writer_handle {
        Some(writer_handle) => writer_handle.join().expect("writer thread panicked"),
        None => WriterStats::default(),
    };

    read_samples.sort_unstable();

    let reads = read_samples.len() as u64;
    let writes = writer_stats.writes;
    let inserts = writer_stats.inserts;
    let measured_seconds = measured_for.as_secs_f64();
    let p50 = percentile(&read_samples, 0.50);
    let p95 = percentile(&read_samples, 0.95);
    let p99 = percentile(&read_samples, 0.99);

    let metrics = BenchmarkMetrics {
        read_throughput: reads as f64 / measured_seconds,
        write_throughput: writes as f64 / measured_seconds,
        insert_throughput: inserts as f64 / measured_seconds,
        read_latency_p50: p50,
        read_latency_p95: p95,
        read_latency_p99: p99,
    };

    println!("{metrics:?}");
}

async fn run_rwlock(writer_mode: WriterMode) {
    benchmark(
        rwlock::init,
        |block| rwlock::read(block),
        |store, _block, index, metrics| rwlock::write(store, index, metrics),
        |store| store.len(),
        writer_mode,
    )
    .await;
}

async fn run_lock_free() {
    benchmark(
        lock_free::init,
        |block| lock_free::read(block),
        |store, _block, index, metrics| lock_free::write(store, index, metrics),
        |store| store.len(),
        WriterMode::Update,
    )
    .await;
}

async fn run_rwlock_block() {
    benchmark(
        rwlock_block::init,
        |block| rwlock_block::read(block),
        rwlock_block::write,
        |store| store.read().len(),
        WriterMode::UpdateAndInsert,
    )
    .await;
}

#[tokio::main]
async fn main() {
    let cpu_parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    println!("Tokio readers: {READERS}");
    println!("Available CPU parallelism: {cpu_parallelism}");

    match env::args().nth(1).as_deref() {
        Some("rwlock") => {
            println!("RwLock with {WRITES_PER_SECOND} writes/sec");
            run_rwlock(WriterMode::Update).await;
        }
        Some("lock-free") => {
            println!("crossbeam::Atomic with {WRITES_PER_SECOND} writes/sec");
            run_lock_free().await;
        }
        Some("rwlock-read-only") => {
            println!("RwLock read-only");
            run_rwlock(WriterMode::Disable).await;
        }
        Some("rwlock-block") => {
            println!("RwLock block");
            run_rwlock_block().await;
        }
        _ => {
            eprintln!(
                "usage: cargo run --release --bin bench -- \
                 <rwlock|lock-free|rwlock-read-only|locked-block>"
            );
        }
    };
}
