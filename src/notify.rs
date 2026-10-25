use std::path::Path;

use tokio::sync::mpsc::UnboundedSender;

/// What someone did with a moment's notification; only the freedesktop server reports these.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum Action {
    Open(u32),
    Reply(u32, String),
    Closed(u32),
}

/// One desktop notification, independent of how the platform shows it; Notification Center ignores the attachments.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct Notice<'a> {
    pub summary: &'a str,
    pub body: &'a str,
    pub quote: Option<&'a str>,
    pub icon: Option<&'a Path>,
    pub card: Option<&'a Path>,
    pub silent: bool,
    pub reply: bool,
    pub timeout_ms: i32,
}

#[cfg(target_os = "linux")]
pub use linux::Notifier;

#[cfg(not(target_os = "linux"))]
pub use other::Notifier;

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;

    use futures_util::StreamExt;
    use zbus::zvariant::{OwnedValue, Value};
    use zbus::{Connection, MatchRule, MessageStream};

    use super::{Action, Notice, UnboundedSender};

    const DESTINATION: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";
    const INTERFACE: &str = "org.freedesktop.Notifications";
    const REPLY_PLACEHOLDER: &str = "What made this happen?";

    /// The freedesktop notification server on the session bus (Plasma, GNOME, mako, dunst).
    pub struct Notifier {
        connection: Connection,
        capabilities: Vec<String>,
    }

    impl Notifier {
        pub async fn connect() -> Option<Self> {
            let connection = Connection::session().await.ok()?;
            let capabilities = connection
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some(INTERFACE),
                    "GetCapabilities",
                    &(),
                )
                .await
                .ok()?
                .body()
                .deserialize::<Vec<String>>()
                .ok()?;
            Some(Self {
                connection,
                capabilities,
            })
        }

        fn can(&self, capability: &str) -> bool {
            self.capabilities.iter().any(|known| known == capability)
        }

        /// Whether do-not-disturb is on; servers without the property count as not inhibited.
        pub async fn inhibited(&self) -> bool {
            let Ok(reply) = self
                .connection
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &(INTERFACE, "Inhibited"),
                )
                .await
            else {
                return false;
            };
            reply
                .body()
                .deserialize::<OwnedValue>()
                .ok()
                .and_then(|value| bool::try_from(value).ok())
                .unwrap_or(false)
        }

        pub async fn show(&self, notice: &Notice<'_>) -> Option<u32> {
            let markup = self.can("body-markup");
            let mut body = if markup {
                escape(notice.body)
            } else {
                notice.body.to_string()
            };
            if let Some(quote) = notice.quote {
                if markup {
                    body.push_str(&format!("\n<i>“{}”</i>", escape(quote)));
                } else {
                    body.push_str(&format!("\n“{quote}”"));
                }
            }
            let mut actions: Vec<&str> = vec!["default", "Open rc-tui", "open", "Open rc-tui"];
            let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
            hints.insert("desktop-entry", Value::from("rc-tui"));
            hints.insert("urgency", Value::from(1u8));
            hints.insert("category", Value::from("x-rc-tui.moment"));
            if notice.silent {
                hints.insert("suppress-sound", Value::from(true));
            }
            let icon = notice
                .icon
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !icon.is_empty() {
                hints.insert("image-path", Value::from(icon.clone()));
            }
            if let Some(card) = notice.card {
                hints.insert(
                    "x-kde-urls",
                    Value::from(vec![format!("file://{}", card.to_string_lossy())]),
                );
            }
            if notice.reply && self.can("inline-reply") {
                actions.extend(["inline-reply", "Add a note"]);
                hints.insert(
                    "x-kde-reply-placeholder-text",
                    Value::from(REPLY_PLACEHOLDER),
                );
            }
            let reply = self
                .connection
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some(INTERFACE),
                    "Notify",
                    &(
                        "rc-tui",
                        0u32,
                        icon.as_str(),
                        notice.summary,
                        body.as_str(),
                        actions,
                        hints,
                        notice.timeout_ms,
                    ),
                )
                .await
                .ok()?;
            reply.body().deserialize::<u32>().ok()
        }

        /// Forwards clicks, inline replies and dismissals to `sender` until the bus goes away.
        pub async fn listen(&self, sender: UnboundedSender<Action>) {
            let Ok(rule) = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(INTERFACE)
                .and_then(|builder| builder.path(PATH))
                .map(|builder| builder.build())
            else {
                return;
            };
            let Ok(mut stream) = MessageStream::for_match_rule(rule, &self.connection, None).await
            else {
                return;
            };
            tokio::spawn(async move {
                while let Some(Ok(message)) = stream.next().await {
                    let header = message.header();
                    let Some(member) = header.member() else {
                        continue;
                    };
                    let action = match member.as_str() {
                        "ActionInvoked" => message
                            .body()
                            .deserialize::<(u32, String)>()
                            .ok()
                            .filter(|(_, key)| key == "default" || key == "open")
                            .map(|(id, _)| Action::Open(id)),
                        "NotificationReplied" => message
                            .body()
                            .deserialize::<(u32, String)>()
                            .ok()
                            .map(|(id, text)| Action::Reply(id, text)),
                        "NotificationClosed" => message
                            .body()
                            .deserialize::<(u32, u32)>()
                            .ok()
                            .map(|(id, _)| Action::Closed(id)),
                        _ => None,
                    };
                    if let Some(action) = action
                        && sender.send(action).is_err()
                    {
                        break;
                    }
                }
            });
        }
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
}

#[cfg(not(target_os = "linux"))]
mod other {
    use std::process::Command;

    use super::{Action, Notice, UnboundedSender};

    /// macOS Notification Center through `osascript`; it has no actions to listen for.
    pub struct Notifier;

    impl Notifier {
        pub async fn connect() -> Option<Self> {
            cfg!(target_os = "macos").then_some(Self)
        }

        pub async fn inhibited(&self) -> bool {
            false
        }

        pub async fn show(&self, notice: &Notice<'_>) -> Option<u32> {
            let quote = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
            let mut script = format!(
                "display notification \"{}\" with title \"rc-tui\" subtitle \"{}\"",
                quote(notice.body),
                quote(notice.summary)
            );
            if !notice.silent {
                script.push_str(" sound name \"Glass\"");
            }
            Command::new("osascript")
                .args(["-e", &script])
                .status()
                .ok()
                .filter(|status| status.success())
                .map(|_| 0)
        }

        pub async fn listen(&self, sender: UnboundedSender<Action>) {
            drop(sender);
        }
    }
}
