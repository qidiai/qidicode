# Installer scripts

| Script | Channel | Platforms |
|---|---|---|
| `install.sh` | stable / alpha | POSIX bash; Git for Windows / MSYS2 bash |
| `install-enterprise.sh` | enterprise (standalone copy) | POSIX bash; Git for Windows / MSYS2 bash |
| `install.ps1` | stable / alpha | Windows PowerShell |
| `install-enterprise.ps1` | enterprise (standalone copy) | Windows PowerShell |

The two enterprise scripts are intentionally full copies (not
wrappers) so changes to the stable installer cannot break
enterprise deployments. Keep the four scripts in sync when
touching shared logic (version gate, download/promote, config
persistence).

## Environment variable naming domains

The installer scripts and the installed CLI speak two disjoint
environment-variable domains. **Do not mix them**: an installer
ignores runtime variables and the CLI ignores installer
variables. The few crossings listed below are deliberate,
retained contract points.

| Domain | Prefix | Read by | Variables |
|---|---|---|---|
| Installer | `QIDI_*` | install scripts only | `QIDI_BIN_DIR` (install-location override), `QIDI_CHANNEL` (stable\|alpha, install.sh/install.ps1 only), `QIDI_PROXY_URL` (deployment-config proxy override) |
| Runtime | `GROK_*` | the installed CLI | `GROK_DEPLOYMENT_KEY` (enterprise auth), `GROK_VERSION` (ps1 `-Version` alternative), plus the CLI's own runtime env |

### Retained contract points (crossings)

- **`GROK_DEPLOYMENT_KEY`** — consumed by the *enterprise
  installers* for deployment-key auth. The runtime contract name
  is kept so existing deployment scripts keep working.
- **`GROK_VERSION`** — consumed by `install.ps1` /
  `install-enterprise.ps1` as an alternative to the `-Version`
  parameter. The runtime contract name is kept.
- **`~/.grok`** — the runtime home directory (`auth.json`,
  `config.toml`, `downloads/`, `completions/`). `QIDI_BIN_DIR`
  overrides only the *binary* location; the runtime home stays
  `~/.grok`.

Old installer-side names (`GROK_BIN_DIR` and friends) are **not**
read by any installer — setting them has no effect.

## Version gate and atomic promote

All four installers:

1. validate an explicit version argument against
   `^[0-9]+\.[0-9]+\.[0-9]+(-suffix)?$` and re-validate the
   resolved (channel-probed) version before downloading;
2. download into a sibling `.tmp` file and promote atomically
   (`mv` / `Move-Item`) — a failed fetch never destroys the
   previous install;
3. on POSIX, execute the downloaded binary once (`--version`)
   before promotion, so a corrupt or truncated artifact is
   rejected while the existing install stays active.

## Tests

`cf-update/tests/test_install_sh.rs` runs the real scripts under
WSL/CI with a fake `curl`: the version-format gate, the
corruption blitz (full/truncate/garbage download matrix) for
both channels, and the shell-rc rewrite matrix (stow
absolute/relative/`..`, plain, first-create, enterprise).
