use std::fs::File;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use suppaftp::native_tls::TlsConnector;
use suppaftp::types::FileType;
use suppaftp::{
    FtpError, FtpStream, ImplFtpStream, NativeTlsConnector, NativeTlsFtpStream, TlsStream,
};

use crate::config::{FtpConfig, FtpSecurity};

/// Timeout für den TCP-Verbindungsaufbau
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout für Lesen/Schreiben auf der Steuerverbindung (inkl. Begrüssung des Servers)
const IO_TIMEOUT: Duration = Duration::from_secs(60);

struct Credentials {
    host: String,
    user: String,
    password: String,
}

fn env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("Umgebungsvariable {name} ist nicht gesetzt"))
}

/// Lädt die Dateien in `cfg.remote_dir` hoch. Bereits vorhandene Dateien werden überschrieben.
/// Bei vorübergehenden Fehlern wird gemäss `max_attempts`/`retry_wait_minutes` wiederholt.
pub async fn upload(cfg: &FtpConfig, files: Vec<PathBuf>) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    if let Some(missing) = files.iter().find(|f| !f.is_file()) {
        bail!("{} nicht lesbar", missing.display());
    }
    let creds = Credentials {
        host: env(&cfg.host_env)?,
        user: env(&cfg.user_env)?,
        password: env(&cfg.password_env)?,
    };
    let cfg = cfg.clone();
    tokio::task::spawn_blocking(move || upload_with_retry(&cfg, &creds, &files))
        .await
        .context("FTP-Task abgebrochen")?
}

fn upload_with_retry(cfg: &FtpConfig, creds: &Credentials, files: &[PathBuf]) -> Result<()> {
    let max_attempts = cfg.max_attempts.max(1);
    let wait = Duration::from_secs(cfg.retry_wait_minutes.saturating_mul(60));
    let mut attempt = 1;
    loop {
        let err = match upload_blocking(cfg, creds, files) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        if !is_transient(&err) {
            return Err(err.context("Fehler ist nicht durch Wiederholen behebbar"));
        }
        if attempt >= max_attempts {
            return Err(err.context(format!("aufgegeben nach {attempt} Versuch(en)")));
        }
        eprintln!("  Versuch {attempt}/{max_attempts} fehlgeschlagen: {err:#}");
        println!(
            "  Neuer Versuch in {} Minute(n) ...",
            cfg.retry_wait_minutes
        );
        std::thread::sleep(wait);
        attempt += 1;
        println!("  FTP-Upload, Versuch {attempt}/{max_attempts} ...");
    }
}

/// Vorübergehend: Verbindungs-/IO-Fehler und temporäre FTP-Antworten (421, 425, 426, 450-452).
/// Alles andere (Login, fehlendes FTPS, Zugriffsrechte, TLS-Konfiguration) ist dauerhaft;
/// wiederholte Fehl-Logins könnten ausserdem zu einer Sperre führen.
fn is_transient(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        if let Some(ftp) = cause.downcast_ref::<FtpError>() {
            match ftp {
                FtpError::ConnectionError(_) => true,
                FtpError::UnexpectedResponse(r) => {
                    matches!(r.status.code(), 421 | 425 | 426 | 450 | 451 | 452)
                }
                _ => false,
            }
        } else {
            cause.is::<std::io::Error>()
        }
    })
}

/// Baut die TCP-Verbindung mit Timeouts auf (auch für Lesen/Schreiben, damit ein
/// hängender Server den Lauf nicht blockiert).
fn tcp_connect(sock: std::net::SocketAddr, addr: &str) -> Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&sock, CONNECT_TIMEOUT)
        .with_context(|| format!("Verbindung zu {addr} fehlgeschlagen"))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(stream)
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
            let mut ftp = FtpStream::connect_with_stream(tcp_connect(sock, &addr)?)
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
            let ftp = NativeTlsFtpStream::connect_with_stream(tcp_connect(sock, &addr)?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use suppaftp::types::Response;
    use suppaftp::Status;

    fn response(status: Status) -> anyhow::Error {
        anyhow::Error::new(FtpError::UnexpectedResponse(Response::new(
            status,
            b"test".to_vec(),
        )))
    }

    #[test]
    fn connection_errors_are_transient() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection closed");
        let err = anyhow::Error::new(FtpError::ConnectionError(io))
            .context("Verbindung zu host:21 fehlgeschlagen");
        assert!(is_transient(&err));
    }

    #[test]
    fn temporary_replies_are_transient() {
        assert!(is_transient(&response(Status::NotAvailable)));
    }

    #[test]
    fn login_and_missing_ftps_are_permanent() {
        assert!(!is_transient(&response(Status::NotLoggedIn)));
        assert!(!is_transient(&response(Status::BadCommand)));
        assert!(!is_transient(&anyhow!("irgendein anderer Fehler")));
    }
}
