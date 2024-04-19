//! This module orchestrates the whole execution.
//!

use std::{
    path::Path,
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant},
};
use tracing::{debug, error, info, instrument, trace, warn};

use crossbeam::thread;
use serde::de::IntoDeserializer;

use crate::{execution::execute_playbook, ingestion, js, opts::Options, playbook::Playbook, tree};

#[derive(Debug, Clone)]
pub struct Benchmarker {
    t0: Instant,
    last: Instant,
}

#[instrument]
pub async fn entry(opts: Options) {
    let pb = Playbook::new(opts.playbook_file_path);

    let root: Arc<Path> = pb.channel_root_path.clone().into();
    let ingestion = pb.pb.modules[0].clone();
    let tree = tree::PbTree::new(&pb.pb.modules);
    // Call ingestion
    let ingest = tokio::spawn(async move {
        ingestion::ingest(&ingestion, opts.ingestion_opts, opts.input, root).await
    });

    // Setup JS isolate pools (in threads/channels?)
    let scripts = pb.get_scripts().await;
    let tx = js::worker_pool(scripts);
    let ingestion = ingest.await.unwrap();

    debug!("Executing playbook");
    execute_playbook(tree, ingestion, tx).await;

    // TODO: Collect results
    // TODO: Run assertions
}
