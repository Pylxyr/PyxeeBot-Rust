use serde::Deserialize;

const API_BASE: &str = "https://ws.audioscrobbler.com/2.0/";

pub struct LastFmClient {
    api_key: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct SimilarArtistsResponse {
    similarartists: SimilarArtists,
}

#[derive(Debug, Deserialize)]
struct SimilarArtists {
    #[serde(default)]
    artist: Vec<ArtistEntry>,
}

#[derive(Debug, Deserialize)]
struct ArtistEntry {
    name: String,
}

impl LastFmClient {
    pub fn new(api_key: String, http_client: reqwest::Client) -> Self {
        Self {
            api_key,
            http_client,
        }
    }

    /// Returns similar artist names for a seed artist, most similar first.
    pub async fn similar_artists(&self, artist: &str, limit: usize) -> anyhow::Result<Vec<String>> {
        let limit_str = limit.to_string();
        let response = self
            .http_client
            .get(API_BASE)
            .query(&[
                ("method", "artist.getsimilar"),
                ("artist", artist),
                ("api_key", self.api_key.as_str()),
                ("format", "json"),
                ("autocorrect", "1"),
                ("limit", limit_str.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<SimilarArtistsResponse>()
            .await?;
        Ok(response
            .similarartists
            .artist
            .into_iter()
            .map(|a| a.name)
            .collect())
    }
}
