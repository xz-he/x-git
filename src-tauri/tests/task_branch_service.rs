use hq_git_lib::application::mutation_coordinator::RepositoryMutationCoordinator;
use hq_git_lib::application::task_branch_service::TaskBranchService;
use hq_git_lib::domain::task_branch::{CreateTaskBranchRequest, TaskBranchRunRequest};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use hq_git_lib::infrastructure::task_branch_repository::TaskBranchRepository;
use serde_json::json;
use std::path::Path;

#[path = "support/nested_work.rs"]
mod nested_work;

#[tokio::test]
async fn commit_pick_preserves_nested_residuals_for_both_destinations() {
    for submodule in [false, true] {
        for return_after in [false, true] {
            let d = fixture().await;
            let root = d.path();
            nested_work::prepare(root, submodule).await;
            git(root, &["branch", "-f", "master", "HEAD"]).await;
            let store = tempfile::tempdir().unwrap();
            let s = service(store.path());
            let binding = create(&s, root, "remoteMaster").await;
            let before = nested_work::dirty(root).await;
            stage(root).await;
            std::fs::write(root.join("task.txt"), "unstaged changes\n").unwrap();
            std::fs::write(root.join("local.txt"), "local\n").unwrap();
            let result = run(&s, root, &binding.id, "commit", return_after).await;
            assert!(result.error.is_none(), "{:?}", result.error);
            assert_eq!(
                serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
                "completed"
            );
            assert_eq!(
                git(root, &["branch", "--show-current"]).await,
                if return_after {
                    "dev"
                } else {
                    &binding.target_branch
                }
            );
            assert_eq!(
                git(
                    root,
                    &["show", &format!("{}:task.txt", binding.target_branch)]
                )
                .await,
                "task"
            );
            assert_eq!(
                std::fs::read(root.join("task.txt")).unwrap(),
                if return_after {
                    b"unstaged changes\n".as_slice()
                } else {
                    b"task\n".as_slice()
                }
            );
            if return_after {
                assert_eq!(std::fs::read(root.join("local.txt")).unwrap(), b"local\n");
            } else {
                assert!(!root.join("local.txt").exists());
                assert_eq!(git(root, &["show", "stash@{0}^3:local.txt"]).await, "local");
            }
            assert_eq!(git(root, &["diff", "--cached"]).await, "");
            assert_eq!(git(root, &["stash", "list"]).await.is_empty(), return_after);
            nested_work::assert_preserved(root, &before).await;
            let tip = git(root, &["rev-parse", &binding.target_branch]).await;
            run(&s, root, &binding.id, "pick", return_after).await;
            assert_eq!(git(root, &["rev-parse", &binding.target_branch]).await, tip);
        }
    }
}

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
        .trim()
        .to_owned()
}
async fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-b", "dev"]).await;
    git(d.path(), &["config", "user.name", "Task Test"]).await;
    git(d.path(), &["config", "user.email", "task@example.test"]).await;
    git(d.path(), &["config", "core.autocrlf", "false"]).await;
    std::fs::write(d.path().join("base.txt"), "base\n").unwrap();
    git(d.path(), &["add", "."]).await;
    git(d.path(), &["commit", "-m", "base"]).await;
    git(d.path(), &["branch", "master"]).await;
    git(d.path(), &["remote", "add", "origin", "."]).await;
    d
}
fn service(store: &Path) -> TaskBranchService {
    TaskBranchService::new(
        GitCommandRunner::default(),
        RepositoryMutationCoordinator::default(),
        TaskBranchRepository::new(store.join("task-branches.json")),
    )
}
async fn create(
    service: &TaskBranchService,
    root: &Path,
    mode: &str,
) -> hq_git_lib::domain::task_branch::TaskBranchBinding {
    let request: CreateTaskBranchRequest = serde_json::from_value(json!({"kind":"feature", "ticket":"R20260915000012345", "slug":"full-ticket", "description":"完整需求说明", "mode":mode, "remote":"origin", "sourceBranch":"dev", "expectedHead":git(root, &["rev-parse", "HEAD"]).await})).unwrap();
    let result = service.create(root, request).await;
    assert!(result.error.is_none(), "{:?}", result.error);
    result.bindings.into_iter().next().unwrap()
}
async fn run(
    service: &TaskBranchService,
    root: &Path,
    id: &str,
    action: &str,
    return_after: bool,
) -> hq_git_lib::domain::task_branch::TaskBranchResult {
    let request: TaskBranchRunRequest = serde_json::from_value(json!({"id":id,"action":action,"message":"R20260915000012345 完整需求说明","returnAfterSuccess":return_after,"expectedHead":git(root, &["rev-parse", "HEAD"]).await})).unwrap();
    service.run(root, request).await
}
async fn stage(root: &Path) {
    std::fs::write(root.join("task.txt"), "task\n").unwrap();
    git(root, &["add", "task.txt"]).await;
}

#[tokio::test]
async fn source_uncommitted_work_is_not_restored_on_the_target() {
    let d = fixture().await;
    let root = d.path();
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    let b = create(&s, root, "remoteMaster").await;
    stage(root).await;
    std::fs::write(root.join("task.txt"), "local unfinished work\n").unwrap();
    std::fs::write(root.join("local.txt"), "untracked local\n").unwrap();
    let result = run(&s, root, &b.id, "commit", false).await;
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        git(root, &["status", "--porcelain=v1"]).await,
        "",
        "target must contain only committed work"
    );
    assert_eq!(
        git(root, &["show", "stash@{0}:task.txt"]).await,
        "local unfinished work"
    );
    assert_eq!(
        git(root, &["show", "stash@{0}^3:local.txt"]).await,
        "untracked local"
    );
    assert!(
        result.bindings[0]
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("dev")
    );
    git(root, &["switch", "dev"]).await;
    git(root, &["stash", "apply", "stash@{0}"]).await;
    assert_eq!(
        std::fs::read(root.join("task.txt")).unwrap(),
        b"local unfinished work\n"
    );
    assert_eq!(
        std::fs::read(root.join("local.txt")).unwrap(),
        b"untracked local\n"
    );
}

#[tokio::test]
async fn source_ignored_files_do_not_block_return_or_pollute_the_target() {
    for return_after in [true, false] {
        let d = fixture().await;
        let root = d.path();
        std::fs::write(root.join(".gitignore"), ".agents/\n.tmp/\n").unwrap();
        git(root, &["add", ".gitignore"]).await;
        git(root, &["commit", "-m", "dev ignore rules"]).await;
        std::fs::create_dir(root.join(".agents")).unwrap();
        std::fs::write(root.join(".agents/local.md"), "local ignored work\n").unwrap();
        let store = tempfile::tempdir().unwrap();
        let s = service(store.path());
        let b = create(&s, root, "remoteMaster").await;
        stage(root).await;
        std::fs::write(root.join("task.txt"), "local tracked work\n").unwrap();
        let result = run(&s, root, &b.id, "commit", return_after).await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(
            git(root, &["branch", "--show-current"]).await,
            if return_after {
                "dev"
            } else {
                &b.target_branch
            }
        );
        assert_eq!(
            git(root, &["show", &format!("{}:task.txt", b.target_branch)]).await,
            "task"
        );
        if return_after {
            assert_eq!(
                std::fs::read(root.join(".agents/local.md")).unwrap(),
                b"local ignored work\n"
            );
            assert_eq!(
                std::fs::read(root.join("task.txt")).unwrap(),
                b"local tracked work\n"
            );
            assert_eq!(git(root, &["stash", "list"]).await, "");
        } else {
            assert_eq!(git(root, &["status", "--porcelain=v1"]).await, "");
            assert!(!root.join(".agents/local.md").exists());
            assert_eq!(
                git(root, &["stash", "list", "--format=%H"])
                    .await
                    .lines()
                    .count(),
                2
            );
            git(root, &["switch", "dev"]).await;
            git(root, &["stash", "apply", "stash@{0}"]).await;
            git(root, &["stash", "apply", "stash@{1}"]).await;
            assert_eq!(
                std::fs::read(root.join(".agents/local.md")).unwrap(),
                b"local ignored work\n"
            );
            assert_eq!(
                std::fs::read(root.join("task.txt")).unwrap(),
                b"local tracked work\n"
            );
        }
    }
}

#[tokio::test]
async fn unlink_persists_and_preserves_git_state_and_other_bindings() {
    let d = fixture().await;
    let other = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    let b = create(&s, d.path(), "remoteMaster").await;
    let other_binding = create(&s, other.path(), "remoteMaster").await;
    let repository = TaskBranchRepository::new(store.path().join("task-branches.json"));
    let mut sibling = b.clone();
    sibling.id = "sibling".into();
    repository.save(&sibling).unwrap();
    // The same id in another repository must not be removed.
    let mut same_id = other_binding.clone();
    same_id.id = b.id.clone();
    repository.save(&same_id).unwrap();
    stage(d.path()).await;
    std::fs::write(d.path().join("task.txt"), "unstaged changes\n").unwrap();
    std::fs::write(d.path().join("local.txt"), "untracked\n").unwrap();
    let refs = git(d.path(), &["show-ref"]).await;
    let status = git(d.path(), &["status", "--porcelain=v1"]).await;
    let index = git(d.path(), &["write-tree"]).await;
    let remaining = s.unlink(d.path(), &b.id).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, sibling.id);
    assert_eq!(git(d.path(), &["branch", "--show-current"]).await, "dev");
    assert_eq!(git(d.path(), &["show-ref"]).await, refs);
    assert_eq!(git(d.path(), &["status", "--porcelain=v1"]).await, status);
    assert_eq!(git(d.path(), &["write-tree"]).await, index);
    assert_eq!(
        std::fs::read_to_string(d.path().join("task.txt")).unwrap(),
        "unstaged changes\n"
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("local.txt")).unwrap(),
        "untracked\n"
    );
    let restarted = service(store.path());
    assert_eq!(
        restarted.snapshot(d.path()).await.unwrap()[0].id,
        sibling.id
    );
    assert_eq!(restarted.snapshot(other.path()).await.unwrap().len(), 2);
    assert_eq!(restarted.unlink(d.path(), &b.id).await.unwrap().len(), 1);
    assert!(
        restarted
            .unlink(d.path(), &sibling.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        service(store.path())
            .snapshot(d.path())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn current_creation_keeps_full_ticket_and_dirty_files() {
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    std::fs::write(d.path().join("local.txt"), "local").unwrap();
    let b = create(&s, d.path(), "current").await;
    assert_eq!(b.target_branch, "feature/R20260915000012345-full-ticket");
    assert_eq!(
        git(d.path(), &["branch", "--show-current"]).await,
        b.target_branch
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("local.txt")).unwrap(),
        "local"
    );
    assert_eq!(
        service(store.path())
            .snapshot(d.path())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn remote_master_fetches_latest_and_stays_on_source() {
    let d = fixture().await;
    let remote = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    git(
        d.path(),
        &[
            "remote",
            "set-url",
            "origin",
            remote.path().to_str().unwrap(),
        ],
    )
    .await;
    git(d.path(), &["fetch", "origin"]).await;
    git(remote.path(), &["switch", "master"]).await;
    std::fs::write(remote.path().join("new.txt"), "new").unwrap();
    git(remote.path(), &["add", "."]).await;
    git(remote.path(), &["commit", "-m", "new remote master"]).await;
    let b = create(&s, d.path(), "remoteMaster").await;
    assert_eq!(
        git(d.path(), &["rev-parse", &b.target_branch]).await,
        git(remote.path(), &["rev-parse", "master"]).await
    );
    assert_eq!(git(d.path(), &["branch", "--show-current"]).await, "dev");
}

#[tokio::test]
async fn commit_pick_supports_both_destinations_and_retry_is_idempotent() {
    for return_after in [true, false] {
        let d = fixture().await;
        let store = tempfile::tempdir().unwrap();
        let s = service(store.path());
        let b = create(&s, d.path(), "remoteMaster").await;
        stage(d.path()).await;
        let expected = git(d.path(), &["rev-parse", "HEAD"]).await;
        let req: TaskBranchRunRequest=serde_json::from_value(json!({"id":b.id,"action":"commit","message":"任务提交","returnAfterSuccess":return_after,"expectedHead":expected})).unwrap();
        let result = s.run(d.path(), req.clone()).await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(
            serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
            "completed"
        );
        assert_eq!(
            git(d.path(), &["branch", "--show-current"]).await,
            if return_after {
                "dev"
            } else {
                &b.target_branch
            }
        );
        let source = git(d.path(), &["rev-parse", "dev"]).await;
        let target = git(d.path(), &["rev-parse", &b.target_branch]).await;
        s.run(d.path(), req).await;
        assert_eq!(git(d.path(), &["rev-parse", "dev"]).await, source);
        assert_eq!(
            git(d.path(), &["rev-parse", &b.target_branch]).await,
            target
        );
    }
}

#[tokio::test]
async fn unstaged_work_is_excluded_and_restored_for_both_destinations() {
    for return_after in [true, false] {
        let d = fixture().await;
        let store = tempfile::tempdir().unwrap();
        let s = service(store.path());
        let b = create(&s, d.path(), "remoteMaster").await;
        stage(d.path()).await;
        std::fs::write(d.path().join("task.txt"), "unstaged changes\n").unwrap();
        std::fs::write(d.path().join("local.txt"), "local").unwrap();
        let result = run(&s, d.path(), &b.id, "commit", return_after).await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(
            serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
            "completed"
        );
        let oid = result.bindings[0].source_commit.clone().unwrap();
        assert_eq!(git(d.path(), &["show", "HEAD:task.txt"]).await, "task");
        assert_eq!(
            std::fs::read_to_string(d.path().join("task.txt")).unwrap(),
            if return_after {
                "unstaged changes\n"
            } else {
                "task\n"
            }
        );
        assert_eq!(oid.len(), 40);
        assert_eq!(
            git(d.path(), &["branch", "--show-current"]).await,
            if return_after {
                "dev"
            } else {
                &b.target_branch
            }
        );
        if return_after {
            assert_eq!(
                std::fs::read_to_string(d.path().join("local.txt")).unwrap(),
                "local"
            );
        } else {
            assert!(!d.path().join("local.txt").exists());
            assert_eq!(
                git(d.path(), &["show", "stash@{0}^3:local.txt"]).await,
                "local"
            );
        }
        assert_eq!(git(d.path(), &["diff", "--cached"]).await, "");
        assert_eq!(
            git(d.path(), &["stash", "list"]).await.is_empty(),
            return_after
        );
        assert_eq!(
            git(
                d.path(),
                &["show", &format!("{}:task.txt", b.target_branch)]
            )
            .await,
            "task"
        );
        assert_eq!(
            service(store.path()).snapshot(d.path()).await.unwrap()[0]
                .source_commit
                .as_deref(),
            Some(oid.as_str())
        );
    }
}

#[tokio::test]
async fn conflict_retains_unstaged_backup_across_restart_and_abort() {
    let d = fixture().await;
    let root = d.path();
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    git(root, &["switch", "master"]).await;
    std::fs::write(root.join("base.txt"), "master\n").unwrap();
    git(root, &["add", "."]).await;
    git(root, &["commit", "-m", "master edit"]).await;
    git(root, &["switch", "dev"]).await;
    let b = create(&s, root, "remoteMaster").await;
    std::fs::write(root.join("base.txt"), "staged dev\n").unwrap();
    git(root, &["add", "base.txt"]).await;
    std::fs::write(root.join("base.txt"), "unstaged local\n").unwrap();
    std::fs::write(root.join("local.txt"), "untracked\n").unwrap();
    let result = run(&s, root, &b.id, "commit", true).await;
    assert!(result.error.unwrap().message.contains("自动贮藏备份"));
    assert_eq!(
        serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
        "conflict"
    );
    assert_eq!(git(root, &["show", "dev:base.txt"]).await, "staged dev");
    assert_eq!(
        git(root, &["show", "stash@{0}:base.txt"]).await,
        "unstaged local"
    );
    assert_eq!(
        git(root, &["show", "stash@{0}^3:local.txt"]).await,
        "untracked"
    );
    let saved = service(store.path()).snapshot(root).await.unwrap();
    assert!(saved[0].message.as_ref().unwrap().contains("自动贮藏备份"));
    git(root, &["cherry-pick", "--abort"]).await;
    git(root, &["switch", "dev"]).await;
    git(root, &["stash", "apply", "stash@{0}"]).await;
    assert_eq!(
        std::fs::read_to_string(root.join("base.txt")).unwrap(),
        "unstaged local\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("local.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(git(root, &["diff", "--cached"]).await, "");
}

#[tokio::test]
async fn conflict_stays_on_target_and_abort_reconciles_without_mutation() {
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    git(d.path(), &["switch", "master"]).await;
    std::fs::write(d.path().join("base.txt"), "master\n").unwrap();
    git(d.path(), &["add", "."]).await;
    git(d.path(), &["commit", "-m", "master edit"]).await;
    git(d.path(), &["switch", "dev"]).await;
    let b = create(&s, d.path(), "remoteMaster").await;
    std::fs::write(d.path().join("base.txt"), "dev\n").unwrap();
    git(d.path(), &["add", "."]).await;
    let result = run(&s, d.path(), &b.id, "commit", true).await;
    assert_eq!(
        serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
        "conflict"
    );
    assert_eq!(
        git(d.path(), &["branch", "--show-current"]).await,
        b.target_branch
    );
    git(d.path(), &["cherry-pick", "--abort"]).await;
    let result = run(&service(store.path()), d.path(), &b.id, "reconcile", true).await;
    assert_eq!(
        serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
        "pendingPick"
    );
    assert_eq!(
        git(d.path(), &["branch", "--show-current"]).await,
        b.target_branch
    );
}

#[tokio::test]
async fn invalid_ticket_and_missing_master_do_not_create() {
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    for (ticket, description, expected) in [
        ("R12x", "说明", git(d.path(), &["rev-parse", "HEAD"]).await),
        ("R12", "", git(d.path(), &["rev-parse", "HEAD"]).await),
        ("R12", "说明", "0000000".into()),
    ] {
        let req=serde_json::from_value(json!({"kind":"feature","ticket":ticket,"slug":"task","description":description,"mode":"current","remote":null,"sourceBranch":"dev","expectedHead":expected})).unwrap();
        assert!(s.create(d.path(), req).await.error.is_some());
    }
    git(
        d.path(),
        &["update-ref", "refs/remotes/origin/master", "HEAD"],
    )
    .await;
    git(d.path(), &["branch", "-D", "master"]).await;
    let req=serde_json::from_value(json!({"kind":"hotfix","ticket":"B1","slug":"task","description":"修复问题","mode":"remoteMaster","remote":"origin","sourceBranch":"dev","expectedHead":git(d.path(), &["rev-parse","HEAD"]).await})).unwrap();
    assert!(s.create(d.path(), req).await.error.is_some());
    assert!(
        git(d.path(), &["branch", "--list", "hotfix/B1-task"])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn conflict_continue_recovers_pick_and_failed_return_never_repeats_it() {
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let worktree = tempfile::tempdir().unwrap();
    let s = service(store.path());
    git(d.path(), &["switch", "master"]).await;
    std::fs::write(d.path().join("base.txt"), "master\n").unwrap();
    git(d.path(), &["add", "."]).await;
    git(d.path(), &["commit", "-m", "master edit"]).await;
    git(d.path(), &["switch", "dev"]).await;
    let b = create(&s, d.path(), "remoteMaster").await;
    std::fs::write(d.path().join("base.txt"), "dev\n").unwrap();
    git(d.path(), &["add", "."]).await;
    run(&s, d.path(), &b.id, "commit", true).await;
    std::fs::write(d.path().join("base.txt"), "resolved\n").unwrap();
    git(d.path(), &["add", "."]).await;
    git(
        d.path(),
        &["-c", "core.editor=true", "cherry-pick", "--continue"],
    )
    .await;
    let result = run(&s, d.path(), &b.id, "reconcile", true).await;
    assert_eq!(
        serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
        "pendingReturn"
    );
    let picked = git(d.path(), &["rev-parse", "HEAD"]).await;
    let checkout = worktree.path().join("checkout");
    git(
        d.path(),
        &["worktree", "add", checkout.to_str().unwrap(), "dev"],
    )
    .await;
    assert!(
        run(&s, d.path(), &b.id, "return", true)
            .await
            .error
            .is_some()
    );
    assert_eq!(git(d.path(), &["rev-parse", "HEAD"]).await, picked);
    git(
        d.path(),
        &["worktree", "remove", checkout.to_str().unwrap()],
    )
    .await;
    let result = run(&service(store.path()), d.path(), &b.id, "return", true).await;
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(
        serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
        "completed"
    );
    assert_eq!(
        git(d.path(), &["rev-parse", &b.target_branch]).await,
        picked
    );
}

#[tokio::test]
async fn persisted_commit_intent_recovers_without_a_second_commit() {
    use hq_git_lib::domain::task_branch::TaskBranchPhase;
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    let b = create(&s, d.path(), "remoteMaster").await;
    stage(d.path()).await;
    std::fs::write(d.path().join("local.txt"), "local").unwrap();
    let result = run(&s, d.path(), &b.id, "commit", true).await;
    let mut intent = result.bindings[0].clone();
    let oid = intent.source_commit.take().unwrap();
    intent.phase = TaskBranchPhase::Committing;
    TaskBranchRepository::new(store.path().join("task-branches.json"))
        .save(&intent)
        .unwrap();
    let result = run(&service(store.path()), d.path(), &b.id, "reconcile", true).await;
    assert_eq!(
        result.bindings[0].source_commit.as_deref(),
        Some(oid.as_str())
    );
    assert_eq!(
        serde_json::to_value(&result.bindings[0]).unwrap()["phase"],
        "pendingPick"
    );
    assert_eq!(git(d.path(), &["rev-parse", "HEAD"]).await, oid);
}

#[tokio::test]
async fn independent_repositories_keep_separate_bindings() {
    let a = fixture().await;
    let b = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    let first = create(&s, a.path(), "remoteMaster").await;
    let second = create(&s, b.path(), "remoteMaster").await;
    assert_ne!(first.id, second.id);
    assert_eq!(s.snapshot(a.path()).await.unwrap()[0].id, first.id);
    assert_eq!(s.snapshot(b.path()).await.unwrap()[0].id, second.id);
}

#[tokio::test]
async fn failed_fetch_can_retry_the_same_creation_without_duplicate_binding() {
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    git(
        d.path(),
        &["remote", "set-url", "origin", "./missing-remote"],
    )
    .await;
    let req: CreateTaskBranchRequest=serde_json::from_value(json!({"kind":"hotfix","ticket":"B12345678901234567890","slug":"fix-task","description":"修复问题","mode":"remoteMaster","remote":"origin","sourceBranch":"dev","expectedHead":git(d.path(), &["rev-parse","HEAD"]).await})).unwrap();
    let failed = s.create(d.path(), req.clone()).await;
    assert!(failed.error.is_some());
    assert_eq!(failed.bindings.len(), 1);
    git(d.path(), &["remote", "set-url", "origin", "."]).await;
    let retried = service(store.path()).create(d.path(), req).await;
    assert!(retried.error.is_none(), "{:?}", retried.error);
    assert_eq!(retried.bindings.len(), 1);
    assert_eq!(retried.bindings[0].id, failed.bindings[0].id);
    assert_eq!(
        git(d.path(), &["rev-parse", &retried.bindings[0].target_branch]).await,
        git(d.path(), &["rev-parse", "master"]).await
    );
}

#[tokio::test]
async fn pending_pick_rejects_target_changes_after_the_source_commit() {
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    let b = create(&s, d.path(), "remoteMaster").await;
    let linked = tempfile::tempdir().unwrap();
    git(
        d.path(),
        &[
            "worktree",
            "add",
            linked.path().to_str().unwrap(),
            &b.target_branch,
        ],
    )
    .await;
    stage(d.path()).await;
    std::fs::write(d.path().join("local.txt"), "local").unwrap();
    let result = run(&s, d.path(), &b.id, "commit", true).await;
    assert!(
        result.error.is_some(),
        "Target is checked out in another worktree"
    );
    let source = result.bindings[0].source_commit.clone().unwrap();
    git(linked.path(), &["switch", "--detach"]).await;
    let reconciled = run(&s, d.path(), &b.id, "reconcile", true).await;
    assert_eq!(
        reconciled.bindings[0].phase,
        hq_git_lib::domain::task_branch::TaskBranchPhase::PendingPick
    );
    std::fs::remove_file(d.path().join("local.txt")).unwrap();
    git(d.path(), &["switch", &b.target_branch]).await;
    std::fs::write(d.path().join("external.txt"), "external").unwrap();
    git(d.path(), &["add", "."]).await;
    git(d.path(), &["commit", "-m", "external target commit"]).await;
    let target = git(d.path(), &["rev-parse", "HEAD"]).await;
    git(d.path(), &["switch", "dev"]).await;
    let result = run(&service(store.path()), d.path(), &b.id, "pick", true).await;
    assert!(
        result.error.is_some(),
        "Must reject a target changed since commit intent"
    );
    assert_eq!(git(d.path(), &["rev-parse", "dev"]).await, source);
    assert_eq!(
        git(d.path(), &["rev-parse", &b.target_branch]).await,
        target
    );
    assert_eq!(git(d.path(), &["branch", "--show-current"]).await, "dev");
}

#[tokio::test]
async fn reconcile_return_does_not_complete_after_target_was_reset() {
    use hq_git_lib::domain::task_branch::TaskBranchPhase;
    let d = fixture().await;
    let store = tempfile::tempdir().unwrap();
    let s = service(store.path());
    let b = create(&s, d.path(), "remoteMaster").await;
    stage(d.path()).await;
    let result = run(&s, d.path(), &b.id, "commit", false).await;
    assert!(result.error.is_none());
    let mut intent = result.bindings[0].clone();
    intent.phase = TaskBranchPhase::PendingReturn;
    intent.return_after_success = true;
    TaskBranchRepository::new(store.path().join("task-branches.json"))
        .save(&intent)
        .unwrap();
    git(d.path(), &["switch", "dev"]).await;
    git(d.path(), &["branch", "-f", &b.target_branch, "master"]).await;
    let result = run(&service(store.path()), d.path(), &b.id, "reconcile", true).await;
    assert_eq!(result.bindings[0].phase, TaskBranchPhase::NeedsAttention);
    git(d.path(), &["branch", "-D", &b.target_branch]).await;
    let result = run(&service(store.path()), d.path(), &b.id, "reconcile", true).await;
    assert_eq!(result.bindings[0].phase, TaskBranchPhase::NeedsAttention);
}
