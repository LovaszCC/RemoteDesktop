# Test lab containers

Start everything with `docker compose -f tests/lab/docker-compose.yml up -d --build`. Stop it with `down`.

| Service | Address | Users | Purpose |
|---|---|---|---|
| `sshd-gnu` | `127.0.0.1:2221` | `tester` / `tester-pass`, `root` / `root-pass` | OpenSSH, GNU userland (`find -printf`, GNU tar) |
| `sshd-busybox` | `127.0.0.1:2222` | `tester` / `tester-pass` | Dropbear, busybox userland (the `ls -lnA` fallback) |
| `xrdp` | `127.0.0.1:3390` | `tester` / `tester-pass` | Linux RDP target (TLS, no NLA) |

The credentials are made up and only valid inside these containers. Host keys are regenerated on each
build, so use `-o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no` for manual checks.

File browser checks: `RD_TEST_SSHD=127.0.0.1:2221 RD_TEST_SSHD_BUSYBOX=127.0.0.1:2222 cargo test -p rd-ssh --test files`
(fixtures go to a fresh `/tmp/rd-it-*` folder in the containers and are removed afterwards).

Embedded RDP engine checks (connect, certificate store, frames, resize, typing, clipboard both ways through
`xclip` in the session, files copied in the session downloaded through the file clipboard, logoff): `RD_TEST_XRDP=127.0.0.1:3390 RD_TEST_XRDP_CONTAINER=lab-xrdp-1 cargo test -p
rd-rdp --test live`. The same test file checks a Windows host with NLA when `RD_TEST_RDP_HOST`,
`RD_TEST_RDP_USER` and `RD_TEST_RDP_PASS` are set in the environment (never written to a file).
