//! LBTerm: parking a tab (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing 4"), from the tab menu's
//! Park tab row and the `LbtermPark` action (Ctrl+Alt+D in Lachlan's config).
//!
//! The flag is the mux tab's (`Tab::lbterm_parked`), kept by the process that holds the tab's
//! panes. A tab of the GUI's own mux is parked here, at once. A tab of a mux domain is the
//! server's, mirrored here: its park is asked of the server (codec's TabParked), and the local
//! tab takes its flag from the server's word back, as every window of that server does, so
//! nothing a window hears goes back (lbterm/MUX.md, "Parked tabs"). A domain whose server
//! hasn't answered GetParkedTabs (an older build) can't park, and its tabs' menus say nothing of
//! it. Parking or unparking notifies `LbtermTabParked`, which paints the window holding the
//! tab (mux's lbterm_repaint.rs), whose rail dims or brightens its row (render/tab_panel.rs).

use crate::termwindow::TermWindow;
use config::lbterm::LbtermPark;
use mux::tab::Tab;
use mux::Mux;
use wezterm_client::domain::ClientDomain;

/// The mux domain `tab` is mirrored from, if it's a mux domain's, as its active pane says
fn client_domain(mux: &Mux, tab: &Tab) -> Option<std::sync::Arc<dyn mux::domain::Domain>> {
    let pane = tab.get_active_pane()?;
    let domain = mux.get_domain(pane.domain_id())?;
    domain.downcast_ref::<ClientDomain>()?;
    Some(domain)
}

/// Whether `tab` can be parked: the GUI's own always, a mux domain's once its server has said
/// it can
pub fn can_park(tab: &Tab) -> bool {
    let mux = Mux::get();
    match client_domain(&mux, tab) {
        Some(domain) => domain
            .downcast_ref::<ClientDomain>()
            .map_or(false, |d| d.lbterm_can_park()),
        None => true,
    }
}

/// Park or unpark `tab`: here for the GUI's own, asked of its server for a mux domain's
pub fn set_parked(tab: &Tab, parked: bool) {
    let mux = Mux::get();
    log::info!(
        "parked: tab {} {}",
        tab.tab_id(),
        if parked { "parked" } else { "unparked" }
    );
    match client_domain(&mux, tab) {
        Some(domain) => {
            if let Some(d) = domain.downcast_ref::<ClientDomain>() {
                d.lbterm_park(tab.tab_id(), parked);
            }
        }
        None => {
            tab.lbterm_set_parked(parked);
        }
    }
}

impl TermWindow {
    /// `LbtermPark`: the window's open tab parked or unparked. Nothing on the Conductor's tab,
    /// which its pad stands for, or on a tab whose server can't park (logged)
    pub(crate) fn lbterm_park_action(&mut self, how: LbtermPark) {
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return;
        };
        if Some(tab.tab_id()) == self.lbterm_panel.conductor_tab {
            log::info!("parked: the Conductor's tab isn't parked");
            return;
        }
        if !can_park(&tab) {
            log::info!("parked: tab {}'s server can't park tabs", tab.tab_id());
            return;
        }
        set_parked(&tab, how.parks(tab.lbterm_parked()));
    }
}
