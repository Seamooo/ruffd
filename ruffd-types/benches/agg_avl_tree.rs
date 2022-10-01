#[macro_use]
extern crate bencher;

use bencher::Bencher;
use hex_literal::hex;
use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};
use ruffd_types::collections::AggAvlTree;

const SIZE: usize = 1_000_000;

fn create_sparse_iterator(size: usize) -> impl Iterator<Item = usize> {
    let mut rng = SmallRng::from_seed(hex!(
        "
        DADADADA DADADADA DADADADA DADADADA
        DADADADA DADADADA DADADADA DADADADA
        "
    ));
    (0..1000)
        .into_iter()
        .map(|_| rng.next_u64() as usize % size)
        .collect::<Vec<_>>()
        .into_iter()
        .cycle()
}

// To determine the time spent on required setup, iter_rng is provided

fn iter_rng(bench: &mut Bencher) {
    let mut next_insert = create_sparse_iterator(SIZE);
    bench.iter(|| next_insert.next());
}

fn accumulate_add(a: &u64, b: &u64) -> u64 {
    *a + *b
}

fn accumulate_max(a: &u64, b: &u64) -> u64 {
    *a.max(b)
}

fn same_insert_add(bench: &mut Bencher) {
    let mut tree = AggAvlTree::from_vec(
        [1u64].into_iter().cycle().take(SIZE).collect::<Vec<_>>(),
        accumulate_add,
    );
    bench.iter(|| tree.insert(500_000, 1));
}

fn sparse_insert_add(bench: &mut Bencher) {
    let mut tree = AggAvlTree::from_vec(
        [1u64].into_iter().cycle().take(SIZE).collect::<Vec<_>>(),
        accumulate_add,
    );
    let mut next_insert = create_sparse_iterator(SIZE);
    bench.iter(|| tree.insert(next_insert.next().unwrap(), 1));
}

fn same_insert_max(bench: &mut Bencher) {
    let mut tree = AggAvlTree::from_vec(
        [1u64].into_iter().cycle().take(SIZE).collect::<Vec<_>>(),
        accumulate_max,
    );
    bench.iter(|| tree.insert(500_000, 1));
}

fn sparse_insert_max(bench: &mut Bencher) {
    let mut tree = AggAvlTree::from_vec(
        [1u64].into_iter().cycle().take(SIZE).collect::<Vec<_>>(),
        accumulate_max,
    );
    let mut next_insert = create_sparse_iterator(SIZE);
    bench.iter(|| tree.insert(next_insert.next().unwrap(), 1));
}

benchmark_group!(
    benches,
    iter_rng,
    same_insert_add,
    sparse_insert_add,
    same_insert_max,
    sparse_insert_max,
);
benchmark_main!(benches);
