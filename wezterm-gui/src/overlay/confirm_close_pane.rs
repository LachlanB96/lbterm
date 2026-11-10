use super::confirm;
use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use mux::pane::PaneId;
use mux::tab::TabId;
use mux::termwiztermtab::TermWizTerminal;
use mux::window::WindowId;
use mux::Mux;
use window::WindowOps;

pub fn confirm_close_pane(
    pane_id: PaneId,
    mut term: TermWizTerminal,
    mux_window_id: WindowId,
    window: ::window::Window,
) -> anyhow::Result<()> {
    if confirm::run_confirmation("🛑 Really kill this pane?", &mut term)? {
        // LBTerm: through the window, so the last tab's only pane gives way to a new tab while
        // the Conductor's pad stays (termwindow/last_tab.rs)
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            // LBTerm: he said yes, so its Claude session is marked closed by hand
            tw.lbterm_mark_pane(pane_id);
            if tw.lbterm_close_last_pane(pane_id) {
                return;
            }
            let mux = Mux::get();
            let tab = match mux.get_active_tab_for_window(mux_window_id) {
                Some(tab) => tab,
                None => return,
            };
            tab.kill_pane(pane_id);
        })));
    }
    TermWindow::schedule_cancel_overlay_for_pane(window, pane_id);

    Ok(())
}

pub fn confirm_close_tab(
    tab_id: TabId,
    mut term: TermWizTerminal,
    _mux_window_id: WindowId,
    window: ::window::Window,
) -> anyhow::Result<()> {
    if confirm::run_confirmation(
        "🛑 Really kill this tab and all contained panes?",
        &mut term,
    )? {
        // LBTerm: through the window, so the last tab gives way to a new one while the
        // Conductor's pad stays (termwindow/last_tab.rs)
        // and he said yes, so its Claude sessions are marked closed by hand
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.lbterm_mark_tab(tab_id);
            tw.lbterm_remove_tab(tab_id)
        })));
    }
    TermWindow::schedule_cancel_overlay(window, tab_id, None);

    Ok(())
}

pub fn confirm_close_window(
    mut term: TermWizTerminal,
    mux_window_id: WindowId,
    window: ::window::Window,
    tab_id: TabId,
) -> anyhow::Result<()> {
    if confirm::run_confirmation(
        "🛑 Really kill this window and all contained tabs and panes?",
        &mut term,
    )? {
        promise::spawn::spawn_into_main_thread(async move {
            let mux = Mux::get();
            mux.kill_window(mux_window_id);
        })
        .detach();
    }
    TermWindow::schedule_cancel_overlay(window, tab_id, None);

    Ok(())
}

pub fn confirm_quit_program(
    mut term: TermWizTerminal,
    window: ::window::Window,
    tab_id: TabId,
) -> anyhow::Result<()> {
    let question = format!("🛑 Really Quit {}?", config::lbterm::APP_NAME);
    if confirm::run_confirmation(&question, &mut term)? {
        promise::spawn::spawn_into_main_thread(async move {
            crate::frontend::quit_application();
        })
        .detach();
    }
    TermWindow::schedule_cancel_overlay(window, tab_id, None);

    Ok(())
}
