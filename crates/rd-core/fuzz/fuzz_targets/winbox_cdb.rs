//! The WinBox `.cdb` parser never panics, and every entry it returns has a host and a name.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rd_core::ConnectionType;
use rd_core::import::parse_winbox;

fuzz_target!(|data: &[u8]| {
    // Also reach the record parser for inputs without the magic.
    let mut with_magic = vec![0x0D, 0xF0, 0x1D, 0xC0];
    with_magic.extend_from_slice(data);
    for input in [data, with_magic.as_slice()] {
        if let Ok(items) = parse_winbox(input) {
            for item in items {
                assert_eq!(item.kind, ConnectionType::Mikrotik);
                assert!(!item.host.is_empty());
                assert!(!item.name.is_empty());
                assert!(item.group_path.len() <= 1);
            }
        }
    }
});
