use crate::core::app::{ActiveBlock, App};
use ratatui::{
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
  Frame,
};

use super::util::{get_color, hint_span};

pub fn draw_playlist_sync(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::PlaylistSync,
    current_route.hovered_block == ActiveBlock::PlaylistSync,
  );
  let [settings_area, playlists_area, help_area] = layout_chunk.layout(&Layout::vertical([
    Constraint::Length(8),
    Constraint::Min(6),
    Constraint::Length(1),
  ]));

  // 1. Settings & Platform Authentication Status Area
  draw_sync_settings(f, app, settings_area, highlight_state);

  // 2. Synced Cloud Playlists (SQLite Cache)
  draw_synced_playlists(f, app, playlists_area, highlight_state);

  // 3. Bottom Help & Action Bar
  draw_help_bar(f, app, help_area);
}

fn draw_sync_settings(f: &mut Frame<'_>, app: &App, area: Rect, highlight_state: (bool, bool)) {
  let theme = app.user_config.theme;
  let spotify_connected = crate::sources::sync::auth::SpotifyAuthHandler::resolve_sp_dc().is_some();
  let youtube_connected = crate::sources::sync::auth::YouTubeAuthHandler::has_auth();
  let sync_in_flight = app.playlist_sync_in_flight();

  let spotify_spans = if spotify_connected {
    vec![
      Span::styled("  ● Spotify:   ", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)),
      Span::styled("CONNECTED (sp_dc authenticated)    ", Style::default().fg(theme.text.into())),
      Span::styled("[Key C] ", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Update sp_dc Cookie", Style::default().fg(theme.inactive.into())),
    ]
  } else {
    vec![
      Span::styled("  ○ Spotify:   ", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
      Span::styled("NOT CONFIGURED                     ", Style::default().fg(theme.inactive.into())),
      Span::styled("[Key C] ", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Set sp_dc Cookie (Required for Spotify Sync)", Style::default().fg(theme.hint.into())),
    ]
  };

  let youtube_spans = if youtube_connected {
    vec![
      Span::styled("  ● YouTube:   ", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)),
      Span::styled("CONNECTED (cookies active)         ", Style::default().fg(theme.text.into())),
      Span::styled("[Key Y] ", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Update YouTube Cookies", Style::default().fg(theme.inactive.into())),
    ]
  } else {
    vec![
      Span::styled("  ○ YouTube:   ", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
      Span::styled("NOT CONFIGURED                     ", Style::default().fg(theme.inactive.into())),
      Span::styled("[Key Y] ", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Set cookies.txt (Optional for Private Playlists)", Style::default().fg(theme.hint.into())),
    ]
  };

  let sync_status_spans = if sync_in_flight {
    vec![
      Span::styled("  ⟳ Sync Engine: ", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
      Span::styled("SYNCING IN BACKGROUND...           ", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
      Span::styled("[Key s / Enter] ", Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Trigger Sync", Style::default().fg(theme.inactive.into())),
    ]
  } else {
    vec![
      Span::styled("  ✔ Sync Engine: ", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Ready (Bidirectional Sync Active)  ", Style::default().fg(theme.text.into())),
      Span::styled("[Key s / Enter] ", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
      Span::styled("Run Full Bidirectional Sync Now", Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD)),
    ]
  };

  let content = vec![
    Line::from(""),
    Line::from(spotify_spans),
    Line::from(youtube_spans),
    Line::from(sync_status_spans),
  ];

  let block = Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .title(Span::styled(
      " ⚡ Cloud Playlist Sync & Platform Settings ",
      get_color(highlight_state, theme).add_modifier(Modifier::BOLD),
    ))
    .border_style(get_color(highlight_state, theme));

  f.render_widget(Paragraph::new(content).block(block), area);
}

fn draw_synced_playlists(f: &mut Frame<'_>, app: &App, area: Rect, highlight_state: (bool, bool)) {
  let theme = app.user_config.theme;

  let imported_playlists = crate::sources::lossless_relay::db::Database::open_default()
    .ok()
    .and_then(|db| db.list_imported_playlists().ok())
    .unwrap_or_default();

  let title = format!(
    " 📁 Synced Playlists (SQLite Local Cache: {} playlists) ",
    imported_playlists.len()
  );

  if imported_playlists.is_empty() {
    let empty_text = vec![
      Line::from(""),
      Line::from(Span::styled(
        "No synced cloud playlists found in local database (~/.local/share/echovena/library.db).",
        Style::default().fg(theme.hint.into()),
      )),
      Line::from(""),
      Line::from(vec![
        Span::styled("  • Press ", Style::default().fg(theme.inactive.into())),
        Span::styled("[s]", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" or ", Style::default().fg(theme.inactive.into())),
        Span::styled("[Enter]", Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" to auto-fetch remote playlists from Spotify and YouTube.", Style::default().fg(theme.text.into())),
      ]),
      Line::from(vec![
        Span::styled("  • Press ", Style::default().fg(theme.inactive.into())),
        Span::styled("[C]", Style::default().fg(theme.hovered.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" to configure your Spotify sp_dc authentication cookie.", Style::default().fg(theme.text.into())),
      ]),
      Line::from(vec![
        Span::styled("  • Press ", Style::default().fg(theme.inactive.into())),
        Span::styled("[Y]", Style::default().fg(theme.error_text.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" to configure your YouTube Netscape cookies.", Style::default().fg(theme.text.into())),
      ]),
      Line::from(vec![
        Span::styled("  • Press ", Style::default().fg(theme.inactive.into())),
        Span::styled("[U]", Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD)),
        Span::styled(" anywhere to paste and import any Spotify or YouTube URL directly.", Style::default().fg(theme.text.into())),
      ]),
    ];

    let empty = Paragraph::new(empty_text)
      .wrap(Wrap { trim: true })
      .block(
        Block::default()
          .borders(Borders::ALL)
          .border_type(BorderType::Rounded)
          .title(Span::styled(&title, get_color(highlight_state, theme)))
          .border_style(get_color(highlight_state, theme)),
      );
    f.render_widget(empty, area);
  } else {
    let selected_idx = app.view.playlist_sync_selected_link.min(imported_playlists.len().saturating_sub(1));

    let items: Vec<ListItem> = imported_playlists
      .iter()
      .enumerate()
      .map(|(idx, pl)| {
        let is_selected = idx == selected_idx;
        let prefix = if is_selected { " ▶ " } else { "   " };

        let (tag_color, tag_text) = match pl.platform.as_str() {
          "spotify" => (theme.hovered.into(), "[Spotify] "),
          "youtube" => (theme.error_text.into(), "[YouTube] "),
          _ => (theme.selected.into(), "[EchoVena] "),
        };

        let mut spans = vec![
          Span::styled(prefix, Style::default().fg(if is_selected { theme.selected.into() } else { theme.inactive.into() })),
          Span::styled(tag_text, Style::default().fg(tag_color).add_modifier(Modifier::BOLD)),
          Span::styled(
            &pl.title,
            if is_selected {
              Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD)
            } else {
              Style::default().fg(theme.text.into())
            },
          ),
        ];

        if let Some(ref author) = pl.author {
          if !author.is_empty() {
            spans.push(Span::styled(format!("  by {}", author), Style::default().fg(theme.inactive.into())));
          }
        }

        spans.push(Span::styled(
          format!("  ({} tracks)", pl.track_count),
          Style::default().fg(theme.hint.into()),
        ));

        if pl.last_synced_at > 0 {
          let synced_time = chrono::DateTime::from_timestamp(pl.last_synced_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "recent".to_string());
          spans.push(Span::styled(format!("  Synced: {}", synced_time), Style::default().fg(theme.inactive.into())));
        }

        let item_style = if is_selected {
          Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)
        } else {
          Style::default()
        };

        ListItem::new(Line::from(spans)).style(item_style)
      })
      .collect();

    let list = List::new(items).block(
      Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(&title, get_color(highlight_state, theme)))
        .border_style(get_color(highlight_state, theme)),
    );

    f.render_widget(list, area);
  }
}

fn draw_help_bar(f: &mut Frame<'_>, app: &App, area: Rect) {
  let theme = app.user_config.theme;
  let label = |text: &'static str| Span::styled(text, Style::default().fg(theme.inactive.into()));

  let line = Line::from(vec![
    hint_span("s/Enter", theme),
    label(" Sync Now   "),
    hint_span("C", theme),
    label(" Spotify sp_dc   "),
    hint_span("Y", theme),
    label(" YouTube Cookies   "),
    hint_span("D", theme),
    label(" Delete   "),
    hint_span("↑/↓", theme),
    label(" Select   "),
    hint_span("←", theme),
    label(" Back"),
  ]);

  f.render_widget(Paragraph::new(line), area);
}
