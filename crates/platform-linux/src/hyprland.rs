use std::{env, path::PathBuf};

use hyprbreathe_core::{MonitorIdentity, OutputInfo};
use serde::Deserialize;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

#[derive(Debug, Error)]
pub enum HyprlandError {
    #[error("HYPRLAND_INSTANCE_SIGNATURE is unavailable")]
    MissingSignature,
    #[error("XDG_RUNTIME_DIR is unavailable")]
    MissingRuntimeDir,
    #[error("Hyprland IPC failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Hyprland returned invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Hyprland command failed: {0}")]
    Command(String),
    #[error("invalid Hyprland submap name: {0}")]
    InvalidSubmap(String),
}

#[derive(Debug, Clone)]
pub struct HyprlandClient {
    command_socket: PathBuf,
    event_socket: PathBuf,
}

impl HyprlandClient {
    pub fn from_env() -> Result<Self, HyprlandError> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or(HyprlandError::MissingRuntimeDir)?;
        let signature =
            env::var_os("HYPRLAND_INSTANCE_SIGNATURE").ok_or(HyprlandError::MissingSignature)?;
        let base = runtime.join("hypr").join(signature);
        Ok(Self {
            command_socket: base.join(".socket.sock"),
            event_socket: base.join(".socket2.sock"),
        })
    }

    pub fn available(&self) -> bool {
        self.command_socket.exists() && self.event_socket.exists()
    }

    pub fn event_socket(&self) -> &PathBuf {
        &self.event_socket
    }

    pub async fn outputs(&self) -> Result<Vec<OutputInfo>, HyprlandError> {
        let monitors: Vec<HyprMonitor> = self.query_json("monitors all").await?;
        Ok(monitors.into_iter().map(OutputInfo::from).collect())
    }

    pub async fn displays_off(&self) -> Result<bool, HyprlandError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct DisplayPower {
            disabled: bool,
            dpms_status: bool,
        }

        let monitors: Vec<DisplayPower> = self.query_json("monitors all").await?;
        Ok(monitors
            .iter()
            .all(|monitor| monitor.disabled || !monitor.dpms_status))
    }

    pub async fn cursor_position(&self) -> Result<(i32, i32), HyprlandError> {
        let cursor: CursorPosition = self.query_json("cursorpos").await?;
        Ok((cursor.x.round() as i32, cursor.y.round() as i32))
    }

    pub async fn locked(&self) -> Result<bool, HyprlandError> {
        #[derive(Deserialize)]
        struct Locked {
            locked: bool,
        }
        Ok(self.query_json::<Locked>("locked").await?.locked)
    }

    pub async fn current_submap(&self) -> Result<String, HyprlandError> {
        self.query_json("submap").await
    }

    pub async fn submap_exists(&self, name: &str) -> Result<bool, HyprlandError> {
        validate_submap_name(name)?;
        let bindings: Vec<HyprBind> = self.query_json("binds").await?;
        Ok(bindings.iter().any(|binding| binding.submap == name))
    }

    pub async fn ensure_submap(&self, name: &str) -> Result<(), HyprlandError> {
        validate_submap_name(name)?;
        if self.submap_exists(name).await? {
            return Ok(());
        }

        let setup_result = async {
            self.command(&format!("keyword submap {name}")).await?;
            self.command("keyword bind CTRL_ALT_SHIFT_SUPER, F24, exec, true")
                .await?;
            Ok::<(), HyprlandError>(())
        }
        .await;
        let reset_result = self.command("keyword submap reset").await;
        setup_result?;
        reset_result?;

        if !self.submap_exists(name).await? {
            return Err(HyprlandError::Command(format!(
                "submap {name} was not registered"
            )));
        }
        Ok(())
    }

    pub async fn set_submap(&self, name: &str) -> Result<(), HyprlandError> {
        validate_submap_name(name)?;
        self.command(&format!("dispatch submap {name}")).await?;
        Ok(())
    }

    pub async fn reset_submap_if_active(&self, name: &str) -> Result<bool, HyprlandError> {
        validate_submap_name(name)?;
        if self.current_submap().await? != name {
            return Ok(false);
        }
        self.set_submap("reset").await?;
        Ok(true)
    }

    pub async fn query_json<T>(&self, command: &str) -> Result<T, HyprlandError>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut stream = UnixStream::connect(&self.command_socket).await?;
        stream.write_all(format!("j/{command}").as_bytes()).await?;
        stream.shutdown().await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(serde_json::from_slice(&response)?)
    }

    async fn command(&self, command: &str) -> Result<String, HyprlandError> {
        let mut stream = UnixStream::connect(&self.command_socket).await?;
        stream.write_all(command.as_bytes()).await?;
        stream.shutdown().await?;
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;
        let response = response.trim().to_owned();
        if response.is_empty() || response.lines().any(|line| line.trim() != "ok") {
            return Err(HyprlandError::Command(response));
        }
        Ok(response)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HyprMonitor {
    name: String,
    description: String,
    make: String,
    model: String,
    serial: String,
    width: u32,
    height: u32,
    physical_width: u32,
    physical_height: u32,
    refresh_rate: f64,
    x: i32,
    y: i32,
    scale: f64,
    transform: i32,
    focused: bool,
    disabled: bool,
}

impl From<HyprMonitor> for OutputInfo {
    fn from(monitor: HyprMonitor) -> Self {
        Self {
            identity: MonitorIdentity {
                connector: Some(monitor.name),
                make: nonempty(monitor.make),
                model: nonempty(monitor.model),
                serial: nonempty(monitor.serial),
                description: nonempty(monitor.description),
                physical_mm: Some((monitor.physical_width, monitor.physical_height)),
            },
            width: monitor.width,
            height: monitor.height,
            x: monitor.x,
            y: monitor.y,
            scale: monitor.scale,
            transform: monitor.transform,
            refresh_hz: monitor.refresh_rate,
            focused: monitor.focused,
            enabled: !monitor.disabled,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CursorPosition {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct HyprBind {
    submap: String,
}

fn validate_submap_name(name: &str) -> Result<(), HyprlandError> {
    if !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        Ok(())
    } else {
        Err(HyprlandError::InvalidSubmap(name.into()))
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn reported_power(response: &'static str) -> Result<bool, HyprlandError> {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("hyprland.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"j/monitors all");
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let client = HyprlandClient {
            command_socket: socket,
            event_socket: directory.path().join("events.sock"),
        };
        let result = client.displays_off().await;
        server.await.unwrap();
        result
    }

    #[tokio::test]
    async fn display_idle_requires_every_output_to_be_unusable() {
        let mixed = r#"[
            {"disabled": false, "dpmsStatus": true},
            {"disabled": false, "dpmsStatus": false}
        ]"#;
        assert!(!reported_power(mixed).await.unwrap());
        let all_off = r#"[
            {"disabled": false, "dpmsStatus": false},
            {"disabled": true, "dpmsStatus": true}
        ]"#;
        assert!(reported_power(all_off).await.unwrap());
        assert!(reported_power("[]").await.unwrap());
    }

    #[tokio::test]
    async fn unknown_power_is_an_error_not_idle() {
        assert!(reported_power(r#"[{"disabled": false}]"#).await.is_err());
        assert!(reported_power("invalid response").await.is_err());
    }

    #[test]
    fn validates_submap_names_before_ipc_use() {
        assert!(validate_submap_name("hyprbreathe").is_ok());
        assert!(validate_submap_name("break-reminder_2").is_ok());
        assert!(validate_submap_name("").is_err());
        assert!(validate_submap_name("hyprbreathe;dispatch workspace 1").is_err());
    }
}
