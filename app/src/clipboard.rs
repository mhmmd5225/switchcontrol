//! Clipboard sharing: when control passes to the other computer, the text on
//! this clipboard goes with it.
//!
//! Only text, at most [`MAX_CLIPBOARD`] bytes. The same text is not sent
//! twice in a row, and text received from the peer is not sent back to it.
//! Logs give sizes only, never the text itself.

use sc_proto::{MAX_CLIPBOARD, Message};

/// The system clipboard.
pub trait Backend {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: String) -> Result<(), String>;
}

/// The OS clipboard through `arboard`, opened on first use. On Linux the
/// object must stay alive for the text it set to remain available.
#[derive(Default)]
pub struct System {
    clipboard: Option<arboard::Clipboard>,
}

impl System {
    fn open(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        Ok(self.clipboard.as_mut().expect("just opened"))
    }
}

impl Backend for System {
    fn get_text(&mut self) -> Result<String, String> {
        self.open()?.get_text().map_err(|e| e.to_string())
    }

    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.open()?.set_text(text).map_err(|e| e.to_string())
    }
}

pub struct Share<B> {
    backend: B,
    enabled: bool,
    /// The text last sent to or received from the peer.
    last: Option<String>,
}

impl<B: Backend> Share<B> {
    pub fn new(backend: B, enabled: bool) -> Self {
        Share { backend, enabled, last: None }
    }

    /// The message carrying this clipboard's text to the peer, if it should
    /// be sent; otherwise why not, for the log (`None` when nothing to say).
    pub fn outgoing(&mut self) -> Result<Message, Option<String>> {
        if !self.enabled {
            return Err(None);
        }
        let text = match self.backend.get_text() {
            Ok(text) => text,
            // Empty, or not text (e.g. an image): nothing to share.
            Err(_) => return Err(None),
        };
        if text.is_empty() || self.last.as_ref() == Some(&text) {
            return Err(None);
        }
        if text.len() > MAX_CLIPBOARD {
            return Err(Some(format!(
                "clipboard not shared: {} KB is over the {} KB limit",
                text.len() / 1024,
                MAX_CLIPBOARD / 1024
            )));
        }
        self.last = Some(text.clone());
        Ok(Message::Clipboard { text })
    }

    /// Puts the peer's text on this clipboard. Returns a line for the log.
    pub fn incoming(&mut self, text: String) -> Option<String> {
        if !self.enabled || text.len() > MAX_CLIPBOARD {
            return None;
        }
        let chars = text.chars().count();
        self.last = Some(text.clone());
        Some(match self.backend.set_text(text) {
            Ok(()) => format!("clipboard received ({chars} characters)"),
            Err(e) => format!("could not set the clipboard: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        text: Option<String>,
    }

    impl Backend for Fake {
        fn get_text(&mut self) -> Result<String, String> {
            self.text.clone().ok_or_else(|| "empty".into())
        }

        fn set_text(&mut self, text: String) -> Result<(), String> {
            self.text = Some(text);
            Ok(())
        }
    }

    fn share(text: Option<&str>) -> Share<Fake> {
        Share::new(Fake { text: text.map(String::from) }, true)
    }

    fn sent(share: &mut Share<Fake>) -> Option<String> {
        match share.outgoing() {
            Ok(Message::Clipboard { text }) => Some(text),
            _ => None,
        }
    }

    #[test]
    fn sends_new_text_once() {
        let mut share = share(Some("merhaba"));
        assert_eq!(sent(&mut share).as_deref(), Some("merhaba"));
        assert_eq!(sent(&mut share), None);
        share.backend.text = Some("dünya".into());
        assert_eq!(sent(&mut share).as_deref(), Some("dünya"));
    }

    #[test]
    fn does_not_echo_received_text() {
        let mut share = share(None);
        assert!(share.incoming("from the peer".into()).is_some());
        assert_eq!(share.backend.text.as_deref(), Some("from the peer"));
        assert_eq!(sent(&mut share), None);
    }

    #[test]
    fn nothing_to_send_when_empty_or_disabled() {
        assert_eq!(sent(&mut share(None)), None);
        assert_eq!(sent(&mut share(Some(""))), None);
        let mut off = Share::new(Fake { text: Some("x".into()) }, false);
        assert_eq!(sent(&mut off), None);
        assert!(off.incoming("y".into()).is_none());
        assert_eq!(off.backend.text.as_deref(), Some("x"));
    }

    #[test]
    fn large_text_is_refused_with_a_reason() {
        let mut share = share(Some(&"a".repeat(MAX_CLIPBOARD + 1)));
        assert!(matches!(share.outgoing(), Err(Some(_))));
    }
}
