use serde::Deserialize;

const API_BASE: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Clone)]
pub struct LastFmClient {
    api_key: String,
    http_client: reqwest::Client,
}

// Last.fm signals API-level failures (e.g. "artist not found") with a 200 OK
// and an error-shaped body, not an HTTP error status — so success/error must
// be distinguished after fetching, not via error_for_status().
#[derive(Debug, Deserialize)]
struct LastFmErrorResponse {
    message: String,
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

#[derive(Debug, Deserialize)]
struct TrackSearchResponse {
    results: TrackSearchResults,
}

#[derive(Debug, Deserialize)]
struct TrackSearchResults {
    trackmatches: TrackMatches,
}

#[derive(Debug, Deserialize)]
struct TrackMatches {
    #[serde(default)]
    track: Vec<TrackMatch>,
}

#[derive(Debug, Deserialize)]
struct TrackMatch {
    name: String,
    artist: String,
}

#[derive(Debug, Deserialize)]
struct SimilarTracksResponse {
    similartracks: SimilarTracks,
}

#[derive(Debug, Deserialize)]
struct SimilarTracks {
    #[serde(default)]
    track: Vec<SimilarTrackEntry>,
}

#[derive(Debug, Deserialize)]
struct SimilarTrackEntry {
    name: String,
    artist: SimilarTrackArtist,
}

#[derive(Debug, Deserialize)]
struct SimilarTrackArtist {
    name: String,
}

impl LastFmClient {
    pub fn new(api_key: String, http_client: reqwest::Client) -> Self {
        Self {
            api_key,
            http_client,
        }
    }

    /// Fetches from the Last.fm API and deserializes into T. Distinguishes
    /// Last.fm's error-shaped body (see LastFmErrorResponse) from a genuine
    /// decode failure so the former surfaces Last.fm's own message.
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let mut query = params.to_vec();
        query.push(("api_key", self.api_key.as_str()));
        query.push(("format", "json"));
        let body = self
            .http_client
            .get(API_BASE)
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        if let Ok(err) = serde_json::from_str::<LastFmErrorResponse>(&body) {
            anyhow::bail!("{}", err.message);
        }

        serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("unexpected response format ({e})"))
    }

    /// Returns similar artist names for a seed artist, most similar first.
    pub async fn similar_artists(&self, artist: &str, limit: usize) -> anyhow::Result<Vec<String>> {
        let limit_str = limit.to_string();
        let response: SimilarArtistsResponse = self
            .get(&[
                ("method", "artist.getsimilar"),
                ("artist", artist),
                ("autocorrect", "1"),
                ("limit", limit_str.as_str()),
            ])
            .await?;
        Ok(response
            .similarartists
            .artist
            .into_iter()
            .map(|a| a.name)
            .collect())
    }

    /// Resolves a free-text query (e.g. "zutomayo saturn") to the best
    /// matching (artist, track) pair via Last.fm's fuzzy track search.
    pub async fn resolve_track(&self, query: &str) -> anyhow::Result<Option<(String, String)>> {
        let response: TrackSearchResponse = self
            .get(&[
                ("method", "track.search"),
                ("track", query),
                ("limit", "1"),
            ])
            .await?;
        Ok(response
            .results
            .trackmatches
            .track
            .into_iter()
            .next()
            .map(|t| (t.artist, t.name)))
    }

    /// Returns similar (artist, track) pairs for a seed track, most similar
    /// first.
    pub async fn similar_tracks(
        &self,
        artist: &str,
        track: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let limit_str = limit.to_string();
        let response: SimilarTracksResponse = self
            .get(&[
                ("method", "track.getsimilar"),
                ("artist", artist),
                ("track", track),
                ("autocorrect", "1"),
                ("limit", limit_str.as_str()),
            ])
            .await?;
        Ok(response
            .similartracks
            .track
            .into_iter()
            .map(|t| (t.artist.name, t.name))
            .collect())
    }
}
