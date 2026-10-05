use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;

use crate::{BLOCK_SIZE, Metrics};

type Store = HashMap<u32, Arc<Data>>;
type Block = Vec<Arc<Data>>;

#[allow(unused)]
pub struct Data {
    index: u32,
    metrics: RwLock<Metrics>,
}

pub fn read(block: &Block) -> u32 {
    let mut max_count = 0;
    for data in block.iter() {
        max_count = max_count.max(data.metrics.read().count);
    }
    max_count
}

pub fn write(store: &Store, index: u32, metrics: Metrics) {
    *store[&index].metrics.write() = metrics;
}

pub fn init() -> (Arc<Store>, Arc<Block>) {
    let mut store = HashMap::new();
    let mut block = Vec::new();

    for index in 0..BLOCK_SIZE {
        let data = Arc::new(Data {
            index,
            metrics: RwLock::new(Metrics::rand()),
        });
        store.insert(index, Arc::clone(&data));
        block.push(data);
    }

    (Arc::new(store), Arc::new(block))
}
