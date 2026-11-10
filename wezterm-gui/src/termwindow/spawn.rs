use crate::spawn::SpawnWhere;
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::TermConfig;
use std::sync::Arc;

impl super::TermWindow {
    pub fn spawn_command(&self, spawn: &SpawnCommand, spawn_where: SpawnWhere) {
        let size = if spawn_where == SpawnWhere::NewWindow {
            self.config.initial_size(
                self.dimensions.dpi as u32,
                crate::cell_pixel_dims(&self.config, self.dimensions.dpi as f64).ok(),
            )
        } else {
            self.terminal_size
        };
        let term_config = Arc::new(TermConfig::with_config(self.config.clone()));

        // LBTerm: timed to the frame that shows it (termwindow/budget.rs)
        match spawn_where {
            SpawnWhere::NewTab => self.lbterm_budget_new_tab(),
            SpawnWhere::SplitPane(_) => self.lbterm_budget_split(),
            SpawnWhere::NewWindow => {}
        }

        crate::spawn::spawn_command_impl(
            spawn,
            spawn_where,
            size,
            Some(self.mux_window_id),
            term_config,
        )
    }

    /// LBTerm: a new tab from `spawn`, as `spawn_command` opens one, handed to `then` once it's
    /// open (crash restore parks one that was parked, restore.rs)
    pub(crate) fn lbterm_spawn_tab_then(
        &self,
        spawn: SpawnCommand,
        then: impl FnOnce(Arc<mux::tab::Tab>) + 'static,
    ) {
        self.lbterm_budget_new_tab();
        let size = self.terminal_size;
        let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
        let window_id = self.mux_window_id;
        promise::spawn::spawn(async move {
            match crate::spawn::spawn_command_internal(
                spawn,
                SpawnWhere::NewTab,
                size,
                Some(window_id),
                term_config,
            )
            .await
            {
                Ok(Some(tab)) => then(tab),
                Ok(None) => {}
                Err(err) => log::error!("Failed to spawn: {:#}", err),
            }
        })
        .detach();
    }

    pub fn spawn_tab(&mut self, domain: &SpawnTabDomain) {
        self.spawn_command(
            &SpawnCommand {
                domain: domain.clone(),
                ..Default::default()
            },
            SpawnWhere::NewTab,
        );
    }
}
