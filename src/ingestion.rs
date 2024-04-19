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

use crate::{
    driver::Benchmarker,
    opts::{IngestionOpts, Input, Message, MessageInner, Payload},
    playbook::Module,
};

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
            let mut b = Benchmarker::new();

            let root = root.clone();
            let metadata = metadata.clone();
            let cachedir = PathBuf::from(".cache");
            std::fs::create_dir_all(&cachedir);
            let regex = opts.regex.as_ref().map(|x| Regex::from_str(x).unwrap());
            b.bm("start");
            async move {
                let res = read_to_string(&input).await.context(input).unwrap();
                b.bm("load input");
                let mut lines = res
                    .trim()
                    .lines()
                    .skip(opts.head as usize)
                    .filter(|line| regex.as_ref().map(|x| x.is_match(line)).unwrap_or(true))
                    .collect_vec();
                b.bm("regex");
                lines.truncate(lines.len() - opts.tail as usize);
                let s = lines.join("\n");

                // std::fs::write("edifact_rust.edi", s.as_bytes());

                let mut h = DefaultHasher::default();
                s.hash(&mut h);
                let hash = h.finish().to_string();
                b.bm("hash");
                let inner: Vec<Arc<str>> =
                    if let Ok(cached) = tokio::fs::read(cachedir.join(&hash)).await {
                        b.bm("read");
                        let cached = unsafe { String::from_utf8_unchecked(cached) };
                        let cached = cached.lines().map(Into::into).collect();
                        b.bm("deserialize");
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
                        let res = call_anon(&s, &root.join("grammar").join(file)).await;
                        assert_eq!(
                            res.len(),
                            lines.len(),
                            "Line mismatch: anonymization gave \n{res:?}\n\ninput was \n{lines:?}"
                        );
                        if (res.len() != lines.len()) {
                            dbg!("{res:?}");
                        }

                        b.bm("anon");
                        let res1 = res.join("\n");
                        b.bm("create output string");
                        let mut b2 = b.clone();
                        tokio::spawn(async move {
                            tokio::fs::write(cachedir.join(hash), res1).await.unwrap();
                            b2.bm("wrote to cache");
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
                b.bm("attach dates");
                ret
            }
        },
    );
    let res = futures::future::join_all(res).await;

    let t = Instant::now();
    let res = res
        .into_par_iter()
        .flatten()
        .map(|(payload, date)| {
            let inner = MessageInner {
                payload,
                billingmediation: Default::default(),
            };

            Message { inner, date }
        })
        .collect();
    println!("deserialize as Message took {:?}", t.elapsed());
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

    let mut child = Command::new("java")
        .args([
            &format!("-DlogFile={nul}"),
            "-jar",
            ANON_PATH,
            "edidumpjson",
            "--input-file",
            "-",
            "--grammar-file",
        ])
        .arg(grammar.as_os_str())
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

    child
        .wait_with_output()
        .await
        .unwrap()
        .stdout
        .trim_ascii()
        .par_split(|x| *x == b'\n')
        .inspect(|x| assert_ne!(x.trim_ascii(), b"{  }"))
        // Safety: Edifact is guaranteed to be valid ascii
        .map(|x| unsafe { std::str::from_utf8_unchecked(x).into() })
        .collect()
}
