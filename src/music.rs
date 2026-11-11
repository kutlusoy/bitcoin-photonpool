//! Optional background music of the web pages: audio files in a folder, served with Range support, plus a playlist.
//! The playlist is `playlist.json` of that folder (title, artist and the credit that licenses such as Uppbeat require), or, without
//! it, a list built from the audio files found. Only plain file names with a known audio extension are ever offered.

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tower_http::services::ServeDir;

const EXTENSIONS: [&str; 6] = ["mp3", "ogg", "opus", "m4a", "wav", "flac"];

pub fn dir(music_dir: &str, ui_dir: &str) -> PathBuf {
    if music_dir.is_empty() { Path::new(ui_dir).join("music") } else { PathBuf::from(music_dir) }
}

/// The audio files of the folder (with Range support for seeking), nothing else: other files in the folder, such as
/// the playlist source or notes, are not served by name.
pub fn files(dir: PathBuf) -> Router {
    Router::new().fallback_service(ServeDir::new(dir)).layer(middleware::from_fn(|req: Request, next: Next| async move {
        let name = req.uri().path().trim_start_matches('/').to_ascii_lowercase();
        if plain_audio_name(&name) && !name.contains("%2f") && !name.contains("%5c") {
            next.run(req).await
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }))
}

fn plain_audio_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.contains(['/', '\\', '\0'])
        && !name.starts_with('.')
        && Path::new(name).extension().and_then(|e| e.to_str()).map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

fn text(v: &Value, key: &str, max: usize) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.chars().take(max).collect::<String>()).filter(|s| !s.is_empty())
}

/// Playlist for the pages: `{"tracks": [{"file", "title", "artist", "credit", "url", "license"}]}`.
pub fn playlist(dir: &Path) -> Value {
    let mut tracks: Vec<Value> = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(dir.join("playlist.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            for t in v.get("tracks").and_then(|t| t.as_array()).into_iter().flatten() {
                let Some(file) = text(t, "file", 200) else { continue };
                if !plain_audio_name(&file) || !dir.join(&file).is_file() {
                    continue;
                }
                let mut o = json!({"file": file, "title": text(t, "title", 120).unwrap_or_else(|| stem(&file))});
                for k in ["artist", "credit", "license"] {
                    if let Some(s) = text(t, k, 400) {
                        o[k] = json!(s);
                    }
                }
                if let Some(u) = text(t, "url", 300).filter(|u| u.starts_with("https://") && !u.contains(['"', '<', '>', ' '])) {
                    o["url"] = json!(u);
                }
                tracks.push(o);
            }
            return json!({"tracks": tracks});
        }
    }
    let mut names: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| plain_audio_name(n)).collect();
    names.sort();
    json!({"tracks": names.iter().map(|n| json!({"file": n, "title": stem(n)})).collect::<Vec<_>>()})
}

fn stem(file: &str) -> String {
    Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file).replace(['_', '-'], " ")
}

pub async fn playlist_response(dir: PathBuf) -> Response {
    match tokio::task::spawn_blocking(move || playlist(&dir)).await {
        Ok(v) => ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")], v.to_string()).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("photond-music-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn without_a_playlist_the_audio_files_are_listed() {
        let d = tmp("auto");
        for f in ["b_song.mp3", "a-song.ogg", "notes.txt", ".hidden.mp3"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let v = playlist(&d);
        let files: Vec<&str> = v["tracks"].as_array().unwrap().iter().map(|t| t["file"].as_str().unwrap()).collect();
        assert_eq!(files, ["a-song.ogg", "b_song.mp3"]);
        assert_eq!(v["tracks"][0]["title"], "a song");
    }

    #[test]
    fn the_playlist_file_gives_titles_and_credits_and_only_offers_real_plain_files() {
        let d = tmp("list");
        std::fs::write(d.join("one.mp3"), b"x").unwrap();
        std::fs::write(
            d.join("playlist.json"),
            r#"{"tracks":[
              {"file":"one.mp3","title":"One","artist":"A","credit":"Music: One by A","url":"https://example.org/a","license":"L1"},
              {"file":"missing.mp3","title":"Gone"},
              {"file":"../etc/passwd.mp3","title":"Traversal"},
              {"file":"one.mp3","title":"Bad url","url":"javascript:alert(1)"}]}"#,
        )
        .unwrap();
        let v = playlist(&d);
        let t = v["tracks"].as_array().unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["credit"], "Music: One by A");
        assert_eq!(t[0]["url"], "https://example.org/a");
        assert!(t[1].get("url").is_none(), "only https links are passed on");
    }
}
