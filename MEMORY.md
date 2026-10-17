# Mémoire du projet pkgx

Point de reprise enregistré le 17 octobre 2026.

## Objectif et préférences

Continuer en autonomie l’implémentation de `pkgx.md`, avec une interface TUI inspirée de `../atfq` et `../omniscient`. Communiquer en français. Ne pas annoncer 100 % : des fonctions et validations natives restent à terminer.

## État livré

- Workspace Rust 0.1.0 : `crates/pkgx-core` et `crates/pkgx-cli`, binaire `pkgx`.
- Inventaire, recherche et inspection des fournisseurs APT, Pacman, DNF, Flatpak et Brew ; capacités explicitement limitées selon le fournisseur.
- Plans typés, validation de l’inventaire avant exécution, plans sauvegardés avec expiration, journal privé et historique.
- Exécution native APT avec confirmation, protections natives, vérification après transaction et annulation bornée. Aucun rollback garanti.
- Manifestes TOML, profils et composition, export, diff et application additive. Mode strict APT limité aux autorisations explicites du registre `managed`, verrouillé pendant l’exécution ; aucune propriété déduite des paquets installés.
- Élévation explicite `--sudo` pour les transactions APT : authentification après confirmation, inspections non privilégiées, seuls les arguments APT générés sont élevés, journal conservé chez l’utilisateur. Nettoyage des processus borné avec signalement honnête des limites.
- Rapports de dépendances natifs sans graphe résolu inventé ; audit consultatif sans suppression de fichiers.
- Recettes : validation et téléchargement borné avec vérification SHA-256 uniquement ; aucun script exécuté.
- TUI avec filtres, navigation, aide, aperçu et revue explicite des transactions ; démonstration synthétique portable via `pkgx demo` et `pkgx demo --snapshot`.
- Complétions Bash/Zsh/Fish, documentation, CI et workflow manuel d’artefacts ; packaging reproductible en Python avec sommes de contrôle.

## Références à lire pour reprendre

- `pkgx.md` : feuille de route initiale.
- `docs/continuation-plan.md`, `docs/completion-plan.md` : suite du travail.
- `docs/validation.md` : preuves et limites des validations.
- `docs/privilege-plan.md`, `docs/adr/006-explicit-sudo.md` : élévation APT.
- `docs/adr/005-managed-strict-scope.md` : périmètre strict géré.
- `docs/support.md`, `docs/security-review.md`, `docs/recovery.md` : capacités et limites.

## Dernier checkpoint de validation

Ces résultats proviennent de la dernière validation du code, antérieure à ce fichier mémoire : 134 tests Rust et 2 tests Python réussis ; un test Rust auxiliaire ignoré seul est invoqué en sous-processus par sa régression parente. Formatage, Clippy sans avertissements et compilation release réussis.

Toujours définir `CARGO_TARGET_DIR=/tmp/pkgx-target` pour les commandes Cargo qui compilent ; la destination partagée par défaut est en lecture seule.

```sh
cargo fmt --check
CARGO_TARGET_DIR=/tmp/pkgx-target cargo clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_TARGET_DIR=/tmp/pkgx-target cargo test --workspace --locked --offline -q
CARGO_TARGET_DIR=/tmp/pkgx-target cargo build --release --locked --offline -p pkgx-cli
python3 scripts/test_package.py
```

Le binaire validé est `/tmp/pkgx-target/release/pkgx`. L’archive locale est `dist/pkgx-v0.1.0-aarch64-unknown-linux-gnu.tar.gz`, accompagnée de son fichier `.sha256` ; elle cible Linux ARM64, pas macOS. Aucune release n’a été publiée. `dist/` est ignoré par Git.

## Travail restant

- Exécuter les validations natives en environnements jetables : transactions Debian/Ubuntu, PAM/sudo interactif, verrouillage et annulation ; puis matrices Arch/Fedora/macOS et Flatpak. Les workflows configurés ne constituent pas une preuve d’exécution.
- Implémenter les exécuteurs natifs hors APT et les garanties associées.
- Implémenter l’installation/désinstallation de recettes, les reçus, la propriété vérifiable des ressources et leur isolation.
- Implémenter le nettoyage effectif et sûr, puis étudier le mode strict multi-fournisseurs.
- Terminer la qualification, la signature et la publication des distributions.

Ne pas tester de transactions natives, de sudo ou de scripts téléchargés sur la machine de travail. Utiliser les mocks, répertoires temporaires et conteneurs jetables prévus.

## Demande de commit et push

Demande utilisateur : sauvegarder la mémoire, créer un commit et pousser.

Les restrictions initiales sur `.git/` ont été levées. Le dépôt local utilise la branche `main` et le remote `origin` est configuré sur `https://github.com/moha-bekh/pkgx.git`. Le checkpoint est préparé pour le commit initial ; consulter `git log` pour son identifiant définitif. Dernière consigne utilisateur : créer les commits locaux et laisser l’utilisateur effectuer le push. Ne pas pousser sans nouvelle demande.
