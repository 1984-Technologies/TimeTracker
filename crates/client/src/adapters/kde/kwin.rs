use std::{path::PathBuf, time::Duration};

use uuid::Uuid;
use zbus::{Connection, proxy};

use crate::ClientError;

const DBUS_TIMEOUT: Duration = Duration::from_secs(2);

#[proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
trait KWinScripting {
    #[zbus(name = "loadScript")]
    fn load_script(&self, file_path: &str, plugin_name: &str) -> zbus::Result<i32>;

    #[zbus(name = "unloadScript")]
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;

    #[zbus(name = "isScriptLoaded")]
    fn is_script_loaded(&self, plugin_name: &str) -> zbus::Result<bool>;
}

#[proxy(interface = "org.kde.kwin.Script", default_service = "org.kde.KWin")]
trait KWinScript {
    #[zbus(name = "run")]
    fn run(&self) -> zbus::Result<()>;

    #[zbus(name = "stop")]
    fn stop(&self) -> zbus::Result<()>;
}

pub(super) async fn probe_scripting() -> Result<(), ClientError> {
    tokio::time::timeout(DBUS_TIMEOUT, async {
        let connection = Connection::session().await?;
        let scripting = KWinScriptingProxy::new(&connection).await?;
        let _ = scripting.is_script_loaded("timetracker_probe").await?;
        Ok::<(), ClientError>(())
    })
    .await?
}

pub(super) struct KWinScriptSession {
    connection: Connection,
    plugin_name: String,
    script_path: PathBuf,
    script_id: i32,
}

impl KWinScriptSession {
    pub(super) async fn start(
        connection: Connection,
        plugin_prefix: &str,
        script_body: &str,
    ) -> Result<Self, ClientError> {
        let plugin_name = format!("{plugin_prefix}_{}", Uuid::new_v4().simple());
        let script_path = std::env::temp_dir().join(format!("{plugin_name}.js"));
        std::fs::write(&script_path, script_body)?;

        let load_result: Result<i32, ClientError> = tokio::time::timeout(DBUS_TIMEOUT, async {
            let scripting = KWinScriptingProxy::new(&connection).await?;
            let path_str = script_path
                .to_str()
                .ok_or("temporary KWin script path is not valid UTF-8")?;
            let script_id = scripting.load_script(path_str, &plugin_name).await?;
            if script_id < 0 {
                return Err(format!("KWin rejected script '{plugin_name}'").into());
            }
            let object_path = format!("/Scripting/Script{script_id}");
            let script = KWinScriptProxy::builder(&connection)
                .path(object_path)?
                .build()
                .await?;
            script.run().await?;
            Ok(script_id)
        })
        .await
        .unwrap_or_else(|_| Err("timed out starting KWin script".into()));

        match load_result {
            Ok(script_id) => Ok(Self {
                connection,
                plugin_name,
                script_path,
                script_id,
            }),
            Err(error) => {
                let _ = std::fs::remove_file(&script_path);
                Err(error)
            }
        }
    }

    pub(super) async fn stop(self) {
        let object_path = format!("/Scripting/Script{}", self.script_id);
        let _ = tokio::time::timeout(DBUS_TIMEOUT, async {
            if let Ok(builder) = KWinScriptProxy::builder(&self.connection).path(object_path)
                && let Ok(script) = builder.build().await
            {
                let _ = script.stop().await;
            }
            if let Ok(scripting) = KWinScriptingProxy::new(&self.connection).await {
                let _ = scripting.unload_script(&self.plugin_name).await;
            }
        })
        .await;
        let _ = std::fs::remove_file(&self.script_path);
    }
}

impl Drop for KWinScriptSession {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.script_path);
    }
}
