//! obs-websocket v5 client (spec 01 §9, 05 §4.1). One short connection per action.

use std::net::TcpStream;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsConfig {
    pub port: u16,
    pub password: String,
}

impl Default for ObsConfig {
    fn default() -> Self {
        ObsConfig { port: 4455, password: String::new() }
    }
}

/// The OBS command vocabulary (exact strings, 05 §4.1).
pub const COMMANDS: &[&str] = &[
    "Next Profile",
    "Previous Profile",
    "Set Current Profile",
    "Next Scene",
    "Previous Scene",
    "Set Current Scene",
    "Set Current Source",
    "Next Transition",
    "Previous Transition",
    "Set Current TransitionName",
    "Set Transition Duration",
    "Mic Volume +",
    "Mic Volume -",
    "Mute Mic",
    "Unmute Mic",
    "Set Mic Volume",
    "Desktop Volume +",
    "Desktop Volume \u{2013}",
    "Mute Desktop Volume",
    "Unmute Desktop Volume",
    "Set Desktop Volume",
    "Start Streaming",
    "Stop Streaming",
    "Start Recording",
    "Stop Recording",
    "Pause Recording",
    "Resume Recording",
    "Start Reply Buffer",
    "Stop Reply Buffer",
    "Save Reply Buffer",
    "Enable Studio Mode",
    "Disable Studio Mode",
    "Open Projector",
    "Play Media",
    "Pause Media",
    "Stop Media",
    "Next Media",
    "Previous Media",
];

pub const QUERIES: &[&str] = &["Get Profiles", "Get Scenes", "Get Scenes Data", "Get Sources", "Get Sources List", "Get Transitions"];

pub struct Obs {
    cfg: ObsConfig,
}

struct Session {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

fn b64sha(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(s.as_bytes()))
}

/// obs-websocket authentication string.
pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let secret = b64sha(&format!("{password}{salt}"));
    b64sha(&format!("{secret}{challenge}"))
}

fn ws_err(e: tungstenite::Error) -> Error {
    Error::Io(format!("OBS: {e}"))
}

impl Session {
    fn connect(cfg: &ObsConfig) -> Result<Session> {
        let addr = format!("127.0.0.1:{}", cfg.port);
        let stream = TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2))
            .map_err(|e| Error::Io(format!("OBS is not reachable on port {} ({e})", cfg.port)))?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let (ws, _) = tungstenite::client::client(format!("ws://{addr}/"), MaybeTlsStream::Plain(stream))
            .map_err(|e| Error::Io(format!("OBS handshake: {e}")))?;
        let mut s = Session { ws, next_id: 1 };
        let hello = s.recv_op(0)?;
        let mut ident = json!({"rpcVersion": 1, "eventSubscriptions": 0});
        if let Some(auth) = hello["d"].get("authentication") {
            let a = auth_string(&cfg.password, auth["salt"].as_str().unwrap_or(""), auth["challenge"].as_str().unwrap_or(""));
            ident["authentication"] = json!(a);
        }
        s.send(json!({"op": 1, "d": ident}))?;
        s.recv_op(2).map_err(|_| Error::Invalid("OBS refused the connection (check the password)".into()))?;
        Ok(s)
    }

    fn send(&mut self, v: Value) -> Result<()> {
        self.ws.send(Message::Text(v.to_string())).map_err(ws_err)
    }

    fn recv_op(&mut self, op: u64) -> Result<Value> {
        loop {
            match self.ws.read().map_err(ws_err)? {
                Message::Text(t) => {
                    let v: Value = serde_json::from_str(&t)?;
                    if v["op"].as_u64() == Some(op) {
                        return Ok(v);
                    }
                }
                Message::Close(_) => return Err(Error::Io("OBS closed the connection".into())),
                _ => {}
            }
        }
    }

    fn request(&mut self, ty: &str, data: Value) -> Result<Value> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        self.send(json!({"op": 6, "d": {"requestType": ty, "requestId": id, "requestData": data}}))?;
        loop {
            let v = self.recv_op(7)?;
            if v["d"]["requestId"].as_str() != Some(&id) {
                continue;
            }
            let st = &v["d"]["requestStatus"];
            if st["result"].as_bool() != Some(true) {
                return Err(Error::Invalid(format!("OBS {ty} failed: {}", st["comment"].as_str().unwrap_or("error"))));
            }
            return Ok(v["d"].get("responseData").cloned().unwrap_or(Value::Null));
        }
    }

    fn special_input(&mut self, which: &str) -> Result<String> {
        let r = self.request("GetSpecialInputs", json!({}))?;
        r[which].as_str().map(String::from).ok_or_else(|| Error::NotFound(format!("OBS {which} input")))
    }

    fn step_volume(&mut self, input: &str, delta: f64) -> Result<()> {
        let r = self.request("GetInputVolume", json!({"inputName": input}))?;
        let v = (r["inputVolumeMul"].as_f64().unwrap_or(1.0) + delta).clamp(0.0, 1.0);
        self.request("SetInputVolume", json!({"inputName": input, "inputVolumeMul": v})).map(|_| ())
    }

    fn cycle(&mut self, list: &[String], current: &str, forward: bool) -> Option<String> {
        if list.is_empty() {
            return None;
        }
        let i = list.iter().position(|s| s == current).unwrap_or(0);
        let n = list.len();
        Some(list[if forward { (i + 1) % n } else { (i + n - 1) % n }].clone())
    }
}

fn names(v: &Value, key: &str, field: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x[field].as_str().or_else(|| x.as_str()).map(String::from)).collect())
        .unwrap_or_default()
}

fn arg_num(arg: Option<&str>) -> Result<f64> {
    arg.and_then(|a| a.trim().parse().ok()).ok_or_else(|| Error::Invalid("a numeric argument is required".into()))
}

fn need(arg: Option<&str>) -> Result<&str> {
    arg.filter(|a| !a.is_empty()).ok_or_else(|| Error::Invalid("an argument is required".into()))
}

impl Obs {
    pub fn new(cfg: ObsConfig) -> Self {
        Obs { cfg }
    }

    /// Run one command from the vocabulary.
    pub fn run(&self, command: &str, arg: Option<&str>) -> Result<()> {
        let mut s = Session::connect(&self.cfg)?;
        let simple = |s: &mut Session, ty: &str| s.request(ty, json!({})).map(|_| ());
        match command {
            "Next Profile" | "Previous Profile" => {
                let r = s.request("GetProfileList", json!({}))?;
                let list = names(&r, "profiles", "");
                if let Some(p) = s.cycle(&list, r["currentProfileName"].as_str().unwrap_or(""), command.starts_with("Next")) {
                    s.request("SetCurrentProfile", json!({"profileName": p}))?;
                }
            }
            "Set Current Profile" => {
                s.request("SetCurrentProfile", json!({"profileName": need(arg)?}))?;
            }
            "Next Scene" | "Previous Scene" => {
                let r = s.request("GetSceneList", json!({}))?;
                // OBS lists scenes bottom-up; reverse to UI order.
                let mut list = names(&r, "scenes", "sceneName");
                list.reverse();
                if let Some(sc) = s.cycle(&list, r["currentProgramSceneName"].as_str().unwrap_or(""), command.starts_with("Next")) {
                    s.request("SetCurrentProgramScene", json!({"sceneName": sc}))?;
                }
            }
            "Set Current Scene" => {
                s.request("SetCurrentProgramScene", json!({"sceneName": need(arg)?}))?;
            }
            "Set Current Source" => {
                // TODO(spec): the exact meaning is not specified; this toggles the source's
                // visibility in the current program scene.
                let src = need(arg)?;
                let scene = s.request("GetCurrentProgramScene", json!({}))?;
                let scene = scene["currentProgramSceneName"].as_str().unwrap_or("").to_string();
                let id = s.request("GetSceneItemId", json!({"sceneName": scene, "sourceName": src}))?;
                let id = id["sceneItemId"].as_i64().unwrap_or(0);
                let en = s.request("GetSceneItemEnabled", json!({"sceneName": scene, "sceneItemId": id}))?;
                let on = en["sceneItemEnabled"].as_bool().unwrap_or(false);
                s.request("SetSceneItemEnabled", json!({"sceneName": scene, "sceneItemId": id, "sceneItemEnabled": !on}))?;
            }
            "Next Transition" | "Previous Transition" => {
                let r = s.request("GetSceneTransitionList", json!({}))?;
                let list = names(&r, "transitions", "transitionName");
                if let Some(t) = s.cycle(&list, r["currentSceneTransitionName"].as_str().unwrap_or(""), command.starts_with("Next")) {
                    s.request("SetCurrentSceneTransition", json!({"transitionName": t}))?;
                }
            }
            "Set Current TransitionName" => {
                s.request("SetCurrentSceneTransition", json!({"transitionName": need(arg)?}))?;
            }
            "Set Transition Duration" => {
                s.request("SetCurrentSceneTransitionDuration", json!({"transitionDuration": arg_num(arg)? as i64}))?;
            }
            "Mic Volume +" | "Mic Volume -" => {
                let i = s.special_input("mic1")?;
                s.step_volume(&i, if command.ends_with('+') { 0.1 } else { -0.1 })?;
            }
            "Desktop Volume +" | "Desktop Volume \u{2013}" | "Desktop Volume -" => {
                let i = s.special_input("desktop1")?;
                s.step_volume(&i, if command.ends_with('+') { 0.1 } else { -0.1 })?;
            }
            "Mute Mic" | "Unmute Mic" | "Mute Desktop Volume" | "Unmute Desktop Volume" => {
                let i = s.special_input(if command.contains("Mic") { "mic1" } else { "desktop1" })?;
                s.request("SetInputMute", json!({"inputName": i, "inputMuted": command.starts_with("Mute")}))?;
            }
            "Set Mic Volume" | "Set Desktop Volume" => {
                let i = s.special_input(if command.contains("Mic") { "mic1" } else { "desktop1" })?;
                let v = (arg_num(arg)? / 100.0).clamp(0.0, 1.0);
                s.request("SetInputVolume", json!({"inputName": i, "inputVolumeMul": v}))?;
            }
            "Start Streaming" => simple(&mut s, "StartStream")?,
            "Stop Streaming" => simple(&mut s, "StopStream")?,
            "Start Recording" => simple(&mut s, "StartRecord")?,
            "Stop Recording" => simple(&mut s, "StopRecord")?,
            "Pause Recording" => simple(&mut s, "PauseRecord")?,
            "Resume Recording" => simple(&mut s, "ResumeRecord")?,
            "Start Reply Buffer" => simple(&mut s, "StartReplayBuffer")?,
            "Stop Reply Buffer" => simple(&mut s, "StopReplayBuffer")?,
            "Save Reply Buffer" => simple(&mut s, "SaveReplayBuffer")?,
            "Enable Studio Mode" | "Disable Studio Mode" => {
                s.request("SetStudioModeEnabled", json!({"studioModeEnabled": command.starts_with("Enable")}))?;
            }
            "Open Projector" => {
                s.request("OpenVideoMixProjector", json!({"videoMixType": "OBS_WEBSOCKET_VIDEO_MIX_TYPE_PROGRAM"}))?;
            }
            "Play Media" | "Pause Media" | "Stop Media" | "Next Media" | "Previous Media" => {
                let action = match command {
                    "Play Media" => "PLAY",
                    "Pause Media" => "PAUSE",
                    "Stop Media" => "STOP",
                    "Next Media" => "NEXT",
                    _ => "PREVIOUS",
                };
                s.request(
                    "TriggerMediaInputAction",
                    json!({"inputName": need(arg)?, "mediaAction": format!("OBS_WEBSOCKET_MEDIA_INPUT_ACTION_{action}")}),
                )?;
            }
            other => return Err(Error::Invalid(format!("unknown OBS command {other:?}"))),
        }
        Ok(())
    }

    /// UI picker queries.
    pub fn query(&self, q: &str) -> Result<Vec<String>> {
        let mut s = Session::connect(&self.cfg)?;
        Ok(match q {
            "Get Profiles" => names(&s.request("GetProfileList", json!({}))?, "profiles", ""),
            "Get Scenes" | "Get Scenes Data" => {
                let mut v = names(&s.request("GetSceneList", json!({}))?, "scenes", "sceneName");
                v.reverse();
                v
            }
            "Get Sources" | "Get Sources List" => names(&s.request("GetInputList", json!({}))?, "inputs", "inputName"),
            "Get Transitions" => names(&s.request("GetSceneTransitionList", json!({}))?, "transitions", "transitionName"),
            other => return Err(Error::Invalid(format!("unknown OBS query {other:?}"))),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn auth() {
        // Example from the obs-websocket protocol documentation.
        let a = super::auth_string(
            "supersecretpassword",
            "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
            "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=",
        );
        assert_eq!(a, "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
    }
}
