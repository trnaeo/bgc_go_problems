# bgc_go_problems

A small Rust command-line tool that fetches Go (Weiqi/Baduk) problems from a
[RapidAPI](https://rapidapi.com/) Go problem API, renders them as PNG images and
optionally uploads the results to a web server via FTP. It was written for the
Berner Go Club (BGC) to publish fresh problems every day.

For every configured level (`anfaenger` 26k–16k, `aufsteiger` 15k–11k, `mittelstufe` 10k–6k,
`fortgeschritten` 5k–1d, `experte` 2d–3d) the tool:

1. picks a random rank within the level's range and requests a problem from the API,
2. validates the SGF and adds a root comment (rank, description, number of approaches),
3. renders two images – the problem and the solution (with numbered moves),
4. writes everything to the output directory (`problems/` by default),
5. uploads the files via FTP (if enabled).

## Output

All generated files are written to the **`problems/`** directory (configurable via
`output_dir` in `config.toml`), not to the repository root. For each level `<name>`:

| File                                 | Content                          |
| ------------------------------------ | -------------------------------- |
| `problems/problem-<name>.sgf`        | The problem as SGF               |
| `problems/problem-<name>.png`        | Image of the initial position    |
| `problems/problem-<name>-solution.png` | Image of the solution          |

Existing files are overwritten on each run. The directory is created automatically.

## Requirements

- Rust (stable, edition 2021)
- OpenSSL development files (needed for FTPS), e.g. `libssl-dev` and `pkg-config` on Debian/Ubuntu
- A RapidAPI key for the Go problem API

## Usage

```sh
# Fetch problems for all levels, render them, upload via FTP (if enabled)
cargo run --release

# Same, but never upload
cargo run --release -- --no-upload

# Only upload the files already present in output_dir (with retries, see [ftp] settings);
# fails if no files are found
cargo run --release -- --upload-only

# Render a local SGF file (no API access needed)
cargo run --release -- --sgf examples/sample.sgf --out problems/sample

# Use a different configuration file
cargo run --release -- --config my-config.toml
```

The API key and FTP credentials are read from environment variables (the names are
configurable in `config.toml`):

| Variable          | Purpose                            |
| ----------------- | ---------------------------------- |
| `RAPIDAPI_KEY`    | RapidAPI key (`x-rapidapi-key`)    |
| `BGC_FTP_HOST`    | FTP host, e.g. `ftp.example.com` or `ftp.example.com:21` |
| `BGC_FTP_USER`    | FTP user                           |
| `BGC_FTP_PASSW`   | FTP password                       |

## Configuration

Everything is configured in [`config.toml`](config.toml):

- `output_dir` – directory for the generated `.sgf` and `.png` files (default: `problems`)
- `[api]` – endpoint URL, RapidAPI host, query parameter names, user id used to mark problems as "seen"
- `[render]` – `coordinates` (show coordinates), `cell_size` (pixels per grid cell), `margin` (extra lines around the stones)
- `[ftp]` – `enabled`, names of the credential environment variables, `remote_dir`,
  `security` (`explicit_tls` or `plain`), `accept_invalid_certs`, and the retry settings
  `retry_wait_minutes` (default 5) and `max_attempts` (default 3, total number of attempts)
- `[[levels]]` – one entry per level with a `name` and a rank range (`from` / `to`, e.g. `"26k"` – `"16k"`; a random rank within the range is requested)

> **Note:** `accept_invalid_certs = true` disables certificate verification for FTPS.
> Set it to `false` as soon as the server has a valid certificate.

### FTP retries

Transient FTP errors (connection closed/timeouts, temporary 4xx replies such as 421)
are retried after `retry_wait_minutes`, up to `max_attempts` attempts in total.
Permanent errors (wrong credentials, server without FTPS, missing permissions) abort
immediately, so that a wrong password is not tried repeatedly.

If you change `max_attempts` or `retry_wait_minutes`, keep `timeout-minutes` of the
`upload` job in the workflow in mind.

### FTP free space and clean-up

FTP servers often have a disk quota, and plain FTP has no standard command to query the
free space. The upload therefore works in two steps:

1. **Before the upload** the tool asks the server for the free space (`AVBL`, a non-standard
   extension) and logs it together with the space needed for the new files, e.g.
   `Freier Speicher auf dem FTP-Server: 12.30 MiB (benötigt für Upload: 1.20 MiB)`.
   If the server does not report it, the log says `unbekannt`. If the free space is
   smaller than needed, the old `problem-*.png` and `problem-*.sgf` files in `remote_dir`
   are deleted first.
2. **During the upload**, if the server answers with `552` (quota exceeded) or `452`
   (insufficient storage), the old `problem-*.png` / `problem-*.sgf` files in `remote_dir`
   are deleted once and the failed file is uploaded again. Files already uploaded in the
   same run are kept. This is the fallback for servers that do not report the free space.

Every step is logged (`gelöscht: <file> (<size>)`, number of deleted files, freed space).
Only files starting with `problem-` and ending in `.png` or `.sgf` are deleted; other
files in `remote_dir` are never touched.

## Deployment (Raspberry Pi)

[`deploy.sh`](deploy.sh) cross-compiles the binary for the Raspberry Pi (aarch64, glibc 2.31)
with [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) and copies it together
with `config.toml` and `secrets.env` (if present) to `bgcuser@raspberrypi2:/home/bgcuser/go_problems/`:

```sh
./deploy.sh
```

Requirements: `cargo-zigbuild`, [Zig](https://ziglang.org/), the Rust target
`aarch64-unknown-linux-gnu` (`rustup target add aarch64-unknown-linux-gnu`), `rsync` and SSH access to the Pi.

## GitHub Actions (Disabled)

[`.github/workflows/daily.yml`](.github/workflows/daily.yml) runs every day at 00:00 UTC
(and on manual dispatch) as two jobs:

1. **`fetch`** – runs the tests, fetches and renders the problems (`--no-upload`) and
   stores `problems/` as the workflow artifact `problems` and the compiled binary as the
   artifact `binary` (both kept for 7 days).
2. **`upload`** (needs `fetch`) – downloads both artifacts and uploads the files via FTP
   (`--upload-only`). It does not compile anything and needs no Rust toolchain.

Both jobs run on the pinned image `ubuntu-24.04`, because the binary links OpenSSL
dynamically and is reused between the jobs.

If the FTP upload fails, use **Re-run failed jobs** in the Actions tab: only the `upload`
job runs again, using the files and the binary already produced by `fetch` (no new
problems are fetched).

The secrets must be created under *Settings → Secrets and variables → **Actions***:
`RAPIDAPI_KEY` (used by `fetch`) and `BGC_FTP_HOST`, `BGC_FTP_USER`, `BGC_FTP_PASSW`
(used by `upload`).

## Project layout

```
src/
  main.rs     CLI, orchestration
  api.rs      API client
  config.rs   configuration and rank handling
  sgf.rs      SGF parsing
  board.rs    board logic (captures etc.)
  render.rs   PNG rendering
  ftp.rs      FTP/FTPS upload (retries, free-space check, clean-up)
assets/       font used for rendering
examples/     sample SGF and rendered images
problems/     generated problems (output)
```

## Tests

```sh
cargo test
```
