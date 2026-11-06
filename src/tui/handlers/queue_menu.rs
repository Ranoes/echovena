use super::common_key_events;
use crate::core::action::Action;
use crate::core::app::App;
use crate::tui::event::Key;

/// The Queue screen is split into:
/// - Section 1: Row 0 (Now playing) followed by user-queued native items (rows 1 ..= native_queue.len()).
/// - Section 2: Upcoming album/playlist/auto-play radio tracks.
pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Esc | Key::Char('q') | Key::Char('Q') => {
      app.pop_navigation_stack();
    }
    k if k == app.user_config.keys.back || k == app.user_config.keys.show_queue => {
      app.pop_navigation_stack();
    }
    k if common_key_events::down_event(k, &app.user_config.keys) => move_selection(1, app),
    k if common_key_events::up_event(k, &app.user_config.keys) => move_selection(-1, app),
    // Shift+J / Shift+K reorder the selected item in native queue
    Key::Char('J') => reorder(1, app),
    Key::Char('K') => reorder(-1, app),
    // Navigation jumps
    k if common_key_events::high_event(k) => app.view.queue_selected_index = 0,
    k if common_key_events::middle_event(k) => {
      app.view.queue_selected_index = (row_count(app).saturating_sub(1)) / 2;
    }
    k if common_key_events::low_event(k) => {
      app.view.queue_selected_index = row_count(app).saturating_sub(1);
    }
    k if k == app.user_config.keys.remove_from_queue || matches!(k, Key::Delete | Key::Char('d')) => {
      remove_selected(app);
    }
    Key::Enter => play_selected(app),
    _ => {}
  }
}

fn queue_item_uri(app: &App, position: usize) -> Option<String> {
  app.native_queue.get(position).and_then(|t| t.uri.clone())
}

/// Jump to the selected queue row.
/// - Row 0: resumes or restarts currently playing track
/// - Row 1 ..= user_queue.len(): pops user queue item to head
/// - Rows past user_queue: jumps to upcoming context track (album, playlist, or auto-play radio)
fn play_selected(app: &mut App) {
  let selected = app.view.queue_selected_index;
  let data = crate::tui::ui::popups::get_queue_data(app);
  let user_q_len = data.user_queue.len();

  if selected == 0 {
    // Restart or resume current track
    app.dispatch(crate::infra::network::IoEvent::StartPlayback(None, None, None));
    return;
  }

  if selected <= user_q_len {
    let position = selected - 1;
    let Some(uri) = queue_item_uri(app, position) else {
      return;
    };
    app.apply(Action::PlayQueueItem { uri, position });
    app.view.queue_selected_index = 1;
    return;
  }

  // Section 2: Jump directly into upcoming context track
  let context_offset = selected - user_q_len - 1;
  if let Some(item) = data.context_tracks.get(context_offset) {
    let target = item.target_index;
    app.jump_to_queue_index(target);
    app.view.queue_selected_index = 0;
  }
}

/// Total selectable rows: Section 1 (now playing + user queue) + Section 2 (upcoming context tracks).
fn row_count(app: &App) -> usize {
  let data = crate::tui::ui::popups::get_queue_data(app);
  1 + data.user_queue.len() + data.context_tracks.len()
}

fn move_selection(delta: i32, app: &mut App) {
  let max_index = row_count(app).saturating_sub(1);
  let current = app.view.queue_selected_index;
  app.view.queue_selected_index = match delta {
    -1 => current.saturating_sub(1),
    _ => (current + 1).min(max_index),
  };
}

/// Remove the selected queue item.
/// - If in Section 1 (user queue): removes from native_queue.
/// - If in Section 2 (upcoming context): removes from upcoming tracks.
fn remove_selected(app: &mut App) {
  let selected = app.view.queue_selected_index;
  let data = crate::tui::ui::popups::get_queue_data(app);
  let user_q_len = data.user_queue.len();

  if selected == 0 {
    return;
  }

  if selected <= user_q_len {
    let position = selected - 1;
    if let Some(uri) = queue_item_uri(app, position) {
      app.apply(Action::RemoveFromQueue { uri, position });
      let max_index = row_count(app).saturating_sub(1);
      if app.view.queue_selected_index > max_index {
        app.view.queue_selected_index = max_index;
      }
    }
    return;
  }

  // Section 2: Remove upcoming track from YouTube playback
  #[cfg(feature = "youtube")]
  {
    let context_offset = selected - user_q_len - 1;
    if let Some(item) = data.context_tracks.get(context_offset) {
      let target = item.target_index;
      if let Some(yt) = app.youtube_playback.as_mut() {
        if target < yt.tracks.len() {
          let removed = yt.tracks.remove(target);
          app.set_status_message(format!("Removed '{}' from upcoming queue", removed.name), 3);
          let max_index = row_count(app).saturating_sub(1);
          if app.view.queue_selected_index > max_index {
            app.view.queue_selected_index = max_index;
          }
        }
      }
    }
  }
}

/// Swap the selected item with its neighbor in user queue.
fn reorder(delta: i32, app: &mut App) {
  let selected = app.view.queue_selected_index;
  let len = app.native_queue.len();
  if selected == 0 || selected > len {
    return;
  }
  let idx = selected - 1;
  let Some(uri) = queue_item_uri(app, idx) else {
    return;
  };
  match delta {
    1 if idx + 1 < len => {
      app.apply(Action::MoveQueueItem {
        uri,
        from: idx,
        to: idx + 1,
      });
      app.view.queue_selected_index = selected + 1;
    }
    -1 if idx > 0 => {
      app.apply(Action::MoveQueueItem {
        uri,
        from: idx,
        to: idx - 1,
      });
      app.view.queue_selected_index = selected - 1;
    }
    _ => {}
  }
}
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::plugin_api::TrackInfo;
  use crate::core::user_config::UserConfig;
  use std::sync::mpsc::channel;
  use std::time::SystemTime;

  fn track(uri: &str, name: &str) -> TrackInfo {
    TrackInfo {
      generated_name: None,
      uri: Some(uri.to_string()),
      name: name.to_string(),
      artists: vec!["Artist".to_string()],
      album: "Album".to_string(),
      duration_ms: 1000,
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

  fn app_with_queue(names: &[&str]) -> App {
    let (tx, _rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    for name in names {
      app.add_track_to_native_queue(track("spotify:track:x", name));
    }
    app
  }

  #[test]
  fn l_selects_the_last_queue_row() {
    let mut app = app_with_queue(&["A", "B", "C"]);
    handler(Key::Char('L'), &mut app);
    assert_eq!(app.view.queue_selected_index, 3);
  }

  #[test]
  fn h_selects_the_now_playing_row_and_m_the_middle_row() {
    let mut app = app_with_queue(&["A", "B", "C"]);
    app.view.queue_selected_index = 3;
    handler(Key::Char('H'), &mut app);
    assert_eq!(app.view.queue_selected_index, 0);
    // Rows: [now playing, A, B, C] => middle index 1.
    handler(Key::Char('M'), &mut app);
    assert_eq!(app.view.queue_selected_index, 1);
  }

  #[test]
  fn move_selection_clamps_to_rows() {
    let mut app = app_with_queue(&["A", "B"]);
    // Rows: [now playing, A, B] => max index 2.
    app.view.queue_selected_index = 0;
    move_selection(-1, &mut app);
    assert_eq!(app.view.queue_selected_index, 0);
    move_selection(1, &mut app);
    move_selection(1, &mut app);
    move_selection(1, &mut app);
    move_selection(1, &mut app);
    assert_eq!(app.view.queue_selected_index, 2);
  }

  #[test]
  fn remove_ignores_now_playing_row() {
    let mut app = app_with_queue(&["A", "B"]);
    app.view.queue_selected_index = 0;
    remove_selected(&mut app);
    assert_eq!(app.native_queue.len(), 2);
  }

  #[test]
  fn remove_deletes_selected_and_clamps() {
    let mut app = app_with_queue(&["A", "B"]);
    // Select last item (row 2 => native_queue[1] == "B").
    app.view.queue_selected_index = 2;
    remove_selected(&mut app);
    assert_eq!(app.native_queue.len(), 1);
    assert_eq!(app.native_queue[0].name, "A");
    // Selection clamped from 2 to the new max (row 1).
    assert_eq!(app.view.queue_selected_index, 1);
  }

  #[test]
  fn reorder_down_swaps_and_follows_item() {
    let mut app = app_with_queue(&["A", "B", "C"]);
    // Select A (row 1).
    app.view.queue_selected_index = 1;
    reorder(1, &mut app);
    assert_eq!(
      app
        .native_queue
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>(),
      vec!["B", "A", "C"]
    );
    // Selection followed A down to row 2.
    assert_eq!(app.view.queue_selected_index, 2);
  }

  #[test]
  fn reorder_up_swaps_and_follows_item() {
    let mut app = app_with_queue(&["A", "B", "C"]);
    // Select C (row 3).
    app.view.queue_selected_index = 3;
    reorder(-1, &mut app);
    assert_eq!(
      app
        .native_queue
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>(),
      vec!["A", "C", "B"]
    );
    assert_eq!(app.view.queue_selected_index, 2);
  }

  #[test]
  fn reorder_at_bounds_is_noop() {
    let mut app = app_with_queue(&["A", "B"]);
    // First item can't move up.
    app.view.queue_selected_index = 1;
    reorder(-1, &mut app);
    assert_eq!(app.native_queue[0].name, "A");
    assert_eq!(app.view.queue_selected_index, 1);
    // Last item can't move down.
    app.view.queue_selected_index = 2;
    reorder(1, &mut app);
    assert_eq!(app.native_queue[1].name, "B");
    assert_eq!(app.view.queue_selected_index, 2);
    // Now-playing row is non-actionable.
    app.view.queue_selected_index = 0;
    reorder(1, &mut app);
    assert_eq!(app.view.queue_selected_index, 0);
  }

  #[test]
  fn enter_drops_the_rows_above_the_selection_and_selects_the_new_head() {
    let mut app = app_with_queue(&["A", "B", "C"]);
    // Select C (row 3): A and B are dropped, C becomes the head.
    app.view.queue_selected_index = 3;
    handler(Key::Enter, &mut app);
    assert_eq!(
      app
        .native_queue
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>(),
      vec!["C"]
    );
    assert_eq!(app.view.queue_selected_index, 1);
  }

  #[test]
  fn enter_on_the_now_playing_row_changes_nothing() {
    let mut app = app_with_queue(&["A", "B"]);
    app.view.queue_selected_index = 0;
    handler(Key::Enter, &mut app);
    assert_eq!(app.native_queue.len(), 2);
    assert_eq!(app.view.queue_selected_index, 0);
  }
}