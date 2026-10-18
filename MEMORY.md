# Mémoire du projet pkgx

Point de reprise enregistré le 18 octobre 2026.

## Objectif et préférences

Continuer en autonomie l’implémentation de `pkgx.md`, avec une interface TUI inspirée de `../atfq` et `../omniscient`. Communiquer en français. Ne pas annoncer 100 % : macOS, CI réelle et publication restent à faire.

## État livré

- Workspace Rust 0.1.0 : `crates/pkgx-core` et `crates/pkgx-cli`, binaire `pkgx`.
- Exécuteurs revus pour APT, Pacman (ADR 007), DNF5 (ADR 010), Flatpak (ADR 011) et Homebrew formules (ADR 012) : préversion, confirmation, revalidation, journal, vérification post-transaction, `--sudo` limité à des modèles exacts (pas pour Flatpak/Homebrew). Aucun rollback garanti.
- Manifestes TOML additifs/stricts (strict limité aux autorisations APT, ADR 005), application par fournisseur dans un ordre stable.
- Recettes : installation/désinstallation suivies avec reçus, ressources `~/` uniquement, confiance explicite, jamais root, pkgx ne supprime jamais de chemin (ADR 008) ; `--observe` expérimental.
- Fichiers de config gérés (ADR 013, M8) : table `files` du manifeste, `diff`/`plan -f`/`apply -f`, `pkgx file adopt|list|forget|remove` avec sauvegardes privées ; copie d'octets uniquement, `~/` seulement ; sandbox `files-userns.sh` réussi ; `file list` donne l'état des reçus, TUI en lecture seule. M8 terminé hors macOS ; idées suivantes : écritures depuis le TUI, templates/secrets (nouvelle ADR).
- Nettoyage : classes safe/suggested/manual, `clean --cache` (apt-get clean / pacman -Sc) et `clean --orphans` via l’exécuteur revu (ADR 009).
- TUI, démo, complétions, packaging déterministe ; archives locales x86_64 gnu/musl dans `dist/` (non publiées).

## Validation native (sans toucher l’hôte)

Racines jetables sans privilèges dans `tests/sandbox/` (namespaces utilisateur, `unshare --map-auto`, tmpfs) : `arch-userns.sh`, `debian-userns.sh` (bookworm, noble ; sudo NOPASSWD et PAM interactif via pty), `fedora-userns.sh`, `flatpak-userns.sh`, `brew-userns.sh`, `recipe-userns.sh`, `files-userns.sh`. Toutes réussies le 18/10/2026 ; voir `docs/validation.md`. Le scratch /tmp est nosuid : les racines Debian vivent sur un tmpfs de namespace. Construire un binaire musl statique pour Debian/Fedora.

## Commandes

Toujours définir `CARGO_TARGET_DIR=/tmp/pkgx-target`.

```sh
cargo fmt --check
CARGO_TARGET_DIR=/tmp/pkgx-target cargo clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_TARGET_DIR=/tmp/pkgx-target cargo test --workspace --locked --offline -q
CARGO_TARGET_DIR=/tmp/pkgx-target cargo build --release --locked --offline --target x86_64-unknown-linux-musl -p pkgx-cli
python3 scripts/test_package.py
```

Dernier checkpoint : 184 tests Rust + 2 tests Python, Clippy sans avertissement.

## Travail restant

- Faire tourner les workflows CI configurés (conteneurs, sandbox recettes, macOS) et valider Homebrew sur macOS.
- Signature/notarisation et publication des distributions ; artefacts multi-plateformes.
- Suspension terminal de la TUI pendant le prompt sudo : couverte seulement par tests unitaires.

Ne jamais exécuter de transaction native, de sudo ou de script téléchargé sur l’hôte ; utiliser les racines jetables.

## Git

Remote `origin` = `https://github.com/moha-bekh/pkgx.git`, branche `main`. Créer des commits locaux et laisser l’utilisateur pousser ; ne pas pousser sans nouvelle demande.
