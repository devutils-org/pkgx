# Validation manuelle restante

Checklist des vérifications qui ne peuvent pas être faites depuis l'environnement de
développement Linux. Chaque scénario qui modifie des paquets tourne dans un
environnement **jetable** (runner CI, machine OrbStack, VM Tart), jamais sur le Mac
de tous les jours.

## 0. Pousser et lire la CI

```sh
git push
```

Le push déclenche `.github/workflows/ci.yml` :

| Job | Ce qu'il valide |
| --- | --- |
| `check` | fmt, clippy, tests Rust, test d'empaquetage Python |
| `apt-containers` | APT réel (inspection, transactions, sudo, mot de passe PAM) sur Debian bookworm et Ubuntu 24.04 |
| `native-inventory` | Pacman (Arch) et DNF5 (Fedora) réels |
| `recipe-sandbox` | Cycle de vie réel d'une recette script |
| `macos-smoke` | Tests Rust sur macOS, puis vraies transactions Homebrew (`tests/containers/brew-transactions.sh`) sur le runner jetable |

À noter :

- [ ] tous les jobs sont verts ;
- [ ] en cas d'échec, garder le lien du run et le log du step en échec.

`artifacts.yml` construit les archives (Linux gnu/musl, macOS) : vérifier qu'elles
sont produites, sans les publier.

## 1. Mot de passe sudo dans le TUI (machine OrbStack Debian)

Seul point encore couvert uniquement par des tests unitaires : le TUI doit rendre
le terminal (mode normal, curseur visible) pendant la demande de mot de passe
sudo, puis reprendre l'affichage proprement.

### Préparer la machine

Sur le Mac :

```sh
orb create debian pkgx-test
orb -m pkgx-test
```

Dans la machine :

```sh
sudo apt-get update
sudo apt-get install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
git clone <url-du-dépôt> ~/pkgx && cd ~/pkgx
CARGO_TARGET_DIR=$HOME/pkgx-target cargo build --release --locked -p pkgx-cli
sudo install -m 0755 $HOME/pkgx-target/release/pkgx /usr/local/bin/pkgx
```

L'utilisateur par défaut d'OrbStack a sudo **sans** mot de passe. Pour voir une
vraie demande de mot de passe, créer un compte jetable limité à `apt-get` :

```sh
sudo useradd -m -s /bin/bash testeur
echo 'testeur:motdepasse-jetable' | sudo chpasswd
echo 'testeur ALL=(root) /usr/bin/apt-get' | sudo tee /etc/sudoers.d/testeur
sudo chmod 0440 /etc/sudoers.d/testeur
sudo -iu testeur
```

Toutes les étapes suivantes se font en tant que `testeur`.

### Installation depuis un manifeste

```sh
cat > ~/hello.toml <<'EOF'
version = 1
[packages.apt]
install = ['hello']
EOF
pkgx tui --allow-apply --sudo -f ~/hello.toml --state-dir ~/pkgx-records
```

1. `m` : aperçu du plan du manifeste (installation de `hello`).
2. `x`, taper `apply`, Entrée.
3. sudo demande le mot de passe.

- [ ] le TUI a disparu et le terminal est normal (écho, curseur) pendant la saisie ;
- [ ] un **mauvais** mot de passe : sudo redemande ou échoue, le TUI revient et
      indique l'échec, `hello` n'est pas installé (`dpkg -s hello` échoue) ;
- [ ] relancer avec le bon mot de passe : `hello` est installé, le TUI revient
      sans artefacts d'affichage et indique un résultat vérifié.

### Suppression depuis l'inventaire

```sh
pkgx tui --allow-apply --sudo --state-dir ~/pkgx-records
```

1. `/` puis `hello` pour filtrer, sélectionner le paquet.
2. `r` : aperçu de la suppression.
3. `x`, taper `apply`, Entrée, puis le mot de passe.

- [ ] mêmes vérifications d'affichage que ci-dessus ;
- [ ] `hello` est supprimé ;
- [ ] Ctrl-C pendant la demande de mot de passe : rien n'est modifié, le TUI
      se ferme ou revient proprement, le terminal reste utilisable (pas besoin de `reset`).

### Journal et nettoyage

```sh
pkgx history --state-dir ~/pkgx-records
exit                     # retour à l'utilisateur par défaut
exit                     # sortie de la machine
orb delete pkgx-test     # sur le Mac
```

- [ ] l'historique contient les transactions avec leur résultat.

## 2. Optionnel : conteneurs Linux en local (Docker d'OrbStack)

Uniquement pour déboguer un job `apt-containers` ou `native-inventory` sans attendre
la CI. Depuis le dépôt sur le Mac, construire le binaire statique comme la CI
(amd64 via Rosetta) :

```sh
docker run --rm --platform linux/amd64 -v "$PWD":/src -w /src rust:1 sh -c \
  'apt-get update && apt-get install -y musl-tools &&
   rustup target add x86_64-unknown-linux-musl &&
   cargo build --release --locked --target x86_64-unknown-linux-musl -p pkgx-cli'
BIN=$PWD/target/x86_64-unknown-linux-musl/release/pkgx
```

Puis, par exemple :

```sh
docker run --rm --platform linux/amd64 -v "$BIN:/usr/local/bin/pkgx:ro" \
  -v "$PWD/tests/containers:/checks:ro" debian:bookworm-slim sh -c \
  'apt-get update && sh /checks/inspection.sh && sh /checks/transactions.sh &&
   sh /checks/privilege.sh && sh /checks/privilege-pam.sh'
docker run --rm --platform linux/amd64 -v "$BIN:/usr/local/bin/pkgx:ro" \
  -v "$PWD/tests/containers:/checks:ro" archlinux:base sh -c \
  'pacman -Sy --noconfirm && sh /checks/pacman.sh && sh /checks/pacman-transactions.sh'
docker run --rm --platform linux/amd64 -v "$BIN:/usr/local/bin/pkgx:ro" \
  -v "$PWD/tests/containers:/checks:ro" fedora:latest sh -c \
  'sh /checks/rpm.sh && sh /checks/dnf-transactions.sh'
```

Les conteneurs sont supprimés (`--rm`) à la fin.

## 3. Optionnel : Homebrew dans une VM macOS (Tart)

Le job `macos-smoke` fait déjà ce travail sur un runner jetable : la VM ne sert
qu'à reproduire un échec en local. Tart demande un Mac Apple Silicon.

```sh
brew install cirruslabs/cli/tart
tart clone ghcr.io/cirruslabs/macos-sequoia-base:latest pkgx-test
tart run pkgx-test &
ssh admin@"$(tart ip pkgx-test)"          # mot de passe : admin
```

Dans la VM (Homebrew est préinstallé dans l'image) :

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
git clone <url-du-dépôt> ~/pkgx && cd ~/pkgx
cargo build --locked -p pkgx-cli
PKGX_DISPOSABLE=1 PATH="$PWD/target/debug:$PATH" sh tests/containers/brew-transactions.sh
```

- [ ] le script se termine par `homebrew checks passed`.

Le script retire `jq`/`oniguruma` s'ils sont présents et `clean --orphans` supprime
**toutes** les formules orphelines : il refuse de tourner sans `PKGX_DISPOSABLE=1`.
Ne jamais définir cette variable sur le Mac de tous les jours.

Nettoyage, sur le Mac : `tart stop pkgx-test && tart delete pkgx-test`.

## 4. Hors périmètre de ces tests

- Signature, notarisation macOS et publication des binaires : il faut les clés et
  le compte Apple Developer.
- Une fois les points 0 et 1 validés, mettre à jour `docs/validation.md` avec la
  date et les résultats.
