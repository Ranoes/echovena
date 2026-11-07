use crate::core::app::{ActiveBlock, App};
use crate::core::source::Source;
use crate::tui::layout::{is_wide_layout, library_constraints, split_input_help_and_settings};
use ratatui::{
  layout::{Constraint, Layout, Rect},
  Frame,
};

use super::{
  search::draw_input_and_help_box,
  util::draw_selectable_list,
};

pub fn draw_library_block(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::Library,
    current_route.hovered_block == ActiveBlock::Library,
  );
  let rows: Vec<&str> = app
    .library_rows()
    .iter()
    .map(|target| target.localized_name(app.strings()))
    .collect();
  draw_selectable_list(
    f,
    app,
    layout_chunk,
    app.strings().sidebar.library,
    &rows,
    highlight_state,
    Some(app.library_cursor()),
    &app.view.library_scroll_offset,
  );
}

/// Sidebar label for the pinned community playlist.
#[allow(dead_code)]
fn community_pin_label(app: &App) -> String {
  app.strings().sidebar.community_pin.to_string()
}

pub fn draw_playlist_block(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let strings = &app.strings().sidebar;
  let highlight_state = {
    let current_route = app.get_current_route();
    (
      current_route.active_block == ActiveBlock::MyPlaylists,
      current_route.hovered_block == ActiveBlock::MyPlaylists,
    )
  };

  // Apple Music: "All songs" first, then the Music playlists, each opening
  // the shared track table. Read-only, so no "Add Playlist".
  if app.active_source == Source::AppleMusic {
    let items: Vec<String> = if app.apple_music_playlists().is_empty() {
      vec![strings.empty_apple_music(&app.user_config.keys.manage_devices.to_string())]
    } else {
      app
        .apple_music_playlists()
        .iter()
        .map(|p| {
          format!(
            "\u{1F3B5} {}",
            strings.playlist_label(Source::AppleMusic, &p.uri, &p.name)
          )
        })
        .collect()
    };
    draw_selectable_list(
      f,
      app,
      layout_chunk,
      strings.apple_music,
      &items,
      highlight_state,
      app.view.selected_playlist_index,
      &app.view.playlist_scroll_offset,
    );
    return;
  }

  // Subsonic: the sidebar Playlists panel lists the server's playlists (no
  // local-write support, so no "Add Playlist").
  if app.active_source == Source::Subsonic {
    let items: Vec<String> = if app.subsonic_playlists().is_empty() {
      vec![strings.empty_subsonic(&app.user_config.keys.manage_devices.to_string())]
    } else {
      app
        .subsonic_playlists()
        .iter()
        .map(|p| format!("\u{1F3B5} {}", p.name))
        .collect()
    };
    draw_selectable_list(
      f,
      app,
      layout_chunk,
      strings.subsonic,
      &items,
      highlight_state,
      app.view.selected_playlist_index,
      &app.view.playlist_scroll_offset,
    );
    return;
  }

  // Qobuz: the sidebar Playlists panel lists the favorites row, the user's
  // playlists, and the favorite albums, each opening the shared track table.
  if app.active_source == Source::Qobuz {
    let items: Vec<String> = if app.qobuz_playlists().is_empty() {
      vec![strings.empty_qobuz(&app.user_config.keys.manage_devices.to_string())]
    } else {
      app
        .qobuz_playlists()
        .iter()
        .map(|p| {
          strings
            .playlist_label(Source::Qobuz, &p.uri, &p.name)
            .to_string()
        })
        .collect()
    };
    draw_selectable_list(
      f,
      app,
      layout_chunk,
      strings.qobuz,
      &items,
      highlight_state,
      app.view.selected_playlist_index,
      &app.view.playlist_scroll_offset,
    );
    return;
  }

  // Internet Radio: the sidebar Playlists panel lists the configured stations.
  // A station is a leaf — Enter plays it directly instead of drilling in.
  if app.active_source == Source::Radio {
    let items: Vec<String> = if app.radio_stations().is_empty() {
      vec![strings.empty_radio.to_string()]
    } else {
      app
        .radio_stations()
        .iter()
        .map(|s| format!("\u{1F4FB} {}", s.name))
        .collect()
    };

    draw_selectable_list(
      f,
      app,
      layout_chunk,
      strings.radio_stations,
      &items,
      highlight_state,
      app.view.selected_playlist_index,
      &app.view.playlist_scroll_offset,
    );
    return;
  }

  // Unified tabbed Playlist panel
  let title = match app.view.playlist_panel_tab {
    0 => " Playlist: EchoVena ",
    1 => " Playlist: Spotify ",
    _ => " Playlist: YouTube ",
  };

  let items = crate::tui::handlers::playlist::get_playlist_tab_items(app);

  draw_selectable_list(
    f,
    app,
    layout_chunk,
    title,
    &items,
    highlight_state,
    app.view.selected_playlist_index,
    &app.view.playlist_scroll_offset,
  );
}

pub fn draw_user_block(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let lib_constraints = library_constraints(&app.runtime_state);
  if is_wide_layout(app) {
    let [input_area, library_area, playlist_area] = layout_chunk.layout(&Layout::vertical([
      Constraint::Length(3),
      lib_constraints[0],
      lib_constraints[1],
    ]));

    // Search input and help
    let [input_text_area, help_area, settings_area] =
      split_input_help_and_settings(app, input_area);
    draw_input_and_help_box(f, app, input_text_area, help_area, settings_area);
    draw_library_block(f, app, library_area);
    draw_playlist_block(f, app, playlist_area);
  } else {
    let [library_area, playlist_area] = layout_chunk.layout(&Layout::vertical(lib_constraints));
    draw_library_block(f, app, library_area);
    draw_playlist_block(f, app, playlist_area);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::plugin_api::PlaylistInfo;
  use ratatui::{backend::TestBackend, Terminal};

  fn rendered(app: &App, area: Rect) -> String {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|f| draw_user_block(f, app, area)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..area.height)
      .flat_map(|y| (0..area.width).map(move |x| (x, y)))
      .filter_map(|(x, y)| buffer.cell((x, y)).map(|c| c.symbol().to_string()))
      .collect()
  }

  fn folder(name: &str) -> PlaylistInfo {
    PlaylistInfo {
      generated_name: None,
      uri: format!("file:///music/{name}"),
      name: name.to_string(),
      owner: "local".to_string(),
      track_count: 0,
      id: None,
      owner_id: None,
      collaborative: false,
      public: None,
      image_url: None,
    }
  }

  #[test]
  fn local_source_sidebar_lists_folders_below_the_free_library_rows() {
    let mut app = App::default_connected();
    app.active_source = Source::Local;
    let db = crate::sources::lossless_relay::db::Database::open_default().unwrap();
    db.upsert_imported_playlist(&crate::sources::lossless_relay::db::ImportedPlaylist {
      id: "local:playlist:jazz".to_string(),
      platform: "local".to_string(),
      title: "Jazz".to_string(),
      author: Some("local".to_string()),
      source_url: "local:playlist:jazz".to_string(),
      cover_art_url: None,
      snapshot_id: None,
      track_count: 0,
      last_synced_at: 0,
    })
    .unwrap();
    let content = rendered(&app, Rect::new(0, 0, 32, 40));
    assert!(
      content.contains("Jazz"),
      "local SQLite playlist should render: {content}"
    );
    assert!(
      content.contains("Playlist: EchoVena"),
      "panel title should be Playlist: EchoVena: {content}"
    );
    assert!(
      content.contains("Stats"),
      "the free library rows stay under Local: {content}"
    );
  }

  /// The foreground of the first cell drawing `symbol`, if any cell does.
  fn fg_of(app: &App, area: Rect, symbol: &str) -> Option<ratatui::style::Color> {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|f| draw_user_block(f, app, area)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
      .content()
      .iter()
      .find(|c| c.symbol() == symbol)
      .map(|c| c.fg)
  }

  #[test]
  fn sidebar_folder_icon_is_a_text_glyph_in_the_theme_text_color() {
    use crate::core::theme::Color;
    let mut app = App::default_connected();
    app.user_config.theme.text = Color::Rgb(1, 2, 3);
    app.view.playlist_panel_tab = 1;
    *app.all_playlists_mut() = vec![crate::core::test_helpers::playlist_info(
      "37i9dQZF1DXcBWIGoYBM5M",
      "Zest",
      "me",
      false,
    )];
    let area = Rect::new(0, 0, 40, 40);
    let content = rendered(&app, area);
    assert!(!content.contains('\u{1F4C1}'), "{content}");
    assert!(content.contains("Zest"), "{content}");
    assert_eq!(
      fg_of(&app, area, "Z"),
      Some(ratatui::style::Color::Rgb(1, 2, 3))
    );
  }

  #[test]
  fn local_source_folder_icon_is_a_text_glyph_in_the_theme_text_color() {
    use crate::core::theme::Color;
    let mut app = App::default_connected();
    app.active_source = Source::Local;
    app.user_config.theme.text = Color::Rgb(1, 2, 3);
    let db = crate::sources::lossless_relay::db::Database::open_default().unwrap();
    db.upsert_imported_playlist(&crate::sources::lossless_relay::db::ImportedPlaylist {
      id: "local:playlist:jazz".to_string(),
      platform: "local".to_string(),
      title: "Jazz".to_string(),
      author: Some("local".to_string()),
      source_url: "local:playlist:jazz".to_string(),
      cover_art_url: None,
      snapshot_id: None,
      track_count: 0,
      last_synced_at: 0,
    })
    .unwrap();
    let area = Rect::new(0, 0, 32, 40);
    let content = rendered(&app, area);
    assert!(!content.contains('\u{1F4C1}'), "{content}");
    assert!(content.contains("Jazz"), "{content}");
    assert_eq!(
      fg_of(&app, area, "J"),
      Some(ratatui::style::Color::Rgb(1, 2, 3))
    );
  }

  #[test]
  fn local_source_sidebar_shows_unified_playlists() {
    let mut app = App::default_connected();
    app.active_source = Source::Local;
    let content = rendered(&app, Rect::new(0, 0, 60, 40));
    assert!(
      content.contains("EchoVena"),
      "Local source should render EchoVena unified playlists: {content}"
    );
  }

  #[test]
  fn spotify_source_sidebar_shows_library() {
    let app = App::default_connected();
    let content = rendered(&app, Rect::new(0, 0, 32, 40));
    assert!(
      content.contains("Song List"),
      "Quick Menu Song List entry should render: {content}"
    );
  }

  #[test]
  fn spotify_scope_without_a_session_keeps_only_the_free_library_rows() {
    let app = App::default();
    let content = rendered(&app, Rect::new(0, 0, 32, 40));
    assert!(
      content.contains("Stats"),
      "Stats needs no session: {content}"
    );
    assert!(
      !content.contains("Liked Songs"),
      "Spotify rows need a session: {content}"
    );
  }

  #[test]
  fn community_pin_renders_below_add_and_above_playlists() {
    use crate::core::test_helpers::playlist_info;
    let mut app = App::default_connected();
    app.view.playlist_panel_tab = 1;
    *app.all_playlists_mut() = vec![playlist_info(
      "37i9dQZF1DXcBWIGoYBM5M",
      "My Mix",
      "me",
      false,
    )];
    let content = rendered(&app, Rect::new(0, 0, 40, 40));
    let import_idx = content
      .find("Import Spotify URL")
      .expect("Import Spotify URL should render");
    let mix_idx = content.find("My Mix").expect("real playlist should render");
    assert!(import_idx < mix_idx, "Import must be above playlists: {content}");
  }

  #[test]
  fn community_pin_absent_under_non_spotify_source() {
    let mut app = App::default();
    app.active_source = Source::Local;
    *app.local_playlists_mut() = vec![folder("Jazz")];
    let content = rendered(&app, Rect::new(0, 0, 40, 40));
    assert!(
      !content.contains("echovena community"),
      "pin must not render off Spotify: {content}"
    );
  }

  #[test]
  fn community_pin_absent_when_toggle_off() {
    let mut app = App::default();
    app.user_config.behavior.pin_community_playlist = false;
    let content = rendered(&app, Rect::new(0, 0, 40, 40));
    assert!(
      !content.contains("echovena community"),
      "pin must not render when toggled off: {content}"
    );
  }

  #[test]
  fn community_pin_absent_when_already_following() {
    use crate::core::app::COMMUNITY_PLAYLIST_ID;
    use crate::core::test_helpers::playlist_info;
    let mut app = App::default_connected();
    app.view.playlist_panel_tab = 1;
    *app.all_playlists_mut() = vec![playlist_info(
      COMMUNITY_PLAYLIST_ID,
      "Community Follow",
      "echovena",
      false,
    )];
    let content = rendered(&app, Rect::new(0, 0, 40, 40));
    assert!(
      content.contains("Community Follow"),
      "followed playlist should render: {content}"
    );
    assert!(
      !content.contains("echovena community"),
      "pin must be suppressed when already following: {content}"
    );
  }

  #[test]
  fn list_shrinking_after_scrolling_resets_offset_and_renders_items() {
    let mut app = App::default_connected();
    app.active_source = Source::Local;
    let area = Rect::new(0, 0, 32, 10);

    // Populate with 20 items and scroll down
    let db = crate::sources::lossless_relay::db::Database::open_default().unwrap();
    for i in 0..20 {
      db.upsert_imported_playlist(&crate::sources::lossless_relay::db::ImportedPlaylist {
        id: format!("local:playlist:f_{i}"),
        platform: "local".to_string(),
        title: format!("Folder {i}"),
        author: Some("local".to_string()),
        source_url: format!("local:playlist:f_{i}"),
        cover_art_url: None,
        snapshot_id: None,
        track_count: 0,
        last_synced_at: 0,
      })
      .unwrap();
    }
    app.view.selected_playlist_index = Some(15);
    rendered(&app, area);
    assert!(
      app.view.playlist_scroll_offset.get() > 0,
      "offset should be scrolled down"
    );

    // Shrink the list to 2 items
    for i in 0..20 {
      db.delete_imported_playlist(&format!("local:playlist:f_{i}")).unwrap();
    }
    for name in ["ShrunkA", "ShrunkB"] {
      db.upsert_imported_playlist(&crate::sources::lossless_relay::db::ImportedPlaylist {
        id: format!("local:playlist:{name}"),
        platform: "local".to_string(),
        title: name.to_string(),
        author: Some("local".to_string()),
        source_url: format!("local:playlist:{name}"),
        cover_art_url: None,
        snapshot_id: None,
        track_count: 0,
        last_synced_at: 0,
      })
      .unwrap();
    }
    app.view.selected_playlist_index = Some(0);
    let content = rendered(&app, area);

    // Offset must clamp to 0 and render the remaining items instead of blank space
    assert_eq!(
      app.view.playlist_scroll_offset.get(),
      0,
      "offset must clamp back to 0"
    );
    assert!(
      content.contains("ShrunkA"),
      "shrunken list must render top item: {content}"
    );
    assert!(
      content.contains("ShrunkB"),
      "shrunken list must render second item: {content}"
    );
  }
}
