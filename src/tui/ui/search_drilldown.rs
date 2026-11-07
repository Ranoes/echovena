use crate::core::app::{App, ArtistDetailSection, SearchDrilldown};
use crate::tui::theme::ThemeExt;
use ratatui::{
  layout::{Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, BorderType, Borders, Paragraph},
  Frame,
};

pub fn draw_search_drilldown(f: &mut Frame<'_>, app: &App, area: Rect) {
  if app.view.search_drilldown == SearchDrilldown::None {
    return;
  }
  f.render_widget(ratatui::widgets::Clear, area);

  let theme = &app.user_config.theme;

  match &app.view.search_drilldown {
    SearchDrilldown::None => {}
    SearchDrilldown::AlbumDetail {
      title,
      artist,
      year,
      tracks,
      selected_index,
      ..
    } => {
      let [header_area, list_area] = area.layout(&Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
      ]));

      // 1. Header Box
      let year_str = year.as_deref().unwrap_or("Release");
      let header_lines = vec![
        Line::from(vec![
          Span::styled("Album: ", Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD)),
          Span::styled(title, Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD)),
          Span::styled("   Artist: ", Style::default().fg(theme.hint.into())),
          Span::styled(artist, Style::default().fg(theme.text.into())),
          Span::styled(format!("   ({year_str})"), Style::default().fg(theme.inactive.into())),
        ]),
        Line::from(vec![
          Span::styled(format!("Track count: {} tracks", tracks.len()), Style::default().fg(theme.inactive.into())),
        ]),
      ];
      let header_widget = Paragraph::new(header_lines)
        .style(theme.base_style())
        .block(
          Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.header.into()))
            .title(Span::styled(" Album Information ", Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD))),
        );
      f.render_widget(header_widget, header_area);

      // 2. Track List
      let mut rows = Vec::new();
      if tracks.is_empty() {
        rows.push(Line::from(Span::styled(
          "  ⟳ Querying album tracklist from InnerTube / Lossless Relay...",
          Style::default().fg(theme.hint.into()).add_modifier(Modifier::BOLD),
        )));
      } else {
        for (idx, track) in tracks.iter().enumerate() {
          let is_selected = idx == *selected_index;
          let marker = if is_selected { "▶ " } else { "  " };
          let dur_sec = track.duration_ms / 1000;
          let dur_str = if dur_sec > 0 {
            format!("[{:02}:{:02}]", dur_sec / 60, dur_sec % 60)
          } else {
            String::new()
          };
          let style = if is_selected {
            Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(theme.text.into())
          };
          rows.push(Line::from(vec![
            Span::styled(format!("{marker}{:02}. ", idx + 1), style),
            Span::styled(track.name.clone(), style),
            Span::styled(format!(" — {}", track.artists.join(", ")), Style::default().fg(theme.inactive.into())),
            Span::styled(format!("  {dur_str}"), Style::default().fg(theme.inactive.into())),
          ]));
        }
      }

      let visible_height = list_area.height.saturating_sub(2) as usize;
      let scroll_y = if *selected_index >= visible_height && visible_height > 0 {
        (*selected_index - visible_height + 1) as u16
      } else {
        0
      };

      let list_widget = Paragraph::new(rows)
        .style(theme.base_style())
        .scroll((scroll_y, 0))
        .block(
          Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.selected.into()))
            .title(Span::styled(format!(" Tracklist ({} songs) ", tracks.len()), Style::default().fg(theme.header.into()))),
        );
      f.render_widget(list_widget, list_area);
    }
    SearchDrilldown::ArtistDetail {
      name,
      albums,
      top_songs,
      focus_section,
      selected_index,
      ..
    } => {
      let [header_area, content_area] = area.layout(&Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
      ]));

      // 1. Header Box
      let header_lines = vec![
        Line::from(vec![
          Span::styled("Artist Profile: ", Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD)),
          Span::styled(name, Style::default().fg(theme.text.into()).add_modifier(Modifier::BOLD)),
          Span::styled("   [Tab] Switch between Top Songs and Albums", Style::default().fg(theme.hint.into())),
        ]),
      ];
      let header_widget = Paragraph::new(header_lines)
        .style(theme.base_style())
        .block(
          Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.header.into()))
            .title(Span::styled(" Artist Discography ", Style::default().fg(theme.header.into()).add_modifier(Modifier::BOLD))),
        );
      f.render_widget(header_widget, header_area);

      // 2. Split Content: Left = Top Songs, Right = Albums
      let [songs_area, albums_area] = content_area.layout(&Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(50),
      ]));

      let is_songs_focused = *focus_section == ArtistDetailSection::TopSongs;
      let songs_border: ratatui::style::Color = if is_songs_focused { theme.selected.into() } else { theme.inactive.into() };

      let mut song_rows = Vec::new();
      if top_songs.is_empty() {
        song_rows.push(Line::from(Span::styled("  ⟳ Fetching top songs...", Style::default().fg(theme.hint.into()))));
      } else {
        for (idx, track) in top_songs.iter().enumerate() {
          let is_selected = is_songs_focused && idx == *selected_index;
          let marker = if is_selected { "▶ " } else { "  " };
          let dur_sec = track.duration_ms / 1000;
          let dur_str = if dur_sec > 0 { format!("[{:02}:{:02}]", dur_sec / 60, dur_sec % 60) } else { String::new() };
          let style = if is_selected {
            Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(theme.text.into())
          };
          song_rows.push(Line::from(vec![
            Span::styled(format!("{marker}{:02}. ", idx + 1), style),
            Span::styled(track.name.clone(), style),
            Span::styled(format!("  {dur_str}"), Style::default().fg(theme.inactive.into())),
          ]));
        }
      }
      let songs_widget = Paragraph::new(song_rows)
        .style(theme.base_style())
        .block(
          Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(songs_border))
            .title(Span::styled(format!(" Top Songs ({} tracks) ", top_songs.len()), Style::default().fg(songs_border).add_modifier(Modifier::BOLD))),
        );
      f.render_widget(songs_widget, songs_area);

      let is_albums_focused = *focus_section == ArtistDetailSection::Albums;
      let albums_border: ratatui::style::Color = if is_albums_focused { theme.selected.into() } else { theme.inactive.into() };

      let mut album_rows = Vec::new();
      if albums.is_empty() {
        album_rows.push(Line::from(Span::styled("  ⟳ Fetching artist albums...", Style::default().fg(theme.hint.into()))));
      } else {
        for (idx, album) in albums.iter().enumerate() {
          let is_selected = is_albums_focused && idx == *selected_index;
          let marker = if is_selected { "▶ " } else { "  " };
          let year_str = album.year.as_deref().unwrap_or("");
          let style = if is_selected {
            Style::default().fg(theme.selected.into()).add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(theme.text.into())
          };
          album_rows.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(album.title.clone(), style),
            Span::styled(format!(" ({})", year_str), Style::default().fg(theme.inactive.into())),
          ]));
        }
      }
      let albums_widget = Paragraph::new(album_rows)
        .style(theme.base_style())
        .block(
          Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(albums_border))
            .title(Span::styled(format!(" Albums ({} releases) ", albums.len()), Style::default().fg(albums_border).add_modifier(Modifier::BOLD))),
        );
      f.render_widget(albums_widget, albums_area);
    }
  }
}

