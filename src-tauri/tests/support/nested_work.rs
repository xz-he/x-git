use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use std::path::Path;

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
        .trim()
        .to_owned()
}

pub async fn prepare(root: &Path, submodule: bool) {
    let nested = root.join("嵌套 repo");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-b", "main"]).await;
    git(&nested, &["config", "user.name", "Nested test"]).await;
    git(&nested, &["config", "user.email", "nested@example.test"]).await;
    git(&nested, &["config", "core.autocrlf", "false"]).await;
    std::fs::write(nested.join("nested.txt"), "base\n").unwrap();
    git(&nested, &["add", "nested.txt"]).await;
    git(&nested, &["commit", "-m", "nested base"]).await;
    if submodule {
        let oid = git(&nested, &["rev-parse", "HEAD"]).await;
        git(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{oid},嵌套 repo"),
            ],
        )
        .await;
        std::fs::write(
            root.join(".gitmodules"),
            "[submodule \"nested\"]\n\tpath = 嵌套 repo\n\turl = ./nested-source\n",
        )
        .unwrap();
        git(root, &["add", ".gitmodules"]).await;
        git(root, &["commit", "-m", "track submodule"]).await;
    }
    git(root, &["config", "submodule.recurse", "true"]).await;
}

pub async fn dirty(root: &Path) -> (String, String) {
    let nested = root.join("嵌套 repo");
    std::fs::write(nested.join("nested.txt"), "staged nested\n").unwrap();
    git(&nested, &["add", "nested.txt"]).await;
    std::fs::write(nested.join("nested.txt"), "unstaged nested\n").unwrap();
    std::fs::write(nested.join("local.bin"), [0, 255, 128]).unwrap();
    (
        git(&nested, &["rev-parse", "HEAD"]).await,
        git(&nested, &["write-tree"]).await,
    )
}

pub async fn assert_preserved(root: &Path, before: &(String, String)) {
    let nested = root.join("嵌套 repo");
    assert_eq!(git(&nested, &["rev-parse", "HEAD"]).await, before.0);
    assert_eq!(git(&nested, &["write-tree"]).await, before.1);
    assert_eq!(
        std::fs::read(nested.join("nested.txt")).unwrap(),
        b"unstaged nested\n"
    );
    assert_eq!(
        std::fs::read(nested.join("local.bin")).unwrap(),
        [0, 255, 128]
    );
}
