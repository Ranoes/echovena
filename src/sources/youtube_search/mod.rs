pub mod client;
pub mod debouncer;
pub mod types;

pub use client::InnerTubeClient;
pub use debouncer::{
    clear_query, init_live_search, trigger_query, DebouncedSearcher, DEFAULT_DEBOUNCE_MS,
};
pub use types::{SearchBadge, SearchResponse, SearchResultItem, SearchScope};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_search_scope_cycle() {
        let s = SearchScope::Music;
        assert_eq!(s.cycle_next(), SearchScope::Album);
        assert_eq!(s.cycle_next().cycle_next(), SearchScope::Artist);
        assert_eq!(s.cycle_next().cycle_next().cycle_next(), SearchScope::Video);
        assert_eq!(s.cycle_next().cycle_next().cycle_next().cycle_next(), SearchScope::Music);


        assert_eq!(s.cycle_prev(), SearchScope::Video);
        assert_eq!(s.cycle_prev().cycle_prev(), SearchScope::Artist);
    }

    #[test]
    fn test_search_scope_params() {

        assert!(SearchScope::Music.innertube_params().is_some());
        assert!(SearchScope::Album.innertube_params().is_some());
        assert!(SearchScope::Artist.innertube_params().is_some());
        assert!(SearchScope::Video.innertube_params().is_some());
    }

    #[test]
    fn test_clean_title_and_artist() {
        let raw = "Queen - Bohemian Rhapsody (Official Video Remastered)";
        let (artist, title) = client::clean_title_and_artist(raw, None);
        assert_eq!(artist, "Queen");
        assert_eq!(title, "Bohemian Rhapsody");

        let raw2 = "Song Name [Explicit 4K]";
        let (artist2, title2) = client::clean_title_and_artist(raw2, Some("Kendrick Lamar - Topic"));
        assert_eq!(artist2, "Kendrick Lamar");
        assert_eq!(title2, "Song Name");
    }

    #[test]
    fn test_duration_parsing() {
        assert_eq!(client::parse_duration_str("3:45"), 225_000);
        assert_eq!(client::parse_duration_str("03:45"), 225_000);
        assert_eq!(client::parse_duration_str("1:00:00"), 3_600_000);
        assert_eq!(client::parse_duration_str("12"), 12_000);
        assert!(client::is_duration_text("3:45"));
        assert!(client::is_duration_text("1:02:30"));
        assert!(!client::is_duration_text("Song Title"));
    }

    #[test]
    fn test_parse_ytm_search_response() {
        let mock_json = json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicCardShelfRenderer": {
                                                "title": {
                                                    "runs": [{
                                                        "text": "Starboy",
                                                        "navigationEndpoint": {
                                                            "watchEndpoint": {
                                                                "videoId": "d38H_rJ0e7g"
                                                            }
                                                        }
                                                    }]
                                                },
                                                "subtitle": {
                                                    "runs": [
                                                        {"text": "Song"},
                                                        {"text": " • "},
                                                        {"text": "The Weeknd"},
                                                        {"text": " • "},
                                                        {"text": "Starboy"},
                                                        {"text": " • "},
                                                        {"text": "3:50"}
                                                    ]
                                                },
                                                "badges": [{
                                                    "musicInlineBadgeRenderer": {
                                                        "accessibilityData": {
                                                            "accessibilityData": {
                                                                "label": "Explicit"
                                                            }
                                                        }
                                                    }
                                                }]
                                            }
                                        },
                                        {
                                            "musicShelfRenderer": {
                                                "contents": [{
                                                    "musicResponsiveListItemRenderer": {
                                                        "playlistItemData": {
                                                            "videoId": "34Na4j8AVgA"
                                                        },
                                                        "flexColumns": [
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": {
                                                                        "runs": [{"text": "Blinding Lights [Official Music Video]"}]
                                                                    }
                                                                }
                                                            },
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": {
                                                                        "runs": [
                                                                            {"text": "The Weeknd"},
                                                                            {"text": " • "},
                                                                            {"text": "After Hours"},
                                                                            {"text": " • "},
                                                                            {"text": "3:20"}
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        ]
                                                    }
                                                }]
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }]
                }
            }
        });

        let results = client::parse_ytm_search_response(&mock_json, SearchScope::Music);
        assert_eq!(results.len(), 2);

        // Top Card
        assert_eq!(results[0].video_id.as_deref(), Some("d38H_rJ0e7g"));
        assert_eq!(results[0].title, "Starboy");
        assert_eq!(results[0].artist, "The Weeknd");
        assert_eq!(results[0].duration_ms, 230_000);
        assert!(results[0].badges.contains(&SearchBadge::Explicit));

        // Shelf Item
        assert_eq!(results[1].video_id.as_deref(), Some("34Na4j8AVgA"));
        assert_eq!(results[1].title, "Blinding Lights");
        assert_eq!(results[1].artist, "The Weeknd");
        assert_eq!(results[1].duration_ms, 200_000);
        assert!(results[1].badges.contains(&SearchBadge::Official));
    }

    #[test]
    fn test_parse_yt_web_search_response() {
        let mock_json = json!({
            "contents": {
                "twoColumnSearchResultsRenderer": {
                    "primaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "itemSectionRenderer": {
                                    "contents": [{
                                        "videoRenderer": {
                                            "videoId": "kJQP7kiw5Fk",
                                            "title": {
                                                "runs": [{"text": "Luis Fonsi - Despacito ft. Daddy Yankee"}]
                                            },
                                            "ownerText": {
                                                "runs": [{"text": "LuisFonsiVEVO"}]
                                            },
                                            "lengthText": {
                                                "simpleText": "4:42"
                                            },
                                            "badges": [{
                                                "metadataBadgeRenderer": {
                                                    "label": "Official"
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        });

        let results = client::parse_yt_web_search_response(&mock_json, SearchScope::Music);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].video_id.as_deref(), Some("kJQP7kiw5Fk"));
        assert_eq!(results[0].artist, "Luis Fonsi");
        assert_eq!(results[0].duration_ms, 282_000);
        assert!(results[0].badges.contains(&SearchBadge::Official));
    }

    #[test]
    fn test_parse_suggestqueries_response() {
        let mock_json = json!([
            "radiohead",
            ["radiohead creep", "radiohead karma police", "radiohead no surprises"]
        ]);

        let suggestions = client::parse_suggestqueries_response(&mock_json).unwrap();
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0], "radiohead creep");
        assert_eq!(suggestions[1], "radiohead karma police");
        assert_eq!(suggestions[2], "radiohead no surprises");
    }

    #[tokio::test]
    async fn test_debouncer_supersedes_rapid_queries() {
        let client = Arc::new(InnerTubeClient::new());
        let (debouncer, mut rx) = DebouncedSearcher::with_duration(client, Duration::from_millis(50));

        // Submit rapid queries
        debouncer.on_query_change("a", SearchScope::Music);
        tokio::time::sleep(Duration::from_millis(10)).await;
        debouncer.on_query_change("ab", SearchScope::Music);
        tokio::time::sleep(Duration::from_millis(10)).await;
        debouncer.on_query_change("abc", SearchScope::Music);

        // Clear immediately
        debouncer.clear();

        // Ensure nothing was dispatched
        let timeout_res = tokio::time::timeout(Duration::from_millis(80), rx.recv()).await;
        assert!(timeout_res.is_err(), "Should have received nothing because it was cleared before debounce finished");
    }

    #[test]
    fn test_app_live_search_navigation_and_scoping() {
        use crate::core::app::App;
        let mut app = App::default();

        // Initial scope
        assert_eq!(app.search_scope, SearchScope::Music);

        // Forward cycling
        app.cycle_search_scope();
        assert_eq!(app.search_scope, SearchScope::Album);
        app.cycle_search_scope();
        assert_eq!(app.search_scope, SearchScope::Artist);
        app.cycle_search_scope();
        assert_eq!(app.search_scope, SearchScope::Video);
        app.cycle_search_scope();
        assert_eq!(app.search_scope, SearchScope::Music);

        // Backward cycling
        app.cycle_search_scope_back();
        assert_eq!(app.search_scope, SearchScope::Video);
        app.cycle_search_scope_back();
        assert_eq!(app.search_scope, SearchScope::Artist);
        app.cycle_search_scope_back();
        assert_eq!(app.search_scope, SearchScope::Album);
        app.cycle_search_scope_back();
        assert_eq!(app.search_scope, SearchScope::Music);

        // Add mock items to live search
        app.live_search_items = vec![
            SearchResultItem {
                id: "yt:video:1".into(),
                video_id: Some("1".into()),
                playlist_id: None,
                title: "Track 1".into(),
                artist: "Artist 1".into(),
                album: None,
                duration_ms: 180_000,
                duration_str: "3:00".into(),
                badges: vec![SearchBadge::Official],
                scope: SearchScope::Music,
                thumbnail_url: None,
                is_playable: true,
            },
            SearchResultItem {
                id: "yt:video:2".into(),
                video_id: Some("2".into()),
                playlist_id: None,
                title: "Track 2".into(),
                artist: "Artist 2".into(),
                album: Some("Album 2".into()),
                duration_ms: 210_000,
                duration_str: "3:30".into(),
                badges: vec![SearchBadge::Explicit],
                scope: SearchScope::Music,
                thumbnail_url: None,
                is_playable: true,
            },
            SearchResultItem {
                id: "yt:video:3".into(),
                video_id: Some("3".into()),
                playlist_id: None,
                title: "Track 3".into(),
                artist: "Artist 3".into(),
                album: None,
                duration_ms: 240_000,
                duration_str: "4:00".into(),
                badges: vec![SearchBadge::Remaster],
                scope: SearchScope::Music,
                thumbnail_url: None,
                is_playable: true,
            },
        ];

        assert_eq!(app.live_search_selected_index, 0);
        assert_eq!(app.selected_live_search_item().unwrap().title, "Track 1");

        // Move down
        app.move_live_search_down();
        assert_eq!(app.live_search_selected_index, 1);
        assert_eq!(app.selected_live_search_item().unwrap().title, "Track 2");

        app.move_live_search_down();
        assert_eq!(app.live_search_selected_index, 2);

        // Wrap around down
        app.move_live_search_down();
        assert_eq!(app.live_search_selected_index, 0);

        // Move up wrap around
        app.move_live_search_up();
        assert_eq!(app.live_search_selected_index, 2);
        assert_eq!(app.selected_live_search_item().unwrap().title, "Track 3");

        app.move_live_search_up();
        assert_eq!(app.live_search_selected_index, 1);
    }

    #[test]
    fn test_app_live_search_response_and_clearing() {
        use crate::core::app::App;
        let mut app = App::default();

        // Simulate typing "bohemian" into input
        app.view.input = "bohemian".chars().collect();

        let response = SearchResponse {
            query: "bohemian".to_string(),
            scope: SearchScope::Music,
            items: vec![SearchResultItem {
                id: "yt:video:fJ9rUzIMcZQ".into(),
                video_id: Some("fJ9rUzIMcZQ".into()),
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
            }],
            suggestions: vec!["bohemian rhapsody".into(), "bohemian rhapsody queen".into()],
        };

        app.set_live_search_response(response);

        assert_eq!(app.live_search_items.len(), 1);
        assert_eq!(app.live_search_suggestions.len(), 2);
        assert_eq!(app.live_search_suggestions[0], "bohemian rhapsody");
        assert_eq!(app.selected_live_search_item().unwrap().title, "Bohemian Rhapsody");
        assert_eq!(app.selected_live_search_item().unwrap().badges.len(), 2);

        // Check compatibility with search_results.tracks
        let legacy_tracks = app.search_results().tracks.as_ref().unwrap();
        assert_eq!(legacy_tracks.items.len(), 1);
        assert_eq!(legacy_tracks.items[0].name, "Bohemian Rhapsody");
        assert_eq!(legacy_tracks.items[0].artists, vec!["Queen"]);
        assert_eq!(legacy_tracks.items[0].duration_ms, 354_000);

        // Clear live search
        app.clear_live_search();
        assert!(app.live_search_items.is_empty());
        assert!(app.live_search_suggestions.is_empty());
        assert_eq!(app.live_search_selected_index, 0);
    }

    #[test]
    fn test_play_and_queue_selected_live_search_item() {
        use crate::core::app::App;
        let mut app = App::default();

        app.live_search_items = vec![
            SearchResultItem {
                id: "yt:video:fJ9rUzIMcZQ".into(),
                video_id: Some("fJ9rUzIMcZQ".into()),
                playlist_id: None,
                title: "Bohemian Rhapsody".into(),
                artist: "Queen".into(),
                album: Some("A Night at the Opera".into()),
                duration_ms: 354_000,
                duration_str: "5:54".into(),
                badges: vec![SearchBadge::Official],
                scope: SearchScope::Music,
                thumbnail_url: None,
                is_playable: true,
            },
        ];
        app.live_search_selected_index = 0;

        // Test queueing on 'z'
        assert!(app.native_queue.is_empty());
        app.queue_selected_live_search_item();
        assert_eq!(app.native_queue.len(), 1);
        assert_eq!(app.native_queue[0].name, "Bohemian Rhapsody");
        assert_eq!(app.native_queue[0].uri.as_deref(), Some("youtube:fJ9rUzIMcZQ"));

        // Test instant playback on 'Enter'
        app.play_selected_live_search_item();
        assert!(app.status_message().is_some());
    }
}
