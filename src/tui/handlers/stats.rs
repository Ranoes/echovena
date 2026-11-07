use super::common_key_events;
use crate::core::action::Action;
use crate::core::app::{App, StatsColumn};
use crate::tui::event::Key;

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Esc => common_key_events::focus_sidebar(app),
    Key::Char('h') | Key::Left => match app.view.stats_column {
      StatsColumn::Albums => app.view.stats_column = StatsColumn::Artists,
      StatsColumn::Artists => app.view.stats_column = StatsColumn::Tracks,
      StatsColumn::Tracks => common_key_events::focus_sidebar(app),
    },
    Key::Char('l') | Key::Right => match app.view.stats_column {
      StatsColumn::Tracks => app.view.stats_column = StatsColumn::Artists,
      StatsColumn::Artists => app.view.stats_column = StatsColumn::Albums,
      StatsColumn::Albums => {}
    },
    Key::Tab => cycle_period(app, true),
    Key::BackTab => cycle_period(app, false),
    Key::Char('[') => cycle_period(app, false),
    Key::Char(']') => cycle_period(app, true),
    k if common_key_events::down_event(k, &app.user_config.keys) => {
      if let Some(stats) = &app.stats_data {
        match app.view.stats_column {
          StatsColumn::Tracks => {
            app.view.stats_selected_track = common_key_events::on_down_press_handler(
              &stats.top_tracks,
              Some(app.view.stats_selected_track),
            );
          }
          StatsColumn::Artists => {
            app.view.stats_selected_artist = common_key_events::on_down_press_handler(
              &stats.top_artists,
              Some(app.view.stats_selected_artist),
            );
          }
          StatsColumn::Albums => {
            app.view.stats_selected_album = common_key_events::on_down_press_handler(
              &stats.top_albums,
              Some(app.view.stats_selected_album),
            );
          }
        }
      }
    }
    k if common_key_events::up_event(k, &app.user_config.keys) => {
      if let Some(stats) = &app.stats_data {
        match app.view.stats_column {
          StatsColumn::Tracks => {
            app.view.stats_selected_track = common_key_events::on_up_press_handler(
              &stats.top_tracks,
              Some(app.view.stats_selected_track),
            );
          }
          StatsColumn::Artists => {
            app.view.stats_selected_artist = common_key_events::on_up_press_handler(
              &stats.top_artists,
              Some(app.view.stats_selected_artist),
            );
          }
          StatsColumn::Albums => {
            app.view.stats_selected_album = common_key_events::on_up_press_handler(
              &stats.top_albums,
              Some(app.view.stats_selected_album),
            );
          }
        }
      }
    }
    k if common_key_events::high_event(k) => {
      if app.stats_data.is_some() {
        match app.view.stats_column {
          StatsColumn::Tracks => {
            app.view.stats_selected_track = common_key_events::on_high_press_handler();
          }
          StatsColumn::Artists => {
            app.view.stats_selected_artist = common_key_events::on_high_press_handler();
          }
          StatsColumn::Albums => {
            app.view.stats_selected_album = common_key_events::on_high_press_handler();
          }
        }
      }
    }
    k if common_key_events::middle_event(k) => {
      if let Some(stats) = &app.stats_data {
        match app.view.stats_column {
          StatsColumn::Tracks => {
            app.view.stats_selected_track =
              common_key_events::on_middle_press_handler(&stats.top_tracks);
          }
          StatsColumn::Artists => {
            app.view.stats_selected_artist =
              common_key_events::on_middle_press_handler(&stats.top_artists);
          }
          StatsColumn::Albums => {
            app.view.stats_selected_album =
              common_key_events::on_middle_press_handler(&stats.top_albums);
          }
        }
      }
    }
    k if common_key_events::low_event(k) => {
      if let Some(stats) = &app.stats_data {
        match app.view.stats_column {
          StatsColumn::Tracks => {
            app.view.stats_selected_track =
              common_key_events::on_low_press_handler(&stats.top_tracks);
          }
          StatsColumn::Artists => {
            app.view.stats_selected_artist =
              common_key_events::on_low_press_handler(&stats.top_artists);
          }
          StatsColumn::Albums => {
            app.view.stats_selected_album =
              common_key_events::on_low_press_handler(&stats.top_albums);
          }
        }
      }
    }
    Key::Enter => {
      let uri = match app.view.stats_column {
        StatsColumn::Tracks => app.stats_data.as_ref().and_then(|stats| {
          stats
            .top_tracks
            .get(app.view.stats_selected_track)
            .and_then(|entry| entry.uri.clone())
        }),
        _ => None,
      };
      match uri {
        Some(uri) => {
          app.apply(Action::PlayUris {
            uris: vec![uri],
            offset: Some(0),
          });
        }
        None => {
          let has_tracks = app
            .stats_data
            .as_ref()
            .is_some_and(|stats| !stats.top_tracks.is_empty());
          if has_tracks {
            app.set_status_message(app.strings().status.app.entry_not_playable.to_string(), 4);
          }
        }
      }
    }
    _ => {}
  }
}

fn cycle_period(app: &mut App, forward: bool) {
  app.apply(Action::CycleStatsPeriod { forward });
  app.view.stats_selected_track = 0;
  app.view.stats_selected_artist = 0;
  app.view.stats_selected_album = 0;
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::user_config::UserConfig;
  use crate::infra::history::{RankedEntry, RecapPeriod, StatsData};
  use crate::infra::network::IoEvent;
  use std::sync::mpsc::{channel, Receiver};
  use std::time::SystemTime;

  fn app_with_track(uri: Option<String>) -> (App, Receiver<IoEvent>) {
    let (tx, rx) = channel();
    let mut app = App::new(tx, UserConfig::new(), Some(SystemTime::now()));
    app.stats_data = Some(StatsData {
      total_plays: 1,
      total_time_ms: 60_000,
      top_tracks: vec![RankedEntry {
        display: "Song - Artist".to_string(),
        detail: "1 plays · 1m".to_string(),
        value: 60_000,
        uri,
        parts: None,
      }],
      top_artists: vec![RankedEntry {
        display: "Artist Name".to_string(),
        detail: "1 plays · 1m".to_string(),
        value: 60_000,
        uri: None,
        parts: None,
      }],
      top_albums: vec![RankedEntry {
        display: "Album Name".to_string(),
        detail: "1 plays · 1m".to_string(),
        value: 60_000,
        uri: None,
        parts: None,
      }],
      days: vec![],
      period_plays: vec![],
      week_tracks: vec![],
      artist_ranks: vec![],
      movements: vec![],
    });
    (app, rx)
  }

  fn app_with_three_tracks() -> App {
    let (mut app, _rx) = app_with_track(None);
    let tracks = &mut app.stats_data.as_mut().expect("stats").top_tracks;
    let first = tracks[0].clone();
    tracks.push(first.clone());
    tracks.push(first);
    app
  }

  #[test]
  fn l_selects_the_last_top_track_and_h_the_first() {
    let mut app = app_with_three_tracks();
    handler(Key::Char('L'), &mut app);
    assert_eq!(app.view.stats_selected_track, 2);
    handler(Key::Char('H'), &mut app);
    assert_eq!(app.view.stats_selected_track, 0);
  }

  #[test]
  fn m_selects_the_middle_top_track() {
    let mut app = app_with_three_tracks();
    handler(Key::Char('M'), &mut app);
    assert_eq!(app.view.stats_selected_track, 1);
  }

  #[test]
  fn cycling_period_dispatches_stats_load() {
    let (mut app, rx) = app_with_track(None);
    handler(Key::Tab, &mut app);
    assert_eq!(app.stats_period, RecapPeriod::Month);
    assert!(app.stats_loading());
    assert!(app.stats_data.is_none());
    assert!(matches!(
      rx.try_recv(),
      Ok(IoEvent::LoadListeningStats(RecapPeriod::Month))
    ));
  }

  #[test]
  fn h_and_l_switch_columns_and_h_on_tracks_focuses_sidebar() {
    let (mut app, _rx) = app_with_track(None);
    assert_eq!(app.view.stats_column, StatsColumn::Tracks);
    handler(Key::Char('l'), &mut app);
    assert_eq!(app.view.stats_column, StatsColumn::Artists);
    handler(Key::Char('l'), &mut app);
    assert_eq!(app.view.stats_column, StatsColumn::Albums);
    handler(Key::Char('h'), &mut app);
    assert_eq!(app.view.stats_column, StatsColumn::Artists);
    handler(Key::Char('h'), &mut app);
    assert_eq!(app.view.stats_column, StatsColumn::Tracks);
    handler(Key::Char('h'), &mut app);
    assert_eq!(app.get_current_route().active_block, crate::core::app::ActiveBlock::Empty);
  }

  #[test]
  fn enter_plays_selected_spotify_track() {
    let (mut app, rx) = app_with_track(Some("spotify:track:abc".to_string()));
    handler(Key::Enter, &mut app);
    assert!(matches!(
      rx.try_recv(),
      Ok(IoEvent::StartPlayback(None, Some(uris), Some(0))) if uris == vec!["spotify:track:abc".to_string()]
    ));
  }

  #[test]
  fn enter_on_unplayable_entry_shows_status_message() {
    let (mut app, rx) = app_with_track(None);
    handler(Key::Enter, &mut app);
    assert!(rx.try_recv().is_err());
    assert!(app.status_message().is_some());
  }

  #[test]
  fn right_from_sidebar_refocuses_stats_content() {
    use crate::core::app::{ActiveBlock, RouteId};

    let (mut app, _rx) = app_with_track(None);
    app.push_navigation_stack(RouteId::Stats, ActiveBlock::Stats);
    // Left puts focus back on the sidebar; right must return it to the screen.
    common_key_events::focus_sidebar(&mut app);
    common_key_events::focus_content(&mut app);
    assert_eq!(app.get_current_route().active_block, ActiveBlock::Stats);
  }
}
