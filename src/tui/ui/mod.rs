#[cfg(feature = "ai-dj")]
pub mod ai_dj;
pub mod artist;
pub mod audio_analysis;
pub mod columns;
pub mod create_playlist;
pub mod discover;
pub mod friends;
pub mod head_unit;
pub mod help;
pub mod home;
pub mod library;
pub mod lyrics;
pub mod player;
pub mod playlist_sync;
pub mod playlist_tree;
pub mod plugin_screen;
pub mod popups;
pub mod retro;
pub mod search;
pub mod search_drilldown;
pub mod settings;
pub mod stats;
pub mod tables;
pub mod util;

use crate::core::app::{App, RouteId};
use crate::tui::layout::{compute_main_layout, is_wide_layout};
use ratatui::{layout::Rect, Frame};

pub use self::artist::draw_artist_albums;
pub use self::create_playlist::draw_create_playlist_form;
pub use self::discover::draw_discover;
pub use self::friends::draw_friends;
pub use self::home::draw_home;
pub use self::library::draw_user_block;
pub use self::lyrics::draw_lyrics_view;
#[cfg(feature = "cover-art")]
pub use self::player::draw_cover_art_view;
pub use self::player::draw_miniplayer;
pub use self::player::{draw_device_list, draw_playbar};
pub use self::playlist_sync::draw_playlist_sync;
pub use self::plugin_screen::draw_plugin_screen;
pub use self::popups::{
  draw_announcement_prompt, draw_community_pin_prompt, draw_dialog, draw_error_screen,
  draw_exit_prompt, draw_help_menu, draw_party, draw_plugin_popup, draw_queue, draw_recap_prompt,
  draw_sort_menu, ensure_help_menu_model,
};
pub use self::search::{draw_input_and_help_box, draw_search_results};
pub use self::stats::draw_stats;
pub use self::tables::{
  draw_album_list, draw_album_table, draw_artist_table, draw_local_browser, draw_podcast_table,
  draw_recently_played_table, draw_recommendations_table, draw_show_episodes, draw_song_table,
};
pub fn draw_main_layout(f: &mut Frame<'_>, app: &App) {
  if let Some(areas) = compute_main_layout(app) {
    // In wide layout the input row lives inside the sidebar and is drawn by
    // draw_user_block (it varies per source); the top row is only drawn here.
    if !is_wide_layout(app) {
      if let (Some(input_area), Some(help_area), Some(settings_area)) =
        (areas.input, areas.help, areas.settings)
      {
        draw_input_and_help_box(f, app, input_area, help_area, settings_area);
      }
    }
    draw_user_block(f, app, areas.sidebar);
    draw_route_content(f, app, areas.content);
    draw_playbar(f, app, areas.playbar);
    draw_action_bar(f, app, areas.action_bar);
  }

  // Possibly draw confirm dialog
  draw_dialog(f, app);

  // Possibly draw sort menu
  draw_sort_menu(f, app);

  // Possibly draw Relay Health modal (R key)
  crate::ui::modals::relay_status::draw_relay_status_modal(f, app);

  // Possibly draw URL Import modal (U key)
  crate::ui::modals::url_import::draw_url_import_modal(f, app);
  // Possibly draw Sync Auth modal (C / Y keys)
  crate::ui::modals::sync_auth::draw_sync_auth_modal(f, app);
  // Possibly draw Manual YouTube Remap modal (m key)
  crate::ui::modals::remap_track::draw_remap_track_modal(f, app);

  // Possibly draw Full-Screen Karaoke Stage View (K key)
  crate::ui::karaoke_view::draw_karaoke_stage_view(f, app);
}

fn draw_route_content(f: &mut Frame<'_>, app: &App, content_area: Rect) {
  let current_route = app.get_current_route();

  match current_route.id {
    RouteId::Search => {
      draw_search_results(f, app, content_area);
    }
    RouteId::TrackTable => {
      draw_song_table(f, app, content_area);
    }
    RouteId::AlbumTracks => {
      draw_album_table(f, app, content_area);
    }
    RouteId::RecentlyPlayed => {
      draw_recently_played_table(f, app, content_area);
    }
    RouteId::Artist => {
      draw_artist_albums(f, app, content_area);
    }
    RouteId::AlbumList => {
      draw_album_list(f, app, content_area);
    }
    RouteId::PodcastEpisodes => {
      draw_show_episodes(f, app, content_area);
    }
    RouteId::Home => {
      draw_home(f, app, content_area);
    }
    RouteId::Discover => {
      draw_discover(f, app, content_area);
    }
    RouteId::Friends => {
      draw_friends(f, app, content_area);
    }
    #[cfg(feature = "ai-dj")]
    RouteId::AiDj => {
      ai_dj::draw_ai_dj(f, app, content_area);
    }
    RouteId::Stats => {
      draw_stats(f, app, content_area);
    }
    RouteId::Playlists => {
      playlist_tree::draw_playlist_tree_view(f, app, content_area);
    }
    RouteId::PlaylistSync => {
      draw_playlist_sync(f, app, content_area);
    }
    RouteId::Artists => {
      draw_artist_table(f, app, content_area);
    }
    RouteId::LocalBrowser => {
      draw_local_browser(f, app, content_area);
    }
    RouteId::Podcasts => {
      draw_podcast_table(f, app, content_area);
    }
    RouteId::Recommendations => {
      draw_recommendations_table(f, app, content_area);
    }
    RouteId::Error
    | RouteId::SelectedDevice
    | RouteId::Analysis
    | RouteId::LyricsView
    | RouteId::CoverArtView
    | RouteId::MiniPlayer
    | RouteId::AnnouncementPrompt
    | RouteId::RecapPrompt
    | RouteId::CommunityPinPrompt
    | RouteId::ExitPrompt
    | RouteId::Settings
    | RouteId::HelpMenu
    | RouteId::Queue
    | RouteId::Party
    | RouteId::PluginScreen(_) => {} // These are drawn outside the main routed content area.
    RouteId::Dialog => {}         // This is handled in draw_dialog.
    RouteId::CreatePlaylist => {} // This is drawn as an overlay via draw_create_playlist_form.
  };
}

/// Renders the permanent bottom action bar with live Vim hotkey hints and program processing status.
pub fn draw_action_bar(f: &mut Frame<'_>, app: &App, area: Rect) {
  if area.height == 0 || area.width == 0 {
    return;
  }

  use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
  };

  let key_style = Style::default()
    .fg(Color::Cyan)
    .add_modifier(Modifier::BOLD);
  let label_style = Style::default().fg(Color::Gray);
  let sep_style = Style::default().fg(Color::DarkGray);

  // Check for any processing message or status message returned by the program
  let status_msg = app.status_message();
  let is_error = app.status_message_is_error();
  let is_syncing = app.playlist_sync_in_flight();
  let is_importing = app.view.url_import_in_progress;
  let is_searching = app.live_search_loading;
  #[cfg(feature = "youtube")]
  let is_advancing = app.youtube_playback.as_ref().map_or(false, |yt| yt.advancing);
  #[cfg(not(feature = "youtube"))]
  let is_advancing = false;

  let mut spans = Vec::new();
  let has_status = status_msg.is_some() || is_syncing || is_importing || is_searching || is_advancing;

  if has_status {
    if is_error {
      let err_text = status_msg.unwrap_or("An unexpected error occurred");
      spans.push(Span::styled("[✖ ERROR] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
      spans.push(Span::styled(err_text, Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)));
    } else if is_syncing {
      spans.push(Span::styled("[⟳ SYNCING] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)));
      spans.push(Span::styled("Bidirectional playlist sync in progress...", Style::default().fg(Color::White)));
    } else if is_importing {
      spans.push(Span::styled("[⟳ IMPORTING] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
      spans.push(Span::styled("Fetching & importing playlist from URL...", Style::default().fg(Color::White)));
    } else if is_advancing {
      spans.push(Span::styled("[⟳ BUFFERING] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
      spans.push(Span::styled("Loading next audio stream...", Style::default().fg(Color::White)));
    } else if is_searching {
      spans.push(Span::styled("[⟳ SEARCHING] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
      spans.push(Span::styled("Fetching YouTube results...", Style::default().fg(Color::White)));
    } else if let Some(msg) = status_msg {
      if msg.contains("Fetching")
        || msg.contains("Loading")
        || msg.contains("Archiving")
        || msg.contains("Importing")
        || msg.contains("Searching")
        || msg.contains("Syncing")
        || msg.contains("in background")
      {
        spans.push(Span::styled("[⟳ PROCESSING] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(msg, Style::default().fg(Color::LightYellow)));
      } else if msg.contains("Playing") || msg.starts_with('♪') || msg.starts_with('\u{266a}') {
        spans.push(Span::styled("[▶ PLAYING] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(msg, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
      } else {
        spans.push(Span::styled("[ℹ STATUS] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(msg, Style::default().fg(Color::White)));
      }
    }

    spans.push(Span::styled("  │  ", sep_style));
  }

  let items = [
    ("/", "Search"),
    ("T", "Dashboard"),
    ("B", "Lyrics"),
    ("G", "Album"),
    ("N", "Art"),
    ("M", "Mini Player"),
    ("e/E", "Queue"),
    ("?", "Help"),
    ("q", "Back"),
  ];

  for (i, (key, label)) in items.iter().enumerate() {
    if !has_status && i > 0 {
      spans.push(Span::styled(" │ ", sep_style));
    } else if has_status && i > 0 {
      spans.push(Span::styled(" │ ", sep_style));
    }
    spans.push(Span::styled(format!("[{key}] "), key_style));
    spans.push(Span::styled(*label, label_style));
  }

  let paragraph = Paragraph::new(Line::from(spans)).block(Block::default());
  f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::app::ActiveBlock;
  use crate::core::geometry::Viewport;
  use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

  fn render(width: u16, height: u16) -> (App, Buffer) {
    let mut app = App::default();
    app.view.size = Viewport { width, height };
    app.push_navigation_stack(RouteId::Home, ActiveBlock::Home);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw_main_layout(f, &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (app, buffer)
  }

  fn row_text(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area().width)
      .filter_map(|x| buffer.cell((x, y)).map(|c| c.symbol().to_string()))
      .collect()
  }

  // The drawn input/help/settings boxes and sidebar panels must land exactly
  // where `compute_main_layout` puts the mouse hitboxes, and the search box
  // must be drawn once (a second copy used to overlay the Library panel in
  // wide layout, shifting it down three rows).
  #[test]
  fn wide_layout_draws_input_row_and_library_at_hitbox_positions() {
    let (app, buffer) = render(160, 50);
    let areas = compute_main_layout(&app).expect("layout areas");
    let input = areas.input.expect("input area");

    let input_row = row_text(&buffer, input.y);
    assert!(input_row.contains("Search"), "input row: {input_row}");
    assert!(input_row.contains("Help"), "input row: {input_row}");
    assert!(input_row.contains("Settings"), "input row: {input_row}");

    let library_row = row_text(&buffer, areas.library.y);
    assert!(
      library_row.contains("Quick Menu") || library_row.contains("Library"),
      "library panel not at hitbox row: {library_row}"
    );
    let playlists_row = row_text(&buffer, areas.playlists.y);
    assert!(
      playlists_row.contains("Playlists"),
      "playlists panel not at hitbox row: {playlists_row}"
    );
  }

  #[test]
  fn narrow_layout_draws_input_row_and_library_at_hitbox_positions() {
    let (app, buffer) = render(100, 40);
    let areas = compute_main_layout(&app).expect("layout areas");
    let input = areas.input.expect("input area");

    let input_row = row_text(&buffer, input.y);
    assert!(input_row.contains("Search"), "input row: {input_row}");
    assert!(input_row.contains("Help"), "input row: {input_row}");
    assert!(input_row.contains("Settings"), "input row: {input_row}");

    let library_row = row_text(&buffer, areas.library.y);
    assert!(
      library_row.contains("Quick Menu") || library_row.contains("Library"),
      "library panel not at hitbox row: {library_row}"
    );
  }

  #[test]
  fn stats_route_renders_period_tabs() {
    let mut app = App::default();
    app.view.size = Viewport {
      width: 160,
      height: 50,
    };
    app.push_navigation_stack(RouteId::Stats, ActiveBlock::Stats);
    let mut terminal = Terminal::new(TestBackend::new(160, 50)).unwrap();
    terminal.draw(|f| draw_main_layout(f, &app)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    let all_text: String = (0..50).map(|y| row_text(&buffer, y)).collect();
    assert!(all_text.contains("Stats"), "missing Stats title");
    assert!(
      all_text.contains("Last 30 Days"),
      "missing selected period label"
    );
    assert!(all_text.contains("Top Tracks"), "missing Top Tracks panel");
  }

  #[test]
  fn action_bar_renders_hints() {
    let (app, buffer) = render(160, 50);
    let areas = compute_main_layout(&app).expect("layout areas");
    let bar_row = row_text(&buffer, areas.action_bar.y);
    assert!(bar_row.contains("[/]"), "missing [/] in action bar: {bar_row}");
    assert!(bar_row.contains("[T]"), "missing [T] in action bar: {bar_row}");
    assert!(bar_row.contains("[B]"), "missing [B] in action bar: {bar_row}");
    assert!(bar_row.contains("[?]"), "missing [?] in action bar: {bar_row}");
  }
}

#[cfg(test)]
mod localization_tests;
