use std::time::Duration;

use hq_git_lib::application::mutation_coordinator::RepositoryMutationCoordinator;

#[tokio::test]
async fn same_repository_mutation_waits_for_existing_read() {
    let directory = tempfile::tempdir().unwrap();
    let coordinator = RepositoryMutationCoordinator::default();
    let read = coordinator.read(directory.path()).await;

    let blocked = tokio::time::timeout(
        Duration::from_millis(25),
        coordinator.write(directory.path()),
    )
    .await;
    assert!(blocked.is_err());

    drop(read);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), coordinator.write(directory.path()),)
            .await
            .is_ok(),
    );
}

#[tokio::test]
async fn different_repositories_do_not_block_each_other() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let coordinator = RepositoryMutationCoordinator::default();
    let _write = coordinator.write(first.path()).await;

    assert!(
        tokio::time::timeout(Duration::from_secs(1), coordinator.write(second.path()),)
            .await
            .is_ok(),
    );
}
