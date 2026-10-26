use super::common_key_events;
use crate::core::action::Action;
use crate::core::app::App;
use crate::core::source::Source;
use crate::tui::event::Key;

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Tab => {}
    k if common_key_events::down_event(k, &app.user_config.keys) => {
      app.view.source_list_index = common_key_events::on_down_press_handler(
        Source::picker_sources(),
        Some(app.view.source_list_index),
      );
    }
    k if common_key_events::up_event(k, &app.user_config.keys) => {
      app.view.source_list_index = common_key_events::on_up_press_handler(
        Source::picker_sources(),
        Some(app.view.source_list_index),
      );
    }
    k if common_key_events::high_event(k) => {
      app.view.source_list_index = 0;
    }
    k if common_key_events::middle_event(k) => {
      app.view.source_list_index =
        common_key_events::on_middle_press_handler(Source::picker_sources());
    }
    k if common_key_events::low_event(k) => {
      app.view.source_list_index =
        common_key_events::on_low_press_handler(Source::picker_sources());
    }
    Key::Enter => select_source(app),
    Key::Esc | Key::Char('q') => {
      app.pop_navigation_stack();
    }
    _ => {}
  }
}

/// Commit the highlighted source as the new active source and close the picker.
/// This is browse-scope only: it never starts or stops playback.
fn select_source(app: &mut App) {
  let source = Source::picker_sources()[app.view.source_list_index];
  // Picking Apple Music again reloads it: Music's library can change under us.
  // Picking Spotify again without a session retries the login.
  if app.active_source != source
    || source == Source::AppleMusic
    || (source == Source::Spotify && !app.spotify_connected)
  {
    app.apply(Action::SelectSource(source));
    // Reset the sidebar playlist cursor to the top of the new source's list.
    app.view.selected_playlist_index = Some(0);
    if source == Source::Local {
      app.view.local_playlists_index = 0;
    }
  }

  app.set_status_message(
    app
      .strings()
      .status
      .app
      .source_selected(app.strings().sidebar.source_label(source)),
    4,
  );
  app.pop_navigation_stack();
}
