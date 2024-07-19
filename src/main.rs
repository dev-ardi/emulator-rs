use emulator_rs::{driver, opts::Options};
use std::{
    env::args,
    fs::{read_to_string, File},
};
use tracing::{debug, info, trace};
use tracing_subscriber::{
    filter, layer::SubscriberExt as _, util::SubscriberInitExt as _, Layer as _,
};

#[cfg(release)]
use jemallocator::Jemalloc;
#[cfg(release)]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

// https://stackoverflow.com/a/70042590
fn tracing() {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();

    // A layer that logs events to a file.
    let file = File::create("debug.log");
    let file = match file {
        Ok(file) => file,
        Err(error) => panic!("Error: {:?}", error),
    };
    let debug_log = tracing_subscriber::fmt::layer().with_writer(std::sync::Arc::new(file));

    tracing_subscriber::registry()
        .with(
            stdout_log
                // Add an `INFO` filter to the stdout logging layer
                .with_filter(filter::LevelFilter::TRACE)
                // Combine the filtered `stdout_log` layer with the
                // `debug_log` layer, producing a new `Layered` layer.
                .and_then(debug_log),
        )
        .init();
}

#[tokio::main]
async fn main() {
    tracing();
    let path = args().nth(1).expect("usage: emulator-rs [config]");
    trace!("reading config file from {path}");
    let opts = read_to_string(path).unwrap();

    let mut de = serde_json::Deserializer::from_str(&opts);
    let opts: Options = serde_path_to_error::deserialize(&mut de).unwrap();
    std::fs::create_dir_all("bmp_emulator").unwrap();
    info!("Starting emulator");
    debug!("{:?}", opts);
    driver::entry(opts).await;

    info!("Emulator finished successfully");
}
