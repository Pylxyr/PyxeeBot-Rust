use serde::Deserialize;

const API_BASE: &str = "https://api.lyrics.ovh";

#[derive(Clone)]
pub struct LyricsClient {
    http_client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct LyricsResponse {
    lyrics: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SuggestResponse {
    #[serde(default)]
    data: Vec<SuggestTrack>,
}

#[derive(Debug, Deserialize)]
struct SuggestTrack {
    title: String,
    artist: SuggestArtist,
}

#[derive(Debug, Deserialize)]
struct SuggestArtist {
    name: String,
}

impl LyricsClient {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self { http_client }
    }

    async fn suggest(&self, query: &str) -> anyhow::Result<Option<(String, String)>> {
        let url = format!("{API_BASE}/suggest/{}", encode_path_segment(query));
        let response: SuggestResponse = self
            .http_client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response
            .data
            .into_iter()
            .next()
            .map(|t| (t.artist.name, t.title)))
    }

    async fn fetch(&self, artist: &str, title: &str) -> anyhow::Result<Option<String>> {
        let url = format!(
            "{API_BASE}/v1/{}/{}",
            encode_path_segment(artist),
            encode_path_segment(title)
        );
        let resp = self.http_client.get(url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body: LyricsResponse = resp.error_for_status()?.json().await?;
        Ok(body.lyrics)
    }

    pub async fn get_lyrics(
        &self,
        query: &str,
    ) -> anyhow::Result<Option<(String, String, String)>> {
        let Some((artist, title)) = self.suggest(query).await? else {
            return Ok(None);
        };
        let Some(lyrics) = self.fetch(&artist, &title).await? else {
            return Ok(None);
        };
        Ok(Some((artist, title, lyrics)))
    }
}

fn encode_path_segment(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            // write! into the existing buffer instead of format!()ing a
            // throwaway String per escaped byte.
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}
