use std::fs;

use headless_chrome::{Browser, LaunchOptions, Tab};
use serde::{Deserialize, Serialize};
use urlencoding;

#[derive(Debug, Deserialize, Serialize)]
struct Track {
    name: String,
    artist: String,
    album: Option<String>,
}

fn main() {
    env_logger::init();
    let options = LaunchOptions::default_builder()
        .headless(false)
        .build()
        .unwrap();
    let browser = Browser::new(options).unwrap();

    // let playlist = fetch_yt_playlist(
    //     &browser,
    //     "https://music.youtube.com/playlist?list=PLvS-PBrFjqzcG6NxKgyTcKnkUbbtIuMIs",
    // )
    // .unwrap();
    // let playlist = serde_json::to_string(&playlist).unwrap();
    // fs::write("playlist.json", playlist).unwrap();

    let playlist = fs::read_to_string("playlist.json").unwrap();
    let playlist: Vec<Track> = serde_json::from_str(&playlist).unwrap();
    let urls = find_apple_links(&browser, &playlist).unwrap();
    let urls = serde_json::to_string(&urls).unwrap();
    fs::write("apple.json", urls).unwrap();
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
        let tab = browser.new_tab()?;
        let query = format!("{} - {}", &track.name, &track.artist);
        let url = format!(
            "https://music.apple.com/us/search?term={}",
            urlencoding::encode(&query)
        );
        tab.navigate_to(&url)?;
        if let Ok(url) = try_find_apple_song_link(&tab, track) {
            log::info!("Song: {:#?}", url);
            result.push(url)
        } else {
            log::warn!("Url not found for {}", track.name);
        }
        let _ = tab.close(false);
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
        .ok_or_else(|| anyhow::anyhow!("Song not found"))
}
