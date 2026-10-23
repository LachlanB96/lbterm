use crate::cli::resolve_relative_cwd;
use clap::{Parser, ValueHint};
use config::keyassignment::SpawnTabDomain;
use config::ConfigHandle;
use mux::pane::PaneId;
use mux::window::WindowId;
use portable_pty::cmdbuilder::CommandBuilder;
use std::ffi::OsString;
use wezterm_client::client::Client;

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
    pub async fn run(self, client: Client, config: &ConfigHandle) -> anyhow::Result<()> {
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
                    window_id
                }
            }
        };

        let workspace = self
            .workspace
            .as_deref()
            .unwrap_or(
                config
                    .default_workspace
                    .as_deref()
                    .unwrap_or(mux::DEFAULT_WORKSPACE),
            )
            .to_string();

        let size = config.initial_size(0, None);

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
            spawn_no_activate(&client, spawn).await?
        } else {
            client.spawn_v2(spawn).await?
        };

        log::debug!("{:?}", spawned);
        println!("{}", spawned.pane_id);
        Ok(())
    }
}

/// LBTerm: a mux server keeps the build it started with until its last tab
/// closes, so one from before --no-activate decodes SpawnNoActivate as
/// Pdu::Invalid and says so. Spawn the usual way then rather than fail:
/// the GUI of a mux server doesn't follow its switch to the new tab (seen
/// 2026-10-21), so the keyboard stays put. An older GUI's own socket does
/// switch, as every spawn did before.
async fn spawn_no_activate(
    client: &Client,
    spawn: codec::SpawnV2,
) -> anyhow::Result<codec::SpawnResponse> {
    match client
        .spawn_no_activate(codec::SpawnNoActivate {
            spawn: spawn.clone(),
        })
        .await
    {
        Err(err) if format!("{err:#}").contains("invalid PDU Invalid { ident: 7502 }") => {
            eprintln!(
                "lbterm: this server is an older build that can't spawn without \
                 activating, so the tab was spawned the usual way"
            );
            client.spawn_v2(spawn).await
        }
        result => result,
    }
}
