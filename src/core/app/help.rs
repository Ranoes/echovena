use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpCategory {
  #[default]
  All,
  Navigation,
  Playback,
  Lyrics,
  Library,
  System,
}

impl HelpCategory {
  pub const ALL: [HelpCategory; 6] = [
    HelpCategory::All,
    HelpCategory::Navigation,
    HelpCategory::Playback,
    HelpCategory::Lyrics,
    HelpCategory::Library,
    HelpCategory::System,
  ];

  pub fn label(&self) -> &'static str {
    match self {
      HelpCategory::All => "ALL",
      HelpCategory::Navigation => "Navigation",
      HelpCategory::Playback => "Playback",
      HelpCategory::Lyrics => "Lyrics & Karaoke",
      HelpCategory::Library => "Playlists & Library",
      HelpCategory::System => "System & Settings",
    }
  }

  pub fn next(&self) -> Self {
    match self {
      Self::All => Self::Navigation,
      Self::Navigation => Self::Playback,
      Self::Playback => Self::Lyrics,
      Self::Lyrics => Self::Library,
      Self::Library => Self::System,
      Self::System => Self::All,
    }
  }

  pub fn prev(&self) -> Self {
    match self {
      Self::All => Self::System,
      Self::Navigation => Self::All,
      Self::Playback => Self::Navigation,
      Self::Lyrics => Self::Playback,
      Self::Library => Self::Lyrics,
      Self::System => Self::Library,
    }
  }
}

/// Formatted Help-menu rows, static between terminal-width, keybinding, or
/// filter changes. Rebuilt outside the draw path (see
/// `tui::ui::popups::ensure_help_menu_model`) so Help rendering reads immutable
/// `App` state instead of rebuilding ~80 owned Strings on every redraw.
pub struct HelpMenuModel {
  pub width: usize,
  pub keys: crate::core::user_config::KeyBindings,
  pub source: Source,
  pub spotify_connected: bool,
  pub category: HelpCategory,
  pub filter: String,
  pub header: String,
  pub rows: Vec<String>,
  /// Per-row filter-match byte ranges, parallel to `rows`. Precomputed here so
  /// the render loop does not lowercase every visible row on every frame.
  pub match_ranges: Vec<Vec<(usize, usize)>>,
}

impl App {
  pub fn calculate_help_menu_offset(&mut self) {
    if self.view.help_menu_max_lines == 0 || self.view.help_docs_size == 0 {
      self.view.help_menu_page = 0;
      self.view.help_menu_offset = 0;
      return;
    }

    let last_page = self.view.help_docs_size.saturating_sub(1) / self.view.help_menu_max_lines;
    self.view.help_menu_page = self.view.help_menu_page.min(last_page);
    self.view.help_menu_offset = self.view.help_menu_page * self.view.help_menu_max_lines;
  }
}
