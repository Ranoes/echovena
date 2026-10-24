use crate::core::paths::streaming_cache_dir;
use crate::sources::lossless_relay::qobuz_client::LosslessStreamInfo;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Sanitizes strings for safe cross-platform file and directory names
pub fn sanitize_filename(name: &str) -> String {
    let invalid = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let s: String = name
        .chars()
        .map(|c| if invalid.contains(&c) { '_' } else { c })
        .collect();
    let trimmed = s.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed
    }
}

/// Extracted FLAC STREAMINFO metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlacStreamInfo {
    pub sample_rate: u32,
    pub channels: u8,
    pub bit_depth: u8,
    pub total_samples: u64,
    pub bitrate_kbps: u32,
}

impl FlacStreamInfo {
    /// Format string, e.g. "FLAC 24-bit / 96.0 kHz | 2304 kbps"
    pub fn telemetry_display(&self) -> String {
        format!(
            "FLAC {}-bit / {:.1} kHz | {} kbps",
            self.bit_depth,
            self.sample_rate as f64 / 1000.0,
            self.bitrate_kbps
        )
    }

    /// Converts to standard LosslessStreamInfo
    pub fn to_lossless_stream_info(&self, provider_id: &str, stream_url: &str) -> LosslessStreamInfo {
        let format_label = if self.bit_depth > 16 || self.sample_rate > 44100 {
            "FLAC_HIRES".to_string()
        } else {
            "FLAC_CD".to_string()
        };
        let duration_secs = if self.sample_rate > 0 {
            Some(self.total_samples as f64 / self.sample_rate as f64)
        } else {
            None
        };

        LosslessStreamInfo {
            provider_id: provider_id.to_string(),
            stream_url: stream_url.to_string(),
            format: format_label,
            bit_depth: Some(self.bit_depth as u32),
            sample_rate: Some(self.sample_rate as f64 / 1000.0),
            bitrate_kbps: Some(self.bitrate_kbps),
            duration_secs,
        }
    }
}

/// Parses FLAC STREAMINFO block directly from binary header bytes
pub fn parse_flac_streaminfo(bytes: &[u8]) -> Option<FlacStreamInfo> {
    if bytes.len() < 42 {
        return None;
    }
    // Verify "fLaC" magic bytes
    if &bytes[0..4] != b"fLaC" {
        return None;
    }
    // Block type in byte 4 (bits 1..7) must be 0 (STREAMINFO)
    let block_type = bytes[4] & 0x7F;
    if block_type != 0 {
        return None;
    }
    // Block length (bytes 5..8, 24 bits) must be at least 34
    let length = ((bytes[5] as usize) << 16) | ((bytes[6] as usize) << 8) | (bytes[7] as usize);
    if length < 34 || bytes.len() < 8 + 34 {
        return None;
    }

    let b = &bytes[8..8 + 34];
    // b[10..14]: sample_rate (20 bits), channels (3 bits), bit_depth (5 bits)
    let sample_rate = ((b[10] as u32) << 12)
        | ((b[11] as u32) << 4)
        | (((b[12] as u32) >> 4) & 0x0F);
    let channels = (((b[12] >> 1) & 0x07) + 1) as u8;
    let bit_depth = ((((b[12] & 0x01) << 4) | ((b[13] >> 4) & 0x0F)) + 1) as u8;

    let total_samples = (((b[13] as u64) & 0x0F) << 32)
        | ((b[14] as u64) << 24)
        | ((b[15] as u64) << 16)
        | ((b[16] as u64) << 8)
        | (b[17] as u64);

    // Calculate typical FLAC compressed bitrate (roughly 50% of raw PCM)
    let uncompressed_kbps = (sample_rate as u64 * bit_depth as u64 * channels as u64) / 1000;
    let bitrate_kbps = (uncompressed_kbps / 2) as u32;

    Some(FlacStreamInfo {
        sample_rate,
        channels,
        bit_depth,
        total_samples,
        bitrate_kbps,
    })
}

/// Disk-backed ring buffer for streaming lossless audio chunks
pub struct RingBuffer {
    cache_dir: PathBuf,
    max_slots: usize,
}

impl RingBuffer {
    pub fn new(cache_dir: Option<PathBuf>, max_slots: usize) -> Result<Self> {
        let dir = match cache_dir {
            Some(d) => d,
            None => streaming_cache_dir().context("unable to determine streaming_cache directory")?,
        };
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed creating cache dir: {}", dir.display()))?;
        Ok(Self {
            cache_dir: dir,
            max_slots,
        })
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Evicts oldest cached files if count exceeds max_slots
    pub fn evict_if_needed(&self) -> Result<()> {
        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(&self.cache_dir) {
            for entry in read_dir.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let modified = meta
                            .modified()
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        entries.push((entry.path(), modified));
                    }
                }
            }
        }

        if entries.len() >= self.max_slots {
            // Sort by modified time ascending (oldest first)
            entries.sort_by_key(|e| e.1);
            let to_remove = entries.len() - self.max_slots + 1;
            for (path, _) in entries.into_iter().take(to_remove) {
                fs::remove_file(&path).ok();
            }
        }

        Ok(())
    }

    /// Allocates a disk slot for the track, rotating the ring buffer if full
    pub fn allocate_slot(&self, track_id: &str) -> Result<PathBuf> {
        self.evict_if_needed()?;
        let safe_name = sanitize_filename(track_id);
        let target_path = self.cache_dir.join(format!("{}.flac", safe_name));
        Ok(target_path)
    }

    /// Streams chunked audio from an HTTP URL into disk cache, parsing STREAMINFO on first chunk
    pub async fn stream_url_to_file(
        &self,
        client: &reqwest::Client,
        stream_url: &str,
        track_id: &str,
    ) -> Result<(PathBuf, Option<FlacStreamInfo>)> {
        use tokio::io::AsyncWriteExt;

        let target_path = self.allocate_slot(track_id)?;
        let mut response = client
            .get(stream_url)
            .send()
            .await
            .with_context(|| format!("failed to request audio stream from {}", stream_url))?;

        if !response.status().is_success() {
            return Err(anyhow!("stream request returned HTTP {}", response.status()));
        }

        let mut file = tokio::fs::File::create(&target_path)
            .await
            .with_context(|| format!("failed to create cache file {}", target_path.display()))?;

        let mut header_buf = Vec::new();
        let mut stream_info = None;

        while let Some(chunk_res) = response.chunk().await? {
            if stream_info.is_none() {
                header_buf.extend_from_slice(&chunk_res);
                if header_buf.len() >= 42 {
                    stream_info = parse_flac_streaminfo(&header_buf);
                }
            }
            file.write_all(&chunk_res).await?;
        }

        file.flush().await?;
        Ok((target_path, stream_info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructs a valid 42-byte FLAC STREAMINFO header for testing
    fn build_test_flac_header(sample_rate: u32, channels: u8, bit_depth: u8) -> Vec<u8> {
        let mut header = Vec::with_capacity(42);
        // "fLaC"
        header.extend_from_slice(b"fLaC");
        // Block type 0 (STREAMINFO) + length 34
        header.push(0x00);
        header.push(0x00);
        header.push(0x00);
        header.push(0x22);

        // 34 bytes payload
        let mut payload = vec![0u8; 34];
        // min/max block size = 4096 (0x1000)
        payload[0] = 0x10;
        payload[1] = 0x00;
        payload[2] = 0x10;
        payload[3] = 0x00;

        // sample_rate (20 bits), channels - 1 (3 bits), bit_depth - 1 (5 bits)
        let ch_val = (channels - 1) & 0x07;
        let bd_val = (bit_depth - 1) & 0x1F;

        payload[10] = ((sample_rate >> 12) & 0xFF) as u8;
        payload[11] = ((sample_rate >> 4) & 0xFF) as u8;
        payload[12] = (((sample_rate & 0x0F) << 4) | ((ch_val as u32) << 1) | ((bd_val as u32) >> 4)) as u8;
        payload[13] = (((bd_val & 0x0F) << 4) | 0x00) as u8; // total samples high 4 bits = 0
        // total samples remaining 32 bits: 96000 * 180 = 17,280,000 samples
        let total_samples: u64 = (sample_rate as u64) * 180;
        payload[14] = ((total_samples >> 24) & 0xFF) as u8;
        payload[15] = ((total_samples >> 16) & 0xFF) as u8;
        payload[16] = ((total_samples >> 8) & 0xFF) as u8;
        payload[17] = (total_samples & 0xFF) as u8;

        header.extend_from_slice(&payload);
        header
    }

    #[test]
    fn test_flac_streaminfo_parser_hires() {
        let header = build_test_flac_header(96000, 2, 24);
        let info = parse_flac_streaminfo(&header).expect("failed parsing hires FLAC header");
        assert_eq!(info.sample_rate, 96000);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bit_depth, 24);
        assert_eq!(info.bitrate_kbps, 2304); // 96000 * 24 * 2 / 2000
        assert_eq!(info.telemetry_display(), "FLAC 24-bit / 96.0 kHz | 2304 kbps");

        let stream_info = info.to_lossless_stream_info("qobuz-primary", "http://test");
        assert_eq!(stream_info.format, "FLAC_HIRES");
        assert_eq!(stream_info.sample_rate, Some(96.0));
        assert_eq!(stream_info.bit_depth, Some(24));
    }

    #[test]
    fn test_flac_streaminfo_parser_cd() {
        let header = build_test_flac_header(44100, 2, 16);
        let info = parse_flac_streaminfo(&header).expect("failed parsing CD FLAC header");
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(info.bitrate_kbps, 705); // 44100 * 16 * 2 / 2000
        assert_eq!(info.telemetry_display(), "FLAC 16-bit / 44.1 kHz | 705 kbps");

        let stream_info = info.to_lossless_stream_info("tidal-hifi-primary", "http://test");
        assert_eq!(stream_info.format, "FLAC_CD");
        assert_eq!(stream_info.sample_rate, Some(44.1));
        assert_eq!(stream_info.bit_depth, Some(16));
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ring = RingBuffer::new(Some(temp_dir.path().to_path_buf()), 3).unwrap();

        // Create 3 files
        let f1 = ring.allocate_slot("track_1").unwrap();
        fs::write(&f1, b"track 1 data").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let f2 = ring.allocate_slot("track_2").unwrap();
        fs::write(&f2, b"track 2 data").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let f3 = ring.allocate_slot("track_3").unwrap();
        fs::write(&f3, b"track 3 data").unwrap();

        assert_eq!(fs::read_dir(temp_dir.path()).unwrap().count(), 3);

        // Allocating a 4th slot should evict the oldest (f1)
        let f4 = ring.allocate_slot("track_4").unwrap();
        fs::write(&f4, b"track 4 data").unwrap();

        assert!(!f1.exists(), "oldest file f1 should be evicted");
        assert!(f2.exists());
        assert!(f3.exists());
        assert!(f4.exists());
    }
}
