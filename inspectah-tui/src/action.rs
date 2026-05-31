/// All actions the TUI can perform, produced by key mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    // Navigation
    CursorUp,
    CursorDown,
    CursorTop,
    CursorBottom,
    FocusSidebar,
    FocusItems,
    CycleFocus,
    JumpToSection(usize),
    NextGroup,
    PrevGroup,
    // Item interaction
    ToggleItem,
    OpenDetail,
    CloseDetail,
    PromoteDetail,
    DetailNext,
    DetailPrev,
    // Session
    Undo,
    Redo,
    Refresh,
    // Overlays
    EnterSearch,
    EnterCommand,
    ShowHelp,
    ToggleContainerfile,
    // Input mode
    SubmitInput,
    CancelInput,
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputLeft,
    InputRight,
    InputHome,
    InputEnd,
    // Tab completion in command mode
    TabComplete,
    // Export confirmation
    ConfirmYes,
    ConfirmNo,
    // No-op (unbound key)
    Noop,
}
