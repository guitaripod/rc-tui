use crate::theme;

/// Starts following the system light/dark setting; the palette only moves while the theme is `auto`.
pub fn follow() {
    platform::follow();
}

#[cfg(target_os = "linux")]
mod platform {
    use futures_util::StreamExt;
    use zbus::zvariant::{OwnedValue, Value};
    use zbus::{Connection, MatchRule, MessageStream};

    use super::theme;

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const INTERFACE: &str = "org.freedesktop.portal.Settings";
    const NAMESPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "color-scheme";

    pub fn follow() {
        let _ = std::thread::Builder::new()
            .name("appearance".into())
            .spawn(|| {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(watch());
            });
    }

    /// Reports each change of the desktop colour scheme until the session bus goes away.
    async fn watch() {
        let Ok(connection) = Connection::session().await else {
            return;
        };
        let Ok(rule) = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(INTERFACE)
            .and_then(|builder| builder.member("SettingChanged"))
            .map(|builder| builder.build())
        else {
            return;
        };
        let Ok(mut stream) = MessageStream::for_match_rule(rule, &connection, None).await else {
            return;
        };
        let mut known = read(&connection).await.map(is_light);
        while let Some(Ok(message)) = stream.next().await {
            let Ok((namespace, key, value)) =
                message.body().deserialize::<(String, String, OwnedValue)>()
            else {
                continue;
            };
            if namespace != NAMESPACE || key != KEY {
                continue;
            }
            let Some(light) = scheme(&value).map(is_light) else {
                continue;
            };
            if known != Some(light) {
                known = Some(light);
                theme::system_changed(light);
            }
        }
    }

    async fn read(connection: &Connection) -> Option<u32> {
        let reply = connection
            .call_method(
                Some(DESTINATION),
                PATH,
                Some(INTERFACE),
                "Read",
                &(NAMESPACE, KEY),
            )
            .await
            .ok()?;
        let value = reply.body().deserialize::<OwnedValue>().ok()?;
        scheme(&value)
    }

    /// The portal wraps the setting in one or two variants depending on its version.
    fn scheme(value: &Value) -> Option<u32> {
        match value {
            Value::U32(scheme) => Some(*scheme),
            Value::Value(inner) => scheme(inner),
            _ => None,
        }
    }

    /// 0 is "no preference", 1 prefers dark and 2 prefers light.
    fn is_light(scheme: u32) -> bool {
        scheme != 1
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_an_explicit_dark_preference_is_dark() {
            assert!(!is_light(1));
            assert!(is_light(2));
            assert!(is_light(0));
        }

        #[test]
        fn nested_variants_are_unwrapped() {
            let nested = Value::Value(Box::new(Value::Value(Box::new(Value::U32(1)))));
            assert_eq!(scheme(&nested), Some(1));
            assert_eq!(scheme(&Value::U32(2)), Some(2));
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;
    use std::time::Duration;

    use super::theme;

    const INTERVAL: Duration = Duration::from_secs(2);

    pub fn follow() {
        let _ = std::thread::Builder::new()
            .name("appearance".into())
            .spawn(|| {
                let mut known = is_light();
                loop {
                    std::thread::sleep(INTERVAL);
                    let light = is_light();
                    if light != known {
                        known = light;
                        theme::system_changed(light);
                    }
                }
            });
    }

    /// `AppleInterfaceStyle` only exists while the system is dark.
    fn is_light() -> bool {
        Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .map(|output| !String::from_utf8_lossy(&output.stdout).contains("Dark"))
            .unwrap_or(true)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    pub fn follow() {}
}
