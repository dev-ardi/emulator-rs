//! Right now there are 5 clones per message:
//! String -> js string -> JS object -> js string -> String -> hashmap
//! This does not spark joy

use std::{
    process::exit,
    sync::{mpsc, Arc, Once, OnceLock},
    thread::{self, JoinHandle},
};

use crossbeam::{
    channel::Receiver,
    deque::{Stealer, Worker},
};
use tracing::{debug, instrument, trace, warn};
use v8::{json, HandleScope, Local, Object, OwnedIsolate, Platform};

use crate::opts::{Message, MessageInner};

static START: Once = Once::new();

#[derive(Debug, Clone)]
pub struct Interner<'s> {
    pub inner: Vec<Local<'s, v8::Value>>,
}

pub struct TaskData {
    pub idx: usize,
    pub data: Message,
    pub module: String,
    pub exported_name: String,
    pub tx: mpsc::Sender<RetData>, // ok?
}

#[derive(Debug, Clone)]
pub struct RetData {
    pub idx: usize,
    pub data: Message,
    pub module: String,
}

pub fn worker_pool(scripts: Vec<String>) -> crossbeam::channel::Sender<TaskData> {
    let (tx, rx) = crossbeam::channel::unbounded();
    let threads = num_cpus::get();
    let threads = 16;
    debug!("starting {threads} v8 isolates");

    for _ in 0..threads {
        let scripts = scripts.clone();
        let rx = rx.clone();
        // We detach the thread. It will clean as soon as tx has been dropped
        thread::spawn(move || {
            init_isolate(rx, scripts);
        });
    }
    tx
}

pub fn init_isolate(rx: Receiver<TaskData>, scripts: Vec<String>) {
    START.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });

    // Init isolate
    let isolate = &mut v8::Isolate::new(Default::default());
    let scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Context::new(scope);
    let scope = &mut v8::ContextScope::new(scope, context);

    // Set bmp global object
    let global_object: Local<Object> = context.global(scope);
    let result = run_script(
        scope,
        "
global = this;
this.bmp = {
  exports: (packageName, object) => { global.bmp.modules[packageName] = object; },
  require: (moduleName) => global.bmp.modules[moduleName],
  modules: {},
  version: '',
};
",
    )
    .unwrap();
    set(scope, global_object, result, "bmp");

    // Initialize all dependencies which will write into global.bmp.modules
    for script in scripts {
        // FIXME: How the fuck do you even do this
        let script = format!("{{{script}}}");
        let code = v8::String::new(scope, &script).unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        script.run(scope).unwrap();
    }

    // We've been initialized, we are ready to get tasks
    let modules: Local<'_, v8::Object> = get(scope, result.try_into().unwrap(), "modules")
        .unwrap()
        .try_into()
        .unwrap();

    debug!("worker ready to receive messages");
    while let Ok(data) = rx.recv() {
        handle_message(scope, data, modules);
    }
}

pub fn handle_message(
    scope: &mut v8::HandleScope<'_>,
    TaskData {
        idx,
        data,
        module,
        tx,
        exported_name,
    }: TaskData,
    modules: Local<'_, v8::Object>,
) {
    let scope = &mut v8::HandleScope::new(scope);
    let scope = &mut v8::TryCatch::new(scope);
    let process: Local<'_, v8::Function> = run_script(
        scope,
        &format!("global.bmp.modules.{exported_name}.process"),
    )
    .unwrap_or_else(|| run_script(scope, "()=>{}").unwrap())
    .try_into()
    .unwrap();

    let date = data.date.clone();
    // Serialize into js

    // PERF: check if I can build the object separately and that improves perf
    let data = data.inner.to_string();
    let data = v8::String::new(scope, &data).unwrap();

    let data = v8::json::parse(scope, data).unwrap();
    let out = v8::Object::new(scope);
    let undef = v8::undefined(scope);

    process.call(scope, undef.into(), &[data]).unwrap();

    // Deserialize from js
    let output = v8::json::stringify(scope, data).unwrap();
    let data = output.to_rust_string_lossy(scope);
    let inner = MessageInner::from_with_bm(&data);

    let data = Message { inner, date };
    let ret_data = RetData { idx, data, module };
    warn!("pre-send {idx}");
    tx.send(ret_data).unwrap();
    warn!("send {idx}");
}

pub fn run_script<'s>(
    scope: &mut v8::HandleScope<'s>,
    script: &'_ str,
) -> Option<Local<'s, v8::Value>> {
    let code = v8::String::new(scope, script)?;
    let script = v8::Script::compile(scope, code, None)?; // TODO: cache the script, unbound etc
    script.run(scope)
}

pub fn eval(scope: &mut v8::HandleScope<'_>, script: &'_ str) {
    let code = v8::String::new(scope, script).unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap(); // TODO: cache the script, unbound etc
    let r = script.run(scope).unwrap();
    let s = v8::json::stringify(scope, r)
        .unwrap()
        .to_rust_string_lossy(scope);
    println!("{s}");
}

pub fn get<'s>(
    scope: &mut v8::HandleScope<'s>,
    obj: Local<'s, Object>,
    key: &'_ str,
) -> Option<Local<'s, v8::Value>> {
    let bmp_key = v8::String::new(scope, key)?;
    let key = Local::new(scope, bmp_key);
    obj.get(scope, key.into())
}

pub fn set<'s>(
    scope: &mut v8::HandleScope<'s>,
    obj: Local<'s, Object>,
    other: Local<'s, v8::Value>,
    key: &'_ str,
) -> Option<bool> {
    let bmp_key = v8::String::new(scope, key)?;
    let key = Local::new(scope, bmp_key);
    obj.set(scope, key.into(), other)
}
