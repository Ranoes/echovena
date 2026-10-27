pub mod client;
pub mod debouncer;
pub mod types;

pub use client::InnerTubeClient;
pub use debouncer::{DebouncedSearcher, DEFAULT_DEBOUNCE_MS};
pub use types::{SearchBadge, SearchResponse, SearchResultItem, SearchScope};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_search_scope_cycle() {
        let s = SearchScope::All;
        assert_eq!(s.cycle_next(), SearchScope::Songs);
        assert_eq!(s.cycle_next().cycle_next(), SearchScope::Albums);
        assert_eq!(s.cycle_next().cycle_next().cycle_next(), SearchScope::Playlists);
        assert_eq!(s.cycle_next().cycle_next().cycle_next().cycle_next(), SearchScope::All);

        assert_eq!(s.cycle_prev(), SearchScope::Playlists);
        assert_eq!(s.cycle_prev().cycle_prev(), SearchScope::Albums);
    }

    #[test]
    fn test_search_scope_params() {
        assert_eq!(SearchScope::All.innertube_params(), None);
        assert!(SearchScope::Songs.innertube_params().is_some());
        assert!(SearchScope::Albums.innertube_params().is_some());
        assert!(SearchScope::Playlists.innertube_params().is_some());
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

        let results = client::parse_ytm_search_response(&mock_json, SearchScope::All);
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

        let results = client::parse_yt_web_search_response(&mock_json, SearchScope::Songs);
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
        debouncer.on_query_change("a", SearchScope::All);
        tokio::time::sleep(Duration::from_millis(10)).await;
        debouncer.on_query_change("ab", SearchScope::All);
        tokio::time::sleep(Duration::from_millis(10)).await;
        debouncer.on_query_change("abc", SearchScope::All);

        // Clear immediately
        debouncer.clear();

        // Ensure nothing was dispatched
        let timeout_res = tokio::time::timeout(Duration::from_millis(80), rx.recv()).await;
        assert!(timeout_res.is_err(), "Should have received nothing because it was cleared before debounce finished");
    }
}
