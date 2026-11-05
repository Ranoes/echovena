use crate::core::app::{ActiveBlock, App, InputContext, SearchResultBlock};
use crate::sources::youtube_search::{SearchBadge, SearchScope};
use crate::tui::layout::COMPACT_TOP_ROW_THRESHOLD;
use crate::tui::theme::ThemeExt;
use ratatui::{
  layout::{Constraint, Layout, Rect},
  style::{Color, Modifier, Style},
  text::{Line, Span, Text},
  widgets::{Block, BorderType, Borders, Paragraph, Wrap},
  Frame,
};

use rspotify::model::PlayableItem;
use rspotify::prelude::Id;

use super::util::{
  draw_selectable_list, get_color, get_search_results_highlight_state, join_artist_names,
};

/// Draws the search input, Help button and Settings button into pre-split
/// areas (see `tui::layout::split_input_help_and_settings`). Splitting is
/// owned by `compute_main_layout` so mouse hit-testing always matches.
pub fn draw_input_and_help_box(
  f: &mut Frame<'_>,
  app: &App,
  input_area: Rect,
  help_area: Rect,
  settings_area: Rect,
) {
  let row_width = input_area
    .width
    .saturating_add(help_area.width)
    .saturating_add(settings_area.width);
  let compact_top_row = row_width < COMPACT_TOP_ROW_THRESHOLD;

  let current_route = app.get_current_route();

  let highlight_state = (
    current_route.active_block == ActiveBlock::Input,
    current_route.hovered_block == ActiveBlock::Input,
  );

  let show_loading = app.live_search_loading
    || (app.is_loading && app.user_config.behavior.show_loading_indicator);
  let border_type = if show_loading {
    BorderType::Double
  } else {
    BorderType::Rounded
  };

  let input_string: String = app.view.input.iter().collect();
  let lines = Text::from(input_string.clone());
  // Compute horizontal scroll so the cursor stays visible within the input box.
  // inner width = total width - 2 (for left and right borders)
  let inner_width = input_area.width.saturating_sub(2);
  let scroll_offset = if inner_width > 0 && app.view.input_cursor_position >= inner_width {
    app.view.input_cursor_position - inner_width + 1
  } else {
    0
  };
  app.view.input_scroll_offset.set(scroll_offset);

  let input_title = match app.view.input_context {
    InputContext::PlaylistTrackSearch => app.strings().screens.search_playlist,
    InputContext::SavedTrackSearch => app.strings().screens.search_liked,
    InputContext::GlobalSearch => {
      if app.live_search_loading {
        " Searching YouTube InnerTube... "
      } else {
        " Search YouTube & YT Music (Live InnerTube) "
      }
    }
  };

  let input = Paragraph::new(lines).scroll((0, scroll_offset)).block(
    Block::default()
      .borders(Borders::ALL)
      .border_type(border_type)
      .title(Span::styled(
        input_title,
        get_color(highlight_state, app.user_config.theme),
      ))
      .style(app.user_config.theme.base_style())
      .border_style(get_color(highlight_state, app.user_config.theme)),
  );
  f.render_widget(input, input_area);

  let help_content = if show_loading {
    (app.user_config.theme.hint, "...")
  } else if compact_top_row {
    (app.user_config.theme.inactive, "?")
  } else {
    (
      app.user_config.theme.inactive,
      app.strings().screens.type_help,
    )
  };

  let block = Block::default()
    .title(Span::styled(
      app.strings().screens.help,
      Style::default().fg(help_content.0.into()),
    ))
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(help_content.0.into()));

  let lines = Text::from(help_content.1);
  let help = Paragraph::new(lines).block(block).style(
    Style::default()
      .fg(help_content.0.into())
      .bg(app.user_config.theme.background.into()),
  );
  f.render_widget(help, help_area);

  let settings_keybind_string = app
    .effective_open_settings_key()
    .to_string()
    .trim_matches(|c| c == '<' || c == '>')
    .to_string();
  let settings_hint = if compact_top_row {
    settings_keybind_string
  } else {
    app.strings().screens.type_key(&settings_keybind_string)
  };
  let settings_color = app.user_config.theme.inactive;
  let settings_block = Block::default()
    .title(Span::styled(
      app.strings().screens.settings,
      Style::default().fg(settings_color.into()),
    ))
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(settings_color.into()));

  let settings = Paragraph::new(settings_hint).block(settings_block).style(
    Style::default()
      .fg(settings_color.into())
      .bg(app.user_config.theme.background.into()),
  );
  f.render_widget(settings, settings_area);
}

/// Internet-radio search results: one full-area station list instead of the
/// five-block Spotify layout (a directory search returns only stations, so
/// Artists/Albums/Playlists/Podcasts would all be dead panels). Backed by the
/// same `SongSearch` block/index as the songs list so selection and Enter
/// share the existing machinery.
fn draw_radio_station_results(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let stations: Vec<String> = match &app.search_results().tracks {
    Some(tracks) => tracks
      .items
      .iter()
      .map(|station| {
        let mut row = format!("\u{1F4FB} {}", station.name);
        // Genre tags land in `artists`, the country/codec/bitrate summary in
        // `album` (see infra::radio::station_to_track_info); both optional.
        if !station.artists.is_empty() {
          row += &format!(" - {}", station.artists.join(", "));
        }
        if !station.album.is_empty() {
          row += &format!("  ({})", station.album);
        }
        row
      })
      .collect(),
    None => vec![],
  };

  draw_selectable_list(
    f,
    app,
    layout_chunk,
    app.strings().screens.radio_stations,
    &stations,
    get_search_results_highlight_state(app, SearchResultBlock::SongSearch),
    app.view.search_selected_tracks_index,
    &app.view.search_tracks_scroll_offset,
  );
}

pub fn draw_live_youtube_search(f: &mut Frame<'_>, app: &App, area: Rect) {
  if let crate::core::app::SearchDrilldown::AlbumDetail { .. } | crate::core::app::SearchDrilldown::ArtistDetail { .. } = &app.view.search_drilldown {
    super::search_drilldown::draw_search_drilldown(f, app, area);
    return;
  }

  let [scope_area, content_area] = area.layout(&Layout::vertical([
    Constraint::Length(3),
    Constraint::Min(4),
  ]));

  // 1. Scope Filter Bar
  let mut scope_spans = vec![
    Span::styled(" [Tab: Scope]  ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
  ];

  let scopes = [
    (SearchScope::Music, "MUSIC"),
    (SearchScope::Album, "ALBUM"),
    (SearchScope::Artist, "ARTIST"),
    (SearchScope::Video, "VIDEO"),
  ];

  for (scope, label) in scopes {
    if app.search_scope == scope {
      scope_spans.push(Span::styled(
        format!(" ● [ {} ] ", label),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
      ));
    } else {
      scope_spans.push(Span::styled(
        format!(" ○ {}  ", label),
        Style::default().fg(app.user_config.theme.inactive.into()),
      ));
    }
  }

  if app.live_search_loading {
    scope_spans.push(Span::styled(
      "  ⟳ [Searching InnerTube...]",
      Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ));
  } else {
    scope_spans.push(Span::styled(
      "  (Zero API Keys)",
      Style::default().fg(app.user_config.theme.hint.into()),
    ));
  }

  let scope_bar = Paragraph::new(Line::from(scope_spans)).block(
    Block::default()
      .borders(Borders::ALL)
      .border_type(BorderType::Rounded)
      .border_style(Style::default().fg(app.user_config.theme.inactive.into()))
      .title(Span::styled(" Filter Scope ", Style::default().fg(app.user_config.theme.hint.into()))),
  );
  f.render_widget(scope_bar, scope_area);

  // 2. Suggestions + Results split
  let (suggestions_area, results_area) = if !app.live_search_suggestions.is_empty() && content_area.width > 70 {
    let [s_area, r_area] = content_area.layout(&Layout::horizontal([
      Constraint::Length(26),
      Constraint::Min(30),
    ]));
    (Some(s_area), r_area)
  } else {
    (None, content_area)
  };

  // Render Autocomplete Suggestions if present
  if let Some(s_area) = suggestions_area {
    let mut suggestion_lines = Vec::new();
    for (idx, suggestion) in app.live_search_suggestions.iter().enumerate() {
      if idx == 0 {
        suggestion_lines.push(Line::from(vec![
          Span::styled("→ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
          Span::styled(suggestion, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
      } else {
        suggestion_lines.push(Line::from(vec![
          Span::styled("  ", Style::default().fg(app.user_config.theme.inactive.into())),
          Span::styled(suggestion, Style::default().fg(app.user_config.theme.inactive.into())),
        ]));
      }
    }

    let suggestions_widget = Paragraph::new(suggestion_lines).block(
      Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.user_config.theme.inactive.into()))
        .title(Span::styled(" Suggestions (→) ", Style::default().fg(Color::Cyan))),
    );
    f.render_widget(suggestions_widget, s_area);
  }

  // Render Live Search Results Table
  let is_search_focused = app.get_current_route().active_block == ActiveBlock::SearchResultBlock;
  let results_border_color = if is_search_focused {
    app.user_config.theme.selected
  } else {
    app.user_config.theme.inactive
  };

  let results_title = if app.live_search_items.is_empty() {
    " YouTube InnerTube Search ".to_string()
  } else {
    format!(
      " YouTube Results [Scope: {}] ({} items) ",
      app.search_scope.as_str(),
      app.live_search_items.len()
    )
  };

  if app.live_search_items.is_empty() {
    let empty_text = if app.live_search_loading {
      vec![
        Line::from(""),
        Line::from(Span::styled(
          "    ⟳ Querying YouTube InnerTube search endpoint...",
          Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
          "    Debouncing 250ms with zero Google API keys required.",
          Style::default().fg(app.user_config.theme.hint.into()),
        )),
      ]
    } else {
      let query_str: String = app.view.input.iter().collect();
      if query_str.trim().is_empty() {
        vec![
          Line::from(""),
          Line::from(Span::styled(
            "    EchoVena 2.0 Live YouTube & YouTube Music Search",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
          )),
          Line::from(""),
          Line::from(Span::styled(
            "    • Type any song title, artist, album, or playlist to search in real-time",
            Style::default().fg(Color::White),
          )),
          Line::from(Span::styled(
            "    • [Tab] / [Shift+Tab]   : Cycle scopes (All | Songs | Albums | Playlists)",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
          Line::from(Span::styled(
            "    • [↓] / [↑] or [j] / [k] : Highlight results live",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
          Line::from(Span::styled(
            "    • [Enter]               : Select result / instant playback",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
          Line::from(Span::styled(
            "    • [z]                   : Add track to queue",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
          Line::from(Span::styled(
            "    • [Ctrl+v] / [p]        : Paste clipboard text into search bar",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
          Line::from(Span::styled(
            "    • [Ctrl+i]              : Clear search query",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
          Line::from(Span::styled(
            "    • [Esc]                 : Return Home (preserves query)",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
        ]
      } else {
        vec![
          Line::from(""),
          Line::from(Span::styled(
            format!("    No YouTube results found for: \"{}\"", query_str.trim()),
            Style::default().fg(Color::Yellow),
          )),
          Line::from(""),
          Line::from(Span::styled(
            "    Try pressing [Tab] to cycle scope (All / Songs / Albums / Playlists) or refine terms.",
            Style::default().fg(app.user_config.theme.hint.into()),
          )),
        ]
      }
    };

    let empty_widget = Paragraph::new(empty_text).block(
      Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(results_border_color.into()))
        .title(Span::styled(results_title, Style::default().fg(Color::Cyan))),
    );
    f.render_widget(empty_widget, results_area);
    return;
  }

  // Calculate scrolling for results list
  let inner_height = results_area.height.saturating_sub(2) as usize;
  let mut scroll_offset = app.view.live_search_scroll_offset.get();

  if app.live_search_selected_index < scroll_offset {
    scroll_offset = app.live_search_selected_index;
  } else if inner_height > 0 && app.live_search_selected_index >= scroll_offset + inner_height {
    scroll_offset = app.live_search_selected_index.saturating_sub(inner_height) + 1;
  }
  app.view.live_search_scroll_offset.set(scroll_offset);

  let visible_items = app
    .live_search_items
    .iter()
    .enumerate()
    .skip(scroll_offset)
    .take(inner_height);

  let mut rows = Vec::new();
  for (idx, item) in visible_items {
    let is_selected = idx == app.live_search_selected_index;
    let mut row_spans = Vec::new();

    // Selection pointer
    if is_selected {
      row_spans.push(Span::styled(
        "▶ ",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
      ));
    } else {
      row_spans.push(Span::styled("  ", Style::default()));
    }

    // Source Badge
    row_spans.push(Span::styled(
      "[YouTube] ",
      Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    ));

    // Badges
    for badge in &item.badges {
      match badge {
        SearchBadge::Official => {
          row_spans.push(Span::styled(
            "[OFFICIAL] ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
          ));
        }
        SearchBadge::Explicit => {
          row_spans.push(Span::styled(
            "[EXPLICIT] ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
          ));
        }
        SearchBadge::Remaster => {
          row_spans.push(Span::styled(
            "[REMASTER] ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
          ));
        }
        SearchBadge::Live => {
          row_spans.push(Span::styled(
            "[LIVE] ",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
          ));
        }
        SearchBadge::Custom(label) => {
          row_spans.push(Span::styled(
            format!("[{}] ", label.to_uppercase()),
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
          ));
        }
      }
    }

    // Title
    let title_style = if is_selected {
      Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(Color::White)
    };
    row_spans.push(Span::styled(&item.title, title_style));

    // Artist
    let artist_style = if is_selected {
      Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
      Style::default().fg(Color::LightCyan)
    };
    row_spans.push(Span::styled(format!(" - {}", item.artist), artist_style));

    // Album if present
    if let Some(album) = &item.album {
      row_spans.push(Span::styled(
        format!("  ({})", album),
        Style::default().fg(app.user_config.theme.hint.into()),
      ));
    }

    // Duration if present
    if !item.duration_str.is_empty() {
      row_spans.push(Span::styled(
        format!("  [{}]", item.duration_str),
        Style::default().fg(Color::DarkGray),
      ));
    } else if item.duration_ms > 0 {
      let secs = item.duration_ms / 1000;
      let min = secs / 60;
      let s = secs % 60;
      row_spans.push(Span::styled(
        format!("  [{}:{:02}]", min, s),
        Style::default().fg(Color::DarkGray),
      ));
    }

    rows.push(Line::from(row_spans));
  }

  let results_widget = Paragraph::new(rows).block(
    Block::default()
      .borders(Borders::ALL)
      .border_type(BorderType::Rounded)
      .border_style(Style::default().fg(results_border_color.into()))
      .title(Span::styled(results_title, Style::default().fg(Color::Cyan))),
  );
  f.render_widget(results_widget, results_area);
}

pub fn draw_search_results(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  if let crate::core::app::SearchDrilldown::AlbumDetail { .. } | crate::core::app::SearchDrilldown::ArtistDetail { .. } = &app.view.search_drilldown {
    super::search_drilldown::draw_search_drilldown(f, app, layout_chunk);
    return;
  }

  if app.active_source == crate::core::source::Source::Radio {
    draw_radio_station_results(f, app, layout_chunk);
    return;
  }

  // Draw real-time YouTube InnerTube search when active, loading, results exist, or YouTube source is active
  if app.active_source == crate::core::source::Source::YouTube
    || !app.live_search_items.is_empty()
    || app.live_search_loading
    || !app.live_search_suggestions.is_empty()
  {
    draw_live_youtube_search(f, app, layout_chunk);
    return;
  }

  let [song_artist_area, albums_playlist_area] =
    layout_chunk.layout(&Layout::vertical([
      Constraint::Percentage(50),
      Constraint::Percentage(50),
    ]));

  {
    let [songs_area, artists_area] = song_artist_area.layout(&Layout::horizontal([
      Constraint::Percentage(50),
      Constraint::Percentage(50),
    ]));

    let currently_playing_id = app
      .current_playback_context
      .clone()
      .and_then(|context| {
        context.item.and_then(|item| match item {
          PlayableItem::Track(track) => track.id.map(|id| id.id().to_string()),
          PlayableItem::Episode(episode) => Some(episode.id.id().to_string()),
          _ => None,
        })
      })
      .unwrap_or_default();

    let songs = match &app.search_results().tracks {
      Some(tracks) => tracks
        .items
        .iter()
        .map(|item| {
          let mut song_name = "[Spotify] ".to_string();
          let id = item.id.clone().unwrap_or_default();
          if currently_playing_id == id {
            song_name += "▶ "
          }
          if app.liked_song_ids_set().contains(&id) {
            song_name += &app.user_config.padded_liked_icon();
          }

          song_name += item.display_name(app.strings());
          song_name += &format!(" - {}", item.artists.join(", "));
          song_name
        })
        .collect(),
      None => vec![],
    };

    draw_selectable_list(
      f,
      app,
      songs_area,
      app.strings().screens.songs,
      &songs,
      get_search_results_highlight_state(app, SearchResultBlock::SongSearch),
      app.view.search_selected_tracks_index,
      &app.view.search_tracks_scroll_offset,
    );

    let artists = match &app.search_results().artists {
      Some(artists) => artists
        .items
        .iter()
        .map(|item| {
          let mut artist = String::new();
          if let Some(ref id) = item.id {
            if app.followed_artist_ids_set().contains(id.as_str()) {
              artist.push_str(&app.user_config.padded_liked_icon());
            }
          }
          artist.push_str(&item.name);
          artist
        })
        .collect(),
      None => vec![],
    };

    draw_selectable_list(
      f,
      app,
      artists_area,
      app.strings().screens.artists,
      &artists,
      get_search_results_highlight_state(app, SearchResultBlock::ArtistSearch),
      app.view.search_selected_artists_index,
      &app.view.search_artists_scroll_offset,
    );
  }

  {
    let [albums_area, playlist_area] = albums_playlist_area.layout(&Layout::horizontal([
      Constraint::Percentage(50),
      Constraint::Percentage(50),
    ]));

    let albums = match &app.search_results().albums {
      Some(albums) => albums
        .items
        .iter()
        .map(|item| {
          let mut album_artist = String::new();
          if let Some(ref id) = item.id {
            if app.saved_album_ids_set().contains(id.as_str()) {
              album_artist.push_str(&app.user_config.padded_liked_icon());
            }
          }
          album_artist.push_str(&format!(
            "{} - {} ({})",
            item.name,
            join_artist_names(&item.artists),
            app.strings().screens.album_type(item.album_type.as_deref())
          ));
          album_artist
        })
        .collect(),
      None => vec![],
    };

    draw_selectable_list(
      f,
      app,
      albums_area,
      app.strings().screens.albums,
      &albums,
      get_search_results_highlight_state(app, SearchResultBlock::AlbumSearch),
      app.view.search_selected_album_index,
      &app.view.search_albums_scroll_offset,
    );

    let playlists = match &app.search_results().playlists {
      Some(playlists) => playlists
        .items
        .iter()
        .map(|item| item.name.to_owned())
        .collect::<Vec<String>>(),
      None => vec![],
    };

    if playlists.is_empty() {
      let warning_text = app.strings().screens.spotify_playlist_warning;
      let warning_paragraph = Paragraph::new(warning_text)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(app.user_config.theme.hint.into()))
        .block(
          Block::default()
            .title(Span::styled(
              app.strings().screens.playlists,
              get_color(
                get_search_results_highlight_state(app, SearchResultBlock::PlaylistSearch),
                app.user_config.theme,
              ),
            ))
            .borders(Borders::ALL)
            .border_style(get_color(
              get_search_results_highlight_state(app, SearchResultBlock::PlaylistSearch),
              app.user_config.theme,
            )),
        );
      f.render_widget(warning_paragraph, playlist_area);
    } else {
      draw_selectable_list(
        f,
        app,
        playlist_area,
        app.strings().screens.playlists,
        &playlists,
        get_search_results_highlight_state(app, SearchResultBlock::PlaylistSearch),
        app.view.search_selected_playlists_index,
        &app.view.search_playlists_scroll_offset,
      );
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::pagination::Paged;
  use crate::core::plugin_api::TrackInfo;
  use crate::core::source::Source;
  use ratatui::{backend::TestBackend, Terminal};

  fn rendered(app: &App, area: Rect) -> String {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
      .draw(|f| draw_search_results(f, app, area))
      .unwrap();
    let buffer = terminal.backend().buffer();
    (0..area.height)
      .flat_map(|y| (0..area.width).map(move |x| (x, y)))
      .filter_map(|(x, y)| buffer.cell((x, y)).map(|c| c.symbol().to_string()))
      .collect()
  }

  fn station(name: &str, tags: Vec<String>, summary: &str) -> TrackInfo {
    TrackInfo {
      generated_name: None,
      uri: Some(format!("radio:https://example.com/{name}")),
      name: name.to_string(),
      artists: tags,
      album: summary.to_string(),
      duration_ms: 0,
      id: None,
      album_id: None,
      artist_refs: vec![],
      is_playable: true,
      is_local: false,
      track_number: 0,
      explicit: false,
      image_url: None,
      release_date: None,
    }
  }

  /// Radio search results must render as one full-area Stations panel, not
  /// the five-block Spotify layout with stations shoved under "Songs".
  #[test]
  fn radio_results_render_single_station_panel() {
    let mut app = App::default();
    app.active_source = Source::Radio;
    app.set_search_results(crate::core::app::SearchResult {
      tracks: Some(Paged {
        items: vec![
          station(
            "Groove Salad",
            vec!["ambient".to_string(), "chillout".to_string()],
            "US \u{2022} MP3 \u{2022} 128 kbps",
          ),
          station("Bare FM", vec![], ""),
        ],
        total: 2,
        ..Default::default()
      }),
      ..Default::default()
    });

    let content = rendered(&app, Rect::new(0, 0, 90, 30));

    assert!(
      content.contains("Radio Stations"),
      "panel title should be Radio Stations: {content}"
    );
    assert!(
      content.contains("Groove Salad - ambient, chillout"),
      "station rows should show name and tags: {content}"
    );
    for spotify_panel in ["Songs", "Artists", "Albums", "Playlists"] {
      assert!(
        !content.contains(spotify_panel),
        "{spotify_panel} panel must not render under Radio: {content}"
      );
    }
    assert!(
      !content.contains("Bare FM -"),
      "a station without tags must not render a dangling separator: {content}"
    );
  }

  /// The Spotify layout renders the active music search blocks (Songs, Artists, Albums, Playlists).
  #[test]
  fn spotify_results_render_four_blocks() {
    let app = App::default();
    let content = rendered(&app, Rect::new(0, 0, 90, 30));
    for spotify_panel in ["Songs", "Artists", "Albums", "Playlists"] {
      assert!(
        content.contains(spotify_panel),
        "{spotify_panel} panel should render under Spotify: {content}"
      );
    }
    assert!(
      !content.contains("Podcasts"),
      "Podcasts panel must be removed from search: {content}"
    );
  }

  #[test]
  fn search_album_types_translate_without_changing_album_titles() {
    use crate::core::i18n::Language;
    use crate::core::plugin_api::AlbumInfo;
    use unicode_width::UnicodeWidthStr;
    let mut app = App::default();
    app.set_search_results(crate::core::app::SearchResult {
      albums: Some(crate::core::pagination::Paged {
        items: vec![AlbumInfo {
          name: "single {title}".into(),
          album_type: Some("single".into()),
          ..Default::default()
        }],
        total: 1,
        ..Default::default()
      }),
      ..Default::default()
    });
    for language in [Language::En, Language::ZhHans, Language::ZhHant] {
      app.user_config.behavior.language = language;
      app.refresh_language();
      let mut terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
      terminal
        .draw(|frame| draw_search_results(frame, &app, frame.area()))
        .unwrap();
      let buffer = terminal.backend().buffer();
      let mut text = String::new();
      for y in 0..40 {
        let mut x = 0;
        while x < 180 {
          let symbol = buffer[(x, y)].symbol();
          text.push_str(symbol);
          x += symbol.width().max(1) as u16;
        }
      }
      assert!(text.contains("single {title}"), "{text}");
      assert!(
        text.contains(&format!("({})", app.strings().screens.album_type_single)),
        "{text}"
      );
    }
  }

  fn rendered_input(app: &App, area: Rect) -> String {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
      .draw(|f| {
        draw_input_and_help_box(
          f,
          app,
          area,
          // The help and settings boxes only matter for the compact-row
          // decision; the title under test is on the input box itself.
          Rect::new(0, 0, 0, 0),
          Rect::new(0, 0, 0, 0),
        )
      })
      .unwrap();
    let buffer = terminal.backend().buffer();
    (0..area.height)
      .flat_map(|y| (0..area.width).map(move |x| (x, y)))
      .filter_map(|(x, y)| buffer.cell((x, y)).map(|c| c.symbol().to_string()))
      .collect()
  }

  /// The input box title names the search it feeds, so a saved-track search is
  /// not mistaken for a global one.
  #[test]
  fn saved_track_search_input_titles_the_box() {
    let mut app = App::default();
    app.view.input_context = InputContext::SavedTrackSearch;
    app.view.input = "cherry".chars().collect();

    let content = rendered_input(&app, Rect::new(0, 0, 40, 3));

    assert!(
      content.contains("Search Liked Songs"),
      "the input box should title the saved-track search: {content}"
    );
    assert!(
      !content.contains("Search Playlist"),
      "the saved-track search is not the playlist one: {content}"
    );
  }

  #[test]
  fn youtube_live_search_renders_scope_badges_and_results() {
    let mut app = App::default();
    app.active_source = crate::core::source::Source::YouTube;
    app.search_scope = SearchScope::Music;
    app.live_search_suggestions = vec!["bohemian rhapsody".into()];
    app.live_search_items = vec![
      crate::sources::youtube_search::SearchResultItem {
        id: "yt:video:1".into(),
        video_id: Some("1".into()),
        playlist_id: None,
        title: "Bohemian Rhapsody".into(),
        artist: "Queen".into(),
        album: Some("A Night at the Opera".into()),
        duration_ms: 354_000,
        duration_str: "5:54".into(),
        badges: vec![SearchBadge::Official, SearchBadge::Remaster],
        scope: SearchScope::Music,
        thumbnail_url: None,
        is_playable: true,
      },
    ];

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
      .draw(|frame| draw_search_results(frame, &app, frame.area()))
      .unwrap();
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..30 {
      for x in 0..120 {
        text.push_str(buffer[(x, y)].symbol());
      }
    }

    assert!(text.contains("Filter Scope"), "Scope bar: {text}");
    assert!(text.contains("[ Songs ]"), "Active Songs scope: {text}");
    assert!(text.contains("Suggestions"), "Suggestions panel: {text}");
    assert!(text.contains("bohemian rhapsody"), "Autocomplete suggestion: {text}");
    assert!(text.contains("Bohemian Rhapsody"), "Track title: {text}");
    assert!(text.contains("Queen"), "Artist name: {text}");
    assert!(text.contains("[OFFICIAL]"), "Official badge: {text}");
    assert!(text.contains("[REMASTER]"), "Remaster badge: {text}");
    assert!(text.contains("[5:54]"), "Duration string: {text}");
  }
}
