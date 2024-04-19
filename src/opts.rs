use crate::{playbook::Playbook, tree::PbTree};
use ijson::{IObject, IString, IValue};
use serde::{Deserialize, Serialize};
use std::{env, fmt::Display, path::PathBuf, sync::Arc};

use itertools::Itertools;

pub type Payload = Arc<String>; // PERF: Experiment with Arc<String>
pub type JsonObj = serde_json::Map<String, serde_json::Value>;
#[derive(Debug, Clone, Deserialize)]
pub struct MessageInner {
    pub billingmediation: serde_json::Map<String, serde_json::Value>,
    // TODO: pick a more compact repr / compress inputs?
    pub payload: Payload,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageInnerTemp {
    #[serde(default)]
    pub billingmediation: JsonObj,
    #[serde(flatten)]
    pub payload: JsonObj,
}

impl MessageInner {
    // PERF: This could be optimized a lot by removing the "billingmediation" key and replacing it
    // with spaces and just constructing the bm object later on. In fact, the bm could be lazily
    // constructed? In fact whether something is needed could be analyzed upfront.
    pub fn from_with_bm(value: &str) -> Self {
        let tmp: MessageInnerTemp = serde_json::from_str(value).unwrap();
        let payload = serde_json::to_string(&tmp.payload).unwrap();
        Self {
            billingmediation: tmp.billingmediation,
            payload: Arc::from(payload),
        }
    }
}

impl Display for MessageInner {
    // fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    //     let key = r#","billingmediation":"#;
    //     let bm: String = serde_json::to_string(&self.billingmediation).unwrap();
    //     let mut ret = String::with_capacity(bm.len() + self.payload.len() + key.len());
    //     assert_ne!(bm.len(), 0);
    //     ret.push_str(&self.payload[..self.payload.len() - 1]);
    //     ret.push_str(key);
    //     ret.push_str(&bm);
    //     ret.push('}');
    //     f.write_str(&ret)?;
    //     Ok(())
    // }
    // PERF: This is not great :P (but it doesn't use memory for too long)
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut base: JsonObj = serde_json::from_str(&self.payload).unwrap();
        base.insert(
            "billingmediation".into(),
            self.billingmediation.clone().into(),
        );
        let bm: String = serde_json::to_string(&base).unwrap();
        f.write_str(&bm)?;
        Ok(())
    }
}

impl<T: Into<Payload>> From<T> for MessageInner {
    fn from(value: T) -> Self {
        Self {
            billingmediation: Default::default(),
            payload: value.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub inner: MessageInner,
    pub date: IString, // TODO Arc/intern this
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct InputMetadata {
    pub process_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct Input {
    pub path: String,
    pub metadata: Option<InputMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Default)]
pub struct IngestionOpts {
    #[serde(default)]
    pub head: u32,
    #[serde(default)]
    pub tail: u32,
    #[serde(default)]
    pub batch_size: u32,
    pub regex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Options {
    pub playbook_file_path: PathBuf,
    pub input: Vec<Input>,
    pub reports_dir: Option<String>,
    #[serde(default)]
    pub excluded_modules: Vec<String>,
    pub process_date: Option<String>,
    pub json_lookup_table: Option<(String, String)>,
    #[serde(default)]
    pub show_bench: bool,
    #[serde(default)]
    pub ingestion_opts: IngestionOpts,
}
