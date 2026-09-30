//! Managed X11 clients may sit below window-manager frame windows. EWMH
//! supplies the actual clients; older WMs need a bounded ICCCM tree walk.

use std::collections::{HashSet, VecDeque};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
use x11rb::rust_connection::RustConnection;

const MAX_WINDOWS: usize = 1_024;
const MAX_DEPTH: usize = 8;

pub(super) fn clients(conn: &RustConnection, root: u32) -> Vec<u32> {
    let managed = ["_NET_CLIENT_LIST", "_NET_CLIENT_LIST_STACKING"]
        .into_iter()
        .find_map(|name| client_list(conn, root, name));
    let state = conn
        .intern_atom(true, b"WM_STATE")
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.atom)
        .filter(|atom| *atom != x11rb::NONE);
    enumerate(
        root,
        managed,
        |window| {
            conn.query_tree(window)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.children)
                .unwrap_or_default()
        },
        |window| {
            state.is_some_and(|state| {
                conn.get_property(false, window, state, state, 0, 2)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|reply| reply.type_ == state && reply.format == 32)
            })
        },
    )
}

fn client_list(conn: &RustConnection, root: u32, name: &str) -> Option<Vec<u32>> {
    let atom = conn
        .intern_atom(true, name.as_bytes())
        .ok()?
        .reply()
        .ok()?
        .atom;
    if atom == x11rb::NONE {
        return None;
    }
    let reply = conn
        .get_property(false, root, atom, AtomEnum::WINDOW, 0, MAX_WINDOWS as u32)
        .ok()?
        .reply()
        .ok()?;
    if reply.type_ != u32::from(AtomEnum::WINDOW) {
        return None;
    }
    let windows = reply.value32()?.take(MAX_WINDOWS).collect();
    Some(windows)
}

fn enumerate(
    root: u32,
    managed: Option<Vec<u32>>,
    mut children: impl FnMut(u32) -> Vec<u32>,
    mut is_client: impl FnMut(u32) -> bool,
) -> Vec<u32> {
    let mut seen = HashSet::from([root, x11rb::NONE]);
    if let Some(windows) = managed {
        return windows
            .into_iter()
            .filter(|window| seen.insert(*window))
            .take(MAX_WINDOWS)
            .collect();
    }
    let mut pending = VecDeque::from([(root, 0)]);
    let mut clients = Vec::new();
    while let Some((window, depth)) = pending.pop_front() {
        if window != root && is_client(window) {
            clients.push(window);
            // Descendant widgets cannot be another managed top-level client.
            continue;
        }
        if depth == MAX_DEPTH {
            continue;
        }
        for child in children(window) {
            if seen.len() >= MAX_WINDOWS {
                break;
            }
            if seen.insert(child) {
                pending.push_back((child, depth + 1));
            }
        }
    }
    clients
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_clients_bypass_frame_windows_and_are_deduplicated() {
        let clients = enumerate(
            1,
            Some(vec![0, 1, 30, 20, 30]),
            |_| panic!("an EWMH client list needs no tree walk"),
            |_| panic!("an EWMH client list needs no state query"),
        );
        assert_eq!(clients, [30, 20]);
        assert!(enumerate(1, Some(vec![]), |_| panic!(), |_| panic!()).is_empty());
    }

    #[test]
    fn fallback_finds_clients_below_frames_and_virtual_roots() {
        let clients = enumerate(
            1,
            None,
            |window| match window {
                1 => vec![10, 11],
                10 => vec![20],
                11 => vec![12],
                12 => vec![30],
                20 | 30 => panic!("client widgets must not be traversed"),
                _ => vec![],
            },
            |window| matches!(window, 20 | 30),
        );
        assert_eq!(clients, [20, 30]);
    }

    #[test]
    fn fallback_bounds_both_depth_and_total_work_and_ignores_cycles() {
        let mut visited = Vec::new();
        assert!(enumerate(
            1,
            None,
            |window| {
                visited.push(window);
                vec![window, 1, window + 1]
            },
            |_| false,
        )
        .is_empty());
        assert_eq!(visited.len(), MAX_DEPTH);

        let mut visited = 0;
        let _ = enumerate(
            1,
            None,
            |window| {
                visited += 1;
                if window == 1 {
                    (2..MAX_WINDOWS as u32 * 2).collect()
                } else {
                    vec![]
                }
            },
            |_| false,
        );
        assert!(visited <= MAX_WINDOWS);
    }
}
