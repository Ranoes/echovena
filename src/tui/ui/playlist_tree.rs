use crate::core::app::App;
use crate::tui::handlers::playlist::{get_playlist_tree_items, PlaylistTreeItem};
use crate::tui::theme::ThemeExt;
use ratatui::{
  layout::{Constraint, Layout, Rect},
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
  Frame,
};

pub fn draw_playlist_tree_view(f: &mut Frame<'_>, app: &App, area: Rect) {
  if area.width == 0 || area.height == 0 {
    return;
  }

  let [header_area, list_area, footer_area] = area.layout(&Layout::vertical([
    Constraint::Length(3),
    Constraint::Min(5),
    Constraint::Length(2),
  ]));

  // 1. Header with Categories & Tabs
  let active_tab = app.view.playlist_panel_tab;
  let tab_titles = ["1: EchoVena / Local", "2: Spotify (Imported)", "3: YouTube & YTMusic"];
  let mut tab_spans = Vec::new();
  tab_spans.push(Span::raw(" Source Category [Tab]: "));
  for (idx, title) in tab_titles.iter().enumerate() {
    if idx == active_tab {
      tab_spans.push(Span::styled(
        format!(" [{title}] "),
        Style::default()
          .fg(Color::Black)
          .bg(Color::Cyan)
          .add_modifier(Modifier::BOLD),
      ));
    } else {
      tab_spans.push(Span::styled(
        format!("  {title}  "),
        Style::default().fg(app.user_config.theme.inactive.into()),
      ));
    }
  }

  let header_block = Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(app.user_config.theme.selected.into()))
    .title(Span::styled(
      " All Playlists — Tree View ",
      Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ));
  let header_widget = Paragraph::new(Line::from(tab_spans))
    .block(header_block)
    .style(app.user_config.theme.base_style());
  f.render_widget(header_widget, header_area);

  // 2. Tree Item List
  let tree = get_playlist_tree_items(app);
  let selected_index = app.view.selected_playlist_index.unwrap_or(0);

  let items: Vec<ListItem> = if tree.is_empty() {
    vec![ListItem::new(Line::from(vec![
      Span::raw("  "),
      Span::styled(
        "(No playlists found in this category — press 'A' to create or 'U' to import)",
        Style::default().fg(app.user_config.theme.hint.into()),
      ),
    ]))]
  } else {
    tree
      .iter()
      .enumerate()
      .map(|(idx, item)| {
        let is_selected = idx == selected_index;
        match item {
          PlaylistTreeItem::ActionItem { label, .. } => {
            let style = if is_selected {
              Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
            } else {
              Style::default().fg(Color::Green)
            };
            ListItem::new(Line::from(vec![
              Span::raw(if is_selected { " \u{25B6} " } else { "   " }),
              Span::styled(label.clone(), style),
            ]))
          }
          PlaylistTreeItem::PlaylistHeader {
            title,
            track_count,
            platform,
            is_expanded,
            ..
          } => {
            let arrow = if *is_expanded { "\u{25BC}" } else { "\u{25B6}" };
            let (tag, tag_color) = match platform.as_str() {
              "spotify" | "spotify_synced" => ("Spotify", Color::Green),
              "youtube" | "ytmusic" => ("YouTube", Color::Red),
              "local" => ("Local", Color::Magenta),
              _ => ("EchoVena", Color::Cyan),
            };

            let prefix = if is_selected { " \u{25B6} " } else { "   " };
            let line = Line::from(vec![
              Span::raw(prefix),
              Span::styled(
                format!("{arrow} "),
                Style::default().fg(if *is_expanded { Color::Yellow } else { Color::White }),
              ),
              Span::styled(
                format!("[{tag}] "),
                Style::default().fg(tag_color).add_modifier(Modifier::BOLD),
              ),
              Span::styled(
                title.clone(),
                if is_selected {
                  Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
                } else {
                  Style::default().fg(Color::White)
                },
              ),
              Span::styled(
                format!("  ({track_count} tracks)"),
                Style::default().fg(app.user_config.theme.hint.into()),
              ),
            ]);
            ListItem::new(line)
          }
          PlaylistTreeItem::TrackLeaf {
            title,
            artist,
            duration_ms,
            position,
            is_last,
            ..
          } => {
            let branch = if *is_last {
              "      \u{2514}\u{2500}\u{2500} "
            } else {
              "      \u{251C}\u{2500}\u{2500} "
            };

            let dur_str = if *duration_ms > 0 {
              let s = *duration_ms / 1000;
              format!(" [{}:{:02}]", s / 60, s % 60)
            } else {
              String::new()
            };

            let num_str = format!("{}. ", position + 1);
            let prefix = if is_selected { " \u{25B6}" } else { "  " };

            let line = Line::from(vec![
              Span::raw(prefix),
              Span::styled(branch, Style::default().fg(Color::DarkGray)),
              Span::styled(num_str, Style::default().fg(app.user_config.theme.hint.into())),
              Span::styled(
                title.clone(),
                if is_selected {
                  Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                } else {
                  Style::default().fg(Color::White)
                },
              ),
              Span::styled(" \u{2014} ", Style::default().fg(Color::DarkGray)),
              Span::styled(artist.clone(), Style::default().fg(Color::Gray)),
              Span::styled(dur_str, Style::default().fg(Color::Yellow)),
            ]);
            ListItem::new(line)
          }
        }
      })
      .collect()
  };

  let list_block = Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(app.user_config.theme.selected.into()))
    .title(Span::styled(
      format!(" Playlist Contents ({} items in tree) ", tree.len()),
      Style::default().fg(Color::White),
    ));

  // Compute scroll offset so the selection stays in view
  let max_visible = list_area.height.saturating_sub(2) as usize;
  let mut scroll_offset = app.view.playlist_scroll_offset.get();
  if selected_index < scroll_offset {
    scroll_offset = selected_index;
  } else if max_visible > 0 && selected_index >= scroll_offset + max_visible {
    scroll_offset = selected_index.saturating_sub(max_visible.saturating_sub(1));
  }
  app.view.playlist_scroll_offset.set(scroll_offset);

  let visible_items: Vec<ListItem> = items.into_iter().skip(scroll_offset).take(max_visible).collect();
  let list_widget = List::new(visible_items)
    .block(list_block)
    .style(app.user_config.theme.base_style());

  f.render_widget(list_widget, list_area);

  // 3. Footer Action Hints
  let footer_spans = vec![
    Span::styled(" [h] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    Span::raw("Collapse  │"),
    Span::styled(" [l] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    Span::raw("Expand  │"),
    Span::styled(" [Enter/o] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    Span::raw("Open / Play  │"),
    Span::styled(" [z] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    Span::raw("Queue  │"),
    Span::styled(" [s] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    Span::raw("Archive  │"),
    Span::styled(" [d] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
    Span::raw("Delete  │"),
    Span::styled(" [Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    Span::raw("Category  │"),
    Span::styled(" [Esc/q] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    Span::raw("Back"),
  ];
  let footer_widget = Paragraph::new(Line::from(footer_spans))
    .style(app.user_config.theme.base_style());
  f.render_widget(footer_widget, footer_area);
}

