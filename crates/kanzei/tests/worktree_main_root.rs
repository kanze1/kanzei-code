//! R-182 内容② 端到端:worktree 里跑 `kz`,登记必须落**主根**。
//!
//! 实测背景(D-267):两棵 worktree 相隔 10 秒各跑一次 `kz defect add`,**都拿到 D-267**。
//! 原因是 `.kanzei/project/*.md` 被 git 跟踪,`git worktree add` 把它们 checkout 成
//! 分支副本;`kz` 从 cwd 发现的是 worktree 自己那份副本,两条线各自在副本上算 next_id。
//! 本文件把「重跑一次实测」这个人工动作换成机械判据。
//!
//! 走的是**免 LLM** 的 tracker 直通路径(`kz defect add`),所以不需要 mock SSE。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// sha256(纯实现,零新依赖:crates/kanzei 的 Cargo.toml 不在本批文件面里)
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for (index, slot) in w.iter_mut().enumerate().take(16) {
            let base = index * 4;
            *slot = u32::from_be_bytes([
                block[base],
                block[base + 1],
                block[base + 2],
                block[base + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[test]
fn sha256_matches_known_vectors() {
    // 自校验:哈希函数本身写错的话,"副本零改动"就成了永远为真的空断言。
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kanzei-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "kanzei-test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "kanzei-test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} 起不来: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} 失败\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 真 `git init` 的主根:`.kanzei/project/defects.md` 被跟踪并提交,
/// 这样 `git worktree add` 才会把它 checkout 成**分支副本**(D-267 的前提)。
fn main_repo(tag: &str) -> PathBuf {
    let root = unique_dir(tag);
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".kanzei").join("project")).unwrap();
    std::fs::write(
        repo.join(".kanzei").join("project").join("defects.md"),
        "# Defects\n",
    )
    .unwrap();
    git(&repo, &["init", "--initial-branch=main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "fixture"]);
    repo
}

fn add_worktree(repo: &Path, name: &str) -> PathBuf {
    let path = repo.parent().unwrap().join(name);
    git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            &format!("kanzei/test-{name}"),
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    path
}

fn defects_path(root: &Path) -> PathBuf {
    root.join(".kanzei").join("project").join("defects.md")
}

/// 在 `cwd` 里跑 `kz defect add <title>`,`KANZEI_PROJECT_ROOT` 显式指向主根。
/// 返回分配到的编号(stdout 形如 `added D-001 [open] <title>`)。
fn spawn_defect_add(cwd: &Path, main_root: &Path, title: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_kz"))
        .args(["defect", "add", title])
        .current_dir(cwd)
        .env("KANZEI_PROJECT_ROOT", main_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kz 起不来")
}

fn wait_for_id(child: std::process::Child) -> String {
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "kz defect add 失败\nstdout={stdout}\nstderr={stderr}"
    );
    stdout
        .split_whitespace()
        .find(|word| word.starts_with("D-"))
        .unwrap_or_else(|| panic!("stdout 里没有编号: {stdout}"))
        .to_string()
}

fn entry_ids(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// R-182 验收①:worktree 里登记,条目落主根,worktree 里的副本逐字节不动
// ---------------------------------------------------------------------------

#[test]
fn 跨worktree登记落主根且副本零改动() {
    let repo = main_repo("r182-e2e-main-root");
    let worktree = add_worktree(&repo, "wt-a");

    let copy = defects_path(&worktree);
    assert!(
        copy.is_file(),
        "前提不成立:worktree 里必须有被 checkout 出来的 .kanzei 副本"
    );
    let copy_before = sha256_hex(&std::fs::read(&copy).unwrap());

    // 对照:不设 KANZEI_PROJECT_ROOT 时,发现式取根拿到的就是 worktree 自己
    //(这正是 D-267 的成因,这里只作为前提陈述,不改它)。
    let id = wait_for_id(spawn_defect_add(&worktree, &repo, "worktree 里登记的缺陷"));

    // 条目落在**主根**。
    let main_ids = entry_ids(&defects_path(&repo));
    assert_eq!(main_ids, vec![id.clone()], "条目必须落主根 defects.md");
    assert!(
        std::fs::read_to_string(defects_path(&repo))
            .unwrap()
            .contains("worktree 里登记的缺陷"),
        "主根 defects.md 里应能读到标题"
    );

    // worktree 里的副本一个字节都不许动。
    let copy_after = sha256_hex(&std::fs::read(&copy).unwrap());
    assert_eq!(
        copy_before, copy_after,
        "worktree 副本被改写了({} → {})",
        copy_before, copy_after
    );
    assert!(entry_ids(&copy).is_empty(), "副本里不该出现任何条目");

    std::fs::remove_dir_all(repo.parent().unwrap()).ok();
}

// ---------------------------------------------------------------------------
// R-182 验收②:两个**真 OS 进程**跨树并发登记,编号必须互异
// ---------------------------------------------------------------------------

/// D-268:进程内多线程冒充并发已经被记成缺陷——这里必须 spawn 真进程。
///
/// 与规格文字的一处出入(如实记):规格写「每轮新建临时仓」+「断言 10 个编号互异」。
/// 每轮换仓的话编号每轮都从 D-001 重新开始,10 个编号必然重复,两句话不能同时成立。
/// 这里取**同一个仓跑 5 轮**——它才是「10 个编号互异 + 主根条目数 == 10」的可判定形态,
/// 也才是实测①(两棵树各拿到 D-267)的直接对照。
#[test]
fn 两个独立os进程跨树并发登记编号互异() {
    let repo = main_repo("r182-e2e-concurrent");
    let tree_a = add_worktree(&repo, "wt-a");
    let tree_b = add_worktree(&repo, "wt-b");

    let mut ids: Vec<String> = Vec::new();
    for round in 0..5 {
        let a = spawn_defect_add(&tree_a, &repo, &format!("A 轮{round}"));
        let b = spawn_defect_add(&tree_b, &repo, &format!("B 轮{round}"));
        ids.push(wait_for_id(a));
        ids.push(wait_for_id(b));
    }

    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        10,
        "10 次跨树登记必须拿到 10 个互异编号,实际: {ids:?}"
    );

    let main_ids = entry_ids(&defects_path(&repo));
    assert_eq!(
        main_ids.len(),
        10,
        "主根 defects.md 必须恰好 10 条,实际: {main_ids:?}"
    );
    let mut sorted_main = main_ids.clone();
    sorted_main.sort();
    assert_eq!(
        sorted_main, unique,
        "主根条目编号与各进程拿到的编号必须一致"
    );

    // 两棵树的副本都不许被写。
    for tree in [&tree_a, &tree_b] {
        assert!(
            entry_ids(&defects_path(tree)).is_empty(),
            "{} 的 .kanzei 副本被写了",
            tree.display()
        );
    }

    std::fs::remove_dir_all(repo.parent().unwrap()).ok();
}
