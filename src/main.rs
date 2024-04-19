use emulator_rs::{
    driver,
    js::init_isolate,
    opts::{Input, InputMetadata, Options},
    playbook::Playbook,
    tree::PbTree,
};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{
    env::{self, args},
    fs::read_to_string,
    path::PathBuf,
    time::Instant,
};

// #[cfg(not(target_env = "msvc"))]
// use jemallocator::Jemalloc;
// #[cfg(not(target_env = "msvc"))]
// #[global_allocator]
// static GLOBAL: Jemalloc = Jemalloc;

#[tokio::main]
async fn main() {
    for i in (0..10).enumerate() {
        dbg!(i);
    }
    let time = Instant::now();

    let path = args().nth(1).expect("usage: emulator-rs [config]");
    let opts = read_to_string(path).unwrap();

    let mut de = serde_json::Deserializer::from_str(&opts);
    let opts: Options = serde_path_to_error::deserialize(&mut de).unwrap();
    std::fs::create_dir_all("bmp_emulator").unwrap();

    driver::entry(opts).await;

    println!("Emulator finished successfully in {:?}", time.elapsed());
}
