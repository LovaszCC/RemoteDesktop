//! The cliprdr side of a session (Swift `RDPClipboard`): plain text both ways (`CF_UNICODETEXT`) and
//! files both ways ([MS-RDPECLIP] file streams). FreeRDP's channel reports what the server sends; this
//! state machine answers with [`Action`]s, which the session loop carries out on the connection.
//!
//! The server's file list comes back raw and goes through the strict parser
//! ([`crate::clipfiles::file_list`]).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use rd_freerdp::{ClipEvent, ContentsRequest, Format};

use crate::clipfiles::{LocalFiles, file_list};

/// `CF_UNICODETEXT`.
pub const CF_UNICODETEXT: u32 = 13;
/// The registered format offered for local files (any id from 0xC000 on).
pub const FILE_LIST_FORMAT: u32 = 0xC0BC;
/// The name of the file list format.
pub const FILE_LIST_NAME: &str = "FileGroupDescriptorW";

/// `CB_USE_LONG_FORMAT_NAMES`.
const CB_USE_LONG_FORMAT_NAMES: u32 = 0x02;
/// `CB_STREAM_FILECLIP_ENABLED`.
pub const CB_STREAM_FILECLIP_ENABLED: u32 = 0x04;
/// `CB_FILECLIP_NO_FILE_PATHS`.
const CB_FILECLIP_NO_FILE_PATHS: u32 = 0x08;
/// `CB_HUGE_FILE_SUPPORT_ENABLED`.
pub const CB_HUGE_FILE_SUPPORT_ENABLED: u32 = 0x20;

/// What the session loop does for the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send the client's capabilities.
    Capabilities(u32),
    /// Send a Format List.
    FormatList(Vec<Format>),
    /// Send a Format List Response.
    FormatListResponse(bool),
    /// Send a Format Data Request.
    DataRequest(u32),
    /// Send a Format Data Response (`None` refuses).
    DataResponse(Option<Vec<u8>>),
    /// Serve a File Contents Request from these files (`None`: refuse), off the session thread.
    Serve(ContentsRequest, Option<Arc<LocalFiles>>),
    /// The remote clipboard's text arrived.
    RemoteText(String),
    /// The remote clipboard changed (a running download is for the old one).
    RemoteChanged,
    /// The server's file list arrived, as sent (`cItems` + descriptors).
    RemoteFileList(Vec<u8>),
    /// A File Contents Response arrived.
    FileContents {
        /// The request it answers.
        stream_id: u32,
        /// The server sent the data.
        ok: bool,
        /// The size (8 bytes) or the range's bytes.
        data: Vec<u8>,
    },
    /// The capability flags both sides support.
    Negotiated(u32),
}

/// What a Format Data Response answers (responses carry no format id: they come in request order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Requested {
    Text,
    Files,
}

/// A session's clipboard state.
#[derive(Debug)]
pub struct SessionClipboard {
    /// File transfers are on (the session has a download folder).
    files_enabled: bool,
    negotiated: u32,
    requests: VecDeque<Requested>,
    /// The local text offered to the server.
    pub local_text: Option<String>,
    /// The local files offered to the server (instead of text).
    pub local_files: Option<Arc<LocalFiles>>,
    /// File lists the server locked (it may still ask for them after the clipboard changed).
    locked: HashMap<u32, Arc<LocalFiles>>,
}

impl SessionClipboard {
    /// A clipboard; `files_enabled` turns on file streams.
    #[must_use]
    pub fn new(files_enabled: bool) -> Self {
        Self {
            files_enabled,
            negotiated: 0,
            requests: VecDeque::new(),
            local_text: None,
            local_files: None,
            locked: HashMap::new(),
        }
    }

    /// The client's capability flags.
    #[must_use]
    pub fn client_capabilities(&self) -> u32 {
        let mut flags = CB_USE_LONG_FORMAT_NAMES;
        if self.files_enabled {
            flags |= CB_STREAM_FILECLIP_ENABLED | CB_FILECLIP_NO_FILE_PATHS | CB_HUGE_FILE_SUPPORT_ENABLED;
        }
        flags
    }

    fn files_negotiated(&self) -> bool {
        self.files_enabled && self.negotiated & CB_STREAM_FILECLIP_ENABLED != 0
    }

    /// The local files the server can take now.
    fn file_offer(&self) -> Option<&Arc<LocalFiles>> {
        self.local_files.as_ref().filter(|_| self.files_negotiated())
    }

    /// The formats to offer: the file list, text, or nothing (files the server can't take are announced
    /// as an empty clipboard).
    #[must_use]
    pub fn local_formats(&self) -> Vec<Format> {
        if self.file_offer().is_some() {
            vec![Format {
                id: FILE_LIST_FORMAT,
                name: Some(FILE_LIST_NAME.to_owned()),
            }]
        } else if self.local_text.is_some() {
            vec![Format {
                id: CF_UNICODETEXT,
                name: None,
            }]
        } else {
            Vec::new()
        }
    }

    /// Announces the local clipboard (after it changed).
    #[must_use]
    pub fn offer(&self) -> Vec<Action> {
        vec![Action::FormatList(self.local_formats())]
    }

    /// Answers what the server's channel sent.
    pub fn handle(&mut self, event: ClipEvent) -> Vec<Action> {
        match event {
            // The capabilities and the first Format List make the channel ready; the list always goes
            // out, empty when there is nothing to offer (like the Swift app).
            ClipEvent::MonitorReady => vec![
                Action::Capabilities(self.client_capabilities()),
                Action::FormatList(self.local_formats()),
            ],
            ClipEvent::Capabilities(flags) => {
                self.negotiated = flags & self.client_capabilities();
                vec![Action::Negotiated(self.negotiated)]
            }
            ClipEvent::FormatList(formats) => self.remote_copy(&formats),
            ClipEvent::DataRequest(format) => vec![Action::DataResponse(self.data_for(format))],
            ClipEvent::DataResponse(data) => self.data_response(data),
            ClipEvent::ContentsRequest(request) => {
                let files = request
                    .data_id
                    .and_then(|id| self.locked.get(&id))
                    .or(self.local_files.as_ref())
                    .cloned();
                vec![Action::Serve(request, files)]
            }
            ClipEvent::ContentsResponse { stream_id, data } => vec![Action::FileContents {
                stream_id,
                ok: data.is_some(),
                data: data.unwrap_or_default(),
            }],
            ClipEvent::Lock { data_id, locked } => {
                if !locked {
                    self.locked.remove(&data_id);
                } else if let Some(files) = &self.local_files {
                    self.locked.insert(data_id, Arc::clone(files));
                }
                Vec::new()
            }
        }
    }

    /// New remote clipboard: files are fetched when there are any (and file streams were negotiated),
    /// text otherwise.
    fn remote_copy(&mut self, formats: &[Format]) -> Vec<Action> {
        let mut actions = vec![Action::FormatListResponse(true), Action::RemoteChanged];
        let files = formats.iter().find(|f| f.name.as_deref() == Some(FILE_LIST_NAME));
        if let Some(files) = files.filter(|_| self.files_negotiated()) {
            self.requests.push_back(Requested::Files);
            actions.push(Action::DataRequest(files.id));
        } else if formats.iter().any(|f| f.id == CF_UNICODETEXT) {
            self.requests.push_back(Requested::Text);
            actions.push(Action::DataRequest(CF_UNICODETEXT));
        }
        actions
    }

    fn data_for(&self, format: u32) -> Option<Vec<u8>> {
        match format {
            CF_UNICODETEXT => self.local_text.as_deref().map(|text| {
                let mut data: Vec<u8> = to_remote_text(text)
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect();
                data.shrink_to_fit();
                data
            }),
            FILE_LIST_FORMAT => self
                .file_offer()
                .map(|files| file_list::encode(&files.descriptors())),
            _ => None,
        }
    }

    fn data_response(&mut self, data: Option<Vec<u8>>) -> Vec<Action> {
        let requested = self.requests.pop_front().unwrap_or(Requested::Text);
        // A refused response is not a protocol error.
        let Some(data) = data else {
            return Vec::new();
        };
        match requested {
            Requested::Files => vec![Action::RemoteFileList(data)],
            Requested::Text => {
                let units: Vec<u16> = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .take_while(|&unit| unit != 0)
                    .collect();
                vec![Action::RemoteText(to_local_text(&String::from_utf16_lossy(
                    &units,
                )))]
            }
        }
    }
}

/// Windows text uses CRLF: lone LFs become CRLF.
#[must_use]
pub fn to_remote_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut previous = '\0';
    for c in text.chars() {
        if c == '\n' && previous != '\r' {
            out.push('\r');
        }
        out.push(c);
        previous = c;
    }
    out
}

/// Remote text for the local clipboard: CRLF becomes LF except on Windows.
#[must_use]
pub fn to_local_text(text: &str) -> String {
    if cfg!(windows) {
        text.to_owned()
    } else {
        text.replace("\r\n", "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn text_format() -> Format {
        Format {
            id: CF_UNICODETEXT,
            name: None,
        }
    }

    fn file_list_format() -> Format {
        Format {
            id: 0xC0FE,
            name: Some(FILE_LIST_NAME.to_owned()),
        }
    }

    #[test]
    fn line_endings_follow_each_side() {
        assert_eq!(to_remote_text("a\nb\r\nc"), "a\r\nb\r\nc");
        assert_eq!(to_remote_text("őű"), "őű");
        if cfg!(windows) {
            assert_eq!(to_local_text("a\r\nb"), "a\r\nb");
        } else {
            assert_eq!(to_local_text("a\r\nb"), "a\nb");
        }
    }

    #[test]
    fn remote_copies_of_text_are_fetched_and_requests_answered() {
        let mut clip = SessionClipboard::new(false);
        assert_eq!(
            clip.handle(ClipEvent::MonitorReady),
            [Action::Capabilities(0x02), Action::FormatList(Vec::new())],
            "nothing to offer yet, but the list goes out"
        );
        assert_eq!(
            clip.handle(ClipEvent::FormatList(vec![text_format()])),
            [
                Action::FormatListResponse(true),
                Action::RemoteChanged,
                Action::DataRequest(CF_UNICODETEXT)
            ]
        );
        assert_eq!(
            clip.handle(ClipEvent::FormatList(vec![Format {
                id: 0xC123,
                name: None
            }])),
            [Action::FormatListResponse(true), Action::RemoteChanged],
            "only text is fetched"
        );
        clip.local_text = Some("x\ny".into());
        assert_eq!(clip.offer(), [Action::FormatList(vec![text_format()])]);
        assert_eq!(
            clip.handle(ClipEvent::DataRequest(CF_UNICODETEXT)),
            [Action::DataResponse(Some(utf16("x\r\ny")))]
        );
        assert_eq!(
            clip.handle(ClipEvent::DataRequest(1)),
            [Action::DataResponse(None)]
        );
        assert_eq!(
            clip.handle(ClipEvent::DataResponse(Some(utf16("hi\r\n")))),
            [Action::RemoteText(to_local_text("hi\r\n"))]
        );
        assert!(clip.handle(ClipEvent::DataResponse(None)).is_empty(), "refused");
    }

    #[test]
    fn remote_files_are_fetched_raw_once_file_streams_are_negotiated() {
        let mut clip = SessionClipboard::new(true);
        assert_ne!(clip.client_capabilities() & CB_STREAM_FILECLIP_ENABLED, 0);
        let both = vec![file_list_format(), text_format()];
        // Not negotiated yet: text.
        assert_eq!(
            clip.handle(ClipEvent::FormatList(both.clone())).last(),
            Some(&Action::DataRequest(CF_UNICODETEXT))
        );
        assert_eq!(
            clip.handle(ClipEvent::Capabilities(CB_STREAM_FILECLIP_ENABLED | 0x10)),
            [Action::Negotiated(CB_STREAM_FILECLIP_ENABLED)]
        );
        assert_eq!(
            clip.handle(ClipEvent::FormatList(both)).last(),
            Some(&Action::DataRequest(0xC0FE))
        );
        // Responses come back in request order: the text one first, then the file list.
        assert!(matches!(
            clip.handle(ClipEvent::DataResponse(Some(utf16("t"))))[..],
            [Action::RemoteText(_)]
        ));
        assert_eq!(
            clip.handle(ClipEvent::DataResponse(Some(vec![1, 2, 3]))),
            [Action::RemoteFileList(vec![1, 2, 3])]
        );
        assert_eq!(
            clip.handle(ClipEvent::ContentsResponse {
                stream_id: 7,
                data: Some(vec![9])
            }),
            [Action::FileContents {
                stream_id: 7,
                ok: true,
                data: vec![9]
            }]
        );
        assert_eq!(
            clip.handle(ClipEvent::ContentsResponse {
                stream_id: 8,
                data: None
            }),
            [Action::FileContents {
                stream_id: 8,
                ok: false,
                data: Vec::new()
            }]
        );
    }

    fn contents_request(stream_id: u32, data_id: Option<u32>) -> ContentsRequest {
        ContentsRequest {
            stream_id,
            index: 0,
            flags: rd_freerdp::FILECONTENTS_RANGE,
            position: 0,
            requested: 100,
            data_id,
        }
    }

    fn served(actions: &[Action]) -> Vec<u8> {
        match actions {
            [Action::Serve(request, files)] => files
                .as_ref()
                .and_then(|files| files.serve(request))
                .unwrap_or_default(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn local_files_are_offered_once_negotiated_and_served_from_locked_lists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"first").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"second").unwrap();
        let list = |name: &str| Arc::new(LocalFiles::scan(&[dir.path().join(name)]).unwrap());
        let mut clip = SessionClipboard::new(true);
        clip.local_files = Some(list("a.txt"));
        assert_eq!(clip.local_formats(), [], "file streams not negotiated yet");
        let _ = clip.handle(ClipEvent::Capabilities(CB_STREAM_FILECLIP_ENABLED));
        assert_eq!(
            clip.handle(ClipEvent::MonitorReady)[1],
            Action::FormatList(vec![Format {
                id: FILE_LIST_FORMAT,
                name: Some(FILE_LIST_NAME.to_owned())
            }])
        );
        match &clip.handle(ClipEvent::DataRequest(FILE_LIST_FORMAT))[..] {
            [Action::DataResponse(Some(data))] => {
                assert_eq!(file_list::parse(data, 10).unwrap()[0].path, "a.txt");
            }
            other => panic!("{other:?}"),
        }
        let _ = clip.handle(ClipEvent::Lock {
            data_id: 3,
            locked: true,
        });
        clip.local_files = Some(list("b.txt"));
        assert_eq!(
            served(&clip.handle(ClipEvent::ContentsRequest(contents_request(1, None)))),
            b"second"
        );
        assert_eq!(
            served(&clip.handle(ClipEvent::ContentsRequest(contents_request(2, Some(3))))),
            b"first"
        );
        let _ = clip.handle(ClipEvent::Lock {
            data_id: 3,
            locked: false,
        });
        assert_eq!(
            served(&clip.handle(ClipEvent::ContentsRequest(contents_request(3, Some(3))))),
            b"second"
        );
        clip.local_files = None;
        assert!(matches!(
            &clip.handle(ClipEvent::ContentsRequest(contents_request(4, None)))[..],
            [Action::Serve(_, None)]
        ));
    }
}
