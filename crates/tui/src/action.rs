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
    /// Left arrow in sidebar: collapse current section's nav group.
    SidebarLeft,
    /// Right arrow in sidebar: expand current section's nav group or focus items.
    SidebarRight,
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
