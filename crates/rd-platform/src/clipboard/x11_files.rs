//! An X11 `CLIPBOARD` owner for a file list (pure Rust x11rb connection, no unsafe code): `arboard` only
//! offers `text/uri-list`, while GTK file managers paste `x-special/gnome-copied-files`. A thread answers
//! selection requests until another client takes the clipboard (a newer copy, ours or not).

use std::path::PathBuf;

use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Requests bigger than this would need the INCR protocol; such lists go through `arboard`.
const MAX_PROPERTY: usize = 256 * 1024;

struct Atoms {
    clipboard: u32,
    targets: u32,
    utf8: u32,
    text_plain: u32,
    uri_list: u32,
    gnome: u32,
}

fn atom(connection: &RustConnection, name: &str) -> Result<u32, String> {
    Ok(connection
        .intern_atom(false, name.as_bytes())
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .atom)
}

/// Takes the clipboard with `paths`; a background thread serves it.
pub fn serve(paths: &[PathBuf]) -> Result<(), String> {
    let uris = super::files::uri_list(paths);
    let gnome = super::files::gnome_copied_files(paths);
    let text = paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    if uris.len().max(gnome.len()) > MAX_PROPERTY {
        return Err("file list too long".into());
    }
    let (connection, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let atoms = Atoms {
        clipboard: atom(&connection, "CLIPBOARD")?,
        targets: atom(&connection, "TARGETS")?,
        utf8: atom(&connection, "UTF8_STRING")?,
        text_plain: atom(&connection, "text/plain;charset=utf-8")?,
        uri_list: atom(&connection, "text/uri-list")?,
        gnome: atom(&connection, "x-special/gnome-copied-files")?,
    };
    let root = connection.setup().roots[screen].root;
    let window = connection.generate_id().map_err(|e| e.to_string())?;
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .map_err(|e| e.to_string())?;
    connection
        .set_selection_owner(window, atoms.clipboard, CURRENT_TIME)
        .map_err(|e| e.to_string())?;
    let owner = connection
        .get_selection_owner(atoms.clipboard)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .owner;
    if owner != window {
        return Err("the clipboard could not be taken".into());
    }
    std::thread::Builder::new()
        .name("clipboard-files".into())
        .spawn(move || {
            while let Ok(event) = connection.wait_for_event() {
                match event {
                    Event::SelectionRequest(request) => {
                        let _ = answer(&connection, &atoms, &request, &uris, &gnome, &text);
                    }
                    Event::SelectionClear(_) => break,
                    _ => {}
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn answer(
    connection: &RustConnection,
    atoms: &Atoms,
    request: &SelectionRequestEvent,
    uris: &str,
    gnome: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Obsolete clients leave the property empty: then the target names it.
    let property = if request.property == u32::from(AtomEnum::NONE) {
        request.target
    } else {
        request.property
    };
    let target = request.target;
    let served = if target == atoms.targets {
        let list = [
            atoms.targets,
            atoms.uri_list,
            atoms.gnome,
            atoms.utf8,
            atoms.text_plain,
        ];
        connection.change_property32(
            PropMode::REPLACE,
            request.requestor,
            property,
            AtomEnum::ATOM,
            &list,
        )?;
        true
    } else {
        let data = if target == atoms.uri_list {
            Some(uris)
        } else if target == atoms.gnome {
            Some(gnome)
        } else if target == atoms.utf8 || target == atoms.text_plain {
            Some(text)
        } else {
            None
        };
        if let Some(data) = data {
            connection.change_property8(
                PropMode::REPLACE,
                request.requestor,
                property,
                target,
                data.as_bytes(),
            )?;
        }
        data.is_some()
    };
    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target,
        property: if served {
            property
        } else {
            u32::from(AtomEnum::NONE)
        },
    };
    connection.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
    connection.flush()?;
    Ok(())
}
