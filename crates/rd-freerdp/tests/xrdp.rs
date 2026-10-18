//! Connects to the lab's xrdp (`RD_TEST_XRDP=127.0.0.1:3390`, user `tester` / `tester-pass`) and checks
//! that the desktop, its paints and the pointer arrive. Skipped without the variable.

use std::time::{Duration, Instant};

use rd_freerdp::{ClipEvent, Config, Connection, Credentials, Handler, Rect};

#[derive(Default)]
struct Seen {
    certificates: usize,
    desktop: Option<(u32, u32)>,
    painted: usize,
    pointers: usize,
    clip: Vec<ClipEvent>,
}

impl Handler for Seen {
    fn verify_certificate(&mut self, pem: &[u8], _: &str, _: u16, _: u32) -> bool {
        assert!(pem.starts_with(b"-----BEGIN CERTIFICATE-----"));
        self.certificates += 1;
        true
    }
    fn authenticate(&mut self) -> Option<Credentials> {
        None
    }
    fn desktop(&mut self, width: u32, height: u32) {
        self.desktop = Some((width, height));
    }
    fn painted(&mut self, rects: &[Rect]) {
        self.painted += rects.len();
    }
    fn pointer_shape(&mut self, rgba: &[u8], width: u32, height: u32, _: u32, _: u32) {
        assert_eq!(rgba.len(), (width * height * 4) as usize);
        self.pointers += 1;
    }
    fn pointer_hidden(&mut self) {}
    fn pointer_default(&mut self) {}
    fn display_control_ready(&mut self) {}
    fn clip(&mut self, event: ClipEvent) {
        self.clip.push(event);
    }
}

#[test]
fn xrdp_desktop_paints_and_pointer() {
    let Ok(address) = std::env::var("RD_TEST_XRDP") else {
        eprintln!("RD_TEST_XRDP not set: skipped");
        return;
    };
    let (host, port) = address.rsplit_once(':').unwrap();
    let mut connection = Connection::new(Seen::default()).unwrap();
    let config_dir = std::env::temp_dir()
        .join("rd-freerdp-test")
        .to_string_lossy()
        .into_owned();
    let config = Config {
        host,
        port: port.parse().unwrap(),
        username: "tester",
        domain: "",
        password: "tester-pass",
        client_name: "rd-freerdp-test",
        config_dir: &config_dir,
        width: 1024,
        height: 768,
        console_session: false,
        clipboard: true,
    };
    assert!(
        connection.connect(&config),
        "connect failed: {}",
        rd_freerdp::last_error_string(connection.last_error())
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        assert!(connection.poll(Duration::from_millis(100)), "dropped");
        let ready = connection.with_handler(|seen| {
            seen.painted > 0 && seen.pointers > 0 && seen.clip.contains(&ClipEvent::MonitorReady)
        });
        if ready {
            break;
        }
    }
    let (width, height, lit) = {
        let frame = connection.framebuffer().expect("framebuffer");
        let lit = frame.data.chunks(4).filter(|p| p[0] | p[1] | p[2] != 0).count();
        (frame.width, frame.height, lit)
    };
    connection.with_handler(|seen| {
        eprintln!(
            "desktop {:?}, {} paints, {} pointers, clip {:?}, {lit} non-black pixels",
            seen.desktop, seen.painted, seen.pointers, seen.clip
        );
        assert_eq!(seen.certificates, 1);
        assert_eq!(seen.desktop, Some((1024, 768)));
        assert!(seen.painted > 0 && seen.pointers > 0 && lit > 0);
    });
    assert_eq!((width, height), (1024, 768));
    connection.disconnect();
}
