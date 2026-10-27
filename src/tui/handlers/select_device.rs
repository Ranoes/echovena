use super::common_key_events;
use crate::core::action::Action;
use crate::core::app::{App, SourceFocus};
use crate::core::source::{DownloadSource, Source};
use crate::tui::event::Key;

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Tab | Key::Right | Key::Left | Key::Char('h') | Key::Char('l') => {
      app.view.source_device_focus = match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => SourceFocus::DownloadSource,
        SourceFocus::DownloadSource | SourceFocus::Devices => SourceFocus::StreamSource,
      };
    }
    k if common_key_events::down_event(k, &app.user_config.keys) => {
      match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => {
          app.view.source_list_index = common_key_events::on_down_press_handler(
            Source::picker_sources(),
            Some(app.view.source_list_index),
          );
        }
        SourceFocus::DownloadSource | SourceFocus::Devices => {
          let count = DownloadSource::ALL.len();
          app.view.download_source_index = (app.view.download_source_index + 1) % count;
        }
      }
    }
    k if common_key_events::up_event(k, &app.user_config.keys) => {
      match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => {
          app.view.source_list_index = common_key_events::on_up_press_handler(
            Source::picker_sources(),
            Some(app.view.source_list_index),
          );
        }
        SourceFocus::DownloadSource | SourceFocus::Devices => {
          let count = DownloadSource::ALL.len();
          app.view.download_source_index = if app.view.download_source_index == 0 {
            count.saturating_sub(1)
          } else {
            app.view.download_source_index - 1
          };
        }
      }
    }
    k if common_key_events::high_event(k) => {
      match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => app.view.source_list_index = 0,
        SourceFocus::DownloadSource | SourceFocus::Devices => app.view.download_source_index = 0,
      }
    }
    k if common_key_events::middle_event(k) => {
      match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => {
          app.view.source_list_index =
            common_key_events::on_middle_press_handler(Source::picker_sources());
        }
        SourceFocus::DownloadSource | SourceFocus::Devices => {
          app.view.download_source_index = DownloadSource::ALL.len() / 2;
        }
      }
    }
    k if common_key_events::low_event(k) => {
      match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => {
          app.view.source_list_index =
            common_key_events::on_low_press_handler(Source::picker_sources());
        }
        SourceFocus::DownloadSource | SourceFocus::Devices => {
          app.view.download_source_index = DownloadSource::ALL.len().saturating_sub(1);
        }
      }
    }
    Key::Enter => {
      match app.view.source_device_focus {
        SourceFocus::StreamSource | SourceFocus::Source => select_source(app),
        SourceFocus::DownloadSource | SourceFocus::Devices => select_download_source(app),
      }
    }
    Key::Esc | Key::Char('q') => {
      app.pop_navigation_stack();
    }
    _ => {}
  }
}

/// Commit the highlighted source as the new active stream source and close the picker.
fn select_source(app: &mut App) {
  let source = Source::picker_sources()[app.view.source_list_index];
  if app.active_source != source
    || source == Source::AppleMusic
    || (source == Source::Spotify && !app.spotify_connected)
  {
    app.apply(Action::SelectSource(source));
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

/// Commit the highlighted download source as the active archiver target.
fn select_download_source(app: &mut App) {
  let dl = DownloadSource::ALL[app.view.download_source_index % DownloadSource::ALL.len()];
  app.active_download_source = dl;
  app.set_status_message(format!("Download quality source set to: {}", dl.label()), 4);
  app.pop_navigation_stack();
}
