use super::*;
use crate::terminalstate::performer::Performer;
use std::sync::Arc;
use wezterm_escape_parser::parser::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub enum ClipboardSelection {
    Clipboard,
    PrimarySelection,
}

pub trait Clipboard: Send + Sync {
    fn set_contents(
        &self,
        selection: ClipboardSelection,
        data: Option<String>,
    ) -> anyhow::Result<()>;
}

impl Clipboard for Box<dyn Clipboard> {
    fn set_contents(
        &self,
        selection: ClipboardSelection,
        data: Option<String>,
    ) -> anyhow::Result<()> {
        self.as_ref().set_contents(selection, data)
    }
}

pub trait DeviceControlHandler: Send + Sync {
    fn handle_device_control(&mut self, _control: wezterm_escape_parser::DeviceControlMode);
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub enum Progress {
    #[default]
    None,
    Percentage(u8),
    Error(u8),
    Indeterminate,
    /// LBTerm: ConEmu's `9;4;4`, shown yellow at the last percentage (WezTerm dropped it)
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub enum Alert {
    Bell,
    ToastNotification {
        /// The title text for the notification.
        title: Option<String>,
        /// The message body
        body: String,
        /// Whether clicking on the notification should focus the
        /// window/tab/pane that generated it
        focus: bool,
    },
    CurrentWorkingDirectoryChanged,
    IconTitleChanged(Option<String>),
    WindowTitleChanged(String),
    TabTitleChanged(Option<String>),
    /// When the color palette has been updated
    PaletteChanged,
    /// A UserVar has changed value
    SetUserVar {
        name: String,
        value: String,
    },
    /// When something bumps the seqno in the terminal model and
    /// the terminal is not focused
    OutputSinceFocusLost,
    /// A change to the progress bar state
    Progress(Progress),
    /// LBTerm: a program reported its status (OSC 7501), or a prompt or
    /// reset ended some
    ProgramStatus,
    /// LBTerm: a kitty notification (OSC 99) to show, close or list
    KittyNotification(lbterm_notify::Command),
}

pub trait AlertHandler: Send + Sync {
    fn alert(&mut self, alert: Alert);

    /// LBTerm: a program set the title it has again (OSC 0 or 2), which raises no alert: an
    /// alert is a PDU to every client and a window update, for nothing the pane shows. Upstream's
    /// mux window takes the title of whichever pane set one last, so mux's LocalPane gives the
    /// window this title again (Window::set_title, which tells anyone only when that changes it)
    fn title_set_again(&mut self, _title: &str) {}
}

pub trait DownloadHandler: Send + Sync {
    fn save_to_downloads(&self, name: Option<String>, data: Vec<u8>);
}

/// Represents an instance of a terminal emulator.
pub struct Terminal {
    /// The terminal model/state
    state: TerminalState,
    /// Baseline terminal escape sequence parser
    parser: Parser,
}

impl Deref for Terminal {
    type Target = TerminalState;

    fn deref(&self) -> &TerminalState {
        &self.state
    }
}

impl DerefMut for Terminal {
    fn deref_mut(&mut self) -> &mut TerminalState {
        &mut self.state
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, FromDynamic, ToDynamic)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub struct TerminalSize {
    pub rows: usize,
    pub cols: usize,
    pub pixel_width: usize,
    pub pixel_height: usize,
    pub dpi: u32,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        }
    }
}

impl Terminal {
    /// Construct a new Terminal.
    /// `physical_rows` and `physical_cols` describe the dimensions
    /// of the visible portion of the terminal display in terms of
    /// the number of text cells.
    ///
    /// `pixel_width` and `pixel_height` describe the dimensions of
    /// that same visible area but in pixels.
    ///
    /// `term_program` and `term_version` are required to identify
    /// the host terminal program; they are used to respond to the
    /// terminal identification sequence `\033[>q`.
    ///
    /// `writer` is anything that implements `std::io::Write`; it
    /// is used to send input to the connected program; both keyboard
    /// and mouse input is encoded and written to that stream, as
    /// are answerback responses to a number of escape sequences.
    pub fn new(
        size: TerminalSize,
        config: Arc<dyn TerminalConfiguration + Send + Sync>,
        term_program: &str,
        term_version: &str,
        // writing to the writer sends data to input of the pty
        writer: Box<dyn std::io::Write + Send>,
    ) -> Terminal {
        // LBTerm: runs of ASCII as one PrintString each, which the performer takes whole
        let mut parser = Parser::new();
        parser.lbterm_print_runs(true);
        Terminal {
            state: TerminalState::new(size, config, term_program, term_version, writer),
            parser,
        }
    }

    /// LBTerm, tests only: print as upstream does, the parser's characters one at a time and each
    /// grapheme into its line on its own, so a test can hold the ASCII runs to what upstream
    /// writes (term/tests/lbterm_print.rs)
    #[doc(hidden)]
    pub fn lbterm_print_as_upstream(&mut self) {
        self.parser.lbterm_print_runs(false);
        self.state.lbterm_upstream_print = true;
    }

    /// Feed the terminal parser a slice of bytes from the output
    /// of the associated program.
    /// The slice is not required to be a complete sequence of escape
    /// characters; it is valid to feed in chunks of data as they arrive.
    /// The output is parsed and applied to the terminal model.
    pub fn advance_bytes<B: AsRef<[u8]>>(&mut self, bytes: B) {
        self.state.increment_seqno();
        {
            let bytes = bytes.as_ref();

            let mut performer = Performer::new(&mut self.state);

            self.parser.parse(bytes, |action| performer.perform(action));
        }
        self.trigger_unseen_output_notif();
    }

    pub fn perform_actions(&mut self, actions: Vec<wezterm_escape_parser::Action>) {
        self.state.increment_seqno();
        {
            let mut performer = Performer::new(&mut self.state);
            for action in actions {
                performer.perform(action);
            }
        }
        self.trigger_unseen_output_notif();
    }
}
