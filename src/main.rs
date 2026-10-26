mod api;
mod board;
mod config;
mod ftp;
mod render;
mod sgf;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use board::Board;
use config::{Config, Level, Rank, RenderConfig};
use render::{compute_crop, render, StoneMark};
use sgf::{Color, Point};

const USAGE: &str = "\
Verwendung:
  go_problems [--config config.toml] [--no-upload]
      Holt pro konfiguriertem Niveau ein Problem, erzeugt .sgf und .png und lädt sie
      per FTP hoch (falls [ftp] konfiguriert ist).
  go_problems --upload-only [--config config.toml]
      Lädt die vorhandenen Dateien aus output_dir hoch, ohne neue Probleme abzurufen
      (mit Wiederholung gemäss [ftp] retry_wait_minutes / max_attempts).
  go_problems --sgf datei.sgf [--out praefix] [--config config.toml]
      Rendert eine lokale SGF-Datei (ohne API) nach <praefix>.png und <praefix>-solution.png.";

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Fehler: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut config_path = "config.toml".to_string();
    let mut sgf_path: Option<String> = None;
    let mut out_prefix: Option<String> = None;
    let mut no_upload = false;
    let mut upload_only = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .with_context(|| format!("{name} braucht einen Wert"))
        };
        match arg.as_str() {
            "--config" => config_path = value("--config")?,
            "--sgf" => sgf_path = Some(value("--sgf")?),
            "--out" => out_prefix = Some(value("--out")?),
            "--no-upload" => no_upload = true,
            "--upload-only" => upload_only = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unbekanntes Argument '{other}'\n{USAGE}"),
        }
    }

    if let Some(sgf_path) = sgf_path {
        // Offline-Modus: Konfiguration ist optional (nur [render] wird gebraucht).
        let render_cfg = match Config::load(&config_path) {
            Ok(c) => c.render,
            Err(_) => RenderConfig::default(),
        };
        let sgf = std::fs::read_to_string(&sgf_path)
            .with_context(|| format!("{sgf_path} nicht lesbar"))?;
        let prefix = PathBuf::from(out_prefix.unwrap_or_else(|| "problem".to_string()));
        return render_files(&sgf, &prefix, &render_cfg);
    }

    let config = Config::load(&config_path)?;

    if upload_only {
        let ftp_cfg =
            config.ftp.as_ref().filter(|f| f.enabled).context(
                "FTP-Upload ist nicht konfiguriert oder mit enabled = false deaktiviert",
            )?;
        let mut files = Vec::new();
        for level in &config.levels {
            let prefix = config.output_dir.join(format!("problem-{}", level.name));
            for suffix in [".sgf", ".png", "-solution.png"] {
                let path = path_with_suffix(&prefix, suffix);
                if path.exists() {
                    files.push(path);
                } else {
                    eprintln!("  Warnung: {} fehlt", path.display());
                }
            }
        }
        if files.is_empty() {
            bail!(
                "keine Dateien zum Hochladen in {} gefunden",
                config.output_dir.display()
            );
        }
        println!("FTP-Upload ({} Dateien) ...", files.len());
        return ftp::upload(ftp_cfg, files).await;
    }

    let key = std::env::var(&config.api.key_env)
        .with_context(|| format!("Umgebungsvariable {} ist nicht gesetzt", config.api.key_env))?;
    std::fs::create_dir_all(&config.output_dir)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut failed = 0;
    let mut files = Vec::new();
    for level in &config.levels {
        match process_level(&client, &config, &key, level).await {
            Ok(f) => files.extend(f),
            Err(e) => {
                failed += 1;
                eprintln!("Niveau {}: Fehler: {e:#}", level.name);
            }
        }
    }

    // Erfolgreich erzeugte Dateien auch dann hochladen, wenn einzelne Niveaus fehlschlugen.
    let mut upload_error = None;
    match (config.ftp.as_ref().filter(|f| f.enabled), no_upload) {
        (Some(ftp_cfg), false) => {
            println!("FTP-Upload ({} Dateien) ...", files.len());
            if let Err(e) = ftp::upload(ftp_cfg, files).await {
                eprintln!("FTP-Upload fehlgeschlagen: {e:#}");
                upload_error = Some(e);
            }
        }
        _ => println!("Kein FTP-Upload (kein [ftp]-Abschnitt, enabled = false oder --no-upload)."),
    }

    if failed > 0 {
        bail!(
            "{failed} von {} Niveaus fehlgeschlagen",
            config.levels.len()
        );
    }
    if upload_error.is_some() {
        bail!("FTP-Upload fehlgeschlagen");
    }
    Ok(())
}

/// Gibt die geschriebenen Dateien zurück.
async fn process_level(
    client: &reqwest::Client,
    config: &Config,
    key: &str,
    level: &Level,
) -> Result<Vec<PathBuf>> {
    let rank = Rank::random_between(level.from, level.to);
    println!(
        "Niveau {}: frage Rang {} an ...",
        level.name,
        rank.display()
    );

    let problem = api::fetch_problem(client, &config.api, key, rank).await?;
    println!(
        "  erhalten: id={} rank={}",
        problem.id.as_deref().unwrap_or("?"),
        problem.rank_display.as_deref().unwrap_or("?")
    );

    // Erst parsen/rendern, dann speichern: es werden nur gültige Dateien geschrieben.
    let prefix = config.output_dir.join(format!("problem-{}", level.name));
    let sgf_file = path_with_suffix(&prefix, ".sgf");
    let parsed = sgf::parse_problem(&problem.sgf).context("SGF ungültig")?;
    let comment = build_comment(
        problem.rank_display.as_deref(),
        problem.description.as_deref(),
        parsed.solution.first().map(|m| m.color),
        parsed.solution_count,
    );
    let mut sgf = if comment.is_empty() {
        problem.sgf.clone()
    } else {
        sgf::with_root_comment(&problem.sgf, &comment).context("SGF ungültig")?
    };
    let images = build_images(&sgf, &config.render)?;
    if !sgf.ends_with('\n') {
        sgf.push('\n');
    }
    std::fs::write(&sgf_file, sgf)
        .with_context(|| format!("{} nicht schreibbar", sgf_file.display()))?;
    println!("  gespeichert: {}", sgf_file.display());
    let mut files = vec![sgf_file];
    files.extend(save_images(&images, &prefix)?);
    Ok(files)
}

/// Kommentar für den SGF-Wurzelknoten: Rang, Beschreibung, ggf. Anzahl der Lösungen.
/// Steht in der Beschreibung "Black/White To Play", wird sie an den Spieler angepasst,
/// der die Lösung tatsächlich beginnt.
fn build_comment(
    rank: Option<&str>,
    description: Option<&str>,
    first_player: Option<Color>,
    solutions: usize,
) -> String {
    let mut lines = Vec::new();
    if let Some(r) = rank {
        lines.push(format!("Rank: {r}"));
    }
    if let Some(d) = description {
        lines.push(match first_player {
            Some(c) => fix_to_play(d, c),
            None => d.to_string(),
        });
    }
    if solutions > 1 {
        lines.push(format!("{solutions} Approaches"));
    }
    lines.join("\n")
}

fn fix_to_play(text: &str, player: Color) -> String {
    let name = match player {
        Color::Black => "Black",
        Color::White => "White",
    };
    // ASCII-Kleinschreibung behält die Byte-Positionen bei
    let lower = text.to_ascii_lowercase();
    for phrase in ["black to play", "white to play"] {
        if let Some(i) = lower.find(phrase) {
            return format!("{}{name} To Play{}", &text[..i], &text[i + phrase.len()..]);
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_to_play_replaces_black() {
        assert_eq!(
            build_comment(Some("17K"), Some("Black To Play"), Some(Color::White), 3),
            "Rank: 17K\nWhite To Play\n3 Approaches"
        );
    }

    #[test]
    fn single_solution_has_no_count() {
        assert_eq!(
            build_comment(Some("10K"), Some("Black To Play"), Some(Color::Black), 1),
            "Rank: 10K\nBlack To Play"
        );
    }
}

fn path_with_suffix(prefix: &Path, suffix: &str) -> PathBuf {
    let mut s = prefix.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

struct Images {
    problem: image::RgbImage,
    solution: image::RgbImage,
}

fn save_images(images: &Images, prefix: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for (img, suffix) in [
        (&images.problem, ".png"),
        (&images.solution, "-solution.png"),
    ] {
        let path = path_with_suffix(prefix, suffix);
        img.save(&path)
            .with_context(|| format!("{} nicht schreibbar", path.display()))?;
        println!("  gespeichert: {}", path.display());
        paths.push(path);
    }
    Ok(paths)
}

fn render_files(sgf: &str, prefix: &Path, cfg: &RenderConfig) -> Result<()> {
    let images = build_images(sgf, cfg)?;
    save_images(&images, prefix)?;
    Ok(())
}

fn build_images(sgf_text: &str, cfg: &RenderConfig) -> Result<Images> {
    let problem = sgf::parse_problem(sgf_text).context("SGF ungültig")?;
    if !problem.solution_marked {
        eprintln!(
            "  Hinweis: keine als richtig markierte Variante gefunden, verwende Hauptvariante"
        );
    }
    let size = problem.size;

    let mut board = Board::new(size);
    for &p in &problem.black {
        board.set(p, Some(Color::Black));
    }
    for &p in &problem.white {
        board.set(p, Some(Color::White));
    }

    // Ausschnitt: Ausgangssteine und alle Lösungszüge, damit beide Bilder gleich geschnitten sind.
    let mut points: Vec<Point> = problem
        .black
        .iter()
        .chain(&problem.white)
        .copied()
        .collect();
    points.extend(problem.solution.iter().filter_map(|m| m.at));
    points.extend(problem.second_solution_start.map(|b| b.at));
    let crop = compute_crop(size, &points, cfg.margin);

    let initial: Vec<StoneMark> = board
        .stones()
        .into_iter()
        .map(|(at, color)| StoneMark {
            at,
            color,
            label: None,
            removed: false,
        })
        .collect();
    let problem_img = render(size, &initial, None, crop, cfg);

    let mut labels: HashMap<Point, (String, Color)> = HashMap::new();
    let mut number = 0;
    for m in &problem.solution {
        if let Some(at) = m.at {
            number += 1;
            board.play(m.color, at);
            labels.insert(at, (number.to_string(), m.color));
        }
    }
    let mut solved: Vec<StoneMark> = board
        .stones()
        .into_iter()
        .map(|(at, color)| StoneMark {
            at,
            color,
            label: labels.get(&at).map(|(l, _)| l.clone()),
            removed: false,
        })
        .collect();
    // Geschlagene Steine: Nummer bleibt auf dem nun leeren Punkt stehen
    for (&at, (label, color)) in &labels {
        if board.get(at).is_none() {
            solved.push(StoneMark {
                at,
                color: *color,
                label: Some(label.clone()),
                removed: true,
            });
        }
    }
    let solution_img = render(size, &solved, problem.second_solution_start, crop, cfg);

    Ok(Images {
        problem: problem_img,
        solution: solution_img,
    })
}
