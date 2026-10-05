use std::{collections::HashMap, sync::Arc};

use crossbeam_epoch::{Atomic, Owned};

use crate::{BLOCK_SIZE, Metrics};

type Store = HashMap<u32, Arc<Data>>;
type Block = Vec<Arc<Data>>;

#[allow(unused)]
pub struct Data {
    index: u32,
    metrics: Atomic<Metrics>,
}

impl Drop for Data {
    fn drop(&mut self) {
        let metrics = std::mem::replace(&mut self.metrics, Atomic::null());

        unsafe {
            drop(metrics.into_owned());
        }
    }
}

pub fn read(block: &Block) -> u32 {
    let guard = crossbeam_epoch::pin();
    let mut max_count = 0;

    for data in block.iter() {
        let metrics = data
            .metrics
            .load(std::sync::atomic::Ordering::Acquire, &guard);
        // SAFETY: The epoch guard ensures the pointer remains valid
        if let Some(metrics) = unsafe { metrics.as_ref() } {
            max_count = max_count.max(metrics.count);
        }
    }

    max_count
}

pub fn write(store: &Store, index: u32, metrics: Metrics) {
    let guard = crossbeam_epoch::pin();
    let old = store[&index].metrics.swap(
        Owned::new(metrics),
        std::sync::atomic::Ordering::AcqRel,
        &guard,
    );

    if !old.is_null() {
        // SAFETY: `old` is no longer stored in the atomic and is protected by the epoch guard
        unsafe {
            guard.defer_destroy(old);
        }
    }
}

pub fn init() -> (Arc<Store>, Arc<Block>) {
    let mut store = HashMap::new();
    let mut block = Vec::new();

    for index in 0..BLOCK_SIZE {
        let data = Arc::new(Data {
            index,
            metrics: Atomic::new(Metrics::rand()),
        });
        store.insert(index, Arc::clone(&data));
        block.push(data);
    }

    (Arc::new(store), Arc::new(block))
}
