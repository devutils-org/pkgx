# Mémoire du projet xpac

Point de reprise enregistré le 19 octobre 2026.

## Objectif et préférences

Continuer en autonomie l’implémentation de `docs/ROADMAP.md`, avec une interface TUI inspirée de `../atfq` et `../omniscient`. Communiquer en français. Ne pas annoncer 100 % : macOS, CI réelle et publication restent à faire.

## État livré

- Workspace Rust 0.1.0 : `crates/xpac-core` et `crates/xpac-cli`, binaire `xpac`.
- Exécuteurs revus pour APT, Pacman (ADR 007), DNF5 (ADR 010), Flatpak (ADR 011) et Homebrew formules (ADR 012) : préversion, confirmation, revalidation, journal, vérification post-transaction, `--sudo` limité à des modèles exacts (pas pour Flatpak/Homebrew). Aucun rollback garanti.
- Manifestes TOML additifs/stricts (strict limité aux autorisations APT, ADR 005), application par fournisseur dans un ordre stable.
- Recettes : installation/désinstallation suivies avec reçus, ressources `~/` uniquement, confiance explicite, jamais root, xpac ne supprime jamais de chemin (ADR 008) ; `--observe` expérimental.
- Fichiers de config gérés (ADR 013, M8) : table `files` du manifeste, `diff`/`plan -f`/`apply -f`, `xpac file adopt|list|forget|remove` avec sauvegardes privées ; copie d'octets uniquement, `~/` seulement ; sandbox `files-userns.sh` réussi ; `file list` donne l'état des reçus, TUI en lecture seule. M8 terminé hors macOS ; idées suivantes : écritures depuis le TUI, templates/secrets (nouvelle ADR).
- Nettoyage : classes safe/suggested/manual, `clean --cache` (apt-get clean / pacman -Sc) et `clean --orphans` via l’exécuteur revu (ADR 009).
- TUI, démo, complétions, packaging déterministe ; archives locales x86_64 gnu/musl dans `dist/` (non publiées).

## Validation native (sans toucher l’hôte)

Racines jetables sans privilèges dans `tests/sandbox/` (namespaces utilisateur, `unshare --map-auto`, tmpfs) : `arch-userns.sh`, `debian-userns.sh` (bookworm, noble ; sudo NOPASSWD et PAM interactif via pty), `fedora-userns.sh`, `flatpak-userns.sh`, `brew-userns.sh`, `recipe-userns.sh`, `files-userns.sh`. Toutes réussies le 18/10/2026 ; voir `docs/validation.md`. Le scratch /tmp est nosuid : les racines Debian vivent sur un tmpfs de namespace. Construire un binaire musl statique pour Debian/Fedora.

## Commandes

Toujours définir `CARGO_TARGET_DIR=/tmp/xpac-target`.

```sh
cargo fmt --check
CARGO_TARGET_DIR=/tmp/xpac-target cargo clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_TARGET_DIR=/tmp/xpac-target cargo test --workspace --locked --offline -q
CARGO_TARGET_DIR=/tmp/xpac-target cargo build --release --locked --offline --target x86_64-unknown-linux-musl -p xpac-cli
python3 scripts/test_package.py
python3 scripts/distroproof/test_check.py
```

Dernier checkpoint : 184 tests Rust + 2 tests Python, Clippy sans avertissement.

## Portabilité Distroproof — 19/10/2026

- `distroproof.toml` : 16 images Linux x86_64 + variantes Debian root/Fedora lockdown ; **18/18** réussies, 35 combinaisons archive/VM applicables, 749 contrôles. GNU non applicable sur Alpine musl, qui passe avec l'archive statique.
- `distroproof-transactions.toml` : **10/10** VM, 440 contrôles d'inspection puis 17 scripts de transactions APT/Pacman/DNF5 ; aucun gestionnaire natif de l'hôte modifié.
- APT abaissé de 2.6 à **1.6**, avec inventaires natifs et transactions réelles mesurés (Ubuntu 18.04 à 24.04, Debian 10 à 12). Consultation Debian 13/sid et Rocky DNF4 ; APK/Portage non implémentés.
- Archives GNU **glibc 2.17** (`cargo-zigbuild`/Zig 0.16) et musl statique via `sh scripts/distroproof/build.sh` ; contrôles ELF ; artefacts Linux GNU CI alignés sur ce plancher.
- `scripts/distroproof/check.py` juge les cellules, empreintes des binaires, couverture et faits envprobe. Tests négatifs `test_check.py`. Workflow manuel `.github/workflows/distroproof.yml` préparé, pas exécuté sur GitHub.
- Rapport : `docs/distroproof.md`. Données locales : `dist/matrix/inspection.json`, `dist/matrix/transactions-final.json`, cellules jugées dans `dist/matrix/*-cells/`. La matrice ne couvre pas aarch64/macOS/Flatpak/Homebrew/TUI interactif.

## Travail restant

- CI verte (run 39143547380) et artefacts Linux/macOS vérifiés (run 39143553843) le 18/10/2026 ; roadmap M0–M8 cochée. Reste : pas de job Flatpak en CI, TUI sur terminal macOS.
- Signature/notarisation et publication des distributions ; artefacts multi-plateformes.
- Suspension terminal de la TUI pendant le prompt sudo : couverte seulement par tests unitaires.

Ne jamais exécuter de transaction native, de sudo ou de script téléchargé sur l’hôte ; utiliser les racines jetables.

## Git

Remote `origin` = `https://github.com/devutils-org/xpac.git`, branche `main`. Créer des commits locaux et laisser l’utilisateur pousser ; ne pas pousser sans nouvelle demande. Le SSH n’a pas d’agent dans les sessions Claude : pousser en HTTPS avec `git -c credential.helper='!gh auth git-credential' push https://github.com/devutils-org/xpac.git main` .
