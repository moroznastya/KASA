//! Unit-тести M1: атомарний local_number (N паралельних → N унікальних).
//! 1:1 Python `tests/unit/repositories/test_prro_repository_m1.py`.

mod common;

use torgashka_prro::prro::{InMemoryPrroRepository, PrroRepository, PrroShift};

#[tokio::test]
async fn next_local_number_sequential() {
    let repo = InMemoryPrroRepository::new();
    let shift = PrroShift::new(1, chrono::Utc::now());
    repo.create_shift(shift.clone()).await.unwrap();

    let n1 = repo.next_local_number(shift.id).await.unwrap();
    let n2 = repo.next_local_number(shift.id).await.unwrap();
    let n3 = repo.next_local_number(shift.id).await.unwrap();
    assert_eq!((n1, n2, n3), (1, 2, 3), "послідовна нумерація");
}

/// Критерій M1: N паралельних фіскалізацій → N унікальних послідовних
/// local_number (без дублікатів і без пропусків).
#[tokio::test]
async fn next_local_number_concurrent_unique_sequential() {
    const N: usize = 50;
    let repo = InMemoryPrroRepository::new();
    let shift = PrroShift::new(1, chrono::Utc::now());
    repo.create_shift(shift.clone()).await.unwrap();

    let repo = std::sync::Arc::new(repo);
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..N {
        let repo = std::sync::Arc::clone(&repo);
        let shift_id = shift.id;
        set.spawn(async move { repo.next_local_number(shift_id).await.unwrap() });
    }
    let mut numbers = Vec::with_capacity(N);
    while let Some(res) = set.join_next().await {
        numbers.push(res.expect("задача не впала"));
    }

    assert_eq!(numbers.len(), N);
    let mut sorted = numbers.clone();
    sorted.sort_unstable();
    // унікальні
    sorted.dedup();
    assert_eq!(sorted.len(), N, "N унікальних номерів");
    // послідовні 1..=N
    for (i, n) in sorted.iter().enumerate() {
        assert_eq!(*n, (i + 1) as i64, "послідовні без пропусків");
    }
}
