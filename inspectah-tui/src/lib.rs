pub mod action;
pub mod theme;
pub mod types;

use inspectah_refine::session::RefineSession;

pub fn run_tui(_session: RefineSession) -> color_eyre::Result<()> {
    Ok(())
}
