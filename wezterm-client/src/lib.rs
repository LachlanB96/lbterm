pub mod client;
mod coalesce;
pub mod discovery;
pub mod domain;
mod lbterm_start;
/// LBTerm: what a connection has handed the GUI thread and it hasn't applied yet
pub mod lbterm_backlog;
/// LBTerm: a read of a held pane, brought current first (Lachlan's 6b of 2026-11-08)
pub mod lbterm_current;
/// LBTerm: which push of each shown pane a frame read, said to its server (PanesDrawn)
pub mod lbterm_drawn;
/// LBTerm: whether a connection is up, and what a write to a pane comes to
pub mod lbterm_link;
/// LBTerm: a push of a pane's title and folder alone, applied on the client thread
pub mod lbterm_names;
/// LBTerm: a tab out of view's title alert and its mux window's title, decided on the client
/// thread
pub mod lbterm_titles;
/// LBTerm: what a domain's windows show, said to its server (SetShownPanes)
pub mod lbterm_shown;
pub mod pane;
mod titles;
