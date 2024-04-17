use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
};

use crossbeam::channel::Sender;
use eyre::ContextCompat;
use futures::{
    future::{join_all, BoxFuture},
    FutureExt,
};
use ijson::IString;
use itertools::Itertools;
use rayon::{
    current_num_threads,
    iter::{IntoParallelIterator, ParallelIterator},
    spawn,
};

use crate::{
    js::TaskData,
    opts::{Message, MessageInner},
    playbook::Module,
    tree::PbTree,
};

pub async fn execute_playbook(pb_tree: PbTree, ingestion: Vec<Arc<Message>>, tx: Sender<TaskData>) {
    println!("EXEC pb");
    recurse(pb_tree, ingestion, tx).await
}

fn recurse(
    PbTree { module, children }: PbTree,
    data: Vec<Arc<Message>>,
    tx: Sender<TaskData>,
) -> BoxFuture<'static, ()> {
    let name = module.name().to_owned();
    println!("{name}");
    use crate::playbook::Module::*;
    async move {
        match module {
            Logic {
                rules,
                routes,
                name,
                input,
            } => {
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

                // Collect all of the messages from the engines and their routes
                let mut out = vec![(IString::new(), Default::default()); len];
                for i in 0..len {
                    let crate::js::RetData { idx, data, .. } = rx.recv().unwrap();
                    let route = data
                        .inner
                        .billingmediation
                        .get("route")
                        .map(|v| v.as_string().unwrap().clone())
                        .unwrap_or_else(|| IString::from("output")); // If no route that means output.
                    out[idx] = (route.clone(), data);
                }

                // Route accordingly
                let iter = children.into_iter().map(|(module, route)| {
                    let data = out
                        .clone()
                        .into_iter()
                        .filter_map(|(m_route, data)| (route == *m_route).then_some(data))
                        .collect_vec();
                    recurse(module, data, tx.clone())
                });
                join_all(iter).await;

                // tokio::spawn(async move {
                //     let data = out.into_iter().map(|x| x.1).collect_vec();
                //     let data = serde_json::to_vec_pretty(&data).unwrap();
                //     tokio::fs::write(PathBuf::from("bmp_emulator").join(&name), data)
                //         .await
                //         .unwrap();
                //     println!("successfully wrote results to disk");
                // })
                // .await;
            }
            Splitting {
                array_path,
                allow_empty,
                name,
                input,
            } => {
                let mut path = array_path.split('.').map(str::to_owned).collect_vec();
                // FIXME: refactor me: something like make all arrays and then append them one
                // by one into the cloned thing. Also remove duplication by returning the target

                let is_bm = if *path[0] == *"billingmediation" {
                    path = path[1..].to_vec();
                    true
                } else {
                    false
                };

                let data = data
                    .into_iter()
                    .flat_map(|msg| {
                        let mut base_1 = if is_bm {
                            msg.inner.billingmediation.clone()
                        } else {
                            msg.inner.payload.clone()
                        };
                        let mut current = base_1.get_mut(path[0].as_str()).unwrap();

                        for c in &path[1..] {
                            let c = c.as_str();
                            current = current.get_mut(c).expect("bad array_path");
                        }
                        let arrays = current
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|x| vec![x.to_owned()])
                            .collect_vec();

                        let path = path.clone();
                        let out = arrays.into_iter().map(move |arr| {
                            let mut base = if is_bm {
                                msg.inner.billingmediation.clone()
                            } else {
                                msg.inner.payload.clone()
                            };

                            let mut current = base.get_mut(path[0].as_str()).unwrap();
                            for c in &path[1..] {
                                let c = c.as_str();
                                current = current.get_mut(c).expect("bad array_path");
                            }
                            *current = arr.into();

                            let inner = if is_bm {
                                MessageInner {
                                    billingmediation: base,
                                    payload: msg.inner.payload.clone(), // This is an Arc, feels good ;)
                                }
                            } else {
                                MessageInner {
                                    billingmediation: msg.inner.billingmediation.clone(),
                                    payload: base, // This is an Arc, feels good ;)
                                }
                            };

                            Arc::from(Message {
                                inner,
                                date: msg.date.clone(),
                            })
                        });
                        out
                    })
                    .inspect(|x| ())
                    .collect_vec();

                route_output(&name, children, data, tx).await;
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
                route_output(&name, children, data, tx).await;
            }
        }
    }
    .boxed()
}

async fn route_output(
    name: &str,
    children: Vec<(PbTree, String)>,
    data: Vec<Arc<Message>>,
    tx: Sender<TaskData>,
) {
    println!("here");
    let name = name.to_owned();
    let iter = children.into_iter().map(|(module, _)| {
        let data = data.clone();
        let tx = tx.clone();
        tokio::spawn(async move { recurse(module.clone(), data, tx).await })
    });

    // let data = data.clone();
    // tokio::spawn(async move {
    //     let data = serde_json::to_vec_pretty(&data).unwrap();
    //     tokio::fs::write(PathBuf::from("bmp_emulator").join(name), data)
    //         .await
    //         .unwrap();
    //     println!("successfully wrote results to disk");
    // })
    // .await
    // .unwrap();
    // println!("finished saving");

    join_all(iter).await.into_iter().for_each(|x| x.unwrap());
    println!("finished recursing");
    // let (a, b) = tokio::join! {
    //     recursive,
    //     save,
    // };
    // for i in a {
    //     i.unwrap();
    //     println!("finished recursing");
    // }
    // b.unwrap();
    // println!("finished saving");
}
