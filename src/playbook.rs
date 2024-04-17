use std::{
    collections::HashMap,
    fs::{self},
    path::{Path, PathBuf},
    thread::available_parallelism,
};

use futures::future::join_all;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tokio::fs::read_to_string;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookData {
    #[serde(rename = "bmpVersion")]
    pub bmp_version: String,
    pub flow: String,
    pub modules: Vec<Module>,
}

pub struct Playbook {
    pub pb: PlaybookData,
    pub channel_root_path: PathBuf,
}

impl Playbook {
    pub fn new<I: AsRef<Path>>(path: I) -> Self {
        let path = &path.as_ref();
        let root = path.parent().unwrap().parent().unwrap().to_owned();

        dbg!(path);
        let pb = fs::read_to_string(path).unwrap();
        let pb = serde_yaml::from_str(&pb).unwrap();
        Self {
            pb,
            channel_root_path: root,
        }
    }

    pub async fn get_scripts(&self) -> Vec<String> {
        let mut paths: Vec<PathBuf> = vec![];
        let lib_path = self.channel_root_path.join("../../libraries");

        let libs = std::fs::read_dir(self.channel_root_path.join("../../libraries"))
            .unwrap()
            .map(Result::unwrap);

        let logics = std::fs::read_dir(self.channel_root_path.join("logic"))
            .unwrap()
            .map(Result::unwrap);

        let sources = logics.chain(libs).collect_vec();
        for md in &self.pb.modules {
            if let Module::Logic { rules, .. } = md {
                assert!(!rules.is_empty());

                for i in rules {
                    if let Some(file) = sources.iter().find(|x| *x.file_name() == **i) {
                        let path = file.path();
                        if !paths.contains(&path) {
                            paths.push(path.clone());
                        }
                    };
                }
            }
        }
        let iter = paths.into_iter().map(|path| async move {
            tokio::fs::read_to_string(&path)
                .await
                .unwrap_or_else(|e| panic!("source file not found in {path:?}: {e}",))
        });
        join_all(iter).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "format")]
pub enum IngestionSchema {
    #[serde(rename = "CSV")]
    Csv {
        file: String,
        separator: String,
        mapping: HashMap<String, String>, // TODO regexes
    },
    Edifact {
        file: String,
    },
    #[serde(rename = "JSON")]
    Json {
        file: String,
    },
}
impl IngestionSchema {
    pub fn file(&self) -> &str {
        match self {
            IngestionSchema::Csv { file, .. } => &file,
            IngestionSchema::Edifact { file } => &file,
            IngestionSchema::Json { file } => &file,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRecords {
    pub records: IngestionSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "frequency")]
pub enum ReportingFrequency {
    #[serde(rename = "daily")]
    Daily,
    #[serde(rename = "weekly")]
    Weekly,
    #[serde(rename = "monthly")]
    Monthly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Module {
    MessageIngestion {
        schema: IngestionSchema,
        name: String,
    },
    FileIngestion {
        schema: SchemaRecords,
        name: String,
    },
    Splitting {
        #[serde(rename = "arrayPath")]
        array_path: String,
        #[serde(rename = "allowEmpty")]
        allow_empty: bool,
        name: String,
        input: Option<String>,
    },
    Reporting {
        // fields: HashMap<String, String>,
        scheduling: ReportingFrequency,
        format: String,
        name: String,
        input: Option<String>,
    },
    Logic {
        #[serde(default)]
        rules: Vec<String>,
        #[serde(default)]
        routes: Vec<String>,
        name: String,
        input: Option<String>,
    },
    Aggregation {
        key: Vec<String>,
        // sums: HashMap<String, String>,
        name: String,
        input: Option<String>,
    },
    Deduplication {
        // afterLastOccurrence: String,
        // afterFirstOccurrence: String,
        key: Vec<String>,
        name: String,
        input: Option<String>,
    },
    Lookup {
        name: String,
        input: Option<String>,
    },
}

impl Module {
    pub fn name(&self) -> &str {
        match self {
            Module::MessageIngestion { name, .. } => name,
            Module::FileIngestion { name, .. } => name,
            Module::Splitting { name, .. } => name,
            Module::Reporting { name, .. } => name,
            Module::Logic { name, .. } => name,
            Module::Aggregation { name, .. } => name,
            Module::Deduplication { name, .. } => name,
            Module::Lookup { name, .. } => name,
        }
    }
    pub fn input(&self) -> Option<&str> {
        match self {
            Module::MessageIngestion { .. } => None,
            Module::FileIngestion { .. } => None,
            Module::Splitting { input, .. } => input.as_deref(),
            Module::Reporting { input, .. } => input.as_deref(),
            Module::Logic { input, .. } => input.as_deref(),
            Module::Aggregation { input, .. } => input.as_deref(),
            Module::Deduplication { input, .. } => input.as_deref(),
            Module::Lookup { input, .. } => input.as_deref(),
        }
    }
}
