pub mod action;
pub mod app;
pub mod event;
pub mod keys;
pub mod screen;
pub mod sections;
pub mod theme;
pub mod types;
pub mod widget;

#[cfg(test)]
pub mod test_helpers;

use std::path::PathBuf;

use inspectah_refine::session::RefineSession;

pub fn run_tui(session: RefineSession, tarball_path: Option<PathBuf>) -> color_eyre::Result<()> {
    let mut app = app::App::new(session);
    if let Some(path) = tarball_path {
        app.set_tarball_path(path);
    }
    app.run()
}
