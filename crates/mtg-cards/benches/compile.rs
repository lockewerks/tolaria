//! Criterion bench: full-pool compile pass over the cached Scryfall pool.
//! Uses the same offline gate as tests/staples.rs; with no local cache the
//! bench registers nothing and criterion_main still exits cleanly.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_compile(c: &mut Criterion) {
    let Ok(paths) = mtg_data::Paths::resolve() else {
        eprintln!("skipping compile bench: no data paths");
        return;
    };
    let opts = mtg_data::EnsureOptions { offline: true, ..Default::default() };
    let Ok((pool, _)) = mtg_data::ensure_pool(&paths, &opts) else {
        eprintln!("skipping compile bench: no cached card pool");
        return;
    };

    // One compile_pool pass is ~20ms, so a full 100-sample run is wasteful;
    // 10 samples still gives a stable estimate.
    let mut group = c.benchmark_group("compile");
    group.sample_size(10);
    group.bench_function("compile_pool", |b| {
        b.iter(|| black_box(mtg_cards::compile_pool(black_box(&pool))))
    });
    group.finish();
}

criterion_group!(benches, bench_compile);
criterion_main!(benches);
