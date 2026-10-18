//! The file browser against real servers (Swift `RemoteFilesIntegrationTests`): listing, folder and file
//! transfers both ways with and without gzip, conflicts with "Apply to all", cancel with partial cleanup,
//! and the SFTP path servers without `tar` use.
//!
//! - `RD_TEST_SSHD=127.0.0.1:2221`: the `sshd-gnu` lab container (OpenSSH, GNU userland, `find -printf`,
//!   SFTP);
//! - `RD_TEST_SSHD_BUSYBOX=127.0.0.1:2222`: `sshd-busybox` (Dropbear, busybox userland: the `ls -lnA`
//!   fallback, busybox tar, no SFTP subsystem).
//!
//! Start them with `docker compose -f tests/lab/docker-compose.yml up -d sshd-gnu sshd-busybox`. Without
//! the variables the tests pass without doing anything. Remote fixtures live in a fresh `/tmp/rd-it-*`
//! folder, local ones in temp dirs; the user's own `~/.ssh` is never read or written.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rd_ssh::SshSecrets;
use rd_ssh::background::{Client, ConnectSpec, HostKeyPrompt, runtime};
use rd_ssh::files::transfer::{self, FOLDERS_NEED_TAR};
use rd_ssh::files::{
    ConflictAnswer, ConflictChoice, ConflictQuestion, Direction, EntryKind, ItemStatus, Progress,
    RemoteFiles, RemoteMode, TransferObserver, TransferRequest,
};
use secrecy::SecretString;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn target(variable: &str) -> Option<(String, u16)> {
    let target = std::env::var(variable).ok()?;
    let (host, port) = target.rsplit_once(':').expect("host:port");
    Some((host.to_owned(), port.parse().expect("a port")))
}

fn accept_all() -> HostKeyPrompt {
    Arc::new(|_| {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(true);
        receiver
    })
}

async fn connect(host: &str, port: u16, home: &Path) -> Client {
    let spec = ConnectSpec {
        host: host.to_owned(),
        port: Some(port),
        user: "tester".to_owned(),
        key_path: None,
        secrets: SshSecrets::new(Some(SecretString::from("tester-pass".to_owned())), None, None),
        home: Some(home.to_path_buf()),
        known_hosts: Some(vec![home.join("known_hosts")]),
        use_agent: false,
    };
    Client::connect(spec, Some(accept_all())).await.expect("connects")
}

/// Answers conflicts from a script and records progress.
#[derive(Default)]
struct Observer {
    answers: Mutex<Vec<ConflictAnswer>>,
    asked: Mutex<Vec<ConflictQuestion>>,
    progress: Mutex<Vec<Progress>>,
}

impl TransferObserver for Observer {
    fn progress(&self, progress: &Progress) {
        self.progress.lock().unwrap().push(progress.clone());
    }

    fn conflict(&self, question: ConflictQuestion) -> oneshot::Receiver<ConflictAnswer> {
        self.asked.lock().unwrap().push(question);
        let (sender, receiver) = oneshot::channel();
        let mut answers = self.answers.lock().unwrap();
        if !answers.is_empty() {
            let _ = sender.send(answers.remove(0));
        }
        receiver
    }
}

/// `/tmp/rd-it-*/data` with a folder of 10 files, a file and a symlink.
async fn fixtures(client: &Client) -> String {
    let base = format!("/tmp/rd-it-{}", std::process::id() ^ rand_suffix());
    let script = format!(
        "set -e; mkdir -p '{base}/data/sub dir/deep' '{base}/up'; cd '{base}/data'; \
         printf 'hello\\n' > 'a file.txt'; ln -s 'a file.txt' link; \
         for i in 1 2 3 4 5 6 7 8 9 10; do printf \"line $i\\n\" > \"sub dir/deep/f$i.txt\"; done"
    );
    let output = client.exec(&script).await.unwrap();
    assert!(output.succeeded(), "{}", output.error_line());
    base
}

fn rand_suffix() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
}

async fn remote_text(client: &Client, command: &str) -> String {
    String::from_utf8_lossy(&client.exec(command).await.unwrap().stdout)
        .trim()
        .to_owned()
}

fn file_count(path: &Path) -> usize {
    std::fs::read_dir(path)
        .unwrap()
        .flatten()
        .map(|e| {
            if e.file_type().unwrap().is_dir() {
                file_count(&e.path())
            } else {
                1
            }
        })
        .sum()
}

/// The checks every server with `tar` passes.
async fn check_listing(remote: &RemoteFiles, base: &str, expect_find: bool) {
    let client = remote.client();
    let data = format!("{base}/data");

    // Listing: absolute path, kinds, sizes, dates, directories first, the home folder, a missing folder.
    let listing = remote.list(&data).await.unwrap();
    assert_eq!(listing.path, data);
    let entry = |name: &str| listing.entries.iter().find(|e| e.name == name).unwrap().clone();
    assert_eq!(entry("sub dir").kind, EntryKind::Directory);
    assert_eq!(entry("a file.txt").kind, EntryKind::File);
    assert_eq!(entry("a file.txt").size, 6);
    assert!(entry("a file.txt").modified.is_some());
    assert_eq!(entry("link").kind, EntryKind::Symlink);
    assert!(listing.entries[0].is_directory());
    let home = remote.list("~").await.unwrap();
    assert_eq!(home.path, "/home/tester");
    assert!(remote.list(&format!("{base}/nope")).await.is_err());
    let script_kind = remote_text(
        client,
        "find . -maxdepth 0 -printf '' >/dev/null 2>&1 && echo find || echo ls",
    )
    .await;
    assert_eq!(
        script_kind == "find",
        expect_find,
        "listing script: {script_kind}"
    );

    // Probes.
    assert!(remote.exists(&format!("{data}/link")).await);
    assert!(!remote.exists(&format!("{data}/none")).await);
    assert!(remote.is_directory(&format!("{data}/sub dir")).await);
    assert!(remote.size(&format!("{data}/sub dir")).await.unwrap() > 0);
}

#[allow(clippy::too_many_lines)] // one scenario, step by step
async fn exercise(remote: &RemoteFiles, base: &str, expect_find: bool) {
    check_listing(remote, base, expect_find).await;
    let client = remote.client();
    let data = format!("{base}/data");

    for compress in [true, false] {
        // A folder down, as one stream.
        let local = tempfile::tempdir().unwrap();
        let observer = Observer::default();
        let request = TransferRequest {
            direction: Direction::Download,
            sources: vec![format!("{data}/sub dir"), format!("{data}/a file.txt")],
            destination: local.path().to_string_lossy().into_owned(),
            compress,
        };
        let report = transfer::run(remote, &request, &observer, &CancellationToken::new()).await;
        assert!(
            report.items.iter().all(|i| i.status == ItemStatus::Done),
            "{report:?}"
        );
        assert_eq!(file_count(&local.path().join("sub dir")), 10);
        assert_eq!(
            std::fs::read_to_string(local.path().join("sub dir/deep/f7.txt")).unwrap(),
            "line 7\n"
        );
        assert_eq!(
            std::fs::read_to_string(local.path().join("a file.txt")).unwrap(),
            "hello\n"
        );
        let last = observer.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last.index, last.count), (1, 2));
        assert!(last.bytes_done > 0);

        // Again: both exist. Skip + Apply to all asks once and copies nothing.
        observer
            .answers
            .lock()
            .unwrap()
            .push((ConflictChoice::Skip, true));
        std::fs::write(local.path().join("a file.txt"), "local").unwrap();
        let report = transfer::run(remote, &request, &observer, &CancellationToken::new()).await;
        assert_eq!(observer.asked.lock().unwrap().len(), 1);
        assert!(observer.asked.lock().unwrap()[0].is_folder);
        assert!(
            report.items.iter().all(|i| i.status == ItemStatus::Skipped),
            "{report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(local.path().join("a file.txt")).unwrap(),
            "local"
        );

        // A folder and a file up; the server doesn't take over local owners.
        let source = tempfile::tempdir().unwrap();
        let folder = source.path().join("up load");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        for index in 1..=20 {
            std::fs::write(
                folder.join(format!("inner/it's {index}.txt")),
                format!("n{index}"),
            )
            .unwrap();
        }
        std::fs::write(source.path().join("single.txt"), "single\n").unwrap();
        let destination = format!("{base}/up/{compress}");
        client.exec(&format!("mkdir -p '{destination}'")).await.unwrap();
        let request = TransferRequest {
            direction: Direction::Upload,
            sources: vec![
                folder.to_string_lossy().into_owned(),
                source.path().join("single.txt").to_string_lossy().into_owned(),
            ],
            destination: destination.clone(),
            compress,
        };
        let observer = Observer::default();
        let report = transfer::run(remote, &request, &observer, &CancellationToken::new()).await;
        assert!(
            report.items.iter().all(|i| i.status == ItemStatus::Done),
            "{report:?}"
        );
        let counts = remote_text(
            client,
            &format!(
                "find '{destination}/up load' -type f | wc -l; cat '{destination}/single.txt'; \
                 ls -ln '{destination}/up load' | tail -1 | awk '{{print $3}}'"
            ),
        )
        .await;
        let lines: Vec<&str> = counts.lines().map(str::trim).collect();
        assert_eq!(lines[..2], ["20", "single"]);
        assert_eq!(
            lines[2],
            remote_text(client, "id -u").await,
            "owned by the remote user"
        );

        // Replace: the file is overwritten.
        std::fs::write(source.path().join("single.txt"), "changed\n").unwrap();
        let observer = Observer::default();
        observer
            .answers
            .lock()
            .unwrap()
            .push((ConflictChoice::Replace, false));
        observer
            .answers
            .lock()
            .unwrap()
            .push((ConflictChoice::Replace, false));
        let report = transfer::run(remote, &request, &observer, &CancellationToken::new()).await;
        assert!(
            report.items.iter().all(|i| i.status == ItemStatus::Done),
            "{report:?}"
        );
        assert_eq!(observer.asked.lock().unwrap().len(), 2);
        assert_eq!(
            remote_text(client, &format!("cat '{destination}/single.txt'")).await,
            "changed"
        );
    }

    // A failure reports the remote tar's line and removes the partial copy.
    let local = tempfile::tempdir().unwrap();
    let request = TransferRequest {
        direction: Direction::Download,
        sources: vec![format!("{data}/missing")],
        destination: local.path().to_string_lossy().into_owned(),
        compress: true,
    };
    let report = transfer::run(remote, &request, &Observer::default(), &CancellationToken::new()).await;
    assert_eq!(report.items[0].status, ItemStatus::Failed);
    assert!(
        report.items[0].error.as_deref().unwrap_or("").contains("missing"),
        "{report:?}"
    );
    assert_eq!(std::fs::read_dir(local.path()).unwrap().count(), 0);
}

/// Cancelling a large download stops it and leaves nothing behind.
#[allow(clippy::too_many_lines)]
async fn cancel_cleans_up(remote: &RemoteFiles, base: &str) {
    let client = remote.client();
    let output = client
        .exec(&format!(
            "mkdir -p '{base}/big' && head -c 100000000 /dev/urandom > '{base}/big/blob'"
        ))
        .await
        .unwrap();
    assert!(output.succeeded(), "{}", output.error_line());
    let local = tempfile::tempdir().unwrap();
    let request = TransferRequest {
        direction: Direction::Download,
        sources: vec![format!("{base}/big"), format!("{base}/data/a file.txt")],
        destination: local.path().to_string_lossy().into_owned(),
        compress: false,
    };
    let cancel = CancellationToken::new();
    let observer = Arc::new(Observer::default());
    let started = std::time::Instant::now();
    let canceller = cancel.clone();
    let watched = Arc::clone(&observer);
    tokio::spawn(async move {
        // Cancel once bytes flow.
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if watched.progress.lock().unwrap().iter().any(|p| p.bytes_done > 0) {
                break;
            }
        }
        canceller.cancel();
    });
    let report = transfer::run(remote, &request, observer.as_ref(), &cancel).await;
    assert!(report.cancelled(), "{report:?}");
    assert_eq!(
        report.items[1].status,
        ItemStatus::Cancelled,
        "later items are cancelled too"
    );
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(!local.path().join("big").exists(), "the partial copy is removed");

    // An upload cancelled midway is removed on the server.
    let source = tempfile::tempdir().unwrap();
    let blob: Vec<u8> = (0..60_000_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    std::fs::create_dir(source.path().join("big up")).unwrap();
    std::fs::write(source.path().join("big up/blob"), blob).unwrap();
    let request = TransferRequest {
        direction: Direction::Upload,
        sources: vec![source.path().join("big up").to_string_lossy().into_owned()],
        destination: format!("{base}/up"),
        compress: false,
    };
    let cancel = CancellationToken::new();
    let observer = Arc::new(Observer::default());
    let canceller = cancel.clone();
    let watched = Arc::clone(&observer);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if watched
                .progress
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.bytes_done > 1_000_000)
            {
                break;
            }
        }
        canceller.cancel();
    });
    let report = transfer::run(remote, &request, observer.as_ref(), &cancel).await;
    assert!(report.cancelled(), "{report:?}");
    assert!(
        !remote.exists(&format!("{base}/up/big up")).await,
        "the remote partial copy is removed"
    );
}

#[test]
fn gnu_server() {
    let Some((host, port)) = target("RD_TEST_SSHD") else {
        eprintln!("RD_TEST_SSHD not set: skipping the file browser check");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    runtime().block_on(async {
        let remote = RemoteFiles::open(connect(&host, port, home.path()).await).await;
        assert_eq!(remote.mode(), RemoteMode::Tar);
        let base = fixtures(remote.client()).await;
        exercise(&remote, &base, true).await;
        cancel_cleans_up(&remote, &base).await;

        // The SFTP path (RouterOS has no tar): listing and single files, folders refused.
        let sftp = RemoteFiles::open_sftp(connect(&host, port, home.path()).await)
            .await
            .expect("OpenSSH has an sftp subsystem");
        assert_eq!(sftp.mode(), RemoteMode::Sftp);
        let listing = sftp.list(&format!("{base}/data")).await.unwrap();
        let file = listing.entries.iter().find(|e| e.name == "a file.txt").unwrap();
        assert_eq!((file.kind, file.size), (EntryKind::File, 6));
        assert_eq!(listing.entries[0].kind, EntryKind::Directory);
        assert_eq!(sftp.list("~").await.unwrap().path, "/home/tester");
        let local = tempfile::tempdir().unwrap();
        let request = TransferRequest {
            direction: Direction::Download,
            sources: vec![format!("{base}/data/a file.txt"), format!("{base}/data/sub dir")],
            destination: local.path().to_string_lossy().into_owned(),
            compress: true,
        };
        let report = transfer::run(&sftp, &request, &Observer::default(), &CancellationToken::new()).await;
        assert_eq!(report.items[0].status, ItemStatus::Done, "{report:?}");
        assert_eq!(report.items[1].error.as_deref(), Some(FOLDERS_NEED_TAR));
        assert_eq!(
            std::fs::read_to_string(local.path().join("a file.txt")).unwrap(),
            "hello\n"
        );
        std::fs::write(local.path().join("up.txt"), "via sftp\n").unwrap();
        let request = TransferRequest {
            direction: Direction::Upload,
            sources: vec![local.path().join("up.txt").to_string_lossy().into_owned()],
            destination: format!("{base}/up"),
            compress: true,
        };
        let report = transfer::run(&sftp, &request, &Observer::default(), &CancellationToken::new()).await;
        assert_eq!(report.items[0].status, ItemStatus::Done, "{report:?}");
        assert_eq!(
            remote_text(remote.client(), &format!("cat '{base}/up/up.txt'")).await,
            "via sftp"
        );

        remote.client().exec(&format!("rm -rf '{base}'")).await.unwrap();
        sftp.disconnect().await;
        remote.disconnect().await;
    });
}

#[test]
fn busybox_server() {
    let Some((host, port)) = target("RD_TEST_SSHD_BUSYBOX") else {
        eprintln!("RD_TEST_SSHD_BUSYBOX not set: skipping the busybox file browser check");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    runtime().block_on(async {
        let client = connect(&host, port, home.path()).await;
        // Dropbear ships no sftp-server: the SFTP path is unavailable here.
        assert!(client.sftp().await.is_err());
        let remote = RemoteFiles::open(client).await;
        assert_eq!(remote.mode(), RemoteMode::Tar, "busybox has tar");
        let base = fixtures(remote.client()).await;
        exercise(&remote, &base, false).await;
        cancel_cleans_up(&remote, &base).await;
        remote.client().exec(&format!("rm -rf '{base}'")).await.unwrap();
        remote.disconnect().await;
    });
}
