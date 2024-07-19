use std::{
    path::PathBuf,
    ptr::{copy_nonoverlapping, write_bytes},
    sync::{mpsc, Arc},
};

use crossbeam::channel::Sender;
use eyre::ContextCompat;
use futures::{
    future::{join, join_all, BoxFuture, JoinAll},
    FutureExt,
};
use ijson::IString;
use itertools::Itertools;
use rayon::{
    current_num_threads,
    iter::{IntoParallelIterator, ParallelIterator},
    spawn,
};
use sonic_rs::OwnedLazyValue;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, trace, warn};

use crate::{
    js::TaskData,
    opts::{Message, MessageInner, Payload},
    playbook::Module,
    tree::PbTree,
};

pub async fn execute_playbook(pb_tree: PbTree, ingestion: Vec<Message>, tx: Sender<TaskData>) {
    recurse(pb_tree, ingestion, tx, 0).await
}

#[instrument(skip(data, tx, children))]
fn recurse(
    PbTree { module, children }: PbTree,
    data: Vec<Message>,
    tx: Sender<TaskData>,
    count: usize,
) -> BoxFuture<'static, ()> {
    let name = module.name().to_owned();
    trace!(flow_name = name);
    if count > 200 {
        panic!("hit recursion limit");
    }
    use crate::playbook::Module::*;
    async move {
        match module {
            Logic {
                rules,
                routes,
                name,
                input,
            } => {
                debug!("Logic flow {name}");
                let (return_tx, rx) = mpsc::channel();
                let len = data.len();

                // First send the messages to be evaluated by the js engines
                for (idx, data) in data.into_iter().enumerate() {
                    let msg = TaskData {
                        idx,
                        data,
                        module: name.clone(),
                        tx: return_tx.clone(),
                        exported_name: rules
                            .last()
                            .unwrap()
                            .split_once(".js")
                            .unwrap()
                            .0
                            .to_owned(),
                    };
                    tx.send(msg).unwrap();
                }
                trace!("Sent all data");

                // Collect all of the messages from the engines and their routes
                let mut out = (0..len)
                    .map(|i| {
                        let crate::js::RetData { idx, data, .. } = rx.recv().unwrap();
                        let route = data
                            .inner
                            .billingmediation
                            .get("route")
                            .map(|v| v.to_string().clone())
                            .unwrap_or_else(|| String::from("output")); // If no route that means output.
                        (idx, route.clone(), data)
                    })
                    .collect_vec();
                out.sort_unstable_by_key(|x| x.0);
                trace!("received all data");
                let save = save(name.to_owned(), out.iter().map(|x| x.2.clone()).collect());

                // Route accordingly
                let iter = children.into_iter().map(|(module, route)| {
                    let data = out
                        .clone()
                        .into_iter()
                        .filter_map(|(_, m_route, data)| (route == *m_route).then_some(data))
                        .collect_vec();
                    recurse(module, data, tx.clone(), count + 1)
                });
                let exec = join_all(iter);
                _ = futures::future::join(exec, save).await;
            }
            Splitting {
                array_path,
                allow_empty,
                name,
                input,
            } => {
                debug!("Splitting flow {name}");

                let mut path = array_path.split('.').collect_vec();
                let is_bm = path[0] == "billingmediation";

                let data = data
                    .iter()
                    .flat_map(|Message { inner, date }| {
                        if is_bm {
                            split_billingmediation(
                                inner.billingmediation.clone(),
                                &path[1..],
                                inner.payload.clone(),
                                date.clone(),
                            )
                        } else {
                            split_payload(
                                inner.payload.clone(),
                                &path,
                                inner.billingmediation.clone(),
                                date.clone(),
                            )
                        }
                    })
                    .collect_vec();

                route_output(&name, children, data, tx, count).await;
            }
            Lookup { name, input } => todo!(),
            Reporting {
                scheduling,
                format,
                name,
                input,
            } => todo!(),
            Aggregation { key, name, input } => todo!(),
            Deduplication { key, name, input } => todo!(),
            _ => {
                debug!("Ingestion flow {name}");
                route_output(&name, children, data, tx, count).await;
            }
        }
    }
    .boxed()
}

fn save(name: String, data: Vec<Message>) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let data = data.iter().map(|x| x.inner.to_string()).join("\n");
        trace!("beginning save for {name}");
        std::fs::write(PathBuf::from("bmp_emulator").join(&name), data).unwrap();
        trace!("finished writing for {name}");
    })
}

async fn route_output(
    name: &str,
    children: Vec<(PbTree, String)>,
    data: Vec<Message>,
    tx: Sender<TaskData>,
    count: usize,
) {
    let save = save(name.to_owned(), data.clone());
    let iter = children.into_iter().map(|(module, _)| {
        let data = data.clone();
        let tx = tx.clone();
        tokio::spawn(async move { recurse(module.clone(), data, tx, count + 1).await })
    });

    let recursive = join_all(iter);
    trace!("finished recursing");
    let (a, b) = futures::future::join(recursive, save).await;
    for i in a {
        i.unwrap();
    }
    b.unwrap();
}

fn split_billingmediation(
    bm: serde_json::Map<String, serde_json::Value>,
    path: &[&str],
    payload: Payload,
    date: IString,
) -> Vec<Message> {
    let mut base = bm.clone();
    let first = path[0];
    let mut current = base.get_mut(first).unwrap();

    for c in path {
        current = current.get_mut(c).expect("bad array_path");
    }
    let arrays = current
        .as_array()
        .unwrap()
        .iter()
        .map(|x| vec![x.to_owned()])
        .collect_vec();

    let out = arrays.into_iter().map(move |arr| {
        let mut base = bm.clone();
        let mut current = base.get_mut(first).unwrap();
        for c in &path[1..] {
            current = current.get_mut(c).expect("bad array_path");
        }
        *current = arr.into();

        Message {
            inner: MessageInner {
                billingmediation: base,
                payload: payload.clone(), // This is an Arc, feels good ;)
            },
            date: date.clone(),
        }
    });
    out.collect()
}

fn split_payload(
    json: Payload,
    path: &[&str],
    bm: serde_json::Map<String, serde_json::Value>,
    date: IString,
) -> Vec<Message> {
    let ret = json_split(json, path);

    ret.iter()
        .map(|payload| Message {
            inner: MessageInner {
                billingmediation: bm.clone(),
                payload: payload.clone(),
            },
            date: date.clone(),
        })
        .collect()
}

fn json_split(json: Payload, path: &[&str]) -> Vec<Payload> {
    let arr = sonic_rs::get_from_str(&json, path).unwrap_or_else(|e| panic!("{} {}", e, json));
    let arr = arr.as_raw_str();

    let owned_arr: sonic_rs::Array = sonic_rs::from_str(arr).unwrap();
    if owned_arr.is_empty() {
        return vec![];
    }
    if owned_arr.len() == 1 {
        return vec![json.clone()];
    }

    let mut local_copy = String::from(&*json);
    let mut ret: Vec<Payload> = Vec::with_capacity(owned_arr.len());

    for (n, i) in owned_arr.iter().enumerate() {
        unsafe {
            // Construct a slice that goes from [1, 2, 3, 4]
            //                                   ^        ^

            let offset = arr.as_ptr().sub_ptr(json.as_ptr());
            let ptr = local_copy.as_mut_ptr().add(1 + offset);
            let len = arr.len() - 2;
            let internal_thing = std::slice::from_raw_parts_mut(ptr, len);

            let i = i.to_string();
            let source = i.as_bytes();

            let (head, rest) = internal_thing.split_at_mut(source.len());
            head.copy_from_slice(source);
            rest.fill(b' ');
        }

        // Move last string
        if n == owned_arr.len() - 1 {
            ret.push(local_copy.into());
            break; // This appeases the borrowck ;)
        } else {
            ret.push(Arc::from(local_copy.clone()));
        }
    }
    ret
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn split_json_nums() {
        let json = r#" {"foo": { "bar": [0,1,2], "unused": null } }"#;

        for (n, i) in json_split(json.to_owned().into(), &["foo", "bar"])
            .iter()
            .enumerate()
        {
            println!("{n}, {i:?}");
            let json: serde_json::Value = serde_json::from_str(i).unwrap();
            let val = json
                .get("foo")
                .unwrap()
                .get("bar")
                .unwrap()
                .as_array()
                .unwrap()[0]
                .as_number()
                .unwrap()
                .as_u64()
                .unwrap();
            assert_eq!(n, val as usize);
        }
    }

    #[test]
    fn split_json_strs() {
        let json = r#" {"foo": { "bar": ["0","1","2"] } }"#;

        for (n, i) in json_split(json.to_owned().into(), &["foo", "bar"])
            .iter()
            .enumerate()
        {
            let json: serde_json::Value = serde_json::from_str(i).unwrap();
            let val = json
                .get("foo")
                .unwrap()
                .get("bar")
                .unwrap()
                .as_array()
                .unwrap()[0]
                .as_str()
                .unwrap();
            assert_eq!(val, n.to_string())
        }
    }
    #[test]
    fn split_json_objs() {
        let json = r#" {"foo": { "bar": [ {"baz": 0}, {"baz": 1}, {"baz": 2} ] } }"#;

        for (n, i) in json_split(json.to_owned().into(), &["foo", "bar"])
            .iter()
            .enumerate()
        {
            let json: serde_json::Value = serde_json::from_str(i).unwrap();
            let val = json
                .get("foo")
                .unwrap()
                .get("bar")
                .unwrap()
                .as_array()
                .unwrap()[0]
                .get("baz")
                .unwrap()
                .as_number()
                .unwrap()
                .as_u64()
                .unwrap();
            assert_eq!(val as usize, n);
        }
    }
}
