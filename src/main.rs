use std::{fs, time::Duration};

use anyhow::anyhow;
use headless_chrome::{Browser, LaunchOptions, Tab};
use serde::{Deserialize, Serialize};
use urlencoding;

use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use url::Url;

/// CLI for converting music playlists between platforms
#[derive(Parser, Debug)]
#[command(
    author = "suzumenobu",
    version = "1.0",
    about = "Converts music playlists between platforms and saves them to a JSON file"
)]
struct Args {
    /// Input source (either a URL or a JSON file)
    #[arg(short, long)]
    source: String,

    /// Output JSON file to save the parsed playlist
    #[arg(long)]
    save: Option<PathBuf>,

    /// Target platform to convert the playlist links (e.g., youtube, spotify, apple)
    #[arg(short, long)]
    dist: Option<Platform>,

    /// Flag to open change browser headless mode
    #[arg(long, default_value_t = false)]
    show_browser: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Track {
    name: String,
    artist: String,
    album: Option<String>,
}
///
/// Enum representing the music platforms
#[derive(Debug, Clone, PartialEq, ValueEnum)]
enum Platform {
    Youtube,
    Apple,
    Spotify,
    Unknown,
}

impl Platform {
    fn from_url(url: &Url) -> Self {
        let host = url.host_str().unwrap_or_default();
        match host {
            "music.youtube.com" => Platform::Youtube,
            "music.apple.com" | "itunes.apple.com" => Platform::Apple,
            "open.spotify.com" | "spotify.com" => Platform::Spotify,
            _ => Platform::Unknown,
        }
    }
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    let options = LaunchOptions::default_builder()
        .headless(!args.show_browser)
        .idle_browser_timeout(Duration::from_secs(1000000))
        .build()
        .unwrap();
    let browser = Browser::new(options).unwrap();

    // Parse playlist
    let playlist = match Url::try_from(args.source.as_str()) {
        Ok(url) => match Platform::from_url(&url) {
            Platform::Youtube => fetch_yt_playlist(&browser, &url),
            Platform::Apple => todo!(),
            Platform::Spotify => fetch_spotify_playlist(&browser, &url),
            Platform::Unknown => todo!(),
        },
        Err(_) => todo!(),
    }
    .expect("Failed to scrape playlist");

    // Save playlist if needed
    if let Some(path) = args.save {
        let playlist = serde_json::to_string(&playlist).unwrap();
        fs::write(path, playlist).unwrap();
    }

    // Convert to another platform links
    let links = match args.dist {
        Some(platform) => match platform {
            Platform::Youtube => todo!(),
            Platform::Apple => find_apple_links(&browser, &playlist),
            Platform::Spotify => todo!(),
            Platform::Unknown => todo!(),
        },
        None => Ok(vec![]),
    }
    .expect("Failed to convert playlist");

    for link in links {
        println!("{link}")
    }
}

fn fetch_yt_playlist(
    browser: &Browser,
    yt_playlist_url: impl AsRef<str>,
) -> anyhow::Result<Vec<Track>> {
    let tab = browser.new_tab()?;
    tab.navigate_to(yt_playlist_url.as_ref())?;
    let tracks = tab
        .wait_for_elements("ytmusic-responsive-list-item-renderer")?
        .into_iter()
        .filter_map(|el| el.find_elements("yt-formatted-string").ok())
        .map(|strings| {
            strings
                .into_iter()
                .filter_map(|el| el.get_inner_text().ok())
                .collect::<Vec<_>>()
        })
        .map(|track_info| match track_info.as_slice() {
            [name, artist, album, _duration, _empty] => Track {
                name: name.to_string(),
                artist: artist.to_string(),
                album: Some(album.to_string()).filter(|s| !s.is_empty()),
            },
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    log::info!("Got tracks: {tracks:?}");
    Ok(tracks)
}

fn fetch_spotify_playlist(
    browser: &Browser,
    playlist_url: impl AsRef<str>,
) -> anyhow::Result<Vec<Track>> {
    log::info!("Starting scraping spotify playlist");
    let tab = browser.new_tab()?;
    tab.navigate_to(playlist_url.as_ref())?;
    tab.wait_until_navigated()?;

    let mut tracks = vec![];

    loop {
        let buf = tab
            .wait_for_elements(r#"div[data-testid="playlist-tracklist"]>div>div>div:has(a[data-testid="internal-track-link"] > div)"#)
            .map(|els| els
            .into_iter()
            .skip(tracks.len())
            .filter_map(|el| {
                if let Err(e) = el.scroll_into_view() {
                    log::warn!("Failed to scroll to element: {e:?}");
                }
                let name = el.find_element("a>div").and_then(|el| el.get_inner_text());
                let artist = el
                    .find_element("span>div")
                    .and_then(|el| el.get_inner_text());

                log::info!("Name: {name:?}, artist: {artist:?}");
                match (name, artist) {
                    (Ok(name), Ok(artist)) => Some(Track {
                        name,
                        artist,
                        album: None,
                    }),
                    _ => {
                        log::warn!("Failed to parse track");
                        None
                    },
                }
            })
            .collect::<Vec<_>>());

        let mut tracks_added = 0;

        match buf {
            Err(e) => {
                log::error!("Failed to collect buffer of tracks: {e:?}");
                continue;
            }
            Ok(buf) => {
                for track in buf {
                    if tracks.contains(&track) {
                        continue;
                    }
                    tracks.push(track);
                    tracks_added += 1;
                }
            }
        }

        log::info!("Added {tracks_added} new tracks");

        if tracks_added == 0 {
            break;
        }
    }

    log::info!("Finished with {} tracks", tracks.len());
    Ok(tracks)
}

fn find_apple_links<'a>(
    browser: &Browser,
    tracks: impl IntoIterator<Item = &'a Track>,
) -> anyhow::Result<Vec<String>> {
    let mut result = vec![];

    for track in tracks {
        log::info!("Creating new tab");
        let tab = browser.new_tab()?;

        let query = format!("{} - {}", &track.name, &track.artist);
        let url = format!(
            "https://music.apple.com/us/search?term={}",
            urlencoding::encode(&query)
        );

        log::info!("Opening url={url}");
        tab.navigate_to(&url)?;

        if let Ok(url) = try_find_apple_song_link(&tab, track) {
            log::info!("Song: {:#?}", url);
            result.push(url)
        } else {
            log::warn!("Url not found for {}", track.name);
        }

        if let Err(e) = tab.close(true) {
            log::error!("Failed to close tab with {e:?}")
        }
    }

    Ok(result)
}

fn try_find_apple_song_link(tab: &Tab, track: &Track) -> anyhow::Result<String> {
    tab.wait_for_element(r#"div[aria-label="Songs"]"#)?
        .wait_for_elements("li")?
        .into_iter()
        .filter_map(|el| el.find_element("a").ok())
        .filter(|el| {
            matches!(
                el.get_inner_text()
                    .map(|title| title.to_lowercase() == track.name.to_lowercase()),
                Ok(true)
            )
        })
        .filter_map(|el| el.get_attribute_value("href").ok())
        .next()
        .flatten()
        .map(|href| {
            urlencoding::decode(&href)
                .ok()
                .map(|href| href.into_owned())
        })
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("Song not found"))
}

#[warn(dead_code)]
fn get_body_scroll_height(tab: &Tab) -> anyhow::Result<u64> {
    tab.evaluate("document.body.scrollHeight", true)
        .ok()
        .map(|obj| match obj.value {
            Some(serde_json::Value::Number(height)) => height.as_u64(),
            unknown => panic!("Unknown height type: {unknown:?}"),
        })
        .flatten()
        .ok_or(anyhow!("Failed to get height"))
}
