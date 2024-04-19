use criterion::{black_box, criterion_group, criterion_main, Criterion};
use emulator_rs::playbook::Playbook;
use emulator_rs::tree::PbTree;

const PLAYBOOK: &str = include_str!("../playbook1.yaml");

fn pb_tree() -> PbTree {
    let pb = Playbook::new(PLAYBOOK);
    PbTree::new(&pb.pb.modules)
}

fn load_bench(c: &mut Criterion) {
    c.bench_function("pb tree", |b| b.iter(|| black_box(pb_tree())));
}

fn js_init(c: &mut Criterion) {
    // c.bench_function("init isolate", |b| b.iter(init_isolate));
}

criterion_group!(benches, js_init, load_bench);
criterion_main!(benches);
