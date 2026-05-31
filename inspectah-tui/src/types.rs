use std::collections::HashSet;
use std::time::Instant;

/// Which panel has keyboard focus.
/// Tab cycles: Sidebar -> ItemList -> DetailPane (when open) -> Sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Sidebar,
    ItemList,
    /// Active when a detail view (info bar or fullscreen) is open.
    DetailPane,
}

/// What the detail pane is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMode {
    /// No detail pane visible.
    None,
    /// Compact 2-row info bar at bottom of item list.
    InfoBar,
    /// Fullscreen detail replacing the item list.
    Fullscreen,
}

/// Current input mode -- determines how keys are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Command,
    /// Export confirmation prompt (y/N).
    Confirm,
    /// Help screen overlay.
    Help,
}

/// A flash message shown in the status bar for a limited duration.
#[derive(Debug, Clone)]
pub struct FlashMessage {
    pub text: String,
    pub expires: Instant,
}

impl FlashMessage {
    pub fn new(text: impl Into<String>, duration_secs: u64) -> Self {
        Self {
            text: text.into(),
            expires: Instant::now() + std::time::Duration::from_secs(duration_secs),
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires
    }
}

/// All TUI-specific state (not session state).
#[derive(Debug)]
pub struct TuiState {
    pub focus: FocusTarget,
    pub active_section: usize,
    pub cursor: usize,
    pub detail_mode: DetailMode,
    pub input_mode: InputMode,
    pub search_query: String,
    pub command_input: String,
    pub show_containerfile: bool,
    pub flash: Option<FlashMessage>,
    /// Which triage groups are collapsed, keyed by (section_index, group_index).
    pub collapsed_groups: HashSet<(usize, usize)>,
    /// Saved cursor position per section, restored on section switch.
    pub section_cursors: Vec<usize>,
    /// Sidebar scroll offset (for overflow).
    pub sidebar_scroll: usize,
}

impl TuiState {
    pub fn new(section_count: usize) -> Self {
        let mut collapsed = HashSet::new();
        // Baseline group (index 2) is collapsed by default in every section.
        for i in 0..section_count {
            collapsed.insert((i, 2));
        }
        Self {
            focus: FocusTarget::Sidebar,
            active_section: 0,
            cursor: 0,
            detail_mode: DetailMode::None,
            input_mode: InputMode::Normal,
            search_query: String::new(),
            command_input: String::new(),
            show_containerfile: false,
            flash: None,
            collapsed_groups: collapsed,
            section_cursors: vec![0; section_count],
            sidebar_scroll: 0,
        }
    }
}

/// Identifies a sidebar section.
///
/// Section model (from spec rev3):
/// - 7 decision/composite above the separator: Packages (with embedded
///   repo bar), Configs, Services (composite: decision states/drop-ins +
///   reference divergent/advisories/warnings/omitted), Containers
///   (composite: decision quadlets/flatpaks + reference running/compose),
///   Sysctls, Tuned, Users
/// - 7 reference-only below the separator: VerChanges, KernelBoot,
///   Network, Storage, ScheduledTasks, NonRpmSoftware, SELinux
///
/// Repos are NOT a standalone sidebar entry -- they are embedded in the
/// Packages section via a repo bar (matching the web UI's RepoBar.tsx).
/// Services and Containers are composite: they contain both decision items
/// (togglable via Space) and reference items (read-only, Space is no-op).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionId {
    // Decision / composite sections (above sidebar separator)
    Packages,
    Configs,
    Services, // composite: decision service_states/dropins + ref divergent/advisories/warnings/omitted
    Containers, // composite: decision quadlets/flatpaks + ref running_containers/compose_files
    Sysctls,
    Tuned,
    Users,
    // Reference-only sections (below sidebar separator)
    VerChanges,
    KernelBoot,
    Network,
    Storage,
    ScheduledTasks,
    NonRpmSoftware,
    SELinux,
}

impl SectionId {
    /// True for sections that contain togglable (decision) items.
    /// Composite sections (Services, Containers) return true -- they
    /// contain BOTH decision and reference items. The triage list
    /// renders decision items with Space toggle and reference items
    /// as read-only within the same section.
    pub fn is_decision(&self) -> bool {
        matches!(
            self,
            Self::Packages
                | Self::Configs
                | Self::Services
                | Self::Containers
                | Self::Sysctls
                | Self::Tuned
                | Self::Users
        )
    }

    /// True for composite sections that mix decision + reference items.
    pub fn is_composite(&self) -> bool {
        matches!(self, Self::Services | Self::Containers)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Packages => "Packages",
            Self::Configs => "Configs",
            Self::Services => "Services",
            Self::Containers => "Containers",
            Self::Sysctls => "Sysctls",
            Self::Tuned => "Tuned",
            Self::Users => "Users",
            Self::VerChanges => "Ver.Chg",
            Self::KernelBoot => "Kernel",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::ScheduledTasks => "Sched.",
            Self::NonRpmSoftware => "Non-RPM",
            Self::SELinux => "SELinux",
        }
    }

    /// The `mark_viewed()` prefix for this section, if reviewed tracking
    /// is supported. Returns `None` for sections whose items cannot be
    /// independently marked as viewed (repos are embedded in packages,
    /// sysctls/tuned have no VALID_SECTIONS prefix).
    ///
    /// VALID_SECTIONS in RefineSession: packages, configs, services,
    /// containers, users_groups, network, storage, scheduled_tasks,
    /// non_rpm_software, kernel_boot, selinux
    pub fn viewed_prefix(&self) -> Option<&'static str> {
        match self {
            Self::Packages => Some("packages"),
            Self::Configs => Some("configs"),
            Self::Services => Some("services"),
            Self::Containers => Some("containers"),
            Self::Users => Some("users_groups"),
            Self::VerChanges => None, // not in VALID_SECTIONS
            Self::KernelBoot => Some("kernel_boot"),
            Self::Network => Some("network"),
            Self::Storage => Some("storage"),
            Self::ScheduledTasks => Some("scheduled_tasks"),
            Self::NonRpmSoftware => Some("non_rpm_software"),
            Self::SELinux => Some("selinux"),
            Self::Sysctls => None, // not in VALID_SECTIONS
            Self::Tuned => None,   // not in VALID_SECTIONS
        }
    }
}

/// A sidebar entry with section metadata.
#[derive(Debug, Clone)]
pub struct SectionEntry {
    pub id: SectionId,
    pub count: usize,
    pub included: usize,
    pub excluded: usize,
}
