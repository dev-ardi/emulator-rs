use crate::{playbook::Playbook, tree::PbTree};
use ijson::{IObject, IString, IValue};
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf, sync::Arc};

use itertools::Itertools;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageInner {
    // PERF: Remove Arc's
    #[serde(default)]
    pub billingmediation: IObject,
    #[serde(flatten)]
    pub payload: IObject,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub inner: MessageInner,
    pub date: IString, // TODO Arc/intern this
}

pub struct Interner {
    pub inner: Vec<MessageInner>,
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
