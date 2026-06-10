pub mod single_host;

use ratatui::Frame;

use inspectah_refine::session::RefineSession;

use crate::theme::ColorTier;
use crate::types::TuiState;

pub enum Screen {
    SingleHost(single_host::SingleHostScreen),
}

impl Screen {
    pub fn render(
        &self,
        frame: &mut Frame,
        session: &RefineSession,
        state: &TuiState,
        tier: ColorTier,
    ) {
        match self {
            Screen::SingleHost(screen) => screen.render(frame, session, state, tier),
        }
    }
}
