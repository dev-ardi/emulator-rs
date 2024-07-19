use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::Stdio,
    str::FromStr,
    sync::Arc,
    time::Instant,
};

use eyre::Context;
use futures::stream::FuturesUnordered;
use rayon::{
    iter::{IntoParallelIterator, ParallelBridge, ParallelIterator},
    slice::ParallelSlice,
};

use ijson::{IObject, IString, IValue as Value};
use itertools::Itertools;
use regex::Regex;
use tokio::{
    fs::{self, read_to_string},
    io::AsyncWriteExt,
    process::Command,
};
use tracing::{debug, error, info, instrument, trace, warn};

use crate::{
    driver::Benchmarker,
    opts::{IngestionOpts, Input, Message, MessageInner, Payload},
    playbook::Module,
};

#[instrument]
pub async fn ingest(
    md: &Module,
    opts: IngestionOpts,
    input: Vec<Input>,
    root: Arc<Path>,
) -> Vec<Message> {
    let res = input.into_iter().map(
        |Input {
             path: input,
             metadata,
         }| {
            debug!("starting ingestion from {input}");
            let root = root.clone();
            let metadata = metadata.clone();
            let cachedir = PathBuf::from(".cache");
            std::fs::create_dir_all(&cachedir);
            let regex = opts.regex.as_ref().map(|x| Regex::from_str(x).unwrap());
            trace!("perf: ingestion start");
            async move {
                // PERF: it's already valid utf8
                let res = read_to_string(&input).await.context(input).unwrap();
                trace!("perf: loaded input to string");
                let mut lines = res
                    .trim()
                    .lines()
                    .skip(opts.head as usize)
                    .filter(|line| regex.as_ref().map(|x| x.is_match(line)).unwrap_or(true))
                    .collect_vec();
                lines.truncate(lines.len() - opts.tail as usize);
                let s = lines.join("\n");
                trace!("perf: Applied ingestion options");
                debug!("got {} messages", lines.len());

                let mut h = DefaultHasher::default();
                s.hash(&mut h);
                let hash = h.finish().to_string();
                // PERF: this is probably not necessary but why not use file names or something
                trace!("perf: hash messages");
                let inner: Vec<Payload> =
                    if let Ok(cached) = tokio::fs::read(cachedir.join(&hash)).await {
                        debug!("reading from cache");
                        let cached = unsafe { String::from_utf8_unchecked(cached) };
                        let cached = cached.lines().map(|x| x.to_owned().into()).collect();
                        trace!("perf: split into lines ('deserialize')");
                        cached
                    } else {
                        let file = match md {
                            // TODO: Handle non edifact
                            Module::MessageIngestion { schema, .. } => schema.file(),
                            Module::FileIngestion { schema, .. } => schema.records.file(),
                            _ => panic!(
                                "Expected first module to be MessageIngestion or FileIngestion "
                            ),
                        };
                        debug!("calling anonymization tool");
                        let res = call_anon(&s, &root.join("grammar").join(file)).await;
                        debug!("Anonymization tool output {} lines", res.len());

                        assert_eq!(
                            res.len(),
                            lines.len(),
                            "Line mismatch: anonymization gave \n{res:?}\n\ninput was \n{lines:?}"
                        );

                        let cache = res.iter().join("\n");
                        trace!("perf: create output string");
                        tokio::spawn(async move {
                            tokio::fs::write(cachedir.join(hash), cache).await.unwrap();
                            trace!("perf: wrote to cache");
                        })
                        .await;

                        res
                    };

                let ret = inner
                    .into_iter()
                    .map(|inner| {
                        let date = metadata
                            .clone()
                            .map_or(IString::from(""), |x| x.process_date.into());
                        (inner, date)
                    })
                    .collect_vec();
                trace!("perf: attach dates");
                ret
            }
        },
    );
    let res = futures::future::join_all(res).await;

    let t = Instant::now();
    let res = res
        .into_iter()
        .flatten()
        .map(|(payload, date)| {
            let inner = MessageInner {
                payload,
                billingmediation: Default::default(),
            };

            Message { inner, date }
        })
        .collect();
    // PERF: This should be almost instant.
    trace!("perf: create messages");
    res
}

const ANON_PATH: &str = "deps/anonymization.jar";
async fn call_anon(input: &str, grammar: &Path) -> Vec<Payload> {
    let nul = if cfg!(windows) {
        "NUL"
    } else if cfg!(unix) {
        "/dev/null"
    } else {
        panic!("unsupported system")
    };

    let grammar = grammar.as_os_str().to_string_lossy();
    let grammar = &grammar;
    let args = [
        &format!("-DlogFile={nul}"),
        "-jar",
        ANON_PATH,
        "edidumpjson",
        "--input-file",
        "-",
        "--grammar-file",
        grammar,
    ];
    debug!("{args:?}");
    let mut child = Command::new("java")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .await
        .unwrap();

    let stdout = child.wait_with_output().await.unwrap().stdout;
    trace!("Anonymization tool returned {} bytes", stdout.len());
    stdout
        .trim_ascii()
        .par_split(|x| *x == b'\n')
        .inspect(|x| assert_ne!(x.trim_ascii(), b"{  }", "Line was empty"))
        // Safety: Edifact is guaranteed to be valid ascii
        .map(|x| unsafe { std::str::from_utf8_unchecked(x).to_owned().into() })
        .collect()
}
