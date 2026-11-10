use crate::cli::resolve_relative_cwd;
use clap::{Parser, ValueHint};
use config::keyassignment::SpawnTabDomain;
use config::ConfigHandle;
use mux::pane::PaneId;
use mux::window::WindowId;
use portable_pty::cmdbuilder::CommandBuilder;
use std::ffi::OsString;
use wezterm_client::client::Client;
use wezterm_term::TerminalSize;

#[derive(Debug, Parser, Clone)]
pub struct SpawnCommand {
    /// Specify the current pane.
    /// The default is to use the current pane based on the
    /// environment variable WEZTERM_PANE.
    /// The pane is used to determine the current domain
    /// and window.
    #[arg(long)]
    pane_id: Option<PaneId>,

    #[arg(long)]
    domain_name: Option<String>,

    /// Specify the window into which to spawn a tab.
    /// If omitted, the window associated with the current
    /// pane is used.
    /// Cannot be used with `--workspace` or `--new-window`.
    #[arg(long, conflicts_with_all=&["workspace", "new_window"])]
    window_id: Option<WindowId>,

    /// Spawn into a new window, rather than a new tab.
    #[arg(long)]
    new_window: bool,

    /// Specify the current working directory for the initially
    /// spawned program
    #[arg(long, value_parser, value_hint=ValueHint::DirPath)]
    cwd: Option<OsString>,

    /// When creating a new window, override the default workspace name
    /// with the provided name.  The default name is "default".
    /// Requires `--new-window`.
    #[arg(long, requires = "new_window")]
    workspace: Option<String>,

    /// Add the tab without switching the window to it, so nothing
    /// typed lands in it. Cannot be used with `--new-window`.
    #[arg(long, conflicts_with = "new_window")]
    no_activate: bool,

    /// Instead of executing your shell, run PROG.
    /// For example: `wezterm cli spawn -- bash -l` will spawn bash
    /// as if it were a login shell.
    #[arg(value_parser, value_hint=ValueHint::CommandWithArguments, num_args=1..)]
    prog: Vec<OsString>,
}

impl SpawnCommand {
    /// LBTerm: `config` loads the config, which runs the whole config file (lbterm.lua took ~470 ms
    /// to run on 2026-10-29, and every `cli spawn` paid it). Only a spawn that opens a window uses
    /// what it gives, the default workspace and the window's first size: one into a window the
    /// server has takes the size of that window's tab and ignores the workspace (mux's
    /// spawn_tab_or_window_activating), so it's never loaded then.
    pub async fn run(
        self,
        client: Client,
        config: impl FnOnce() -> anyhow::Result<ConfigHandle>,
    ) -> anyhow::Result<()> {
        let window_id = if self.new_window {
            None
        } else {
            match self.window_id {
                Some(w) => Some(w),
                None => {
                    let pane_id = client.resolve_pane_id(self.pane_id).await?;

                    let panes = client.list_panes().await?;
                    let mut window_id = None;
                    'outer: for tabroot in panes.tabs {
                        let mut cursor = tabroot.into_tree().cursor();

                        loop {
                            if let Some(entry) = cursor.leaf_mut() {
                                if entry.pane_id == pane_id {
                                    window_id.replace(entry.window_id);
                                    break 'outer;
                                }
                            }
                            match cursor.preorder_next() {
                                Ok(c) => cursor = c,
                                Err(_) => break,
                            }
                        }
                    }
                    window_for_pane(self.pane_id.is_some(), pane_id, window_id)?
                }
            }
        };

        let (workspace, size) = workspace_and_size(window_id, self.workspace.as_deref(), config)?;

        let spawn = codec::SpawnV2 {
            domain: self
                .domain_name
                .map_or(SpawnTabDomain::DefaultDomain, |name| {
                    SpawnTabDomain::DomainName(name)
                }),
            window_id,
            command: if self.prog.is_empty() {
                None
            } else {
                let builder = CommandBuilder::from_argv(self.prog);
                Some(builder)
            },
            command_dir: resolve_relative_cwd(self.cwd)?,
            size,
            workspace,
        };
        let spawned = if self.no_activate {
            client
                .spawn_no_activate(codec::SpawnNoActivate { spawn })
                .await?
        } else {
            client.spawn_v2(spawn).await?
        };

        log::debug!("{:?}", spawned);
        println!("{}", spawned.pane_id);
        Ok(())
    }
}

/// LBTerm: the workspace and size a spawn sends. Into a window the server has (`window_id` some),
/// the server ignores both, so the config isn't loaded for them; a new window takes the default
/// workspace (or `--workspace`) and its first size from the config.
fn workspace_and_size(
    window_id: Option<WindowId>,
    workspace: Option<&str>,
    config: impl FnOnce() -> anyhow::Result<ConfigHandle>,
) -> anyhow::Result<(String, TerminalSize)> {
    if window_id.is_some() {
        return Ok((mux::DEFAULT_WORKSPACE.to_string(), TerminalSize::default()));
    }
    let config = config()?;
    let workspace = workspace
        .or(config.default_workspace.as_deref())
        .unwrap_or(mux::DEFAULT_WORKSPACE)
        .to_string();
    Ok((workspace, config.initial_size(0, None)))
}

/// LBTerm: the window a tab spawned beside a pane goes into. A --pane-id
/// given on the command line that the server doesn't have is refused, since
/// with no window the server opens a new one: on 2026-10-22 the Conductor
/// came up alone in a window of its own that way, list-clients still naming
/// a tab that had just closed. A pane taken from WEZTERM_PANE or the
/// server's choice keeps upstream's new window.
fn window_for_pane(
    named: bool,
    pane_id: PaneId,
    found: Option<WindowId>,
) -> anyhow::Result<Option<WindowId>> {
    if named && found.is_none() {
        anyhow::bail!(
            "pane {} isn't one of this server's panes, so nothing was spawned \
             (--new-window opens a window of its own)",
            pane_id
        );
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{window_for_pane, workspace_and_size};
    use config::ConfigHandle;

    #[test]
    fn a_spawn_into_a_window_never_loads_the_config() {
        let (workspace, size) =
            workspace_and_size(Some(3), None, || panic!("the config was loaded")).unwrap();
        assert_eq!(workspace, mux::DEFAULT_WORKSPACE);
        assert_eq!(size, wezterm_term::TerminalSize::default());
    }

    #[test]
    fn a_new_window_loads_it_for_its_workspace_and_size() {
        let mut loaded = false;
        let (workspace, _) = workspace_and_size(None, None, || {
            loaded = true;
            Ok(ConfigHandle::default_config())
        })
        .unwrap();
        assert!(loaded);
        assert_eq!(workspace, mux::DEFAULT_WORKSPACE);
        let (named, _) =
            workspace_and_size(None, Some("work"), || Ok(ConfigHandle::default_config())).unwrap();
        assert_eq!(named, "work");
    }

    #[test]
    fn a_config_that_fails_fails_only_a_new_window() {
        assert!(workspace_and_size(Some(1), None, || anyhow::bail!("broken")).is_ok());
        assert!(workspace_and_size(None, None, || anyhow::bail!("broken")).is_err());
    }

    #[test]
    fn a_named_pane_spawns_in_its_window() {
        assert_eq!(window_for_pane(true, 3, Some(1)).unwrap(), Some(1));
    }

    #[test]
    fn a_named_pane_the_server_lacks_is_refused() {
        let err = window_for_pane(true, 42, None).unwrap_err();
        let text = format!("{:#}", err);
        assert!(text.contains("pane 42 isn't"), "it said {:?}", text);
    }

    #[test]
    fn a_pane_not_named_and_not_found_still_opens_a_window() {
        assert_eq!(window_for_pane(false, 42, None).unwrap(), None);
        assert_eq!(window_for_pane(false, 3, Some(1)).unwrap(), Some(1));
    }
}
