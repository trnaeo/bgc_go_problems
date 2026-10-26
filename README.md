# bgc_go_problems

A small Rust command-line tool that fetches Go (Weiqi/Baduk) problems from a
[RapidAPI](https://rapidapi.com/) Go problem API, renders them as PNG images and
optionally uploads the results to a web server via FTP. It was written for the
Berner Go Club (BGC) to publish fresh problems every day.

For every configured level (e.g. `26k`, `20k`, `14k`, `8k`, `2k`) the tool:

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
- `[[levels]]` – one entry per level with a `name` and a rank range (`from` / `to`, e.g. `"20k"` – `"25k"`)

> **Note:** `accept_invalid_certs = true` disables certificate verification for FTPS.
> Set it to `false` as soon as the server has a valid certificate.

### FTP retries

Transient FTP errors (connection closed/timeouts, temporary 4xx replies such as 421)
are retried after `retry_wait_minutes`, up to `max_attempts` attempts in total.
Permanent errors (wrong credentials, server without FTPS, missing permissions) abort
immediately, so that a wrong password is not tried repeatedly.

If you change `max_attempts` or `retry_wait_minutes`, keep `timeout-minutes` of the
`upload` job in the workflow in mind.

## GitHub Actions

[`.github/workflows/daily.yml`](.github/workflows/daily.yml) runs every day at 00:00 UTC
(and on manual dispatch) as two jobs:

1. **`fetch`** – runs the tests, fetches and renders the problems (`--no-upload`) and
   stores `problems/` as the workflow artifact `problems` (kept for 7 days).
2. **`upload`** (needs `fetch`) – downloads the artifact and uploads it via FTP
   (`--upload-only`).

If the FTP upload fails, use **Re-run failed jobs** in the Actions tab: only the `upload`
job runs again, using the files already produced by `fetch` (no new problems are fetched).

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
  ftp.rs      FTP/FTPS upload
assets/       font used for rendering
examples/     sample SGF and rendered images
problems/     generated problems (output)
```

## Tests

```sh
cargo test
```
