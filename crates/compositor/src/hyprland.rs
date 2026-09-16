//! Hyprland IPC over its per-instance UNIX socket. Read-only queries return
//! the compositor's JSON; dispatches are explicit Lua calls. Nothing here
//! implies another compositor exposes the same window identities.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};
use unimation::{Effect, NativeError, Point, Result, geometry::Rect};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Workspace {
    pub id: i64,
    #[serde(default)]
    pub name: String,
}

/// One mapped or unmapped window as reported by `hyprctl clients -j`.
/// `at` and `size` are logical layout coordinates; scaling is per monitor.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Client {
    pub address: String,
    #[serde(default)]
    pub mapped: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub at: [i64; 2],
    #[serde(default)]
    pub size: [i64; 2],
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub pseudo: bool,
    #[serde(default)]
    pub monitor: i64,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, rename = "initialClass")]
    pub initial_class: String,
    #[serde(default, rename = "initialTitle")]
    pub initial_title: String,
    #[serde(default)]
    pub pid: i64,
    #[serde(default)]
    pub xwayland: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub fullscreen: i64,
    #[serde(default, rename = "focusHistoryID")]
    pub focus_history_id: i64,
}
impl Client {
    /// The window handle used by Hyprland protocols that take a 32-bit id.
    pub fn handle(&self) -> Option<u32> {
        u64::from_str_radix(self.address.trim_start_matches("0x"), 16)
            .ok()
            .map(|a| a as u32)
    }
    /// Logical layout rectangle.
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.at[0] as f64,
            y: self.at[1] as f64,
            width: self.size[0] as f64,
            height: self.size[1] as f64,
        }
    }
    pub fn contains(&self, point: &Point) -> bool {
        self.rect().contains(point)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub x: i64,
    #[serde(default)]
    pub y: i64,
    #[serde(default)]
    pub scale: f64,
    #[serde(default)]
    pub transform: i64,
    #[serde(default)]
    pub focused: bool,
    #[serde(default, rename = "activeWorkspace")]
    pub active_workspace: Workspace,
    #[serde(default, rename = "specialWorkspace")]
    pub special_workspace: Workspace,
    #[serde(default)]
    pub disabled: bool,
}
impl Monitor {
    /// Logical size after scaling and rotation, matching client coordinates.
    pub fn logical_size(&self) -> (f64, f64) {
        let scale = if self.scale > 0. { self.scale } else { 1. };
        let (w, h) = (self.width as f64 / scale, self.height as f64 / scale);
        if self.transform % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        }
    }
    /// Logical layout rectangle.
    pub fn rect(&self) -> Rect {
        let (width, height) = self.logical_size();
        Rect {
            x: self.x as f64,
            y: self.y as f64,
            width,
            height,
        }
    }
    pub fn contains(&self, point: &Point) -> bool {
        self.rect().contains(point)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

fn fail(message: impl ToString) -> NativeError {
    NativeError::new("hyprland_ipc", message)
}

/// Client for one Hyprland instance. Each request opens a fresh socket
/// connection, matching hyprctl; there is no persistent state to corrupt.
#[derive(Debug, Clone)]
pub struct Hyprland {
    socket: PathBuf,
    timeout: Duration,
}
impl Hyprland {
    /// The instance named by the environment, or `None` outside Hyprland.
    pub fn from_env() -> Option<Self> {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
        let socket = PathBuf::from(runtime)
            .join("hypr")
            .join(signature)
            .join(".socket.sock");
        socket.exists().then_some(Self {
            socket,
            timeout: Duration::from_secs(5),
        })
    }
    /// The instance's event stream (`.socket2.sock`), one `NAME>>DATA` line
    /// per event, in non-blocking mode so a poll loop can include its fd.
    pub fn events(&self) -> Result<UnixStream> {
        let stream =
            UnixStream::connect(self.socket.with_file_name(".socket2.sock")).map_err(fail)?;
        stream.set_nonblocking(true).map_err(fail)?;
        Ok(stream)
    }
    pub fn request(&self, command: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket).map_err(fail)?;
        stream.set_read_timeout(Some(self.timeout)).map_err(fail)?;
        stream.set_write_timeout(Some(self.timeout)).map_err(fail)?;
        stream.write_all(command.as_bytes()).map_err(fail)?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).map_err(fail)?;
        Ok(reply)
    }
    fn json<T: for<'de> Deserialize<'de>>(&self, command: &str) -> Result<T> {
        let reply = self.request(&format!("j/{command}"))?;
        serde_json::from_str(reply.trim()).map_err(|e| fail(format!("{command}: {e}")))
    }
    pub fn clients(&self) -> Result<Vec<Client>> {
        self.json("clients")
    }
    pub fn monitors(&self) -> Result<Vec<Monitor>> {
        self.json("monitors")
    }
    pub fn cursor_position(&self) -> Result<CursorPosition> {
        self.json("cursorpos")
    }
    pub fn active_window(&self) -> Result<Option<Client>> {
        let value: Value = self.json("activewindow")?;
        if value.get("address").is_none() {
            return Ok(None);
        }
        serde_json::from_value(value).map(Some).map_err(fail)
    }
    pub fn version(&self) -> Result<Value> {
        self.json("version")
    }
    pub fn client_by_address(&self, address: &str) -> Result<Option<Client>> {
        Ok(self.clients()?.into_iter().find(|c| c.address == address))
    }
    /// The window with a 32-bit protocol handle owned by a pid.
    pub fn client_by_handle(&self, handle: u32, pid: i64) -> Result<Option<Client>> {
        Ok(self
            .clients()?
            .into_iter()
            .find(|c| c.handle() == Some(handle) && c.pid == pid))
    }
    /// The unique mapped window of a pid with this title, or the pid's only
    /// mapped window when titles differ (Xwayland, decorations). Ambiguity
    /// yields `None` rather than a guess.
    pub fn client_for(&self, pid: i64, title: &str) -> Result<Option<Client>> {
        Ok(client_for(&self.clients()?, pid, title))
    }
    /// Whether a window is on the workspace currently shown on its monitor.
    /// Pinned windows and special workspaces are reported as visible.
    pub fn is_on_active_workspace(&self, client: &Client) -> Result<bool> {
        Ok(is_on_active_workspace(client, &self.monitors()?))
    }
    /// Runs a dispatcher expression such as `hl.dsp.window.close()`; the
    /// compositor wraps it in `hl.dispatch(...)`. Anything but `ok` is an error.
    pub fn dispatch(&self, lua: &str) -> Result<()> {
        let reply = self.request(&format!("dispatch {lua}"))?;
        if reply.trim() == "ok" {
            Ok(())
        } else {
            Err(fail(reply.trim().to_owned()).with_effect(Effect::Unknown))
        }
    }
    /// Starts a shell command with window rule effects applied to the
    /// windows its process opens. Rules use the Hyprland Lua table syntax.
    pub fn exec_with_rules(&self, command: &str, rules: &str) -> Result<()> {
        self.dispatch(&format!(
            "hl.dsp.exec_cmd({}, {{ {rules} }})",
            lua_string(command)
        ))
    }
}

/// Selection rule shared by every pid-and-title lookup; see `Hyprland::client_for`.
pub fn client_for(clients: &[Client], pid: i64, title: &str) -> Option<Client> {
    let mine: Vec<&Client> = clients
        .iter()
        .filter(|c| c.pid == pid && c.mapped)
        .collect();
    let titled: Vec<&&Client> = mine.iter().filter(|c| c.title == title).collect();
    match titled.as_slice() {
        [one] => Some((**one).clone()),
        [] if mine.len() == 1 => Some(mine[0].clone()),
        _ => None,
    }
}
/// See `Hyprland::is_on_active_workspace`, with monitors fetched once by the caller.
pub fn is_on_active_workspace(client: &Client, monitors: &[Monitor]) -> bool {
    client.pinned
        || monitors.iter().any(|m| {
            m.id == client.monitor
                && (m.active_workspace.id == client.workspace.id
                    || m.special_workspace.id == client.workspace.id)
        })
}

/// Where a window sits in Hyprland's render order: tiled windows first, then
/// maximized, floating, pinned and fullscreen windows. Within a tier the
/// `clients` list order is the stacking order, bottom first.
fn stacking_tier(client: &Client) -> u8 {
    match (client.fullscreen, client.pinned, client.floating) {
        (2, _, _) => 4,
        (_, true, _) => 3,
        (_, _, true) => 2,
        (1, _, _) => 1,
        _ => 0,
    }
}

/// What covers a window on screen, in Hyprland's render order.
#[derive(Debug, Clone, PartialEq)]
pub enum Stacking {
    /// Another window is fullscreen on the target's workspace.
    Covered,
    /// Rectangles of the visible windows rendered above the target.
    Above(Vec<Rect>),
}

/// The visible windows rendered above `target`, so an overlay attached to it
/// can hide under them the way a window-level attachment would. `clients` is
/// the `hyprctl clients` list in its original order.
pub fn stacking_above(target: &Client, clients: &[Client], monitors: &[Monitor]) -> Stacking {
    let tier = stacking_tier(target);
    let index = clients.iter().position(|c| c.address == target.address);
    let mut above = vec![];
    for (i, client) in clients.iter().enumerate() {
        if client.address == target.address
            || !client.mapped
            || client.hidden
            || client.monitor != target.monitor
            || !is_on_active_workspace(client, monitors)
        {
            continue;
        }
        if client.fullscreen == 2 && client.workspace.id == target.workspace.id {
            return Stacking::Covered;
        }
        let other = stacking_tier(client);
        if other > tier || (other == tier && index.is_some_and(|t| i > t)) {
            above.push(client.rect());
        }
    }
    Stacking::Above(above)
}

/// Quotes arbitrary text as a Lua string literal.
pub fn lua_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn client(address: &str, at: [i64; 2], floating: bool) -> Client {
        Client {
            address: address.into(),
            mapped: true,
            at,
            size: [100, 100],
            workspace: Workspace {
                id: 2,
                name: "2".into(),
            },
            floating,
            ..serde_json::from_str(r#"{"address":""}"#).unwrap()
        }
    }
    fn monitor() -> Monitor {
        serde_json::from_str(
            r#"{"id":0,"name":"eDP-1","width":1920,"height":1200,"x":0,"y":0,"scale":1.0,"transform":0,
            "focused":true,"activeWorkspace":{"id":2,"name":"2"},"specialWorkspace":{"id":0,"name":""}}"#,
        )
        .unwrap()
    }
    #[test]
    fn stacking_follows_tiers_then_list_order() {
        let tiled = client("0x1", [0, 0], false);
        let float_low = client("0x2", [10, 10], true);
        let float_high = client("0x3", [20, 20], true);
        let hidden = Client {
            hidden: true,
            ..client("0x4", [30, 30], true)
        };
        let elsewhere = Client {
            workspace: Workspace {
                id: 5,
                name: "5".into(),
            },
            ..client("0x5", [40, 40], true)
        };
        let clients = [
            float_high.clone(),
            tiled.clone(),
            float_low.clone(),
            hidden,
            elsewhere,
        ];
        let monitors = [monitor()];
        assert_eq!(
            stacking_above(&tiled, &clients, &monitors),
            Stacking::Above(vec![float_high.rect(), float_low.rect()])
        );
        assert_eq!(
            stacking_above(&float_high, &clients, &monitors),
            Stacking::Above(vec![float_low.rect()])
        );
        assert_eq!(
            stacking_above(&float_low, &clients, &monitors),
            Stacking::Above(vec![])
        );
        let fullscreen = Client {
            fullscreen: 2,
            ..client("0x6", [0, 0], false)
        };
        assert_eq!(
            stacking_above(&tiled, &[tiled.clone(), fullscreen.clone()], &monitors),
            Stacking::Covered
        );
        assert_eq!(
            stacking_above(&fullscreen, &[tiled.clone(), fullscreen.clone()], &monitors),
            Stacking::Above(vec![])
        );
    }
    #[test]
    fn parses_client_and_monitor_records() {
        let clients: Vec<Client> = serde_json::from_str(
            r#"[{"address":"0x58d5bd33b570","mapped":true,"hidden":false,"at":[5,31],"size":[1526,924],
            "workspace":{"id":2,"name":"2"},"floating":false,"monitor":0,"class":"ghostty","title":"t",
            "pid":100,"xwayland":false,"pinned":false,"fullscreen":0,"focusHistoryID":3,"extra":1}]"#,
        )
        .unwrap();
        assert_eq!(clients[0].handle(), Some(0xbd33b570));
        assert!(clients[0].contains(&Point { x: 5., y: 31. }));
        assert!(!clients[0].contains(&Point { x: 1531., y: 31. }));
        assert_eq!(
            client_for(&clients, 100, "other").map(|c| c.address),
            Some("0x58d5bd33b570".into())
        );
        assert!(client_for(&clients, 7, "t").is_none());
        let monitor: Monitor = serde_json::from_str(
            r#"{"id":0,"name":"eDP-1","width":1920,"height":1200,"x":0,"y":0,"scale":1.25,"transform":0,
            "focused":true,"activeWorkspace":{"id":2,"name":"2"},"specialWorkspace":{"id":0,"name":""}}"#,
        )
        .unwrap();
        assert_eq!(monitor.logical_size(), (1536., 960.));
        assert!(monitor.contains(&Point { x: 1535., y: 959. }));
        assert!(!monitor.contains(&Point { x: 1536., y: 0. }));
        assert!(is_on_active_workspace(
            &clients[0],
            std::slice::from_ref(&monitor)
        ));
        assert!(!is_on_active_workspace(
            &Client {
                workspace: Workspace {
                    id: 9,
                    name: "9".into()
                },
                ..clients[0].clone()
            },
            &[monitor]
        ));
    }
    #[test]
    fn lua_strings_are_escaped() {
        assert_eq!(lua_string("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }
    #[test]
    #[ignore = "requires a running Hyprland instance"]
    fn live_queries() {
        let hypr = Hyprland::from_env().expect("Hyprland");
        assert!(!hypr.monitors().unwrap().is_empty());
        hypr.clients().unwrap();
        hypr.cursor_position().unwrap();
    }
}
