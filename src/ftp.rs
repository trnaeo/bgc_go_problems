use std::fs::File;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use suppaftp::native_tls::TlsConnector;
use suppaftp::types::FileType;
use suppaftp::{FtpStream, ImplFtpStream, NativeTlsConnector, NativeTlsFtpStream, TlsStream};

use crate::config::{FtpConfig, FtpSecurity};

const TIMEOUT: Duration = Duration::from_secs(30);

struct Credentials {
    host: String,
    user: String,
    password: String,
}

fn env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("Umgebungsvariable {name} ist nicht gesetzt"))
}

/// Lädt die Dateien in `cfg.remote_dir` hoch. Bereits vorhandene Dateien werden überschrieben.
pub async fn upload(cfg: &FtpConfig, files: Vec<PathBuf>) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let creds = Credentials {
        host: env(&cfg.host_env)?,
        user: env(&cfg.user_env)?,
        password: env(&cfg.password_env)?,
    };
    let cfg = cfg.clone();
    tokio::task::spawn_blocking(move || upload_blocking(&cfg, &creds, &files))
        .await
        .context("FTP-Task abgebrochen")?
}

fn upload_blocking(cfg: &FtpConfig, creds: &Credentials, files: &[PathBuf]) -> Result<()> {
    // "host" oder "host:port"
    let (domain, addr) = match creds.host.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), creds.host.clone())
        }
        _ => (creds.host.clone(), format!("{}:21", creds.host)),
    };
    let sock = addr
        .to_socket_addrs()
        .with_context(|| format!("FTP-Host {addr} nicht auflösbar"))?
        .next()
        .ok_or_else(|| anyhow!("FTP-Host {addr} nicht auflösbar"))?;

    match cfg.security {
        FtpSecurity::Plain => {
            eprintln!("  Warnung: FTP ohne Verschlüsselung, Passwort wird im Klartext übertragen");
            let mut ftp = FtpStream::connect_timeout(sock, TIMEOUT)
                .with_context(|| format!("Verbindung zu {addr} fehlgeschlagen"))?;
            transfer(&mut ftp, cfg, creds, files)
        }
        FtpSecurity::ExplicitTls => {
            let mut builder = TlsConnector::builder();
            if cfg.accept_invalid_certs {
                eprintln!(
                    "  Warnung: Zertifikat des FTP-Servers wird nicht geprüft (accept_invalid_certs = true)"
                );
                builder.danger_accept_invalid_certs(true);
                builder.danger_accept_invalid_hostnames(true);
            }
            let connector = builder
                .build()
                .context("TLS-Initialisierung fehlgeschlagen")?;
            let ftp = NativeTlsFtpStream::connect_timeout(sock, TIMEOUT)
                .with_context(|| format!("Verbindung zu {addr} fehlgeschlagen"))?;
            let mut ftp = ftp
                .into_secure(NativeTlsConnector::from(connector), &domain)
                .context("TLS-Handshake mit dem FTP-Server fehlgeschlagen (Zertifikat ungültig? Server ohne FTPS?)")?;
            transfer(&mut ftp, cfg, creds, files)
        }
    }
}

fn transfer<T: TlsStream>(
    ftp: &mut ImplFtpStream<T>,
    cfg: &FtpConfig,
    creds: &Credentials,
    files: &[PathBuf],
) -> Result<()> {
    ftp.login(creds.user.as_str(), creds.password.as_str())
        .context("FTP-Login fehlgeschlagen")?;
    ftp.transfer_type(FileType::Binary)?;
    change_dir(ftp, &cfg.remote_dir)?;

    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("ungültiger Dateiname {}", path.display()))?;
        let mut file =
            File::open(path).with_context(|| format!("{} nicht lesbar", path.display()))?;
        // STOR überschreibt eine vorhandene Datei gleichen Namens
        ftp.put_file(name, &mut file)
            .with_context(|| format!("Upload von {name} fehlgeschlagen"))?;
        println!("  hochgeladen: {name}");
    }
    let _ = ftp.quit();
    Ok(())
}

/// Wechselt in das Zielverzeichnis und legt fehlende Verzeichnisse an.
fn change_dir<T: TlsStream>(ftp: &mut ImplFtpStream<T>, dir: &str) -> Result<()> {
    if dir.starts_with('/') {
        ftp.cwd("/")?;
    }
    for part in dir.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if ftp.cwd(part).is_err() {
            ftp.mkdir(part)
                .with_context(|| format!("Verzeichnis {part} nicht anlegbar"))?;
            ftp.cwd(part)?;
        }
    }
    Ok(())
}
