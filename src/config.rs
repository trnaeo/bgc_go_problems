use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    pub api: ApiConfig,
    #[serde(default)]
    pub render: RenderConfig,
    /// Ohne diesen Abschnitt findet kein Upload statt.
    pub ftp: Option<FtpConfig>,
    pub levels: Vec<Level>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FtpConfig {
    /// Schalter, um den Upload abzuschalten, ohne den Abschnitt zu löschen
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Namen der Umgebungsvariablen, aus denen Zugangsdaten gelesen werden
    pub host_env: String,
    pub user_env: String,
    pub password_env: String,
    #[serde(default = "default_remote_dir")]
    pub remote_dir: String,
    #[serde(default)]
    pub security: FtpSecurity,
    /// Zertifikat des Servers nicht prüfen (nur bis ein gültiges Zertifikat aktiv ist!)
    #[serde(default)]
    pub accept_invalid_certs: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FtpSecurity {
    Plain,
    #[default]
    ExplicitTls,
}

fn default_true() -> bool {
    true
}

fn default_remote_dir() -> String {
    "/".to_string()
}

#[derive(Debug, Deserialize)]
pub struct ApiConfig {
    pub url: String,
    pub host: Option<String>,
    #[serde(default = "default_rank_param")]
    pub rank_param: String,
    #[serde(default = "default_key_env")]
    pub key_env: String,
    pub user_id: String,
    #[serde(default = "default_user_id_param")]
    pub user_id_param: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RenderConfig {
    pub coordinates: bool,
    pub cell_size: u32,
    pub margin: usize,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            coordinates: true,
            cell_size: 40,
            margin: 1,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Level {
    pub name: String,
    pub from: Rank,
    pub to: Rank,
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("problems")
}

fn default_rank_param() -> String {
    "rank".to_string()
}

fn default_user_id_param() -> String {
    "user_id".to_string()
}

fn default_key_env() -> String {
    "RAPIDAPI_KEY".to_string()
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Konfiguration {path} nicht lesbar"))?;
        toml::from_str(&text).with_context(|| format!("Konfiguration {path} ungültig"))
    }
}

/// Spielstärke. Intern als fortlaufende Zahl: 30k = -30 ... 1k = -1, 1d = 0 ... 9d = 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Rank(i32);

impl TryFrom<String> for Rank {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let v = value.trim().to_ascii_lowercase();
        let (num, unit) = v.split_at(v.len().saturating_sub(1));
        let n: i32 = num
            .parse()
            .map_err(|_| format!("ungültiger Rang '{value}' (erwartet z.B. 18k oder 3d)"))?;
        match unit {
            "k" if (1..=30).contains(&n) => Ok(Rank(-n)),
            "d" if (1..=9).contains(&n) => Ok(Rank(n - 1)),
            _ => Err(format!(
                "ungültiger Rang '{value}' (erwartet z.B. 18k oder 3d)"
            )),
        }
    }
}

impl Rank {
    /// Darstellung wie im API-Feld `rank_display`, z.B. "18K" oder "3D".
    pub fn display(&self) -> String {
        if self.0 < 0 {
            format!("{}K", -self.0)
        } else {
            format!("{}D", self.0 + 1)
        }
    }

    /// Zufälliger Rang im (inklusiven) Bereich a..=b, Reihenfolge egal.
    pub fn random_between(a: Rank, b: Rank) -> Rank {
        use rand::Rng;
        let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
        Rank(rand::thread_rng().gen_range(lo..=hi))
    }
}
