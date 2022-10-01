#[macro_use]
extern crate bencher;

use bencher::Bencher;
use hex_literal::hex;
use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};
use ruffd_types::collections::Rope;

const TEST_STR: &str = "insert me";

// To determine the time spent on required setup, string_clone
// and iter_rng are provided
fn string_clone(bench: &mut Bencher) {
    let insert_str = TEST_STR.chars().collect::<Vec<_>>();
    bench.iter(|| insert_str.clone());
}

fn iter_rng(bench: &mut Bencher) {
    let mut rng = SmallRng::from_seed(hex!(
        "
        DADADADA DADADADA DADADADA DADADADA
        DADADADA DADADADA DADADADA DADADADA
        "
    ));
    let mut next_insert = (0..1000)
        .into_iter()
        .map(|_| rng.next_u64() as usize % 1_000_000)
        .collect::<Vec<_>>()
        .into_iter()
        .cycle();
    bench.iter(|| next_insert.next());
}

fn same_insert(bench: &mut Bencher) {
    let chars = "a".chars().cycle().take(1_000_000).collect::<Vec<_>>();
    let mut doc = Rope::from_document(chars);
    let insert_str = TEST_STR.chars().collect::<Vec<_>>();
    bench.iter(|| doc.insert(insert_str.clone(), 500_000));
}

fn sparse_insert(bench: &mut Bencher) {
    let chars = "a".chars().cycle().take(1_000_000).collect::<Vec<_>>();
    let mut doc = Rope::from_document(chars);
    let insert_str = TEST_STR.chars().collect::<Vec<_>>();
    let mut rng = SmallRng::from_seed(hex!(
        "
        DADADADA DADADADA DADADADA DADADADA
        DADADADA DADADADA DADADADA DADADADA
        "
    ));
    let mut next_insert = (0..1000)
        .into_iter()
        .map(|_| rng.next_u64() as usize % 1_000_000)
        .collect::<Vec<_>>()
        .into_iter()
        .cycle();
    bench.iter(|| doc.insert(insert_str.clone(), next_insert.next().unwrap()));
}

fn same_delete(bench: &mut Bencher) {
    let chars = "a".chars().cycle().take(1_000_000).collect::<Vec<_>>();
    let mut doc = Rope::from_document(chars);
    bench.iter(|| doc.delete(100..100));
}

fn sparse_delete(bench: &mut Bencher) {
    let chars = "a".chars().cycle().take(1_000_000).collect::<Vec<_>>();
    let mut doc = Rope::from_document(chars);
    let mut rng = SmallRng::from_seed(hex!(
        "
        DADADADA DADADADA DADADADA DADADADA
        DADADADA DADADADA DADADADA DADADADA
        "
    ));
    let mut next_insert = (0..1000)
        .into_iter()
        .map(|_| rng.next_u64() as usize % 1_000_000)
        .collect::<Vec<_>>()
        .into_iter()
        .cycle();
    bench.iter(|| {
        let idx = next_insert.next().unwrap();
        doc.delete(idx..idx);
    });
}

benchmark_group!(
    benches,
    string_clone,
    iter_rng,
    same_insert,
    sparse_insert,
    same_delete,
    sparse_delete
);
benchmark_main!(benches);
