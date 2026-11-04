use crate::core::app::{App, ArtistDetailSection, SearchDrilldown};
use ratatui::{
  layout::{Alignment, Constraint, Layout, Rect},
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Block, BorderType, Borders, Paragraph},
  Frame,
};

pub fn draw_search_drilldown(f: &mut Frame<'_>, app: &App, area: Rect) {
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
      let [header_area, list_area, footer_area] = area.layout(&Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(5),
        Constraint::Length(2),
      ]));

      // 1. Header Box
      let year_str = year.as_deref().unwrap_or("Release");
      let header_lines = vec![
        Line::from(vec![
          Span::styled("Album: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
          Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
          Span::styled("   Artist: ", Style::default().fg(Color::Yellow)),
          Span::styled(artist, Style::default().fg(Color::White)),
          Span::styled(format!("   ({year_str})"), Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
          Span::styled(format!("Track count: {} tracks", tracks.len()), Style::default().fg(Color::DarkGray)),
        ]),
      ];
      let header_widget = Paragraph::new(header_lines).block(
        Block::default()
          .borders(Borders::ALL)
          .border_type(BorderType::Rounded)
          .border_style(Style::default().fg(Color::Cyan))
          .title(Span::styled(" Album Information ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
      );
      f.render_widget(header_widget, header_area);

      // 2. Track List
      let mut rows = Vec::new();
      if tracks.is_empty() {
        rows.push(Line::from(Span::styled(
          "  (Loading album tracks or no tracks found...)",
          Style::default().fg(Color::Yellow),
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
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(Color::White)
          };
          rows.push(Line::from(vec![
            Span::styled(format!("{marker}{:02}. ", idx + 1), style),
            Span::styled(track.name.clone(), style),
            Span::styled(format!(" — {}", track.artists.join(", ")), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("  {dur_str}"), Style::default().fg(Color::DarkGray)),
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
        .scroll((scroll_y, 0))
        .block(
          Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(app.user_config.theme.selected.into()))
            .title(Span::styled(format!(" Tracklist ({} songs) ", tracks.len()), Style::default().fg(Color::Cyan))),
        );
      f.render_widget(list_widget, list_area);

      // 3. Footer
      let footer = Line::from(vec![
        Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Play Track & Queue Album   ", Style::default().fg(Color::White)),
        Span::styled("[z] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled("Queue Track   ", Style::default().fg(Color::White)),
        Span::styled("[s] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("Download   ", Style::default().fg(Color::White)),
        Span::styled("[Esc / q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled("Back to Search", Style::default().fg(Color::White)),
      ]);
      f.render_widget(Paragraph::new(footer).alignment(Alignment::Center), footer_area);
    }
    SearchDrilldown::ArtistDetail {
      name,
      albums,
      top_songs,
      focus_section,
      selected_index,
      ..
    } => {
      let [header_area, content_area, footer_area] = area.layout(&Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
      ]));

      // 1. Header Box
      let header_lines = vec![
        Line::from(vec![
          Span::styled("Artist Profile: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
          Span::styled(name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
          Span::styled("   [Tab] Switch between Top Songs and Albums", Style::default().fg(Color::Yellow)),
        ]),
      ];
      let header_widget = Paragraph::new(header_lines).block(
        Block::default()
          .borders(Borders::ALL)
          .border_type(BorderType::Rounded)
          .border_style(Style::default().fg(Color::Cyan))
          .title(Span::styled(" Artist Discography ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
      );
      f.render_widget(header_widget, header_area);

      // 2. Split Content: Left = Top Songs, Right = Albums
      let [songs_area, albums_area] = content_area.layout(&Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(50),
      ]));

      let is_songs_focused = *focus_section == ArtistDetailSection::TopSongs;
      let songs_border = if is_songs_focused { Color::Cyan } else { Color::DarkGray };

      let mut song_rows = Vec::new();
      if top_songs.is_empty() {
        song_rows.push(Line::from(Span::styled("  (No songs found)", Style::default().fg(Color::DarkGray))));
      } else {
        for (idx, track) in top_songs.iter().enumerate() {
          let is_selected = is_songs_focused && idx == *selected_index;
          let marker = if is_selected { "▶ " } else { "  " };
          let dur_sec = track.duration_ms / 1000;
          let dur_str = if dur_sec > 0 { format!("[{:02}:{:02}]", dur_sec / 60, dur_sec % 60) } else { String::new() };
          let style = if is_selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(Color::White)
          };
          song_rows.push(Line::from(vec![
            Span::styled(format!("{marker}{:02}. ", idx + 1), style),
            Span::styled(track.name.clone(), style),
            Span::styled(format!("  {dur_str}"), Style::default().fg(Color::DarkGray)),
          ]));
        }
      }
      let songs_widget = Paragraph::new(song_rows).block(
        Block::default()
          .borders(Borders::ALL)
          .border_type(BorderType::Rounded)
          .border_style(Style::default().fg(songs_border))
          .title(Span::styled(format!(" Top Songs ({} tracks) ", top_songs.len()), Style::default().fg(songs_border).add_modifier(Modifier::BOLD))),
      );
      f.render_widget(songs_widget, songs_area);

      let is_albums_focused = *focus_section == ArtistDetailSection::Albums;
      let albums_border = if is_albums_focused { Color::Cyan } else { Color::DarkGray };

      let mut album_rows = Vec::new();
      if albums.is_empty() {
        album_rows.push(Line::from(Span::styled("  (No albums found)", Style::default().fg(Color::DarkGray))));
      } else {
        for (idx, album) in albums.iter().enumerate() {
          let is_selected = is_albums_focused && idx == *selected_index;
          let marker = if is_selected { "▶ " } else { "  " };
          let year_str = album.year.as_deref().unwrap_or("");
          let style = if is_selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
          } else {
            Style::default().fg(Color::White)
          };
          album_rows.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(album.title.clone(), style),
            Span::styled(format!(" ({})", year_str), Style::default().fg(Color::DarkGray)),
          ]));
        }
      }
      let albums_widget = Paragraph::new(album_rows).block(
        Block::default()
          .borders(Borders::ALL)
          .border_type(BorderType::Rounded)
          .border_style(Style::default().fg(albums_border))
          .title(Span::styled(format!(" Albums ({} releases) ", albums.len()), Style::default().fg(albums_border).add_modifier(Modifier::BOLD))),
      );
      f.render_widget(albums_widget, albums_area);

      // 3. Footer
      let footer = Line::from(vec![
        Span::styled("[Tab] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("Switch Songs/Albums   ", Style::default().fg(Color::White)),
        Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Play Song / Open Album   ", Style::default().fg(Color::White)),
        Span::styled("[Esc / q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled("Back to Search", Style::default().fg(Color::White)),
      ]);
      f.render_widget(Paragraph::new(footer).alignment(Alignment::Center), footer_area);
    }
  }
}

