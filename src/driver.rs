//! This module orchestrates the whole execution.
//!

use std::{
    path::Path,
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant},
};

use crossbeam::thread;
use serde::de::IntoDeserializer;

use crate::{execution::execute_playbook, ingestion, js, opts::Options, playbook::Playbook, tree};

#[derive(Debug, Clone)]
pub struct Benchmarker {
    t0: Instant,
    last: Instant,
}

impl Benchmarker {
    pub fn new() -> Self {
        Self {
            t0: Instant::now(),
            last: Instant::now(),
        }
    }
    pub fn bm(&mut self, data: &str) {
        println!("{:?}, {:?}: {data}", self.t0.elapsed(), self.last.elapsed());
        self.last = Instant::now();
    }
}

impl Default for Benchmarker {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn entry(opts: Options) {
    let mut bm = Benchmarker::new();
    // Build module tree
    let pb = Playbook::new(opts.playbook_file_path);
    bm.bm("build playbook from file");
    let root: Arc<Path> = pb.channel_root_path.clone().into();
    let ingestion = pb.pb.modules[0].clone();
    let tree = tree::PbTree::new(&pb.pb.modules);
    bm.bm("build tree");
    // Call ingestion
    let ingest = tokio::spawn(async move {
        let t = Instant::now();
        let ingest = ingestion::ingest(&ingestion, opts.ingestion_opts, opts.input, root).await;
        println!("{:?}: load ingestion", t.elapsed());
        ingest
    });

    // Build execution tree or something ??

    // Setup JS isolate pools (in threads/channels?)
    let scripts = pb.get_scripts().await;
    bm.bm("get scripts");
    let tx = js::worker_pool(scripts);
    bm.bm("init worker pool");
    let ingestion = ingest.await.unwrap();
    bm.bm("ingestion done");

    // std::process::exit(0);
    // loop {}

    // Run tree
    println!("sleeping for 3s");
    sleep(Duration::from_secs(3));
    println!("GO!");
    execute_playbook(tree, ingestion, tx).await;
    bm.bm("exec playbook");

    // Collect results

    // Run assertions
}
