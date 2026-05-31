pub mod action;
pub mod app;
pub mod event;
pub mod keys;
pub mod theme;
pub mod types;
pub mod widget;

#[cfg(test)]
pub mod test_helpers;

use inspectah_refine::session::RefineSession;

pub fn run_tui(session: RefineSession) -> color_eyre::Result<()> {
    app::App::new(session).run()
}
