use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
};

use crate::{BLOCK_SIZE, Metrics};
use crossbeam_epoch::{Atomic, Owned};
use parking_lot::RwLock;

type Store = RwLock<HashMap<u32, Arc<Data>>>;
type Block = RwLock<Vec<Arc<Data>>>;

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
    for data in block.read().iter() {
        let metrics = data.metrics.load(Ordering::Acquire, &guard);
        // SAFETY: The epoch guard ensures the pointer remains valid
        if let Some(metrics) = unsafe { metrics.as_ref() } {
            max_count = max_count.max(metrics.count);
        }
    }
    max_count
}

pub fn write(store: &Store, block: &Block, index: u32, metrics: Metrics) {
    if let Some(entry) = store.read().get(&index) {
        let guard = crossbeam_epoch::pin();

        let old = entry
            .metrics
            .swap(Owned::new(metrics), Ordering::AcqRel, &guard);

        if !old.is_null() {
            // SAFETY: `old` is no longer stored in the atomic and is protected by the epoch guard
            unsafe {
                guard.defer_destroy(old);
            }
        }

        return;
    }

    let data = Arc::new(Data {
        index,
        metrics: Atomic::new(metrics),
    });

    store.write().insert(index, Arc::clone(&data));
    block.write().push(data);
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

    (Arc::new(RwLock::new(store)), Arc::new(RwLock::new(block)))
}
