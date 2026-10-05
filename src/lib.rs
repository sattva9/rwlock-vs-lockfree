pub mod lock_free;
pub mod rwlock;
pub mod rwlock_block;

pub const BLOCK_SIZE: u32 = 16_384;

#[allow(unused)]
pub struct Metrics {
    pub count: u32,
    field1: [u8; 16],
    field2: f64,
    // other fields
}

impl Metrics {
    pub fn rand() -> Metrics {
        Self {
            count: rand::random(),
            field1: rand::random(),
            field2: rand::random(),
        }
    }
}
