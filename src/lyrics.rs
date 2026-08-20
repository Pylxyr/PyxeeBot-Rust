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

// Strips bracketed/parenthesized clutter (both ASCII and full-width CJK
// brackets) from a raw video title before it's used as a lyrics search
// query — e.g. "【MV】 MYTH&ROID - STYX HELIX(OFFICIAL)" becomes
// "MYTH&ROID - STYX HELIX". Deliberately doesn't touch a bare " - "
// separator: on YouTube that's overwhelmingly "Artist - Title", and
// leaving it in place means the cleaned query still carries the artist
// name for catalog searches that need it to disambiguate.
pub fn clean_query(title: &str) -> String {
    let mut cleaned = String::with_capacity(title.len());
    let mut depth: i32 = 0;
    for ch in title.chars() {
        match ch {
            '(' | '[' | '{' | '【' | '「' | '『' | '〈' | '《' => depth += 1,
            ')' | ']' | '}' | '】' | '」' | '』' | '〉' | '》' => depth = (depth - 1).max(0),
            _ if depth == 0 => cleaned.push(ch),
            _ => {}
        }
    }
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::clean_query;

    #[test]
    fn strips_bracketed_clutter_but_keeps_the_artist_separator() {
        assert_eq!(
            clean_query("【MV】 MYTH&ROID - STYX HELIX(OFFICIAL)"),
            "MYTH&ROID - STYX HELIX"
        );
    }

    #[test]
    fn leaves_a_clean_title_unchanged() {
        assert_eq!(clean_query("Coldplay - Yellow"), "Coldplay - Yellow");
    }

    #[test]
    fn handles_multiple_and_nested_bracket_styles() {
        assert_eq!(
            clean_query("Artist - Title [Official Video] (4K) {Remastered}"),
            "Artist - Title"
        );
    }

    #[test]
    fn collapses_leftover_whitespace() {
        assert_eq!(clean_query("  Artist   -   Title  "), "Artist - Title");
    }
}
