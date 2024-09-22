use std::fs;

use headless_chrome::{Browser, LaunchOptions, Tab};
use serde::{Deserialize, Serialize};
use urlencoding;

use clap::{Parser, Subcommand, ValueEnum};
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
    #[command(subcommand)]
    from: InputSource,

    /// Output JSON file to save the parsed playlist
    #[arg(short, long)]
    save_to: Option<PathBuf>,

    /// Target platform to convert the playlist links (e.g., youtube, spotify, apple)
    #[arg(short, long)]
    to: Option<Platform>,

    /// Flag to open change browser headless mode
    #[arg(short, long, default_value_t = false)]
    show_browser: bool,
}

#[derive(Subcommand, Debug)]
enum InputSource {
    /// Link to the input music playlist
    Link {
        #[arg(value_parser)]
        url: Url,
    },
    /// JSON file containing the input music playlist
    Json {
        #[arg(value_parser)]
        file: PathBuf,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct Track {
    name: String,
    artist: String,
    album: Option<String>,
}
///
/// Enum representing the music platforms
#[derive(Debug, Clone, PartialEq, ValueEnum)]
enum Platform {
    YouTube,
    AppleMusic,
    Spotify,
    Unknown,
}

impl Platform {
    fn from_url(url: &Url) -> Self {
        let host = url.host_str().unwrap_or_default();
        match host {
            "music.youtube.com" => Platform::YouTube,
            "music.apple.com" | "itunes.apple.com" => Platform::AppleMusic,
            "open.spotify.com" | "spotify.com" => Platform::Spotify,
            _ => Platform::Unknown,
        }
    }
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    let options = LaunchOptions::default_builder()
        .headless(args.show_browser)
        .build()
        .unwrap();
    let browser = Browser::new(options).unwrap();

    // Parse playlist
    let playlist = match args.from {
        InputSource::Link { url } => match Platform::from_url(&url) {
            Platform::YouTube => fetch_yt_playlist(&browser, &url),
            Platform::AppleMusic => todo!(),
            Platform::Spotify => todo!(),
            Platform::Unknown => todo!(),
        },
        InputSource::Json { file } => todo!(),
    }
    .expect("Failed to scrape playlist");

    // Save playlist if needed
    if let Some(path) = args.save_to {
        let playlist = serde_json::to_string(&playlist).unwrap();
        fs::write(path, playlist).unwrap();
    }

    // Convert to another platform links
    let links = match args.to {
        Some(platform) => match platform {
            Platform::YouTube => todo!(),
            Platform::AppleMusic => find_apple_links(&browser, &playlist),
            Platform::Spotify => todo!(),
            Platform::Unknown => todo!(),
        },

        None => todo!(),
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
